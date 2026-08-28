#!/usr/bin/env python3
"""Measure a large HWPX dump through both the plugin and real OfficeCLI.

The fixture uses stored ZIP entries so it exercises the expanded-document
budget without tripping the compression-ratio defense. Each section remains
below the plugin's 16 MiB per-XML-resource limit.

    python3 scripts/verify-large-file.py
    python3 scripts/verify-large-file.py --sections 8 --text-mib 12
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import zipfile

from executable_paths import cargo_release_directory, preferred_executable, resolve_tool

try:
    import resource
except ImportError:  # Windows does not provide the Unix resource module.
    resource = None


NS = (
    ' xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section"'
    ' xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"'
    ' xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"'
)

HEADER = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    f'<hh:head{NS} version="1.4" secCnt="1"><hh:refList>'
    '<hh:charProperties itemCnt="1">'
    '<hh:charPr id="0" height="1000" textColor="#000000"/>'
    '</hh:charProperties><hh:paraProperties itemCnt="1">'
    '<hh:paraPr id="0"/></hh:paraProperties>'
    '</hh:refList></hh:head>'
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_fixture(path: Path, sections: int, text_mib: int) -> None:
    if sections < 1:
        raise ValueError("sections must be positive")
    if not 1 <= text_mib <= 15:
        raise ValueError("text-mib must be between 1 and 15")

    section_items = []
    spine_items = []
    for index in range(sections):
        section_items.append(
            f'<opf:item id="section{index}" href="Contents/section{index}.xml" '
            'media-type="application/xml"/>'
        )
        spine_items.append(f'<opf:itemref idref="section{index}" linear="yes"/>')

    hpf = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<opf:package xmlns:opf="http://www.idpf.org/2007/opf/">'
        '<opf:manifest>'
        '<opf:item id="header" href="Contents/header.xml" '
        'media-type="application/xml"/>'
        f'{"".join(section_items)}</opf:manifest><opf:spine>'
        '<opf:itemref idref="header" linear="yes"/>'
        f'{"".join(spine_items)}</opf:spine></opf:package>'
    )

    text = "가" * ((text_mib * 1024 * 1024) // len("가".encode("utf-8")))
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_STORED) as archive:
        archive.writestr("mimetype", "application/hwp+zip")
        archive.writestr("Contents/content.hpf", hpf)
        archive.writestr("Contents/header.xml", HEADER)
        for index in range(sections):
            section = (
                '<?xml version="1.0" encoding="UTF-8"?>'
                f'<hs:sec{NS}><hp:p id="0" paraPrIDRef="0" styleIDRef="0">'
                '<hp:run charPrIDRef="0"><hp:t>'
                f'{text}</hp:t></hp:run></hp:p></hs:sec>'
            )
            archive.writestr(f"Contents/section{index}.xml", section)


def locate_officecli(explicit: Path | None) -> Path | None:
    return resolve_tool(
        explicit,
        "OFFICECLI",
        "officecli",
        cache_dir=Path.home() / ".local" / "officecli-verify",
    )


def peak_child_rss_mib() -> tuple[float | None, str]:
    if resource is None:
        return None, "unsupported: Python resource module is unavailable"

    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    peak_rss_bytes = int(usage.ru_maxrss)
    if sys.platform != "darwin":
        peak_rss_bytes *= 1024
    return round(peak_rss_bytes / (1024 * 1024), 1), "measured"


def run_plugin(plugin: Path, fixture: Path) -> dict[str, object]:
    before_hash = sha256(fixture)
    before_mtime = fixture.stat().st_mtime_ns
    started = time.monotonic()
    first_output: float | None = None
    output_bytes = 0
    output_lines = 0

    process = subprocess.Popen(
        [str(plugin), "dump", str(fixture), "--quiet"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert process.stdout is not None
    assert process.stderr is not None
    while True:
        # BufferedReader.read(size) waits for `size` bytes or EOF and therefore
        # overstates the time to first output. os.read returns the first bytes
        # currently available from the pipe.
        chunk = os.read(process.stdout.fileno(), 1024 * 1024)
        if not chunk:
            break
        if first_output is None:
            first_output = time.monotonic() - started
        output_bytes += len(chunk)
        output_lines += chunk.count(b"\n")
    stderr = process.stderr.read()
    exit_code = process.wait()
    wall_seconds = time.monotonic() - started

    peak_rss_mib, peak_rss_status = peak_child_rss_mib()

    if exit_code != 0:
        raise RuntimeError(
            f"plugin exited {exit_code}: {stderr[-4096:].decode('utf-8', 'replace')}"
        )
    if output_lines == 0:
        raise RuntimeError("plugin produced no JSONL")
    if before_hash != sha256(fixture) or before_mtime != fixture.stat().st_mtime_ns:
        raise RuntimeError("source fixture changed during dump")

    return {
        "exit_code": exit_code,
        "first_output_seconds": round(first_output or wall_seconds, 3),
        "wall_seconds": round(wall_seconds, 3),
        "peak_rss_mib": peak_rss_mib,
        "peak_rss_status": peak_rss_status,
        "jsonl_lines": output_lines,
        "jsonl_mib": round(output_bytes / (1024 * 1024), 1),
        "heartbeats": stderr.count(b'{"heartbeat":true}'),
        "source_unchanged": True,
    }


def run_officecli(officecli: Path, plugin: Path, fixture: Path) -> dict[str, object]:
    env = os.environ.copy()
    env["OFFICECLI_PLUGIN_IDLE_TIMEOUT_SECONDS"] = "30"
    started = time.monotonic()
    completed = subprocess.run(
        [
            str(officecli),
            "plugins",
            "lint",
            str(plugin),
            "--fixture",
            str(fixture),
            "--json",
        ],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=300,
        check=False,
    )
    wall_seconds = time.monotonic() - started
    if completed.returncode != 0:
        diagnostic = (completed.stdout + completed.stderr)[-4096:].decode(
            "utf-8", "replace"
        )
        raise RuntimeError(f"OfficeCLI lint exited {completed.returncode}: {diagnostic}")

    payload = json.loads(completed.stdout)
    data = payload.get("data") or {}
    unknown = data.get("unknown_prop_count")
    if unknown != 0:
        raise RuntimeError(f"OfficeCLI reported unknown props: {unknown!r}")
    return {
        "version": subprocess.check_output(
            [str(officecli), "--version"], text=True
        ).strip(),
        "wall_seconds": round(wall_seconds, 3),
        "idle_timeout_seconds": 30,
        "unknown_prop_count": unknown,
        "success": True,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sections", type=int, default=4)
    parser.add_argument("--text-mib", type=int, default=12)
    parser.add_argument("--plugin", type=Path)
    parser.add_argument("--officecli", type=Path)
    parser.add_argument("--skip-officecli", action="store_true")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    plugin = (
        args.plugin.expanduser().resolve()
        if args.plugin is not None
        else preferred_executable(
            cargo_release_directory(root), "officecli-dump-reader-hwpx"
        )
    )
    if not plugin.is_file():
        raise SystemExit(f"release plugin not found: {plugin}")

    officecli = locate_officecli(args.officecli)
    if not args.skip_officecli and (officecli is None or not officecli.is_file()):
        raise SystemExit("officecli not found; pass --officecli or use --skip-officecli")

    with tempfile.TemporaryDirectory(prefix="officecli-hwpx-large-") as directory:
        fixture = Path(directory) / "large.hwpx"
        build_fixture(fixture, args.sections, args.text_mib)
        result: dict[str, object] = {
            "fixture": {
                "sections": args.sections,
                "text_mib_per_section": args.text_mib,
                "file_mib": round(fixture.stat().st_size / (1024 * 1024), 1),
            },
            "plugin": run_plugin(plugin.resolve(), fixture),
        }
        if not args.skip_officecli:
            assert officecli is not None
            result["officecli"] = run_officecli(
                officecli.resolve(), plugin.resolve(), fixture
            )
        print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
