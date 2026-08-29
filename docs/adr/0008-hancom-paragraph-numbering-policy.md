# ADR-0008: Preserve Hancom paragraph numbering with bounded layout profiles

- Status: Accepted
- Decision date: 2026-08-29
- Scope: HWPX/OWPML → DOCX conversion in `plugins/hancom`
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

OWPML paragraph shapes refer to `hh:numbering` and `hh:bullet` definitions
through `hh:heading`. Section outline paragraphs instead resolve the active
numbering definition through `hp:secPr/@outlineShapeIDRef`. The two header
tables use separate source ID namespaces, so a NUMBER and BULLET with the same
numeric ID are not the same definition.

The marker template is also structural rather than display text. The official
format defines `^n`, `^N`, and `^1` through `^9` tokens. Flattening the marker
into paragraph text would freeze counters and lose multilevel behavior. DOCX
represents the same concept with an `abstractNum`, a live `num` instance, and
paragraph `numId`/`ilvl` properties.

Some OWPML layout controls have no one-to-one OOXML field. In particular,
`useInstWidth`, `autoIndent`, `widthAdjust`, `textOffsetType`, and `textOffset`
cannot safely be converted into a guessed fixed indent. The converter therefore
needs an evidence-bounded compatibility subset and must reject active values
outside it before emitting JSONL.

## Decision

1. Parse NUMBER and BULLET definitions into typed source namespaces and assign
   one collision-free target ID space in header definition order. Resolve
   OUTLINE against the section's `outlineShapeIDRef`. Missing, incomplete,
   duplicate, or ambiguous active definitions are corrupt input.
2. Materialize only definitions and levels reached by authored paragraphs.
   Unused header definitions are catalog entries, commonly built-in defaults;
   unsupported dormant entries neither affect the result nor produce a warning.
   This includes HWP's preserved one-based level 10: DOCX has only nine levels,
   so it is ignored when no active paragraph reaches it and rejected when active.
   The same active source definition produces one `abstractNum` and one `num`,
   shared by NUMBER and OUTLINE paragraphs across intervening plain paragraphs.
3. Emit document-level numbering resources before body content. Use child
   `level` items rather than dotted `levelN.*` properties because the actual
   OfficeCLI lint contract accepts the former. Suppress per-instance start
   overrides with `continue=true`, matching Hancom's native one-instance export.
4. Interpret marker tokens exactly:

   | OWPML token | DOCX marker text |
   |---|---|
   | `^n` | the current path, such as `%1.%2` |
   | `^N` | the current path plus `.` |
   | `^1` … `^9` | the referenced level placeholder, when already available |

   Reject forward references, incomplete or unknown `^` tokens, and literal
   `%` followed by a digit because DOCX would reinterpret it as a placeholder.
   The level's `paraHead/@start` is authoritative; the enclosing
   `numbering/@start` is not substituted for it.
5. Use only verified format mappings:

   | OWPML | DOCX |
   |---|---|
   | `DIGIT` | `decimal` |
   | `CIRCLED_DIGIT` | `decimalEnclosedCircle` |
   | `ROMAN_CAPITAL` / `ROMAN_SMALL` | `upperRoman` / `lowerRoman` |
   | `LATIN_CAPITAL` / `LATIN_SMALL` | `upperLetter` / `lowerLetter` |
   | `HANGUL_SYLLABLE` / `HANGUL_JAMO` | `ganada` / `chosung` |

   Preserve marker font, size, color, bold, and italic. Reject active image or
   checkable bullets and marker formatting the host vocabulary cannot express.
   Preserve PUA bullet code points and include them in the existing G6
   diagnostic instead of guessing a Unicode replacement.
6. Keep numbering level geometry neutral (`indent=0`, `hanging=0`) because the
   source paragraph margins are already carried by `ParaStyle`. Accept only
   these observed profiles:

   | Kind | `autoIndent` | `textOffsetType` | `textOffset` |
   |---|---:|---|---|
   | NUMBER | 1 | `PERCENT` | 50 |
   | BULLET | 1 | `PERCENT` | 10, 15, or 50 |
   | either | 0 | `PERCENT` or `HWPUNIT` | 0 |

   Validate `useInstWidth` as a boolean but do not invent geometry from it.
   Require integer fields, zero `widthAdjust`, a DOCX-range start value, and a
   left/center/right marker alignment. Any other active profile returns exit 3
   before stdout.

## Consequences

- Numbered, bulleted, and section-outline paragraphs remain live DOCX lists;
  counters are not copied into text.
- NUMBER and BULLET IDs cannot collide, while NUMBER and OUTLINE can correctly
  share one live counter when they use the same source definition.
- Active image/checkable markers, unsupported formats, and unverified geometry
  fail explicitly. Dormant built-in definitions do not block otherwise exact
  conversion.
- PUA bullets remain source-faithful but can still depend on a Hancom-specific
  font outside Hancom Office. The existing diagnostic makes that portability
  risk visible.

## Evidence and verification

- Official source provenance and pinned digests:
  [`../spec-sources.md`](../spec-sources.md). The HWPML r1.2 numbering section
  defines the token and layout fields used above.
- Public `hwpxlib` `ParaHead.hwpx` exercises NUMBER, BULLET, OUTLINE, multilevel
  paths, Korean formats, and a PUA marker. Hancom's native OOXML export and the
  plugin produce the same visible counter progression.
- A second Hancom native export contains automatic BULLET offsets 10, 15, and
  50 in one document. All three native definitions use a space suffix and no
  explicit level indent or hanging value, which bounds the compatibility row
  above without extending it to NUMBER or unobserved offsets.
- Actual OfficeCLI replay of `ParaHead.hwpx` emits 47 batch items with zero
  unknown properties. An offset-15/10 corpus document preserves dynamic
  `numId`/`numLevel` values and produces a DOCX with zero validation errors.
- The pinned RHWP 0.8.4 `english.hwp` fixture preserves a dormant HWP level 10.
  Its active level 1 converts successfully through the installed OfficeCLI
  plugin and the generated DOCX validates with zero errors.
- The 49-file public corpus finishes 47 conversions with zero unknown
  properties. The remaining two failures are the pre-existing unsupported
  mid-section header/footer activation cases, not numbering failures.
- Local acceptance gates: 502 Rust tests, Clippy with `-D warnings`, release
  build, formatting and diff checks, and a .NET host build with zero errors.
- Remote acceptance gates:
  [HWPX plugin run 33243420505](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33243420505)
  passed Linux/Windows native HWP/HWPX replay, Rust 1.88 MSRV, and host
  contracts; [action pin run 33243420494](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33243420494)
  also passed.
- Implementation commits: `025418bc` and RHWP compatibility fix `2289ca60`.

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
