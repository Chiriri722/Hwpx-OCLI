#!/usr/bin/env python3
"""Regression tests for cross-platform verifier executable discovery."""

from __future__ import annotations

import os
import unittest
from pathlib import Path
from unittest.mock import patch

from executable_paths import (
    cargo_release_directory,
    executable_names,
    first_existing_executable,
    preferred_executable,
    resolve_tool,
)


class ExecutablePathTests(unittest.TestCase):
    def test_platform_name_is_native_only(self) -> None:
        self.assertEqual(
            executable_names("officecli", windows=True),
            ("officecli.exe",),
        )
        self.assertEqual(
            executable_names("officecli", windows=False),
            ("officecli",),
        )

    def test_windows_release_plugin_resolves_exe_without_an_override(self) -> None:
        root = Path(__file__).resolve().parent / "fixture-release"
        plugin = root / "officecli-dump-reader-hwpx.exe"
        with patch(
            "pathlib.Path.is_file",
            autospec=True,
            side_effect=lambda candidate: candidate == plugin,
        ):
            self.assertEqual(
                preferred_executable(
                    root, "officecli-dump-reader-hwpx", windows=True
                ),
                plugin.resolve(),
            )

    def test_linux_does_not_select_a_stale_windows_executable(self) -> None:
        root = Path(__file__).resolve().parent / "fixture-cache"
        foreign = root / "officecli.exe"
        with patch(
            "pathlib.Path.is_file",
            autospec=True,
            side_effect=lambda candidate: candidate == foreign,
        ):
            self.assertIsNone(
                first_existing_executable(root, "officecli", windows=False)
            )
            self.assertEqual(
                preferred_executable(root, "officecli", windows=False),
                (root / "officecli").resolve(),
            )

    def test_linux_rejects_a_file_without_execute_permission(self) -> None:
        root = Path(__file__).resolve().parent / "fixture-cache"
        candidate = root / "officecli"
        with patch("pathlib.Path.is_file", return_value=True), patch(
            "executable_paths.os.access", return_value=False
        ):
            self.assertIsNone(
                first_existing_executable(root, "officecli", windows=False)
            )

    def test_cargo_release_directory_honors_target_dir(self) -> None:
        crate_root = Path(__file__).resolve().parent.parent
        target = crate_root / "alternate-target"
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target)}, clear=True):
            self.assertEqual(
                cargo_release_directory(crate_root),
                (target / "release").resolve(),
            )
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(
                cargo_release_directory(crate_root),
                (crate_root / "target" / "release").resolve(),
            )

    def test_resolve_tool_uses_explicit_then_env_then_path_then_cache(self) -> None:
        root = Path(__file__).resolve().parent / "fixture-tools"
        explicit = root / "explicit.exe"
        configured = root / "configured.exe"
        on_path = root / "path.exe"
        cached = root / "officecli.exe"

        with patch.dict(os.environ, {"OFFICECLI": str(configured)}, clear=True), patch(
            "executable_paths.shutil.which", return_value=str(on_path)
        ):
            self.assertEqual(
                resolve_tool(
                    explicit,
                    "OFFICECLI",
                    "officecli",
                    cache_dir=root,
                    windows=True,
                ),
                explicit.resolve(),
            )
            self.assertEqual(
                resolve_tool(
                    None,
                    "OFFICECLI",
                    "officecli",
                    cache_dir=root,
                    windows=True,
                ),
                configured.resolve(),
            )

        with patch.dict(os.environ, {}, clear=True), patch(
            "executable_paths.shutil.which", return_value=str(on_path)
        ):
            self.assertEqual(
                resolve_tool(
                    None,
                    "OFFICECLI",
                    "officecli",
                    cache_dir=root,
                    windows=True,
                ),
                on_path.resolve(),
            )

        with patch.dict(os.environ, {}, clear=True), patch(
            "executable_paths.shutil.which", return_value=None
        ), patch(
            "pathlib.Path.is_file",
            autospec=True,
            side_effect=lambda candidate: candidate == cached,
        ):
            self.assertEqual(
                resolve_tool(
                    None,
                    "OFFICECLI",
                    "officecli",
                    cache_dir=root,
                    windows=True,
                ),
                cached.resolve(),
            )


if __name__ == "__main__":
    unittest.main()
