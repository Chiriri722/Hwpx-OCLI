# Workspace guide

- Treat this checkout as the canonical workspace for HWPX/OfficeCLI work.
- The OfficeCLI host is under `src/officecli`; the Rust Hancom plugins live in
  the workspace under `plugins/hancom`.
- Before building, run `source scripts/dev-env.sh` so the project-local .NET SDK
  and the rustup toolchain are on `PATH`.
- Validate host changes with `dotnet build src/officecli/officecli.csproj
  --nologo`; use `./build.sh` when a self-contained native binary is required.
- Validate Hancom changes from `plugins/hancom` with
  `cargo test --workspace --locked --all-targets` and the matching clippy
  command in `DEVELOPMENT.md`.
- Do not copy `target/`, private `.hwpx` documents, or the private regression
  corpus into Git. Preserve `HWPX_CORPUS` as an external path.
- Keep `origin` for the personal fork and `upstream` for iOfficeAI/OfficeCLI.
- Spec-kit uses Codex skills in `.agents/skills/speckit-*`. Current execution
  inputs are `specs/001-hancom-unified/plan.md` and `tasks.md`; `task-plan.md`
  retains the full feature history.
- Read `docs/development-integrations.md` for the existing Linear issue mapping,
  Codex Security review procedure, and read-only Sentry queries. Keep credentials
  in local environment variables or ignored files, never in tracked configuration.
