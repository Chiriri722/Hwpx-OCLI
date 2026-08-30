#!/usr/bin/env python3
"""실제 문서 코퍼스 회귀 검증.

합성 픽스처가 잡지 못하는 것을 잡는다. 지금까지 나온 버그 14개 중 9개가
실제 문서에서만 드러났다.

문서 원본은 저장소에 넣지 않는다. 공개 기관 문서지만 개인정보(마스킹된
성명·주소)가 있고, 저작·배포 조건도 문서마다 다르다. 대신 **기대 요약**만
`tests/corpus/expected.json`에 커밋하고, 원본 경로는 환경변수로 받는다.

    # 기준선 생성 (문서를 처음 추가할 때, 또는 의도한 변경 후)
    HWPX_CORPUS=~/corpus scripts/verify-corpus.py --update

    # 회귀 검증
    HWPX_CORPUS=~/corpus scripts/verify-corpus.py

검증 항목:
  - 플러그인 exit code, 배치 항목 수
  - officecli plugins lint 미지 prop 0
  - 변환 성공 및 OpenXML 스키마 validate 통과
  - 문단/표/셀/폼필드 개수
  - OOXML 지표: w:br, w:hanging, 음수 w:firstLine, w:vMerge, 중첩 w:tbl
  - 계약 불변식: raw 개행 없음, 최상위 배열 아님
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

from executable_paths import (
    preferred_executable,
    resolve_tool,
)

REPO = Path(__file__).resolve().parent.parent
EXPECTED_PATH = REPO / "tests" / "corpus" / "expected.json"
PLUGIN = preferred_executable(
    Path.home() / ".officecli" / "plugins" / "dump-reader" / "hwp", "plugin"
)
# 요약에 넣을 지표만 센다. 절대 좌표나 ID는 넣지 않는다(불안정).
OOXML_PATTERNS = {
    "w_br": r"<w:br\b",
    "w_hanging": r'w:hanging="\d+"',
    "w_first_line_negative": r'w:firstLine="-',
    "w_vmerge": r"<w:vMerge\b",
    "w_tbl": r"<w:tbl>",
    "w_check_box": r"<w:checkBox\b",
    "form_text": r"FORMTEXT",
}


def find_officecli() -> str:
    resolved = resolve_tool(None, "OFFICECLI", "officecli")
    checkout = preferred_executable(
        REPO / ".." / ".." / "src" / "officecli" / "bin" / "Release" / "net10.0",
        "officecli",
    )
    candidates = [path for path in (resolved, checkout) if path is not None]
    for candidate in dict.fromkeys(candidates):
        try:
            subprocess.run([str(candidate), "--version"], capture_output=True, check=True)
            return str(candidate)
        except (OSError, subprocess.CalledProcessError):
            continue
    sys.exit(
        "current officecli host not found.\n"
        "  put it on PATH or set OFFICECLI to its absolute path."
    )


def corpus_files() -> list[Path]:
    root = os.environ.get("HWPX_CORPUS")
    if not root:
        sys.exit(
            "HWPX_CORPUS is not set.\n"
            "  point it at a directory containing the real .hwpx corpus:\n"
            "    HWPX_CORPUS=~/corpus scripts/verify-corpus.py"
        )
    d = Path(root).expanduser()
    if not d.is_dir():
        sys.exit(f"HWPX_CORPUS is not a directory: {d}")
    files = sorted(d.glob("*.hwpx"))
    if not files:
        sys.exit(f"no .hwpx files in {d}")
    return files


def count_json(cli: str, docx: Path, element: str) -> int:
    r = subprocess.run(
        [cli, "query", str(docx), element, "--json"], capture_output=True, text=True
    )
    try:
        return len(json.loads(r.stdout)["data"]["results"])
    except Exception:
        return 0


def summarize(cli: str, src: Path, work: Path) -> dict:
    """문서 하나를 변환하고 안정적인 요약을 만든다."""
    s: dict = {}
    local = work / src.name
    local.write_bytes(src.read_bytes())

    # 1. 플러그인 직접 실행
    r = subprocess.run(
        [str(PLUGIN), "dump", str(local), "--quiet"], capture_output=True, text=True
    )
    s["exit_code"] = r.returncode
    lines = [ln for ln in r.stdout.splitlines() if ln.strip()]
    s["batch_items"] = len(lines)

    # 계약 불변식
    s["top_level_array"] = r.stdout.lstrip().startswith("[")
    raw_newline = 0
    batch_items = []
    for ln in lines:
        try:
            item = json.loads(ln)
            batch_items.append(item)
            props = item.get("props") or {}
        except json.JSONDecodeError:
            s["invalid_jsonl"] = True
            continue
        for v in props.values():
            if isinstance(v, str) and ("\n" in v or "\r" in v):
                raw_newline += 1
    s["raw_newline_props"] = raw_newline

    # 2. lint
    r = subprocess.run(
        [cli, "plugins", "lint", str(PLUGIN), "--fixture", str(local), "--json"],
        capture_output=True,
        text=True,
    )
    try:
        s["unknown_props"] = json.loads(r.stdout)["data"]["unknown_prop_count"]
    except Exception:
        s["unknown_props"] = "lint-failed"

    # 3. dump-reader JSONL을 명시적으로 빈 DOCX에 재생한다. HWPX 자체는 이제
    # format-handler가 소유하므로 `officecli view local.hwpx`는 DOCX를 만들지 않는다.
    docx = local.with_suffix(".docx")
    docx.unlink(missing_ok=True)
    batch_path = local.with_suffix(".batch.json")
    batch_path.write_text(
        json.dumps(batch_items, ensure_ascii=False), encoding="utf-8"
    )
    created = subprocess.run(
        [cli, "create", str(docx)], capture_output=True, text=True
    )
    replayed = subprocess.run(
        [cli, "batch", str(docx), "--input", str(batch_path), "--stop-on-error"],
        capture_output=True,
        text=True,
    )
    s["docx_created"] = (
        created.returncode == 0 and replayed.returncode == 0 and docx.is_file()
    )
    if not s["docx_created"]:
        return s

    # 4. 스키마 검증
    r = subprocess.run([cli, "validate", str(docx)], capture_output=True, text=True)
    s["validates"] = "no errors" in r.stdout

    # 5. 구조
    for name, element in (
        ("paragraphs", "paragraph"),
        ("tables", "table"),
        ("cells", "cell"),
        ("formfields", "formfield"),
    ):
        s[name] = count_json(cli, docx, element)

    # 6. OOXML 지표
    r = subprocess.run(
        [cli, "raw", str(docx), "/word/document.xml"], capture_output=True, text=True
    )
    xml = r.stdout
    s["ooxml"] = {k: len(re.findall(p, xml)) for k, p in OOXML_PATTERNS.items()}

    # 7. 원본 대비 텍스트 존재 여부 (한컴 PrvText가 있을 때만)
    try:
        prv = zipfile.ZipFile(local).read("Preview/PrvText.txt").decode("utf-8")
        # PrvText의 `<`/`>`는 한컴이 셀 경계를 표시하는 구분자다. 내용이 아니다.
        want = re.sub(r"[\s<>]", "", prv)
        got = re.sub(r"\s", "", xml)
        missing = sum(1 for ch in set(want) if want.count(ch) > got.count(ch))
        s["prvtext_missing_chars"] = missing
    except Exception:
        s["prvtext_missing_chars"] = None

    return s


def diff(name: str, want, got, path: str = "") -> list[str]:
    if isinstance(want, dict) and isinstance(got, dict):
        out = []
        for k in sorted(set(want) | set(got)):
            out += diff(name, want.get(k), got.get(k), f"{path}.{k}" if path else k)
        return out
    if want != got:
        return [f"  {name}{'.' + path if path else ''}: expected {want!r}, got {got!r}"]
    return []


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--update", action="store_true", help="기준선을 현재 결과로 갱신")
    args = ap.parse_args()

    if not PLUGIN.is_file():
        sys.exit(f"plugin not installed: {PLUGIN}\n  run scripts/install.sh first.")

    cli = find_officecli()
    files = corpus_files()
    print(f"officecli: {cli}")
    print(f"corpus:    {len(files)} documents\n")

    results = {}
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        for f in files:
            results[f.name] = summarize(cli, f, work)
            s = results[f.name]
            print(
                f"  {f.name[:52]:52s} items={s.get('batch_items'):<4} "
                f"validate={s.get('validates')} unknown={s.get('unknown_props')}"
            )

    if args.update:
        EXPECTED_PATH.parent.mkdir(parents=True, exist_ok=True)
        EXPECTED_PATH.write_text(
            json.dumps(results, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        )
        print(f"\nbaseline written: {EXPECTED_PATH}")
        print("diff 를 눈으로 검토한 뒤 커밋하세요.")
        return 0

    if not EXPECTED_PATH.is_file():
        print(f"\nno baseline at {EXPECTED_PATH}")
        print("  create one with: scripts/verify-corpus.py --update")
        return 1

    expected = json.loads(EXPECTED_PATH.read_text())
    problems: list[str] = []

    # 기준선에 없는 문서는 경고만 (코퍼스를 늘리는 중일 수 있다)
    for name in results:
        if name not in expected:
            print(f"\n  note: {name} is not in the baseline (run --update to add)")

    for name, want in expected.items():
        got = results.get(name)
        if got is None:
            problems.append(f"  {name}: missing from the corpus directory")
            continue
        problems += diff(name, want, got)

    # 기준선과 무관하게 항상 지켜야 하는 것
    for name, s in results.items():
        if s.get("exit_code") != 0:
            problems.append(f"  {name}: exit_code {s.get('exit_code')} (must be 0)")
        if s.get("top_level_array"):
            problems.append(f"  {name}: emitted a top-level JSON array")
        if s.get("raw_newline_props"):
            problems.append(
                f"  {name}: {s['raw_newline_props']} props contain a raw newline"
            )
        if s.get("invalid_jsonl"):
            problems.append(f"  {name}: emitted a non-JSON line")
        if s.get("validates") is False:
            problems.append(f"  {name}: docx failed OpenXML validation")
        if s.get("ooxml", {}).get("w_first_line_negative"):
            problems.append(f"  {name}: negative w:firstLine (use w:hanging)")

    print()
    if problems:
        print("CORPUS REGRESSION")
        for p in dict.fromkeys(problems):
            print(p)
        return 1
    print("CORPUS OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
