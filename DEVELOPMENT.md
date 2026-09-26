# Fork development environment

This checkout combines the current OfficeCLI upstream with the Hancom plugins
previously developed in the standalone `hwpx-ocli` directory. The Cargo
workspace at `plugins/hancom` builds an HWP/HML dump-reader, a separate editable
HWPX/OWPML format-handler, and bounded Cell/Show OOXML carrier dump-readers.

Start with the [internal documentation index](docs/README.md) and the
[latest review](docs/reviews/2026-09-08-code-review.md) before resuming work.

## Repository layout

- `src/officecli`: OfficeCLI host (.NET 10)
- `plugins/plugin-protocol.md`: plugin contract, including the fork-local
  `direct-native` / `byte-preserving` extension
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

### Windows PowerShell

`scripts/dev-env.sh` is a Bash helper. In PowerShell, select a Windows SDK that
matches `global.json`; a Linux `.dotnet/dotnet` copied into the checkout cannot
run as a Windows SDK. `10.0.400` alone does not satisfy the pinned `10.0.302`
with `rollForward=latestPatch`.

For this checkout, the existing Windows SDK was verified on 2026-09-08:

```powershell
$env:DOTNET_ROOT = (Resolve-Path 'plugins/hancom/.officecli/dotnet-10.0.302').Path
$env:DOTNET_CLI_HOME = (Resolve-Path '.dotnet-cli-home').Path
$env:NUGET_PACKAGES = (Resolve-Path '.nuget/packages').Path
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:PATH = "$env:DOTNET_ROOT;$env:USERPROFILE/.cargo/bin;$env:PATH"
dotnet --version # 10.0.302
```

These ignored SDK/cache directories are local conveniences, not files supplied
by Git. On a fresh Windows checkout, install the SDK version in `global.json`
and use its directory instead; create the two cache directories before calling
`Resolve-Path` if they do not yet exist.

## Validation

From the repository root:

```bash
source scripts/dev-env.sh

dotnet build src/officecli/officecli.csproj --nologo
dotnet run --project tests/OfficeCli.Tests/OfficeCli.Tests.csproj -p:NuGetAudit=false
cargo test --workspace --locked --all-targets --manifest-path plugins/hancom/Cargo.toml
cargo clippy --workspace --locked --all-targets --manifest-path plugins/hancom/Cargo.toml -- -D warnings
python plugins/hancom/scripts/test_executable_paths.py
python scripts/test-workflow-action-pins.py
```

The commands after `source` also run in the configured PowerShell session.
The host contract project is present in `officecli.slnx` and is an executable
harness, so use `dotnet run`, not `dotnet test`, to execute its checks.
`NuGetAudit=false` matches the host-contract CI invocation; it does not verify
dependency advisories. The action-pin checker requires the dependencies in
`scripts/requirements-action-pins.txt`.

For the self-contained native binary used by releases, run `./build.sh`.
For the declared Rust MSRV, also run `cargo +1.88.0 check --workspace --locked
--all-targets --manifest-path plugins/hancom/Cargo.toml` when that toolchain is
installed. Windows ACL/process tests must run with a normal user token; a
restricted sandbox may reject the test's own protected temporary directories.

For end-to-end plugin validation, run from the plugin directory:

```bash
cd plugins/hancom
scripts/verify-roundtrip.sh
```

The round-trip verifier requires the current host lifecycle implementation.
Point `OFFICECLI` at a current publish when it is not already on `PATH`; it
does not fall back to the pre-promotion v1.0.145 release.

The private HWPX corpus remains outside Git. Set `HWPX_CORPUS` before running
`scripts/verify-corpus.py`; only `tests/corpus/expected.json` is versioned.

## Branch workflow

Keep `main` aligned with `origin/main`. Start feature work from the newest
`upstream/main`, then push the feature branch to `origin`. The initial migrated
work lives on `feat/hwpx-plugin`. As observed on 2026-09-08, it tracks
`origin/feat/hwpx-plugin`. Inspect `git status -sb` and `git remote -v` before
publishing; keep the personal fork and upstream destinations distinct.

## Spec-kit and connected development tools

The current Spec-kit integration is Codex (`.agents/skills/speckit-*`).
Use `specs/001-hancom-unified/plan.md` and `tasks.md` for the current repair work;
`task-plan.md` retains the full feature history. See
[development integrations](docs/development-integrations.md) for Linear issue
mapping, the Codex Security review procedure, and read-only Sentry queries.
