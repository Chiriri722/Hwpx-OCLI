# ADR-0010: Lower only the evidence-backed Hancom shape and textbox subset

- Status: Accepted
- Decision date: 2026-08-29
- Scope: HWPX/OWPML → DOCX conversion in `plugins/hancom`
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

OWPML stores drawing geometry, placement, wrapping, decoration, and optional
text in several related `hp:*` families. Previously the parser skipped those
subtrees. That avoided invalid DOCX, but it also made a document appear to
convert successfully after active drawings had disappeared.

The official HWPML r1.2 grammar defines when `POSITION` fields are active and
defines rectangle `Ratio` as a 0–50 corner-curvature scale. It does not prove
that every HWP geometry has a lossless OfficeCLI/DOCX equivalent. Hancom 2020
native DOCX oracles show ordinary rectangles and text boxes as rectangular
Word drawings, whole ellipses as oval drawings, and line objects as endpoint-
based lines. The line oracle also retains a compound `SLIM_THICK` stroke that
the typed OfficeCLI shape surface cannot express exactly.

The 281-file public corpus reinforces the need for a closed boundary. Across
275 readable packages it contains 1,665 rectangles, 94 ellipses, 227 lines,
205 polygons, 337 curves, 20 connector lines, 909 containers, 35 OLE objects,
and 30 charts. It contains no arc, text-art, or video sample, so absence cannot
serve as compatibility evidence.

## Decision

1. Materialize only `hp:rect` and whole `hp:ellipse` profiles that can be
   represented exactly by the typed OfficeCLI `shape`/`textbox` vocabulary.
   Every other active shape family remains an explicit `unsupported_feature`;
   charts are handled separately by T2-7.
2. Both supported geometries require absolute positive size, no lock,
   protection, group membership, hyperlink, caption, rotation, or flip, an
   opaque solid/no fill, `NONE` shadow, and a `SOLID` or `NONE` outline.
   Rectangle corner ratios are limited to 0–50. Ellipses require
   `intervalDirty=0`, `hasArcPr=0`, and `arcType=NORMAL`; ellipse text is not
   accepted without a native round-trip oracle.
3. Convert HWPUNIT to EMU exactly by multiplying by 127. Reject values outside
   the corresponding DOCX extent, position, wrap-distance, text-inset, or line-
   width domain before JSONL emission. Do not clamp, saturate, or substitute a
   host default.
4. Map `treatAsChar=1` to `wp:inline`. Map only verified floating
   `PAPER/PAPER`, vertical `TOP` placement to `wp:anchor`, with either a left X
   offset or semantic page-center alignment. Preserve Y, z-order,
   `allowOverlap`, outer distances, and the verified square/top-and-bottom/
   behind/in-front wrapping modes and flow side. Paragraph-relative flow and
   unverified alignments fail closed.
5. For rectangle `drawText`, preserve structural paragraphs, runs, tables,
   images, notes, numbering, and named styles instead of flattening text.
   Support `HORIZONTAL` and native-verified `VERTICALALL`, `BREAK` wrapping,
   top/center/bottom vertical alignment, and exact text margins. Reject nested
   drawings because the host and OOXML textbox surface do not permit them.
6. Preserve non-empty `hp:shapeComment` as `wp:docPr/@descr`. Corpus and public
   Hancom tooling identify it as the object/shape description, not a disposable
   rendering cache. Keep the separate drawing name in `@name`.
7. Extend the host surface symmetrically for inline/floating placement, wrap
   side and distances, `behindDoc`, `allowOverlap`, relative height, page
   alignment, preset adjustment, no-fill, and object description. A legacy
   `wrap=behind` value implies `behindDoc=true` only when the explicit property
   is absent. Textbox dump remains typed; generic shapes remain raw carriers on
   dump so arbitrary existing drawing XML is not normalized destructively.
8. Textbox follow-up paths count descendant cell drawings in their enclosing
   body/header/footer story ordinal while retaining cell-local ordinals. A late
   unsupported shape aborts before any earlier BatchItem reaches stdout.

## Consequences

- Verified rectangles, rounded rectangles, text boxes, and whole ellipses now
  survive with editable DOCX semantics and their authored descriptions.
- Active lines, custom paths, containers, OLE, and other unproved geometries no
  longer disappear silently. Documents using them now fail explicitly, so the
  public corpus moves from the former permissive 226/23/32 baseline to 173
  success, 22 corrupt, and 86 intentionally unsupported.
- The typed host drawing vocabulary is more complete for non-Hancom callers as
  well, while explicit properties retain precedence over legacy aliases.
- Supporting another geometry requires both a source profile and a native DOCX
  oracle that proves geometry, placement, wrapping, and decoration together.

## Evidence and verification

- Official HWPML r1.2 pages 95 and 103 establish the active placement fields
  and the rectangle corner-ratio semantics.
- Hancom 2020 native conversions establish floating page offsets, inline
  `treatAsChar`, page-center alignment despite an inactive unsigned X sentinel,
  opaque alpha zero, 1 mm text margins, vertical-all text, whole-ellipse
  geometry/no-fill, and line endpoint/compound-stroke behavior.
- A full 370-item OfficeCLI replay of the centered textbox document validates
  with zero OpenXML errors and retains semantic page centering and its
  description. The actual plugin ellipse command was separately replayed and
  validates its ellipse preset, exact page offsets, no-fill, and overlap flag;
  the source document's unrelated WMF MIME issue remains outside this decision.
- `plugins lint` reports zero unknown properties for representative 145- and
  370-item dumps. The complete public corpus result is 173 success, 22 corrupt,
  86 unsupported, and zero other failures.
- Local acceptance gates pass 526 Rust tests, 42 .NET host contracts, locked
  Clippy with `-D warnings`, locked release binary build, schema/help loading,
  formatting and diff checks, and the exact SDK 10.0.302 solution build with
  zero errors (the pre-existing nullable warning remains).
- Implementation commit: `b379b4d3`.

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
