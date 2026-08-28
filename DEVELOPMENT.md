# Fork development environment

This checkout combines the current OfficeCLI upstream with the HWPX dump-reader
plugin that was previously developed in the standalone `hwpx-ocli` directory.
The canonical plugin source is now the Cargo workspace at `plugins/hancom`.

## Repository layout

- `src/officecli`: OfficeCLI host (.NET 10)
- `plugins/plugin-protocol.md`: upstream plugin contract
- `plugins/hancom`: Hancom plugin workspace (Rust, MSRV 1.88)
- `.dotnet`: project-local .NET SDK (ignored by Git)

The Git remotes are intentionally split:

- `origin`: `Chiriri722/Hwpx-OCLI`
- `upstream`: `iOfficeAI/OfficeCLI`

## First-time setup

```bash
./scripts/bootstrap-dev.sh
source scripts/dev-env.sh
```

`bootstrap-dev.sh` installs .NET SDK 10.0.302 into `.dotnet` without requiring
administrator privileges. Rust is managed by rustup; the HWPX crate selects the
stable toolchain with clippy and rustfmt.

## Validation

From the repository root:

```bash
source scripts/dev-env.sh

dotnet build src/officecli/officecli.csproj --nologo
cargo test --workspace --locked --all-targets --manifest-path plugins/hancom/Cargo.toml
cargo clippy --workspace --locked --all-targets --manifest-path plugins/hancom/Cargo.toml -- -D warnings
```

For the self-contained native binary used by releases, run `./build.sh`. The
upstream `officecli.slnx` currently references a test project that is not part
of the repository, so use the existing host project directly for a development
build.

For end-to-end plugin validation, run from the plugin directory:

```bash
cd plugins/hancom
scripts/verify-roundtrip.sh
```

The private HWPX corpus remains outside Git. Set `HWPX_CORPUS` before running
`scripts/verify-corpus.py`; only `tests/corpus/expected.json` is versioned.

## Branch workflow

Keep `main` aligned with `origin/main`. Start feature work from the newest
`upstream/main`, then push the feature branch to `origin`. The initial migrated
work lives on `feat/hwpx-plugin` and deliberately has no upstream tracking
branch, preventing accidental pushes to the upstream repository.
