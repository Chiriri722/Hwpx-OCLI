#!/usr/bin/env python3
"""Verify equivalent HWP/HWPX pairs through the RHWP bridge and OfficeCLI.

Source documents stay outside the repository. Pair names are matched after
Unicode NFC normalization so macOS decomposed filenames still match Korean
filenames downloaded in composed form.

    python3 scripts/verify-hwp-pairs.py \
      --hwp-dir ~/Downloads \
      --hwpx-dir ~/hwpx-corpus \
      --converter ~/.local/rhwp/rhwp
"""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unicodedata


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fingerprint(path: Path) -> tuple[str, int, int]:
    stat = path.stat()
    return sha256_file(path), stat.st_mtime_ns, stat.st_size


def pair_key(path: Path) -> str:
    return unicodedata.normalize("NFC", path.stem).casefold()


def indexed_files(root: Path, suffix: str) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for path in sorted(root.rglob(f"*{suffix}")):
        key = pair_key(path)
        if key in result:
            raise RuntimeError(
                f"duplicate normalized pair name: {result[key]} and {path}"
            )
        result[key] = path
    return result


def require_success(
    command: list[str],
    *,
    env: dict[str, str] | None = None,
    timeout: int = 300,
) -> subprocess.CompletedProcess[bytes]:
    completed = subprocess.run(
        command,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    if completed.returncode != 0:
        diagnostic = (completed.stdout + completed.stderr)[-8192:].decode(
            "utf-8", "replace"
        )
        raise RuntimeError(
            f"command exited {completed.returncode}: {' '.join(command)}\n{diagnostic}"
        )
    return completed


def dump(
    plugin: Path,
    source: Path,
    converter: Path | None,
) -> tuple[bytes, list[dict[str, object]]]:
    env = os.environ.copy()
    if converter is None:
        env.pop("OFFICECLI_HWPX_CONVERTER", None)
    else:
        env["OFFICECLI_HWPX_CONVERTER"] = str(converter)
    completed = require_success(
        [str(plugin), "dump", str(source), "--quiet"], env=env
    )
    items: list[dict[str, object]] = []
    for number, line in enumerate(completed.stdout.splitlines(), start=1):
        try:
            item = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"invalid JSONL at {source}:{number}: {error}") from error
        if not isinstance(item, dict):
            raise RuntimeError(f"non-object JSONL item at {source}:{number}")
        items.append(item)
    if not items:
        raise RuntimeError(f"plugin emitted no JSONL for {source}")
    return completed.stdout, items


def summarize(items: list[dict[str, object]]) -> dict[str, object]:
    commands: Counter[str] = Counter()
    types: Counter[str] = Counter()
    text_chars: Counter[str] = Counter()
    for item in items:
        command = item.get("command")
        item_type = item.get("type")
        if isinstance(command, str):
            commands[command] += 1
        if isinstance(item_type, str):
            types[item_type] += 1
        props = item.get("props")
        if isinstance(props, dict):
            text = props.get("text")
            if isinstance(text, str):
                text_chars.update(
                    char
                    for char in unicodedata.normalize("NFC", text)
                    if not char.isspace()
                )
    text_payload = json.dumps(
        sorted(text_chars.items()), ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    return {
        "batch_items": len(items),
        "commands": dict(sorted(commands.items())),
        "types": dict(sorted(types.items())),
        "text_characters": sum(text_chars.values()),
        "text_multiset_sha256": sha256_bytes(text_payload),
    }


def parse_success_json(completed: subprocess.CompletedProcess[bytes]) -> dict:
    payload = json.loads(completed.stdout)
    if not isinstance(payload, dict) or payload.get("success") is not True:
        raise RuntimeError(f"OfficeCLI returned an unsuccessful payload: {payload!r}")
    return payload


def lint(
    officecli: Path,
    plugin: Path,
    source: Path,
    converter: Path | None,
) -> int:
    env = os.environ.copy()
    if converter is None:
        env.pop("OFFICECLI_HWPX_CONVERTER", None)
    else:
        env["OFFICECLI_HWPX_CONVERTER"] = str(converter)
    completed = require_success(
        [
            str(officecli),
            "plugins",
            "lint",
            str(plugin),
            "--fixture",
            str(source),
            "--json",
        ],
        env=env,
    )
    payload = parse_success_json(completed)
    unknown = payload.get("data", {}).get("unknown_prop_count")
    if unknown != 0:
        raise RuntimeError(f"OfficeCLI reported unknown props for {source}: {unknown!r}")
    return unknown


def query_count(officecli: Path, document: Path, element: str) -> int:
    completed = require_success(
        [str(officecli), "query", str(document), element, "--json"]
    )
    payload = parse_success_json(completed)
    return len(payload.get("data", {}).get("results", []))


def roundtrip(
    officecli: Path,
    work: Path,
    label: str,
    items: list[dict[str, object]],
) -> dict[str, object]:
    commands = work / f"{label}.json"
    document = work / f"{label}.docx"
    commands.write_text(
        json.dumps(items, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    parse_success_json(
        require_success(
            [
                str(officecli),
                "create",
                str(document),
                "--locale",
                "ko",
                "--force",
                "--json",
            ]
        )
    )
    batch = parse_success_json(
        require_success(
            [
                str(officecli),
                "batch",
                str(document),
                "--input",
                str(commands),
                "--json",
            ]
        )
    )
    summary = batch.get("data", {}).get("summary", {})
    if summary.get("failed") != 0 or summary.get("succeeded") != len(items):
        raise RuntimeError(f"OfficeCLI batch did not apply every item: {summary!r}")
    validation = parse_success_json(
        require_success([str(officecli), "validate", str(document), "--json"])
    )
    errors = validation.get("data", {}).get("count")
    if errors != 0:
        raise RuntimeError(f"OfficeCLI validation found {errors!r} errors in {document}")
    return {
        "validated": True,
        "paragraphs": query_count(officecli, document, "paragraph"),
        "tables": query_count(officecli, document, "table"),
        "cells": query_count(officecli, document, "cell"),
        "formfields": query_count(officecli, document, "formfield"),
    }


def resolve_tool(explicit: Path | None, env_name: str, command: str) -> Path | None:
    if explicit is not None:
        return explicit.expanduser().resolve()
    configured = os.environ.get(env_name)
    if configured:
        return Path(configured).expanduser().resolve()
    found = shutil.which(command)
    if found:
        return Path(found).resolve()
    if env_name == "OFFICECLI":
        cache = Path.home() / ".local" / "officecli-verify"
        names = (
            ("officecli.exe", "officecli")
            if os.name == "nt"
            else ("officecli", "officecli.exe")
        )
        for name in names:
            candidate = cache / name
            if candidate.is_file():
                return candidate.resolve()
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--hwp-dir", type=Path, required=True)
    parser.add_argument("--hwpx-dir", type=Path, required=True)
    parser.add_argument("--converter", type=Path)
    parser.add_argument("--plugin", type=Path)
    parser.add_argument("--officecli", type=Path)
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    plugin = (args.plugin or root / "target/release/officecli-dump-reader-hwpx")
    plugin = plugin.expanduser().resolve()
    converter = resolve_tool(args.converter, "OFFICECLI_HWPX_CONVERTER", "rhwp")
    officecli = resolve_tool(args.officecli, "OFFICECLI", "officecli")
    for label, path in (("plugin", plugin), ("converter", converter), ("officecli", officecli)):
        if path is None or not path.is_file():
            raise SystemExit(f"{label} executable not found: {path}")

    hwp_files = indexed_files(args.hwp_dir.expanduser().resolve(), ".hwp")
    hwpx_files = indexed_files(args.hwpx_dir.expanduser().resolve(), ".hwpx")
    pair_names = sorted(set(hwp_files) & set(hwpx_files))
    if not pair_names:
        raise SystemExit("no NFC-normalized HWP/HWPX filename pairs found")

    results: list[dict[str, object]] = []
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="officecli-hwp-pairs-") as directory:
        work = Path(directory)
        for index, key in enumerate(pair_names):
            hwp = hwp_files[key]
            hwpx = hwpx_files[key]
            before_hwp = fingerprint(hwp)
            before_hwpx = fingerprint(hwpx)
            hwp_output, hwp_items = dump(plugin, hwp, converter)
            hwpx_output, hwpx_items = dump(plugin, hwpx, None)
            hwp_summary = summarize(hwp_items)
            hwpx_summary = summarize(hwpx_items)
            exact = hwp_output == hwpx_output
            summary_equal = hwp_summary == hwpx_summary
            hwp_lint = lint(officecli, plugin, hwp, converter)
            hwpx_lint = lint(officecli, plugin, hwpx, None)
            hwp_roundtrip = roundtrip(officecli, work, f"{index}-hwp", hwp_items)
            hwpx_roundtrip = roundtrip(officecli, work, f"{index}-hwpx", hwpx_items)
            sources_unchanged = (
                before_hwp == fingerprint(hwp) and before_hwpx == fingerprint(hwpx)
            )
            if not summary_equal:
                failures.append(f"{hwp.name}: bridge/direct summaries differ")
            if hwp_roundtrip != hwpx_roundtrip:
                failures.append(f"{hwp.name}: OfficeCLI round-trip structures differ")
            if not sources_unchanged:
                failures.append(f"{hwp.name}: a source document changed")
            results.append(
                {
                    "pair": unicodedata.normalize("NFC", hwp.stem),
                    "exact_jsonl": exact,
                    "summary_equal": summary_equal,
                    "hwp_jsonl_sha256": sha256_bytes(hwp_output),
                    "hwpx_jsonl_sha256": sha256_bytes(hwpx_output),
                    "summary": hwp_summary,
                    "unknown_props": {"hwp": hwp_lint, "hwpx": hwpx_lint},
                    "roundtrip": hwp_roundtrip,
                    "sources_unchanged": sources_unchanged,
                }
            )

    report = {
        "pairs": results,
        "pair_count": len(results),
        "all_exact_jsonl": all(item["exact_jsonl"] for item in results),
        "all_summary_equal": all(item["summary_equal"] for item in results),
        "all_sources_unchanged": all(item["sources_unchanged"] for item in results),
        "success": not failures,
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}", file=os.sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
