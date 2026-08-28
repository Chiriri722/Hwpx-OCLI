"""Shared executable discovery for the HWPX verification scripts."""

from __future__ import annotations

import os
import shutil
from pathlib import Path


def executable_names(stem: str, *, windows: bool | None = None) -> tuple[str, ...]:
    """Return the native executable name for the selected platform."""

    if windows is None:
        windows = os.name == "nt"
    base = stem[:-4] if stem.lower().endswith(".exe") else stem
    return (f"{base}.exe",) if windows else (base,)


def cargo_release_directory(crate_root: Path) -> Path:
    """Resolve Cargo's release directory, including an alternate target root."""

    configured = os.environ.get("CARGO_TARGET_DIR")
    target = Path(configured).expanduser() if configured else crate_root / "target"
    return (target / "release").resolve()


def first_existing_executable(
    directory: Path, stem: str, *, windows: bool | None = None
) -> Path | None:
    """Find a native executable in a known directory."""

    if windows is None:
        windows = os.name == "nt"
    directory = directory.expanduser()
    for name in executable_names(stem, windows=windows):
        candidate = directory / name
        if candidate.is_file() and (windows or os.access(candidate, os.X_OK)):
            return candidate.resolve()
    return None


def preferred_executable(
    directory: Path, stem: str, *, windows: bool | None = None
) -> Path:
    """Return an existing executable or the native path for a useful diagnostic."""

    found = first_existing_executable(directory, stem, windows=windows)
    if found is not None:
        return found
    preferred = executable_names(stem, windows=windows)[0]
    return (directory.expanduser() / preferred).resolve()


def resolve_tool(
    explicit: Path | None,
    env_name: str,
    command: str,
    *,
    cache_dir: Path | None = None,
    windows: bool | None = None,
) -> Path | None:
    """Resolve a tool using explicit, environment, PATH, then cache precedence."""

    if explicit is not None:
        return explicit.expanduser().resolve()
    configured = os.environ.get(env_name)
    if configured:
        return Path(configured).expanduser().resolve()
    found = shutil.which(command)
    if found:
        return Path(found).resolve()
    if cache_dir is not None:
        return first_existing_executable(cache_dir, command, windows=windows)
    return None
