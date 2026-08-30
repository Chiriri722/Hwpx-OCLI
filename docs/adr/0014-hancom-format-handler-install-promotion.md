# ADR-0014: Promote HWPX discovery with an active-first, rollback-safe install

- Status: Accepted
- Decision date: 2026-08-30
- Scope: Hancom plugin manifests, user installation, migration, and native CI
- Related: [ADR-0013](0013-hancom-package-preserving-editor-policy.md)
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

An installer cannot atomically rename six independent paths as one filesystem
operation. It can, however, prevent the dangerous intermediate state where the old
HWPX registration has been retired before all new active registrations are valid,
and it can roll back only files whose current identity still belongs to that
installer attempt.

## Decision

1. `officecli-hancom-hwp` declares exactly `dump-reader` with
   `.hwp`/`.hml` and `target: docx`. `officecli-hancom-hwpx` declares
   exactly `format-handler` with `.hwpx`/`.owpml` and its closed
   text-edit vocabulary.
2. The installer owns one six-target rollback domain:

   - active: `dump-reader/hwp`, `dump-reader/hml`,
     `format-handler/hwpx`, `format-handler/owpml`;
   - retired compatibility paths: `dump-reader/hwpx` and
     `dump-reader/owpml`.

3. Every active staged file is hash-checked and probed. Its full manifest
   semantics—name, protocol, kind, exact extensions, and target presence/value—must
   match the intended slot. The four active paths are committed before either old
   dump-reader path is retired.
4. Commit, retirement, and uninstall failures roll back the same six-target domain.
   Before removing an installed value or restoring over it, rollback verifies that
   its current hash or link shape is the value written by this attempt. A concurrent
   external change is never overwritten; the recovery backup is retained and the
   installer fails visibly.
5. Unix keeps one physical binary per role at `dump-reader/hwp/plugin` and
   `format-handler/hwpx/plugin`. HML links relatively to `../hwp/plugin`,
   and OWPML links relatively to `../hwpx/plugin`. Windows avoids symlink
   privileges and copies the appropriate role binary into each active slot.
6. Uninstall removes only the four active and two retired exact plugin files. It
   leaves unrelated plugins and directories intact. All existing ancestors below
   the trusted absolute home root must be ordinary directories; symlink, junction,
   reparse-point, and non-directory ancestors fail closed.
7. Native CI publishes the current OfficeCLI host from this checkout. It verifies
   exact two-kind discovery, absence of the retired registrations, direct
   HWPX/OWPML view and durable text edits without sibling DOCX files, HWP/HML
   dump-reader behavior, and six-path uninstall on Linux and Windows. The
   pre-promotion v1.0.145 host is not used as lifecycle evidence.
8. This is transactional best effort across paths, not crash-atomic multi-path
   installation. Process termination between sequential renames can leave backups
   or a partial layout; rerunning the installer is the recovery operation. The
   documentation must not call the six-path operation filesystem-atomic.

## Consequences

- A completed install has one unambiguous owner for each extension and kind.
- Migration never intentionally removes a working legacy path until every active
  replacement has passed its probe and reached its final slot.
- Rollback may stop with a retained backup rather than destroy an external
  concurrent modification. That requires manual inspection but preserves evidence
  and user data.
- The two physical Unix binaries use more space than the old one-binary layout, and
  Windows stores four copies. This is accepted in exchange for truthful manifests
  and link-free Windows operation.
- Future format-handler verbs can grow behind ADR-0013, while HWP/HML conversion
  stays isolated from writable package ownership.

## Evidence

- `protocol_contract.rs` asserts the exact HWP/HML dump-reader extension set.
- `hwpx_format_handler.rs` asserts the exact HWPX/OWPML format-handler manifest
  and lifecycle.
- `install_contract.rs` exercises fresh install, exact manifests, legacy
  migration, retirement failure, path safety, idempotent uninstall, cleanup, and
  rollback conflict handling. The current suite passes 18 Windows and 26 Unix
  installer scenarios.
- A clean Linux current-host smoke enumerated the four active registrations, ran
  `set → save → close → reopen → validate` for both HWPX and OWPML, verified the
  source hash and escaped XML delta, and created no sibling DOCX.
- `.github/workflows/hwpx-plugin.yml` carries the same actual-host smoke on
  Linux and Windows.

This ADR is re-indexed into codebase-memory with the implementing commit.
