# ADR-0007: Lower Hancom section stories and note policy without silent loss

- Status: Accepted
- Decision date: 2026-08-29
- Scope: HWPX/OWPML → DOCX conversion in `plugins/hancom`
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

OWPML stores body content, header/footer controls, first-page visibility, and
footnote/endnote policy in a section. DOCX also has sections, but the two
formats do not have identical capabilities:

- OWPML header/footer controls use `BOTH`, `ODD`, and `EVEN` page scopes and
  can occur on a timeline inside a section. DOCX has one default, even, and
  first-page relationship per section, while even/odd activation is a
  document-wide setting.
- OWPML `PAGE` and `TOTAL_PAGE` counters are dynamic content. Copying their
  displayed digits would make them stale as soon as pagination changed.
- Number format, restart, start, position, prefix, suffix, and superscript
  note policy have DOCX equivalents or can be represented by dynamic note
  references.
- OWPML `noteLine` and `noteSpacing` are section-scoped future-authoring and
  rendering policy. DOCX has no equivalent section property. Treating values
  that happen to look like defaults as harmless would be corpus-dependent and
  would silently lose authored policy.

The converter follows the repository's fail-closed rule: an exact projection
or an explicit, bounded loss disclosure is required before any JSONL reaches
stdout.

## Decision

1. Preserve the `content.hpf` section spine in the shared model and emit one
   DOCX section carrier per source section.
2. Project header/footer page scopes as follows:

   | OWPML scope | DOCX relationship |
   |---|---|
   | `BOTH` | identical `default` and, when parity is active anywhere, `even` parts |
   | `ODD` | `default` part, with an empty `even` part when absent |
   | `EVEN` | `even` part, with an empty `default` part when absent |

   Enable parity globally when any section uses `ODD` or `EVEN`. Materialize
   missing slots as empty parts in every affected section so Word cannot
   inherit a previous section's story. Use `titlePage` and explicit first-page
   parts for `hideFirstHeader`/`hideFirstFooter`.
3. Accept one unambiguous `BOTH`, `ODD`, or `EVEN` story, or one `ODD`+`EVEN`
   pair per kind and section. Reject repeated/overlapping definitions and any
   story activated after authored body content; widening such a timeline to
   the whole DOCX section would be a semantic change.
4. Preserve paragraph, run, table, image, and field order inside stories. Use
   the host-created first paragraph as the first source paragraph; remove that
   seed only when the source story starts with a table.
5. Lower `PAGE` and `TOTAL_PAGE` to dynamic DOCX `PAGE` and `NUMPAGES` fields.
   Reject other auto-number kinds until their numbering structures are
   implemented. A footnote/endnote marker is consumed only inside a matching
   note container; it may never disappear from ordinary body content.
6. Lower note number format, restart, start, placement, prefix, suffix, and
   superscript through section properties and decorated dynamic references.
   Reject custom `userChar` marks until automatic-number semantics can be kept.
7. Parse every `noteLine` and `noteSpacing` value into a closed typed model.
   Validate cardinality, numbers, line type, width, and RGB syntax. Apply this
   value-independent policy:

   - If the corresponding section contains an active note, return
     `unsupported_feature` before stdout, regardless of whether the authored
     values resemble a common default.
   - If no corresponding note is active, conversion may proceed only after a
     compact structured warning containing the exact source values is written
     and flushed to stderr. This warning is mandatory under `--quiet` and
     `--log-file`; a failed or oversized warning channel aborts conversion.
   - The OfficeCLI host forwards only bounded, valid one-line JSON warning
     envelopes from a successful dump-reader. Ordinary plugin stderr is not
     promoted into host warnings.

## Consequences

- Common headers, footers, page counters, first-page hiding, and parity are
  dynamic in the resulting DOCX instead of flattened text.
- Mid-section activation and ambiguous overlap fail explicitly rather than
  changing which pages display a story.
- Note numbering and marker decoration survive when the source note layout is
  representable.
- A typical Hancom document authors `noteLine`/`noteSpacing` even when it has
  no notes. Such a document converts with a mandatory warning. If it contains
  an actual corresponding note, conversion currently stops with exit 3. This
  is deliberate until a generally equivalent DOCX lowering is demonstrated.
- The warning contract is reusable by other dump-readers, but remains bounded
  to eight warnings and 12 KiB aggregate host output.

## Evidence and verification

- Official source provenance and pinned digests:
  [`../spec-sources.md`](../spec-sources.md)
- Hancom's Apache-2.0 OWPML model confirms the closed note line/width/type and
  spacing fields used by the parser.
- A Hancom-created header/footer fixture was replayed through the actual host:
  7 batch items and zero unknown properties.
- A public Korean exam fixture containing a styled footer `PAGE` counter was
  replayed as 75 batch items with zero unknown properties.
- Hancom 2020-created footnote and endnote files both stop with exit 3 and zero
  stdout bytes when authored `noteLine`/`noteSpacing` would be lost.
- Local gates at acceptance: 486 Rust tests, 38 host contract tests, Rust 1.88
  Clippy with `-D warnings`, release build, and the fixed .NET SDK build.
- Implementation commit `393785ab` passed the
  [HWPX plugin workflow](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33241159576)
  and the
  [action-pin audit](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33241159629).

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
