# ADR-0012: Preserve Hancom private-use characters until font identity is provable

- Status: Accepted
- Decision date: 2026-08-30
- Scope: HWPX/OWPML text conversion in `plugins/hancom`
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

Hancom documents can contain Unicode Private Use Area (PUA) code points whose
glyph meaning depends on a particular font. The converter already preserves
these characters and reports their count, but T2-8 asked whether a reliable
mapping could justify substitution with standard Unicode characters.

The public-domain KTUG Hanyang old-Hangul table provides 5,660 BMP PUA-to-Jamo
mappings. It is useful evidence for that named legacy font profile, but it is
not a universal Hancom PUA registry. Public renderer code also contains separate
supplementary symbol maps, including provisional entries.

Across the 281-file corpus, 30 files contain 25,759 PUA occurrences at 85 unique
code points. Only 110 occurrences are in the BMP old-Hangul range; 25,649 are
supplementary PUA characters. `U+F080F` alone accounts for most occurrences, and
codes such as `U+F0854`, `U+F0855`, and `U+F00DA` occur under several font
profiles. The current model collapses OWPML's seven script/font slots into one
`CharStyle.font`, preferring HANGUL and then LATIN, so it cannot reliably retain
the USER or SYMBOL font identity required to choose a font-specific mapping.

## Decision

1. Preserve every PUA code point unchanged through the Unicode text model.
   Continue reporting the total diagnostic count and never substitute based on
   surrounding text, apparent glyph shape, code-point range, or a generic table.
2. Do not apply the Hanyang old-Hangul table globally. Its provenance supports
   a particular font family, while the converter cannot yet prove that the
   character originated from that source slot and font.
3. Treat the Hancom 2020 native DOCX exporter as the compatibility oracle for
   the current read path. In the `exam_kor` sample it preserved all 44 BMP PUA
   and all 83 supplementary PUA occurrences, including the same counts for 25
   BMP code points and `U+F00DA`/`U+F0854`/`U+F0855`; it introduced no standard
   or extended Jamo substitution.
4. A future substitution profile requires all of the following: preservation of
   all seven source font slots in the model, a closed allowlist keyed by exact
   source font identity and code point, a controlled native/PDF glyph oracle for
   each mapping, explicit profile selection, and tests proving no change outside
   that allowlist. It must not silently replace the default preservation policy.

## Consequences

- Conversion remains information-preserving and matches Hancom's own DOCX
  export behavior, even when a non-Hancom font later renders a missing glyph.
- Some recipients may still see tofu boxes when the required Hancom font is not
  installed. The existing diagnostic makes that compatibility risk visible.
- A factual mapping table alone is insufficient: applying it without exact font
  identity could turn a valid font-specific symbol into the wrong Unicode text.
- T2-8 is complete as a policy decision; no code change is required because the
  existing behavior and regression tests already enforce it.

## Evidence and verification

- Corpus scan: 25,759 PUA occurrences, 85 unique code points, 30 files; 110 BMP
  old-Hangul-range occurrences and 25,649 supplementary PUA occurrences.
- Hancom 2020 native oracle: source HWPX and exported DOCX both contain 44 BMP
  and 83 supplementary PUA occurrences, with zero Jamo substitutions.
- Existing tests cover detection in text and nested tables and assert that PUA
  characters are reported but not altered.
- The public mapping source is retained as research evidence, not vendored into
  the runtime or represented as a universal registry.

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
