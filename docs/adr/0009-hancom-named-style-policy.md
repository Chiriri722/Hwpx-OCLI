# ADR-0009: Preserve active Hancom named paragraph styles without inferred inheritance

- Status: Accepted
- Decision date: 2026-08-29
- Scope: HWPX/OWPML → DOCX conversion in `plugins/hancom`
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

OWPML paragraphs carry two independent references. `paraPrIDRef` selects the
paragraph's authored formatting, while `styleIDRef` selects one definition in
`hh:styles`. Treating either reference as a substitute for the other loses
information: real documents frequently apply a named style and then override
alignment, spacing, or run formatting directly.

DOCX also separates paragraph `pStyle` from direct `pPr`/`rPr`, but a `pStyle`
reference is useful only when its `w:style` definition exists. Earlier corpus
work deliberately omitted `styleIDRef` because the host did not synthesize
missing definitions and the five available documents used only dormant outline
styles. The expanded public corpus now contains active named and outline styles,
and Hancom 2020 can provide native DOCX conversion oracles.

Hancom outline styles have an additional dependency on the containing section.
Their `paraPr` identifies the outline level, while `secPr/@outlineShapeIDRef`
selects the numbering definition. A source value of `0` can denote Hancom's
implicit default outline even when no `hh:numbering id="0"` exists.

## Decision

1. Preserve `paraPrIDRef` as direct paragraph formatting and `styleIDRef` as a
   separate named-style reference. Emit both layers when both are present; do
   not collapse direct formatting into the style or discard it as redundant.
2. Materialize only styles reached from authored body, table, note,
   header, or footer paragraphs, plus the transitive non-self
   `nextStyleIDRef` closure. Dormant malformed definitions do not block an
   otherwise exact conversion. Once any style is active, require exactly one
   `hh:styles` container, a valid `itemCnt`, unique active IDs, and complete
   active dependencies.
3. Preserve the exact source style ID, including numeric IDs, and the primary
   `name`. Do not substitute `engName`, infer Word built-in IDs, or invent a
   `basedOn` chain. Active definitions must be paragraph styles with valid
   `paraPrIDRef` and `charPrIDRef` targets. Emit non-self `nextStyleIDRef` as
   `next`; the host supports a forward `next` reference, so source order can be
   retained.
4. Emit `customStyle=false`. HWPX does not encode the same built-in/custom
   distinction as Word, and Hancom's native export leaves the flag absent even
   for numeric IDs. Use a numeric source ID as `uiPriority` when it fits the
   host integer field. Validate `lockForm` as a source boolean but do not map it
   to DOCX `locked`: Hancom's native export does not do so.
5. Preserve the named style's supported paragraph and run formatting through
   the existing typed models. This includes alignment, indents, spacing, line
   spacing, font, size, color, shading, emphasis, and vertical alignment.
6. Resolve named-style numbering as follows:

   - NUMBER and BULLET use the definition selected by their style `paraPr`.
   - OUTLINE uses the style level and each consuming section's
     `outlineShapeIDRef`, even when the paragraph's direct `paraPr` says NONE.
   - If one global DOCX style would resolve to different outline definitions in
     different source sections, return `unsupported_feature` before stdout.
   - A missing ordinary numbering reference remains an error. Only missing
     outline source ID `0` selects the verified Hancom implicit profile:
     decimal levels starting at one with `%1.` repeated through the active
     depth.

7. Emit numbering resources first, named styles second, and every body/story
   consumer last. Validate the complete active graph before writing JSONL so a
   late missing style or numbering cannot leak partial output.

## Consequences

- DOCX paragraphs retain semantic style identity for navigation and TOC use
  while preserving authored direct overrides.
- Numeric Hancom style IDs and forward next-style links survive without a
  synthetic Word naming scheme.
- Active style corruption and section-dependent outline ambiguity fail
  explicitly. Dormant built-in templates remain harmless.
- `lockForm`, `langID`, and `engName` are not guessed into superficially similar
  Word fields. A future mapping requires a new native oracle and an explicit
  policy change.
- Hancom's implicit outline-zero profile is narrowly recognized only in the
  section-outline path; it cannot mask a dangling NUMBER or BULLET reference.

## Evidence and verification

- An expanded 281-file public HWPX corpus contains 256 parseable style tables
  and 993 active unique definitions. Active IDs are numeric and active styles
  have a primary `name`. The completed converter retains the established
  corpus baseline: 226 successes, 23 corrupt inputs, and 32 explicitly
  unsupported inputs.
- A Hancom 2020 native conversion of a Korean policy report has active outline
  styles 2 through 8. Its 51 consuming paragraphs use direct heading type NONE,
  while the generated DOCX styles carry `outlineLvl` 0 through 6 and live
  numbering; the paragraphs carry only `pStyle`.
- A separate native conversion has `outlineShapeIDRef="0"`, no explicit
  numbering definition, and active style 18 at outline level 4. Hancom emits a
  nine-level decimal outline whose level text repeats `%1.`, establishing the
  narrow implicit-zero profile.
- A public document with four active `lockForm="1"` styles was converted by
  Hancom 2020. None of the corresponding DOCX styles has `w:locked`.
- The actual OfficeCLI host preserves numeric style IDs and accepts a style
  with `next=0` before style 0 is added; the permanent host contract covers
  both properties.
- Representative dump/replay emits 224 batch items, reports zero unknown
  properties, applies all 224 items, and produces a DOCX with zero validation
  errors. `/styles/18` reads back its exact ID and name, `customStyle=false`,
  `outlineLvl=4`, and live `numId`/`ilvl` values.
- Local acceptance gates: 513 Rust tests, Clippy with `-D warnings`, release
  build, formatting and diff checks, and 40 .NET host contract tests.

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
