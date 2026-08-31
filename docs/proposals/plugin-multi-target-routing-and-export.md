# Proposal: explicit multi-target manifests, routing, and reverse export

- Status: Design proposal only
- Date: 2026-08-30
- Scope: post-v1 OfficeCLI plugin protocol
- Non-goal: changing the current v1 wire contract or implementing Cell/Show
  write-back in the Hancom plugin

## Why this is separate

Protocol v1 gives a dump-reader one `target` string and discovers by
`(kind, extension)`. That is sufficient for the current four Hancom binaries,
but it forces separate executables when one implementation spans DOCX, XLSX,
and PPTX targets. Separately, the host currently checks dump-readers before
format-handlers for the same extension. Neither limitation should be bypassed
with `argv[0]` or installation-path inference.

This document completes the design investigation in plan P7. It does not mark
any protocol implementation or upstream acceptance as complete.

The current repository also carries the narrowly negotiated downstream v1
capabilities `direct-native` plus `byte-preserving` for the Cell/Show carrier
bridge. Those strings are not present in the upstream protocol. They remain a
fork-local exception documented in
[`plugins/plugin-protocol.md`](../../plugins/plugin-protocol.md), not a preview
of this proposal; every upstream sync must explicitly re-audit or remove the
exception rather than assuming compatibility.

## 1. Per-extension targets

Add an optional v2 manifest field:

```json
{
  "kinds": ["dump-reader"],
  "extensions": [".hwp", ".cell", ".show"],
  "targets": {
    ".hwp": "docx",
    ".cell": "xlsx",
    ".show": "pptx"
  }
}
```

Validation rules:

- `targets` is allowed only when `dump-reader` is declared.
- Keys must exactly equal the normalized `extensions` set, with no duplicates
  after case folding; values remain restricted to `docx|xlsx|pptx`.
- A manifest must use either legacy `target` or `targets`, never both.
- Protocol-v1 hosts reject a v2 manifest rather than guessing a target. A v2
  host continues accepting legacy `target` and treats it as the same target for
  every extension.
- The host resolves the target from the selected extension before creating a
  sibling. The plugin cannot override it at runtime.

This removes duplicated binaries without weakening deterministic discovery.
It should be adopted only with manifest parser, lint, environment override,
registry, installer, and each-native-format sibling tests.

## 2. Same-extension routing

Do not let a plugin choose priority through registration order. Add a host-owned
policy, for example `routing` per extension:

```json
{
  "routing": {
    ".hwpx": { "preferred_kind": "format-handler", "fallback": "dump-reader" }
  }
}
```

The safe rules are:

- one authoritative choice per extension after all manifests are probed;
- an explicit host/user policy outranks plugin preference;
- ambiguous declarations without policy fail with a diagnostic listing paths,
  rather than first-match-wins;
- writable `format-handler` selection requires its normal capability and
  lifecycle checks; a fallback cannot silently gain write authority;
- cache keys include the chosen kind and executable identity so a routing
  change cannot reuse a sibling/session from another owner.

A smaller alternative is a host configuration map outside manifests. That is
preferable if upstream wants plugins to describe capability but never policy.

## 3. Reverse export feasibility

The existing `exporter` kind can model native-to-foreign conversion, but the
current Hancom evidence supports no honest `.cell` or `.show` writer. A future
exporter proposal must declare source formats and exact output profiles, expose
loss/capability metadata, write only to an explicit output path, and commit the
output atomically after validation.

Minimum release evidence for each reverse exporter:

1. a public specification or versioned converter/SDK contract with compatible
   redistribution terms;
2. provenance-recorded fixtures covering text, structure, style, media, and
   active content;
3. a deterministic unsupported-feature policy rather than approximation;
4. native Hancom recovery-free open, render, save, and reopen on a recorded
   build, plus source-native semantic comparison;
5. resource, path, scratch, timeout, process-tree, and output-reclassification
   tests equivalent to the RHWP boundary.

Until those gates exist, DOCX/XLSX/PPTX → HWPX/Cell/Show reverse export remains
unimplemented. HWPX's existing closed-subset format-handler is not evidence for
Cell or Show serialization.

## Compatibility and rollout

These changes require a protocol version bump, fixture manifests for old and
new shapes, and mixed-version tests. The host should land validation and clear
errors first; plugin adoption follows later. Existing v1 plugins and their
single `target` continue to work unchanged.
