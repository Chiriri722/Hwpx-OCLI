# Bug: Verification scripts selected the wrong platform executable

**Date Reported**: 2026-08-14
**Date Fixed**: 2026-08-14
**Reporter**: Windows/Linux repository verification
**Assignee**: Codex
**Severity**: Medium
**Status**: Fixed

## Problem

The HWPX verification scripts used extensionless default paths. On Windows,
`verify-large-file.py` and `verify-hwp-pairs.py` did not find the release
`officecli-dump-reader-hwpx.exe`. `verify-corpus.py` had the same issue for the
installed `plugin.exe` and cached `officecli.exe`.

A second failure appeared in a shared checkout: when Linux used a separate
`CARGO_TARGET_DIR`, the verifier ignored it and selected a stale Windows `.exe`
from the repository target directory.

## Reproduction

1. Build the plugin on Windows with `cargo build --release --locked`.
2. Run `python plugins/hwpx/scripts/verify-large-file.py --skip-officecli`.
3. Observe `release plugin not found` even though the `.exe` exists.
4. Leave that Windows target directory in the checkout.
5. On Linux, build with an alternate `CARGO_TARGET_DIR` and run the verifier
   without `--plugin`.
6. Observe that the verifier selects the stale Windows executable instead of
   the Linux build.

## Root cause

- Default plugin and cache paths hard-coded Unix executable names.
- Executable discovery was duplicated across three scripts.
- The initial portability fix allowed the opposite platform's spelling as an
  automatic fallback, which is unsafe in a shared Windows/Linux checkout.
- Default release discovery assumed `<crate>/target` and did not honor
  `CARGO_TARGET_DIR`.

## Fix

- Added `scripts/executable_paths.py` as the single executable discovery helper.
- Select only the native automatic name: `.exe` on Windows and extensionless
  on Linux/macOS. Explicit paths remain available for cross-built binaries.
- Resolve tools in explicit path, environment variable, `PATH`, then cache
  order.
- Honor `CARGO_TARGET_DIR` when locating the release plugin.
- Updated all three Python verifiers to use the helper.
- Added six platform/path regression tests and a 1 MiB large-file smoke to the
  Linux and Windows GitHub Actions jobs.

## Verification

- Windows executable-path tests: 6/6 passed.
- Windows default-path large-file smoke: exit 0, one 1.0 MiB JSONL line, source
  unchanged.
- Linux read-only checkout with alternate `CARGO_TARGET_DIR`: 6/6 tests passed;
  the verifier selected the ELF binary in the alternate target directory and
  the same smoke passed without `--plugin`.
- Full Windows Rust suite: 210 passed; Clippy and release build passed.
- Full Linux Rust suite: 220 passed; Clippy and release build passed.

## Prevention

- Keep executable discovery in one helper.
- Do not automatically fall back to another OS executable spelling in a shared
  checkout.
- Require the Unix execute bit for automatically discovered files.
- Keep the path regression test and post-build smoke in both OS jobs.
- Add every future Cargo target-directory override to the same helper instead
  of constructing `target/release` paths in individual scripts.

## Files modified

- `.github/workflows/hwpx-plugin.yml`
- `.gitignore`
- `plugins/hwpx/scripts/executable_paths.py`
- `plugins/hwpx/scripts/test_executable_paths.py`
- `plugins/hwpx/scripts/verify-corpus.py`
- `plugins/hwpx/scripts/verify-hwp-pairs.py`
- `plugins/hwpx/scripts/verify-large-file.py`
