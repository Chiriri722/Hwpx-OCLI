# Research memory: unified Hancom format expansion

- Date: 2026-08-28
- Status: P0 complete; P1 workspace expansion unblocked
- Canonical plan: `specs/001-hancom-unified/task-plan.md`
- Architecture decision: `docs/adr/0006-hancom-unified-plugin-boundaries.md`

## Confirmed findings

- Hancom's current official HWP/OWPML page lists five HWP-family specification
  PDFs and requires the exact Korean attribution sentence on every available UI,
  manual, help, and source surface.
- All five current PDF URLs returned HTTP 200, `application/pdf`, a `%PDF-`
  signature, and stable byte counts on 2026-08-28. Their lowercase SHA-256
  digests are in `docs/spec-sources.md`; the PDFs were deleted after hashing.
- OfficeCLI already wires format-handler sessions, despite a stale source
  comment. Dump-readers are resolved first for an extension, and a dump-reader
  manifest exposes only one native target.
- The existing HWP plugin's Windows and Linux HWP smoke tests pass with
  OfficeCLI 1.0.145 and RHWP 0.8.4. GitHub Actions run `33157787880` proved the
  native HWP/HWPX and 35-contract host paths on both operating systems.
- Host discovery, probing, manifest identity, installer path safety, rollback,
  and external GitHub Action pinning have local contract coverage.

## Unknowns and blockers

- No representative `.cell` or `.show` sample is available, so their container
  signatures, format generations, and parser behavior are unknown.
- KS X 6101 full text is not locally available; purchasing it is relevant only
  when the HWPX writing phase needs evidence beyond the official DVC validator.
- Repository publication authorization was granted on 2026-08-28. Commit
  `e77fb77c` and successful runs `33157787880`/`33157787944` provide the remote
  P0 evidence.

## Durable decisions

- Finish P0, including the remote HWP gate, before the P1 workspace move.
- Use three target-specific binaries with a shared core.
- Keep `.cell`/`.show` parsing fail-closed until real samples establish the
  container and generation boundaries.
- Keep HWPX read-only until an official-validator-backed writer and durable
  format-handler exist.
- Do not redistribute Hancom PDF specifications; retain URL, revision, byte
  length, and digest provenance only.

## Next evidence

1. Move the existing plugin into the P1 Cargo workspace without changing behavior.
2. Obtain representative `.cell` and `.show` samples before T4-1/T5-1.
3. Revisit JVM and KS X 6101 decisions only when P3 begins.
