# ADR-0006: Boundaries for the unified Hancom plugin suite

- Status: Accepted
- Decision date: 2026-08-28
- Scope: `plugins/hwpx` and the planned `plugins/hancom` workspace
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

The current plugin reads HWPX directly and optionally converts binary HWP to
HWPX through RHWP. OfficeCLI discovers plugins per `(kind, extension)`, a
dump-reader manifest has one native `target`, and dump-reader resolution takes
precedence over format-handler resolution for the same extension. HWPX writing
therefore cannot be enabled by merely adding a second kind to the current
manifest.

Hancom publishes HWP/HWPML specifications, but no public `.cell` or `.show`
structure specification has been verified. No representative `.cell` or
`.show` sample is available in the repository. Guessing either format would
violate the project's explicit-failure policy and could silently corrupt data.

Hancom's official HWP format page also requires a specific attribution on the
available product UI, manual, help, and source surfaces. The official source
URLs and byte digests are pinned in [`../spec-sources.md`](../spec-sources.md);
the PDF files are not redistributed.

## Decision

1. Finish P0 before beginning the workspace move in P1. In particular, retain
   the current layout until a new GitHub-hosted Linux and Windows run proves the
   native HWP discovery and RHWP-backed `officecli view` path.
2. Keep HWP/HWPX read-only as a dump-reader until a validated OWPML writer and
   durable format-handler implementation are ready. The future promotion must
   remove the `.hwpx`/`.owpml` dump-reader declarations in the same change that
   enables their format-handler declarations.
3. Implement the unified suite as three target-specific binaries sharing one
   core: Hancom HWP (`docx`), Hancom Cell (`xlsx`), and Hancom Show (`pptx`). Do
   not infer the target from `argv[0]` or the installation directory.
4. For `.cell` and `.show`, identify the container from real samples before
   parsing. Until a parser or explicitly configured converter exists, return an
   explicit unsupported-feature result; never emit guessed document content.
5. External converters must reuse the RHWP bridge security boundary: no shell,
   private staging, bounded resources and time, process-tree cleanup, output
   re-identification, and an immutable source file.
6. Do not add a JVM runtime dependency merely to accelerate HWPX writing.
   Apache-2.0 Java libraries may be consulted as reference implementations; a
   runtime-sidecar decision requires a separate ADR and user approval.

## Consequences

- P1 cannot hide or invalidate the still-open cross-platform HWP gate by moving
  files first; the last known-good layout remains available for diagnosis.
- The three binaries duplicate only their thin protocol entry points while
  sharing parsing, budgets, diagnostics, and emitter code.
- `.cell`/`.show` parser work remains blocked on representative samples. A
  converter boundary may provide value earlier, but its executable contract
  still needs a separate test-first design.
- HWPX editing remains deferred until official validation and round-trip
  durability evidence exist.

## Evidence and verification

- Official Hancom terms and downloads:
  <https://www.hancom.com/support/downloadCenter/hwpOwpml>
- Pinned specification provenance: [`../spec-sources.md`](../spec-sources.md)
- Curated repository research:
  [`../../.agents/brain/research/hancom-unified-20260828.md`](../../.agents/brain/research/hancom-unified-20260828.md)
- Current host behavior:
  `DocumentHandlerFactory.TryOpenViaPlugin` and
  `PluginManifestExtensions.ResolveTargetFormat`
- Local P0 evidence is recorded in
  [`../../plugins/hwpx/notes.md`](../../plugins/hwpx/notes.md). Commit `e77fb77c`
  satisfied T0-3 in [HWPX plugin run 33157787880](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33157787880),
  and [action pin run 33157787944](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33157787944)
  passed independently. P1 is therefore unblocked.

This ADR is indexed into codebase-memory after changes to these decisions so
future graph searches can recover the boundary and its source files.
