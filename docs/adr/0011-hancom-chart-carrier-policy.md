# ADR-0011: Preserve only self-contained Hancom charts through a verified raw carrier

- Status: Accepted
- Decision date: 2026-08-30
- Scope: HWPX/OWPML → DOCX chart conversion in `plugins/hancom`
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

OWPML places a chart frame in a section and stores the corresponding OOXML
`c:chartSpace` document under `Chart/*.xml`. Reconstructing the chart through
the typed OfficeCLI vocabulary would normalize details that are not represented
by that vocabulary. Silently skipping the frame would lose authored content.

The official chart-format r1.2 PDF was verified against the pinned SHA-256
`E014DB3E4B55BC57D93B3ABA0B186151B3487575E3A6397A2983715B43BEEEB1`.
The public OWPML model was inspected at commit
`1453388472c703a4b299a0834f425cdac16644b9`. The 281-file corpus contains 30
chart parts in 29 files: 28 ordinary self-contained charts and two captioned
charts in one report. Hancom 2020 exports an ordinary chart to DOCX by retaining
the chart XML nearly unchanged and adding a drawing relationship.

The same native DOCX also contains child-order combinations rejected by the
Open XML SDK schema validator. Renderer tolerance is not evidence that arbitrary
invalid XML should be accepted or rewritten generically.

## Decision

1. Preserve a chart only when its `Chart/*.xml` part is a standalone UTF-8
   `c:chartSpace` document with no relationships or external resources. Reject
   DTDs, processing instructions, unknown entities, foreign active namespaces,
   path traversal, missing parts, duplicate structural roots, and oversized
   content before emitting JSONL.
2. Accept only the native-verified OWPML frame profile: `numberingType=PICTURE`,
   `textWrap=SQUARE`, `textFlow=BOTH_SIDES`, unlocked, no drop-cap or caption,
   and floating `COLUMN/PARA TOP/LEFT` placement with `flowWithText=1`, the
   remaining placement flags false, and zero X/Y offsets. Convert HWPUNIT to EMU
   exactly and map `zOrder + 1` to DOCX `relativeHeight`. Preserve size and outer
   margins; reject values outside the corresponding DOCX numeric domains.
3. Add a generic raw chart carrier to OfficeCLI, but keep its default `strict`
   profile non-mutating: base64-decode strictly, require UTF-8 and the security
   boundary above, and accept only XML that already passes SDK validation.
4. The Hancom parser may explicitly select `hwpxChartOrderRepairV1`. This
   versioned profile repairs only the exact structured SDK validation-error
   fingerprints observed for `c:catAx`, `c:valAx`, and `c:view3D`. It does not
   repair `c:dateAx`, `c:serAx`, unknown or duplicate children, or content across
   an `extLst`/markup-compatibility boundary.
5. The repair obtains canonical child order from SDK particle metadata and moves
   the original XML nodes. It does not recreate, filter, or normalize nodes,
   attributes, text, prefixes, or values. Every candidate must have only the
   expected pre-validation errors and zero validation errors afterward; otherwise
   the whole operation fails atomically.
6. Add the raw chart part and its drawing as one atomic host operation. Never
   leave an orphan part, relationship, or drawing when validation or insertion
   fails. Generic callers must opt into a named repair profile; source-specific
   tolerance is not the default.
7. Apply a 16 MiB source-part limit, 512 chart-reference limit, 64 MiB emitted
   chart-XML limit, shared 256 MiB expanded-package budget, and XML depth limit
   256. A chart part is cached by canonical source path so repeated references do
   not multiply decompression or validation work.

## Consequences

- The 28 common corpus charts survive as editable DOCX charts without translating
  or normalizing their data, series, axes, layout, or formatting.
- Captioned/TOP_AND_BOTTOM frames, relationship-bearing charts, external data,
  embedded workbooks, and unverified placement profiles fail explicitly instead
  of producing a partial document.
- The OfficeCLI host gains a reusable raw chart surface with a strict default,
  while the only tolerant behavior remains source-selected, versioned, narrow,
  and post-validated.
- Supporting another profile requires an OWPML source example, a native DOCX
  oracle, an explicit security/relationship policy, and a regression fixture.

## Evidence and verification

- Full 29-file parser classification: 28 success, 0 corrupt, 1 intentionally
  unsupported, and 0 other failures. The unsupported file reports its
  `TOP_AND_BOTTOM` frame rather than dropping either captioned chart.
- All 28 supported charts complete dump → new DOCX → atomic batch → query →
  Open XML validation with zero final errors. Each package has exactly one
  document-to-chart relationship and no outbound chart relationship.
- Structural fingerprints match 28/28 after ignoring sibling order only;
  element/attribute/text content and parentage are unchanged.
- `plugins lint` reports zero unknown properties. Local gates pass 533 Rust
  tests, 44 .NET host contracts, locked Clippy with `-D warnings`, locked release
  build, formatting, diff checks, and the host solution build with zero errors
  apart from the pre-existing nullable warning.
- Implementation commit: `cd110588`.

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
