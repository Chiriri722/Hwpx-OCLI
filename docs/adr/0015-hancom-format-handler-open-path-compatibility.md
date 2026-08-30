# ADR-0015: Treat the format-handler CLI path as authoritative when the frame omits its duplicate

- Status: Accepted
- Decision date: 2026-08-30
- Scope: Hancom HWPX/OWPML format-handler open handshake
- Related: [ADR-0013](0013-hancom-package-preserving-editor-policy.md), [ADR-0014](0014-hancom-format-handler-install-promotion.md)
- Plan: [`../../specs/001-hancom-unified/task-plan.md`](../../specs/001-hancom-unified/task-plan.md)

## Context

Protocol v1 requires the host to spawn a format handler as `open <file>` and to
repeat the file path in the first JSONL `open` frame. The current OfficeCLI host
has a three-OS contract test that observes the repeated top-level `path` field.

GitHub Actions run `33301109847` nevertheless produced the same native-smoke
failure on Linux, Windows, and macOS: the released Rust handler received an
`open` frame with no `path`. The same commit was rebuilt locally with .NET SDK
10.0.302, executed on .NET runtimes 10.0.10 and 10.0.11, and combined with a
fresh Rust release binary, a clean plugin home, the CI installation layout, and
an RHWP-produced HWPX sample. Those controlled reproductions all carried the
path and succeeded. The evidence therefore establishes an interoperability
failure but does not establish a platform or serializer root cause.

Rejecting the session is safe but leaves all three supported native workflows
unusable. Blindly accepting any malformed `path` would instead weaken the
source-identity check.

## Decision

1. The process argument `open <file>` is the authoritative source identity for
   this handler. It is supplied by the host when the process is created and is
   canonicalized before the package is opened.
2. A completely absent JSON `path` may use that authoritative CLI value. This
   is a narrow released-host compatibility rule, not a change to the protocol-v1
   requirement that hosts send the field.
3. If `path` is present, it must be a JSON string. `null`, Boolean, numeric,
   array, and object values remain invalid; no malformed value falls back.
4. A present string is canonicalized and must identify the same filesystem
   object as the CLI argument. A mismatch remains an error.
5. `protocol`, `msg_type=open`, and Boolean `editable` remain mandatory. The
   fallback does not infer editability or accept a command frame as a handshake.
6. Diagnostics state only the observed failure. They must not claim a .NET,
   GitHub-hosted-runner, or OS root cause until a captured failing frame and a
   minimized reproducer prove one.

## Consequences

- A host that omits only the redundant path can still open the exact source it
  already selected on the command line.
- A conflicting or ill-typed frame cannot redirect the handler to another file
  or bypass the identity check.
- Host implementations and conformance tests must continue to emit `path`; this
  compatibility behavior must not be copied into the normative protocol text.
- The native workflow remains the end-to-end regression gate because the
  in-process host contract alone did not expose the observed release failure.

## Evidence

- `hwpx_format_handler.rs::open_frame_without_redundant_path_uses_the_cli_source`
  covers the sole accepted omission.
- `hwpx_format_handler.rs::open_frame_present_path_keeps_strict_type_and_identity_checks`
  covers `null` and a different existing file.
- `OfficeCli.Tests::FormatHandlerLifecycleFramesMatchProtocolV1` continues to
  require the host's canonical top-level path and editable flag.
- The implementing CI run must pass the native HWPX/OWPML smoke on all three
  operating systems before this decision is considered released.

This ADR is re-indexed into codebase-memory with the implementing commit.
