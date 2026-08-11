# Workspace guide

- Treat this checkout as the canonical workspace for HWPX/OfficeCLI work.
- The OfficeCLI host is under `src/officecli`; the Rust HWPX plugin is the
  independent crate under `plugins/hwpx`.
- Before building, run `source scripts/dev-env.sh` so the project-local .NET SDK
  and the rustup toolchain are on `PATH`.
- Validate host changes with `dotnet build src/officecli/officecli.csproj
  --nologo`; use `./build.sh` when a self-contained native binary is required.
- Validate HWPX changes with `cargo test --locked --manifest-path
  plugins/hwpx/Cargo.toml` and the matching clippy command in `DEVELOPMENT.md`.
- Do not copy `target/`, private `.hwpx` documents, or the private regression
  corpus into Git. Preserve `HWPX_CORPUS` as an external path.
- Keep `origin` for the personal fork and `upstream` for iOfficeAI/OfficeCLI.
