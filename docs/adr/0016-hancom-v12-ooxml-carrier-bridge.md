# ADR-0016: Bridge only evidence-backed Cell 12.0300 and Show 12.0000 carrier subsets

- Status: Accepted
- Decision date: 2026-08-30
- Last reviewed: 2026-08-31
- Scope: read-only Cell/Show discovery, validation, native sibling creation,
  host integration, installation, and release evidence
- Related: [ADR-0014](0014-hancom-format-handler-install-promotion.md),
  [plugin protocol](../../plugins/plugin-protocol.md)
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)
- Provenance: [`../spec-sources.md`](../spec-sources.md)

## Context

Hancom does not publish a Cell or Show binary-format specification suitable for
implementing a proprietary parser. Fifteen government Cell/Show attachments
provide a narrower fact: one Cell 12.0300 sample is inside the supported OOXML
spreadsheet subset, and eight Show 12.0000 samples are inside the supported
OOXML presentation subset. A second Cell and five more Show attachments carry
the same application-profile versions but cross the VBA, OLE, external HTTP,
video, or Microsoft media boundaries. This does not establish the
representation used by older releases, `.nxl`, CFB containers, or every v12
build.

The Cell publication also provides a separately exported XLSX. Its digest,
size, entry set, and many payloads differ from the Cell attachment, so the pair
does not justify a universal rename or semantic-equivalence claim. It does
provide an independent read oracle: with the current OfficeCLI host, `view
text` is byte-for-byte equal at 40,401 characters, and `view stats` is exactly
equal at 7 sheets, 4,082 total cells, 2,611 empty cells, 0 formula/error cells,
2,625 numeric values, and 1,457 shared strings.

The supported Cell package declares a macro-enabled spreadsheet main content
type even though the publication's paired export uses an `.xlsx` filename. No
VBA part was observed in that package. The second Cell package contains
`xl/vbaProject.bin` and is rejected. The bridge permits the exact main type but
rejects VBA/XLM macro parts, ActiveX, OLE/embedded packages, external
data/media, and unapproved relationship/action classes outside the allowlists.
Some supported Show samples contain HTTPS hyperlink relationships; those exact
external links are preserved, not followed or sanitized by the bridge. HTTP
hyperlinks, OLE embeddings, and video/media relationships seen in the negative
samples remain unsupported. Consumers must not interpret the sibling extension
as a general sanitization guarantee.

## Decision

1. Ship two separate dump-reader executables:
   `officecli-hancom-cell` declares exactly `.cell → xlsx`, and
   `officecli-hancom-show` declares exactly `.show → pptx`. `.nxl` is not
   advertised without a real sample and generation discriminator.
2. Accept only the two observed application profiles:

   - Cell: extended properties `Application=Cell`, `AppVersion=12.0300`,
     spreadsheet `xl/workbook.xml`, one direct `fileVersion` child with
     `appName=HCell`, and one direct `calcPr` child carrying
     `{http://schemas.haansoft.com/office/spreadsheet/8.0}hclCalcId`;
   - Show: extended properties `Application=Show`, `AppVersion=12.0000`, and
     presentation `ppt/presentation.xml`.

   A syntactically similar but unobserved `12.*` build is unsupported until a
   provenance-recorded sample extends this allowlist. These properties and
   main-part markers classify the supported subset; any writer can forge them,
   so they are not producer authentication or provenance proof.
3. A recognized package is copied byte-for-byte to an adjacent `.xlsx` or
   `.pptx` sibling. The source is opened read-only without following its final
   symlink/reparse component. On Windows, that retained handle permits only
   read sharing: write/delete sharing is denied, so the source pathname cannot
   be renamed or rebound while pathname-based ADS and DACL checks run. A
   regression test exercises the blocked rename and the release after handle
   close. One private candidate is copied and hashed, and
   an anonymous derivative of those exact candidate bytes is validated through
   the retained candidate identity. The source hash, length, and modification
   time are rechecked before commit. The
   candidate is flushed, synchronized, and published by a same-directory
   no-clobber operation; a different existing sibling, a reparse point, an
   alias, or a multi-link target is rejected rather than replaced. Fresh
   siblings copy the source modification time and an enumerated metadata policy:
   Windows read-only state, `Zone.Identifier`, and canonical DACL access rules plus
   protected state; Linux mode and the complete bounded xattr set visible and
   readable from the retained descriptor under the plugin credentials; macOS mode,
   that same process-visible xattr set (including quarantine when exposed there),
   and its separate extended ACL. Darwin's `ENOENT` result for a regular file with
   no extended ACL is normalized to the empty ACL; every other enumeration, read,
   application, or verification failure fails closed. EFS-encrypted Windows sources
   and alternate data streams
   other than the primary stream and `Zone.Identifier` are unsupported. A cached
   sibling is reused only when its primary/default stream, modification time, and
   those enumerated attributes match. This is not complete file-object or security
   identity: Windows owner/SACL/MIC, Unix UID/GID, process-invisible attributes,
   creation time, hard-link identity, allocation/compression layout, and unlisted
   filesystem policy are outside the guarantee.
4. Before copying, validate the entire ZIP:

   - at most 512 MiB source, 4,096 entries, 64 MiB per expanded entry,
     256 MiB cumulative expanded bytes, 16 MiB per XML part, 1,000:1 expansion,
     XML depth 256, 1,000,000 XML events, 1,024 attributes, 256 namespace
     declarations, 1,024-byte names, and 1 MiB attribute values per element;
   - canonical ASCII entry names, with percent/fragment/query syntax and
     traversal/backslash/control/drive syntax rejected, no case-colliding names,
     no symlink/special entries, no encryption, only
     stored/deflate compression, exact local/central names, flags, methods,
     CRCs and sizes, non-overlapping contiguous local regions, only the observed
     `0x5455` extra-field shapes with no duplicate ID, and an actual expanded-byte
     count equal to the declaration;
   - one well-formed root per XML/rels part, only a first-position XML 1.0
     declaration with optional UTF-8 encoding and `yes|no` standalone value,
     valid UTF-8/XML 1.0 characters, resolved namespaces, unique expanded
     attribute names, and no DTD or other processing instruction;
   - exact family relationship/content-type allowlists and metadata attribute
     sets (so `xml:base` is rejected), unique relationship IDs, case-folded
     canonical part identity, existing normalized internal targets, duplicate
     `Default`/`Override` rejection, one effective content type per part, exactly
     one root `officeDocument` and extended-properties relationship, at most one
     root core-properties and thumbnail relationship, and complete reachability
     of every non-directory part from `_rels/.rels`. Show alone permits the
     exact `gif → image/gif` and `wav → audio/wav` default mappings and bounded
     whitespace-free `https://` external hyperlinks. A GIF override is never
     accepted. Every GIF part must have a non-empty filename under `ppt/media/`,
     use the lowercase `.gif` suffix, and be the target of an internal image
     relationship from `ppt/slides/slideN.xml`; that slide
     must reference the relationship through DrawingML `a:blip r:embed`. The
     GIF part may not own a relationship part and must begin with `GIF87a` or
     `GIF89a`. This
     six-byte check identifies the declared opaque payload; it is not a full
     GIF parser, polyglot defense, decoder-safety guarantee, or sanitization.
     Media payloads are copied, not decoded. GIF overrides, mismatched GIF
     extension/content-type pairs, and all other external relationships are
     unsupported. The allowlists reject the active classes enumerated above but
     do not certify arbitrary formulas, defined names, or presentation actions
     as inert and do not replace consumer security controls;
   - exact root relationships, main content type, application properties,
     family root, Cell markers, and sheet/slide collection relationship IDs.
5. Three public Show files contain a malformed extended-timestamp field that
   zip-rs rejects. Validation may neutralize that field only in an anonymous
   scratch copy, only for a Show-profile candidate, and only for tag `0x5455`
   whose value is exactly flags `0b10`, length 13, and the same four-byte
   timestamp repeated three times. The source and sibling retain the original
   bytes. Other malformed extra fields remain corrupt input.
6. CFB and unknown containers, opposite OOXML families, unobserved producer
   versions, ZIP64/multidisk packages, and legacy generations return
   `unsupported_feature` (exit 3). A claimed recognized package that is
   malformed or violates a resource/security invariant returns corrupt input
   (exit 2). No partial sibling is published.
7. Direct-native mode requires both exact manifest capabilities
   `direct-native` and `byte-preserving` and is exclusive. The host invokes it
   on every open, rejects a different pre-existing sibling before launch, and
   accepts only exit 0, exactly zero raw stdout bytes, and a non-reparse sibling
   byte-identical to the current source. BOM-only, whitespace, JSONL, missing
   output, nonzero-exit output, or a changed length/mtime/hash snapshot for an
   existing sibling is a contract failure. On failure, the host never deletes a
   sibling path because a post-validation replacement race would make ownership
   ambiguous. Plugins clean private candidates before publication; any published
   output is retained. JSONL plugins may not mutate a sibling and never
   overwrite an unrelated existing native file. These two capability strings
   are a fork-local downstream v1 exception, not an accepted upstream protocol;
   every upstream sync must re-audit them against the linked post-v1 proposal.
8. Cell/Show are read-only source formats. Users may edit the generated native
   sibling, but no command writes those edits back to `.cell` or `.show`.
   Do not claim support for all Cell/Show files, lossless conversion, native
   Hancom round-trip, macro removal, or equivalence with a separately exported
   OOXML document.

## Installation and release boundary

The installer owns six active slots and two retired compatibility slots as one
best-effort rollback domain. Unix installs four physical role/family binaries
and uses relative links only for HML and OWPML. Windows installs six verified
copies. Every physical binary is preflighted; name, protocol, exact singleton
kind, exact extension set, and exact target presence/value must match its slot.
All roles must report one semantic version before staging begins. Install and
uninstall mutations are serialized per plugin root with an atomic Unix lock
directory and per login session plus plugin root with a named Windows mutex, so
transactions within those scopes cannot interleave their backups and commits. A
force-killed Unix transaction can leave the empty
lock directory behind; recovery requires confirming no installer is active and
removing only that exact lock directory.

As fixed by ADR-0014, this is a serialized, transactional-best-effort update, not
a filesystem-atomic or crash-atomic generation switch. Success is returned only
after postflight verification of all six active paths as one suite version and
absence of both retired paths. Failure or forced termination can leave a partial
layout or recovery backups; the suite must not be used until the same installer
has been rerun successfully. Conflict-safe rollback does not overwrite a path
changed by another actor and does not guarantee recovery after termination.
Cooperative installers are serialized within those scopes; concurrent hosts have
no immutable-generation observation guarantee.

Windows uninstall retries a locked plugin file at most 20 times with a 250 ms
delay. Every attempt revalidates the managed directory and target against
reparse points. A permanent error still fails after the bounded five-second
window; the retry is not permission bypass.

## Evidence and limits

- The fifteen public Cell/Show samples, paired XLSX, and exact digests are in
  `docs/spec-sources.md` and remain outside the repository.
- On 2026-08-31, after the retained-handle sharing change, the current Windows
  release host and release plugins rechecked the four exact digests. Every
  `view` produced a fresh byte-identical sibling, preserved source and sibling
  mtime, and left exactly the carrier and its sibling in the case directory.
  A second direct release-plugin invocation returned zero with zero-byte stdout.
  Host observations took 1,043–1,388 ms. All
  39 Cell parts and all 95/82/83 Show parts were inside the root relationship
  closure. Each denominator counts `[Content_Types].xml`, relationship parts,
  and ordinary internal parts; directory entries and external URIs are excluded.
  The stricter closure/content-type allowlists accepted all four.
- An additional 2026-08-31 sweep covered one Cell and ten Show attachments.
  Release plugins accepted five Show packages byte-for-byte and rejected the
  other Cell plus five Show packages at the intended VBA, OLE, external HTTP,
  video, or Microsoft media boundary. Two accepted packages required only the
  exact Show `gif → image/gif` mapping; no Cell or global MIME allowance was
  added. The two GIF-bearing packages contained three `GIF89a` parts, all under
  `ppt/media`, internally embedded from slides, and without outgoing
  relationship parts. Every source retained its hash and modification time.
  Successful runs created only a fresh byte-identical sibling; rejected runs
  created no target.
  The current Windows host, published with the locally available .NET 10.0.400
  SDK, also read text from all five accepted packages and preserved the same
  source, sibling, and two-file case-directory invariants. It rejected the
  other six without creating a target or changing the one-file case directory.
  The repository's pinned .NET 10.0.302 and three operating systems remain CI
  gates.
- The Cell source and independently published XLSX produced exactly equal
  OfficeCLI text and statistics, while their differing package topology proved
  they are not the same artifact.
- Three release-binary observations per family over synthetic 48 MiB payloads
  (50,335,305-byte Cell and 50,340,266-byte Show carriers) took 413.1–484.2 ms
  for Cell and 439.4–486.7 ms for Show. The greatest working set observed by
  1 ms polling was 5,902,336 bytes. Every
  run exited successfully with zero-byte stdout/stderr, left source hash and mtime
  unchanged, and produced a byte-identical sibling. These are likewise
  environment-specific observations rather than formal upper bounds.
- Synthetic fixtures exercise all supported operating systems in CI without
  redistributing government attachments. They prove discovery and the protocol
  path, not additional producer generations.
- [CI run `33349242319`](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33349242319)
  passed those content-bearing Cell and Show fixtures against the current host
  and release plugins on Linux, Windows, and macOS at implementing HEAD
  `5df4d34e`. Each platform verified direct-native view, unchanged source hash
  and mtime, and a fresh byte-identical sibling; the same run also passed the
  complete carrier test suite, release build, and Rust 1.88 MSRV checks.
- Native Hancom Office open/save was not available as an oracle. Any future
  write-back or claim of native round-trip requires a recorded product build,
  recovery-free open, render inspection, save, and reopen.

## Consequences

Modern evidence-backed files gain useful cross-platform reads without guessing
a proprietary layout. The cost is an intentionally narrow allowlist and a
native sibling whose package is preserved rather than normalized. Legacy and
different-generation parser work remains blocked on provenance-recorded samples
or a public specification; it is not silently counted as completed by this ADR.

This ADR is re-indexed into codebase-memory with the implementing commit.
