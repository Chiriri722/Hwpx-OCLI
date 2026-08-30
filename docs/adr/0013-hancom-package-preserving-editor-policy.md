# ADR-0013: Limit HWPX writing to a package-preserving closed edit subset

- Status: Accepted
- Decision date: 2026-08-30
- Scope: HWPX/OWPML editing and format-handler promotion in `plugins/hancom`
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

The existing Rust reader projects HWPX into a deliberately smaller semantic
model for DOCX conversion. That model does not represent every OWPML element,
attribute, namespace, child order, package part, relationship, or extension.
Serializing it back as a whole document would therefore destroy information even
when a requested edit touches only one paragraph.

The Java `neolord0/hwpxlib` writer at commit
`96ff157eb5973ba1bcf96c00c1b0993d61a718a0` is a useful ordering and typed-model
reference, and it preserves unparsed XML and attached binary parts. Adding its
JVM runtime is inconsistent with the single-Rust-binary boundary. The official
`hancom-io/hwpx-owpml-model` at commit
`1453388472c703a4b299a0834f425cdac16644b9` exposes open/save and package-part
write APIs, but is a legacy Windows/Visual Studio reference and is not a
conformance validator.

`hancom-io/dvc` at commit
`19a985ec047df629240cbcbe2cec17f19ad1a014` was previously described in the plan
as an official general validator. Its README and implementation instead require
a caller-supplied JSON policy and inspect a bounded set of semantic features such
as character/paragraph shapes, tables, numbering, styles, hyperlinks, and
macros. It does not establish ZIP, package-topology, XSD, or KS X 6101
conformance. Treating a DVC policy pass as general validity would overstate the
evidence.

## Decision

1. The product boundary is a **package-preserving HWPX editor with a closed,
   proven edit subset**, not a general HWPX serializer. The reader's semantic
   conversion model is never used to regenerate an entire source package.
2. Opening an editable document creates an immutable package snapshot containing
   entry order, canonical name, raw payload fingerprint, compression method, and
   relevant ZIP metadata. A mutation plan names every part it may change.
   Unchanged entries retain the same uncompressed payload bytes and order; the
   implementation uses raw ZIP entry copying when the library and source entry
   permit it.
3. Edits patch the smallest independently verified XML subtree. Unknown elements,
   attributes, namespaces, child order, and unrelated package parts remain
   untouched. The initial text subset addresses a direct `hp:p/hp:run/hp:t`
   child by paragraph XML ordinal, optional source-id precondition, and text
   ordinal, then replaces only the text element's inner byte range. If a
   requested mutation requires whole-part reconstruction,
   unresolved ID/reference allocation, unsupported active content, or an
   unproved topology change, it fails with `unsupported_feature` before writing.
4. Every candidate save passes the following named layers. Each layer reports
   only what it actually proves.

   - **G0 package safety and preservation**: ZIP/CRC integrity, resource budgets,
     no encryption, links, unsafe paths, duplicates, case-insensitive aliases, or
     preambles; first byte-exact `application/hwp+zip` `mimetype` entry stored
     without compression; unchanged-entry payload fingerprints match.
   - **G1 XML safety**: every XML-like part is UTF-8 without BOM, well formed,
     bounded in depth/size, and contains no DTD, processing instruction,
     undeclared prefix, or unknown entity reference. This is not XSD validation.
   - **G2 project package-topology profile v1**: required `version.xml`,
     `META-INF/manifest.xml`, `META-INF/container.xml`, `Contents/content.hpf`,
     header, and contiguous section parts exist; container rootfiles and HPF
     manifest/spine references resolve; IDs, section sets, and `secCnt` agree.
   - **G3 save verification**: reopen the completed temporary package through a
     fresh strict Rust package reader, verify the requested semantic delta,
     reject unexpected known-semantic changes, and compare every unchanged
     payload fingerprint. A no-op proves this through exact ordered snapshot
     equality; mutations additionally require an explicit scoped semantic
     expectation. The lossy DOCX conversion reader is used only when the edited
     subset is fully representable in that model, never as a prerequisite for
     preserving unrelated active HWPX objects.

5. Independent tools are compatibility or policy oracles, not substitutes for
   G0-G3. A pinned official OWPML-model open smoke and a version-recorded native
   Hancom open/render/save/reopen smoke are pre-release interoperability evidence.
   DVC may run only as a pinned Windows smoke for a named, hashed policy and the
   features that policy promises. None of these results is called standards
   conformance.
6. Claims such as “KS X 6101 conformant”, “OWPML schema-valid”, “official HWPX
   validator”, or “lossless round-trip” are prohibited until the exact normative
   revision, its XSD set, and the additional semantic requirements have been
   acquired and tested. An XSD pass, if added, is reported as validation against
   that named schema revision rather than as blanket KS conformance.
7. Save writes a sibling temporary file, flushes file contents and relevant
   directory metadata, runs G0-G3 before replacement, and atomically replaces the
   source only after every gate succeeds. Failure leaves the source untouched;
   backup retention is an explicit user policy rather than an accidental side
   effect of a failed save.
8. The `.hwpx`/`.owpml` manifest promotion remains atomic with removal of their
   dump-reader declarations. `.hwp` and `.hml` remain read-only dump-reader
   formats. No editable capability is advertised until the full protocol and
   durable save path pass their integration gates.

## Consequences

- A no-op save and a small supported edit can preserve package information that
  the current semantic model cannot interpret.
- Topology-changing operations such as adding/removing sections, media, fonts,
  charts, signatures, or encrypted parts remain unavailable until each reference
  and integrity rule has an independent fixture and oracle.
- Some permissively readable third-party HWPX files will not be editable. The
  ordinary reader keeps its compatibility fallbacks, while writer output follows
  the stricter profile and fails before source replacement.
- Mutating a package whose ZIP extra fields cannot be reproduced byte-for-byte
  is currently unsupported. No-op raw archive copies remain available because
  they preserve those entries without reconstruction.
- DVC remains useful for an explicitly promised institutional policy without
  blocking portable Rust development or lending a false standards claim.
- The format-handler can grow verb by verb, but unsupported verbs cannot return a
  successful no-op and `save` cannot acknowledge before durable validation.

## Evidence and verification

- The three reference repositories above were inspected at the exact commits
  listed here. DVC's policy input, implemented checker classes, VS2017/v141
  project, local native dependencies, and unpinned bootstrap script make it
  unsuitable as a portable mandatory general validator.
- Hancom 2020 `exam_kor.hwpx` and the inspected reference packages place
  `mimetype` first; the native package stores it uncompressed. The remaining
  entry order varies, so the profile fixes only the first-entry rule and preserves
  the source order thereafter.
- A whole-file one-off probe passed the 49-entry native Hancom 2020
  `exam_kor.hwpx` through G0-G2, including every binary CRC, XML part, container
  rootfile, HPF manifest/spine reference, three sections, and header `secCnt`.
- `owpml::conformance::validate_output_package` implements the initial G0-G2
  boundary. Fourteen tests cover canonical output, physical/central first-entry
  rules, byte-exact stored mimetype, required parts, portable paths, path aliases,
  symlinks, CRC, UTF-8/XML safety, canonical topology parentage, container
  rootfiles, HPF references, contiguous sections, spine equality, and header
  section count.
- `owpml::editor` captures ordered decompressed and compressed SHA-256 payload
  fingerprints plus relevant ZIP metadata, requires an explicit mutation plan,
  and reopens candidates for G3. Six regressions cover raw no-op equality,
  unplanned payload/recompression changes, required planned changes, preserved
  metadata on changed parts, and exact known-semantic delta acceptance/rejection.
  A retained 49-entry native Hancom
  package also passed a one-off raw-copy G0-G3 no-op probe without requiring its
  unrelated polygon to be representable by the DOCX conversion model.
- Raw-entry COW and surgical text mutation are implemented behind one verified
  candidate path. Eleven regressions cover exact replacement hashes, source
  TOCTOU detection, key-set closure, namespace and parent-chain checks, repeated
  Hancom paragraph ids, XML escaping and forbidden characters, non-plain target
  rejection, ZIP extra-field failure closure, and restoration of the source
  central-directory `version made by` bytes. A one-off edit of retained native
  `exam_kor.hwpx` changed one `hp:t` in `Contents/section0.xml`; all other 48
  entries retained their compressed/decompressed fingerprints and ZIP metadata,
  and the candidate passed G0-G3 while its unrelated polygon stayed opaque.
- The host lifecycle prerequisite is commit `0429890a`: canonical top-level
  `open`, lifecycle `save`, and fail-closed save capability enforcement pass 46
  host contract tests.
- Atomic durable replacement remains an explicit P3 implementation task; this
  ADR does not mark it complete.

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
