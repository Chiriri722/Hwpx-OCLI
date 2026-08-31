# ADR-0014: Promote HWPX discovery with an active-first, rollback-safe install

- Status: Accepted
- Decision date: 2026-08-30
- Scope: Hancom plugin manifests, user installation, migration, and native CI
- Related: [ADR-0013](0013-hancom-package-preserving-editor-policy.md),
  [ADR-0016](0016-hancom-v12-ooxml-carrier-bridge.md)
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

OfficeCLI resolves a plugin by the pair `(kind, extension)`. HWPX and OWPML were
previously installed below `dump-reader`, while the new package-preserving editor
must be discovered as a `format-handler`. Leaving both declarations active is not
an additive compatibility measure: an old HWPX dump-reader can create a sibling DOCX
and shadow direct editing, depending on the discovery candidate that wins.

The HWP/HML path still has a DOCX target and read-only dump semantics. A single
manifest cannot truthfully describe both that target-bearing dump-reader and the
targetless HWPX format-handler vocabulary. Promotion therefore requires two
binaries and a coordinated migration of existing user paths.

An installer cannot atomically rename eight independent paths as one filesystem
operation. It can, however, prevent the dangerous intermediate state where the old
HWPX registration has been retired before all new active registrations are valid,
and it can roll back only files whose current identity still belongs to that
installer attempt.

## Decision

1. `officecli-hancom-hwp` declares exactly `dump-reader` with
   `.hwp`/`.hml` and `target: docx`. `officecli-hancom-hwpx` declares
   exactly `format-handler` with `.hwpx`/`.owpml` and its closed
   text-edit vocabulary. `officecli-hancom-cell` and
   `officecli-hancom-show` are separate singleton-extension dump-readers with
   `target: xlsx` and `target: pptx`, respectively, as bounded by ADR-0016.
2. The installer owns one eight-target rollback domain:

   - active: `dump-reader/hwp`, `dump-reader/hml`,
     `format-handler/hwpx`, `format-handler/owpml`, `dump-reader/cell`, and
     `dump-reader/show`;
   - retired compatibility paths: `dump-reader/hwpx` and
     `dump-reader/owpml`.

3. Every active staged file is hash-checked and probed. Its full manifest
   semantics—name, protocol, kind, exact extensions, and target presence/value—must
   match the intended slot. The six active paths are committed before either old
   dump-reader path is retired.
4. Commit, retirement, and uninstall failures roll back the same eight-target domain.
   Before removing an installed value or restoring over it, rollback verifies that
   its current hash or link shape is the value written by this attempt. A concurrent
   external change is never overwritten; the recovery backup is retained and the
   installer fails visibly.
5. Unix keeps four physical binaries at `dump-reader/hwp/plugin`,
   `format-handler/hwpx/plugin`, `dump-reader/cell/plugin`, and
   `dump-reader/show/plugin`. HML links relatively to `../hwp/plugin`, and
   OWPML links relatively to `../hwpx/plugin`. Windows avoids symlink privileges
   and copies the appropriate binary into each of the six active slots.
6. Uninstall removes only the six active and two retired exact plugin files. It
   leaves unrelated plugins and directories intact. All existing ancestors below
   the trusted absolute home root must be ordinary directories; symlink, junction,
   reparse-point, and non-directory ancestors fail closed.
7. Native CI publishes the current OfficeCLI host from this checkout. It verifies
   exact two-kind/six-slot discovery, absence of the retired registrations,
   direct HWPX/OWPML view and durable text edits without sibling DOCX files,
   HWP/HML dump-reader behavior, Cell/Show direct-native byte-preserving
   siblings, and eight-path uninstall on Linux, Windows, and macOS. The
   pre-promotion v1.0.145 host is not used as lifecycle evidence.
8. This is a serialized, transactional-best-effort update across eight independent
   paths, not filesystem-atomic or crash-atomic installation. Success is returned
   only after all six active paths have passed postflight manifest checks as one
   suite version and both retired paths are absent. Failure or forced termination
   can leave backups or a partial layout; the suite must not be used until the same
   installer has been rerun successfully. Conflict-safe rollback never overwrites a
   path that no longer matches installer-owned state and does not guarantee recovery
   after process or machine termination. Cooperative install/uninstall writers are
   serialized per plugin root on Unix and per Windows login session plus plugin
   root on Windows, but concurrent host execution is not guaranteed to observe one
   immutable generation.

## Consequences

- A completed install has one unambiguous owner for each extension and kind.
- Migration never intentionally removes a working legacy path until every active
  replacement has passed its probe and reached its final slot.
- Rollback may stop with a retained backup rather than destroy an external
  concurrent modification. That requires manual inspection but preserves evidence
  and user data.
- The four physical Unix binaries and six Windows copies use more space than the
  old one-binary layout. This is accepted in exchange for truthful manifests
  and link-free Windows operation.
- Future format-handler verbs can grow behind ADR-0013, while HWP/HML conversion
  stays isolated from writable package ownership.

## Evidence

- `protocol_contract.rs` asserts the exact HWP/HML dump-reader extension set.
- `hwpx_format_handler.rs` asserts the exact HWPX/OWPML format-handler manifest
  and lifecycle.
- `install_contract.rs` exercises fresh install, exact manifests, legacy
  migration, retirement failure, path safety, idempotent uninstall, cleanup, and
  rollback conflict handling. It also verifies every physical binary and exact
  singleton kind, extension set, and target presence/value before mutation.
- A clean Linux current-host smoke enumerated the four active registrations, ran
  `set → save → close → reopen → validate` for both HWPX and OWPML, verified the
  source hash and escaped XML delta, and created no sibling DOCX.
- `.github/workflows/hwpx-plugin.yml` carries the same actual-host smoke on
  Linux, Windows, and macOS. The implementing run must be green on all three
  before this expanded decision is considered released.

This ADR is re-indexed into codebase-memory with the implementing commit.
