# ADR-0015: Bound released-host lifecycle compatibility without granting write access

- Status: Accepted
- Decision date: 2026-08-30
- Scope: Hancom HWPX/OWPML format-handler open and save lifecycle frames
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

After the path-only compatibility rule was released, GitHub Actions run
`33302040476` advanced past that validation and then failed identically on
Linux, Windows, and macOS because the same native `view` handshake omitted
`editable`. The source-level host contract still observed a Boolean value, so
this second failure likewise proves an interoperability discrepancy without
proving which build or transport layer removed the field.

Run `33302693952` then passed native HWPX/OWPML reads on all three systems but
rejected the first `set` because the session advertised only read commands. Its
error wording exactly matched the host implementation immediately preceding
commit `0429890a`; inspection of that implementation shows that it did not
discard the fields, but serialized both under `args`. An independent full
CLI-to-resident-to-plugin capture using .NET SDK 10.0.302 confirmed that the
current host sends canonical top-level fields. This narrows the compatibility
target to a known legacy envelope without establishing why that implementation
was used in the failing workflow.

Run `33303406446` accepted that exact legacy open envelope and completed the
first mutation on all three operating systems. It then failed on save with the
handler's diagnostic `save uses msg_type=save rather than a command envelope`.
That is the unique response to the lifecycle-fix predecessor's exact
`{"msg_type":"command","command":"save","args":{}}` frame. The same
historical host implementation that nested open fields sent this save shape,
so the two observations form one bounded compatibility target rather than a
new inferred serializer variant.

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
5. A completely absent `editable` field is interpreted as `false`. This is a
   fail-closed compatibility rule for read-only operations: omission can never
   advertise `set`/`save`, establish a writer baseline, or grant write access.
6. If `editable` is present, it must be a JSON Boolean. `null` and every other
   type remain invalid. Hosts and their conformance tests must continue to send
   the field as required by protocol v1.
7. `protocol` and `msg_type=open` remain mandatory. No compatibility rule
   accepts a command frame as a handshake.
8. A legacy `args` object is accepted only when it contains exactly `path` and
   `editable`, with the same string/Boolean type and canonical identity checks
   as their top-level equivalents. Missing or extra keys, non-object `args`,
   and any mixture with top-level lifecycle fields are rejected.
9. A legacy command-style save is accepted only when the already-open session
   is editable and the request contains exactly `protocol`, `msg_type=command`,
   `command=save`, and an explicitly present empty `args` object. Missing,
   null, or nonempty `args`; `props`; and every extra field are rejected. This
   does not grant write authority and does not replace canonical
   `msg_type=save` in the normative contract.
10. Diagnostics state only the observed failure. They must not claim a .NET,
   GitHub-hosted-runner, or OS root cause until a captured failing frame and a
   minimized reproducer prove one.

## Consequences

- A host that omits only the redundant path can still open the exact source it
  already selected on the command line.
- A conflicting or ill-typed frame cannot redirect the handler to another file
  or bypass the identity check.
- An omitted editability hint can only reduce authority to a read-only session;
  malformed present values remain errors.
- A known pre-lifecycle-fix host retains its explicit edit intent through the
  exact nested open and command-save envelopes, while ambiguous mixed or
  extended shapes fail closed.
- Host implementations and conformance tests must continue to emit `path` and
  Boolean `editable`; these compatibility rules must not be copied into the
  normative protocol text.
- The native workflow remains the end-to-end regression gate because the
  in-process host contract alone did not expose the observed release failure.

## Evidence

- `hwpx_format_handler.rs::open_frame_without_redundant_path_uses_the_cli_source`
  covers the sole accepted omission.
- `hwpx_format_handler.rs::open_frame_present_path_keeps_strict_type_and_identity_checks`
  covers `null` and a different existing file.
- `hwpx_format_handler.rs::open_frame_without_editable_defaults_to_read_only`
  verifies that omission excludes `set`/`save`, permits reading, and preserves
  the source after a rejected mutation.
- `hwpx_format_handler.rs::open_frame_present_editable_keeps_strict_type_checks`
  verifies that a present `null` does not use the omission fallback.
- `hwpx_format_handler.rs::legacy_lifecycle_envelopes_preserve_explicit_editability_and_durable_save`
  verifies that the exact historical open and save envelopes preserve an
  explicit writer session through durable save and reopen.
- `hwpx_format_handler.rs::legacy_nested_open_fields_reject_mixed_or_extended_shapes`
  rejects mixed, extra-key, and incomplete legacy envelopes.
- `hwpx_format_handler.rs::legacy_command_save_rejects_incomplete_or_extended_shapes`
  rejects missing/null/nonempty args, props, and extra fields on command-save.
- `OfficeCli.Tests::FormatHandlerLifecycleFramesMatchProtocolV1` continues to
  require the host's canonical top-level path and editable flag.
- [CI run `33349242319`](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33349242319)
  passed the native current-host HWPX/OWPML lifecycle smoke on Linux, Windows,
  and macOS at implementing HEAD `5df4d34e`. This closes the end-to-end release
  gate for the compatibility decision.

This ADR is re-indexed into codebase-memory with the implementing commit.
