# Hwpx-OCLI Constitution

## Core Principles

### I. Protocol Compatibility and Explicit Failure

Plugins MUST preserve the documented OfficeCLI JSONL protocol: stdout is machine
output only, diagnostics go to stderr or a log file, and unsupported input fails
with the documented non-zero status instead of guessed or partial content. A
protocol or vocabulary change MUST update its contract documentation and tests in
the same change.

### II. Source Integrity and Bounded Execution

Source documents MUST remain byte-for-byte and metadata unchanged unless the user
explicitly invokes an editing workflow. Archives, XML, external converters,
plugin discovery, and installers MUST enforce finite byte, item, time, and process
budgets. External tools run without a shell, in private staging where applicable,
and their outputs are re-identified before use.

### III. Test-First Contracts

Every defect fix or new behavior MUST begin with a focused failing test when a
reproducible automated test is possible. Boundary values require both the exact
accepted limit and the first rejected value. Unit tests alone do not prove
installer, process, filesystem, or host-integration behavior; those require
contract or integration tests on the affected platform.

### IV. Native Cross-Platform Evidence

Claims covering Linux and Windows MUST be proven on both native GitHub-hosted
runners with the repository-pinned Rust and .NET toolchains. Local emulation or a
container is useful preflight evidence but does not replace a named remote gate.
Plan phases MUST remain open while their required remote evidence is missing.

### V. Provenance, Licensing, and No Guessing

Format behavior MUST be grounded in official specifications or representative
fixtures. Unverified proprietary formats remain fail-closed until evidence is
available. External actions and downloaded binaries MUST be pinned and verified.
Hancom specifications are referenced by URL, revision, byte length, and digest;
the PDFs are not redistributed and the required attribution remains visible in
UI/help, manual, and source surfaces.

## Technical and Security Constraints

- The HWPX plugin supports Rust 1.88 or newer; the host follows `global.json`.
- Host/plugin contracts in `plugins/plugin-protocol.md` are normative.
- Rust plugin code is MIT and host code is Apache-2.0; incompatible runtime
  dependencies require an explicit architecture decision.
- CI action references use immutable full commit SHAs. Downloaded release assets
  require a pinned SHA-256 before execution.
- Unknown `.cell` and `.show` structures MUST NOT be inferred from extensions or
  anecdotal descriptions; representative samples are a prerequisite.

## Development Workflow and Quality Gates

1. Inspect the canonical spec and task plan before implementation.
2. Use the codebase-memory graph first for code discovery and impact tracing.
3. Add the narrowest failing contract, implement the complete behavior, then run
   focused and repository-wide regression suites.
4. Validate changed shell scripts, workflows, documentation links, line endings,
   and `git diff --check` before commit.
5. Record architectural boundary changes in an ADR and refresh the knowledge
   graph after source changes.
6. Do not advance a phase until every acceptance gate named by its plan is backed
   by direct evidence. External-input blockers stay explicit rather than being
   silently re-scoped.

## Governance

This constitution governs spec-kit plans in this repository. A conflicting task
plan MUST be corrected before implementation. Amendments require a documented
rationale, an updated version and date, and migration notes when existing code or
tests are affected. Reviews MUST call out any intentionally deferred rule and the
evidence required to close it.

**Version**: 1.0.0 | **Ratified**: 2026-08-28 | **Last Amended**: 2026-08-28
