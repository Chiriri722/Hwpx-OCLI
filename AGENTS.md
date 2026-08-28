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
