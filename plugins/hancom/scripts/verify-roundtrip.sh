#!/usr/bin/env bash
#
# 현재 checkout의 officecli 바이너리로 두 플러그인 프로토콜을 검증한다.
#
# 단위/통합 테스트는 우리 코드 안에서만 도는 반면, 이 스크립트는 실제 메인
# 바이너리가 format-handler로 HWPX를 편집하는 경로와 dump-reader 출력을 DOCX로
# 재생하는 경로를 모두 본다.
#
#   scripts/verify-roundtrip.sh
#   OFFICECLI=/absolute/path/to/officecli scripts/verify-roundtrip.sh
#
# 승격 전 v1.0.145 릴리스에는 필요한 format-handler lifecycle 계약이 없으므로
# 다운로드 fallback을 두지 않는다. PATH 또는 OFFICECLI에는 current host를 둔다.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${1:-}" == "--help" ]]; then
  sed -n '3,12p' "$0"
  exit 0
elif [[ "$#" -ne 0 ]]; then
  echo "usage: OFFICECLI=/absolute/path/to/officecli $0" >&2
  exit 64
fi

if [[ -n "${OFFICECLI:-}" ]]; then
  :
elif command -v officecli >/dev/null 2>&1; then
  OFFICECLI="$(command -v officecli)"
elif [[ -x "${REPO_ROOT}/../../src/officecli/bin/Release/net10.0/officecli" ]]; then
  OFFICECLI="${REPO_ROOT}/../../src/officecli/bin/Release/net10.0/officecli"
else
  echo "current officecli host not found." >&2
  echo "publish src/officecli, put it on PATH, or set OFFICECLI to its absolute path." >&2
  exit 69
fi
[[ -x "${OFFICECLI}" ]] || { echo "OFFICECLI is not executable: ${OFFICECLI}" >&2; exit 69; }
echo "using officecli: ${OFFICECLI} ($("${OFFICECLI}" --version))"

DUMP_PLUGIN="${OFFICECLI_PLUGIN_DUMP_READER_HWP:-${HOME}/.officecli/plugins/dump-reader/hwp/plugin}"
FORMAT_PLUGIN="${OFFICECLI_PLUGIN_FORMAT_HANDLER_HWPX:-${HOME}/.officecli/plugins/format-handler/hwpx/plugin}"
if [[ ! -x "${DUMP_PLUGIN}" || ! -x "${FORMAT_PLUGIN}" ]]; then
  echo "Hancom plugins not installed. running scripts/install.sh ..." >&2
  "${REPO_ROOT}/scripts/install.sh"
fi
[[ -x "${DUMP_PLUGIN}" ]] || { echo "dump-reader not executable: ${DUMP_PLUGIN}" >&2; exit 69; }
[[ -x "${FORMAT_PLUGIN}" ]] || { echo "format-handler not executable: ${FORMAT_PLUGIN}" >&2; exit 69; }

# Environment candidates have discovery priority over stale user installations.
export OFFICECLI_PLUGIN_DUMP_READER_HWP="${DUMP_PLUGIN}"
export OFFICECLI_PLUGIN_DUMP_READER_HML="${DUMP_PLUGIN}"
export OFFICECLI_PLUGIN_FORMAT_HANDLER_HWPX="${FORMAT_PLUGIN}"
export OFFICECLI_PLUGIN_FORMAT_HANDLER_OWPML="${FORMAT_PLUGIN}"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

FAIL=0
check() {  # check <설명> <실제> <기대>
  if [[ "$2" == "$3" ]]; then
    printf '  ok   %s\n' "$1"
  else
    printf '  FAIL %s\n       expected: %s\n       actual:   %s\n' "$1" "$3" "$2"
    FAIL=1
  fi
}

# 셀 prop 하나를 읽는다.
cell_prop() {  # cell_prop <docx> <path> <prop>
  "${OFFICECLI}" get "$1" "$2" --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
if not d.get('success'): print(''); raise SystemExit
f=d['data']['results'][0].get('format',{})
v=f.get(sys.argv[1],'')
print(v if not isinstance(v,bool) else str(v).lower())
" "$3"
}
cell_text() {
  "${OFFICECLI}" get "$1" "$2" --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('' if not d.get('success') else (d['data']['results'][0].get('text') or ''))
"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

echo
echo "── 1. 플러그인 디스커버리 ──"
PLUGINS_JSON="$("${OFFICECLI}" plugins list --json)"
DISCOVERY="$(printf '%s' "${PLUGINS_JSON}" | python3 -c '
import json
import pathlib
import sys

payload = json.load(sys.stdin)
expected = {
    (str(pathlib.Path(sys.argv[1]).resolve()), "officecli-hancom-hwp",
     "dump-reader", frozenset({".hwp", ".hml"})),
    (str(pathlib.Path(sys.argv[2]).resolve()), "officecli-hancom-hwpx",
     "format-handler", frozenset({".hwpx", ".owpml"})),
}
observed = {
    (str(pathlib.Path(item["path"]).resolve()), item.get("name"),
     item.get("kinds", [""])[0], frozenset(item.get("extensions", [])))
    for item in payload.get("data", [])
}
print("yes" if payload.get("success") is True and expected <= observed else "no")
' "${DUMP_PLUGIN}" "${FORMAT_PLUGIN}")"
check "두 kind의 정확한 매니페스트를 찾는다" "${DISCOVERY}" "yes"

echo
echo "── 2. 픽스처 생성 ──"
python3 "${REPO_ROOT}/scripts/make_fixture.py" "${WORK}/full.hwpx" >/dev/null
python3 "${REPO_ROOT}/scripts/generate-editable-fixture.py" \
  "${WORK}/editable.hwpx" --text before --second-text second >/dev/null
[[ -f "${WORK}/full.hwpx" ]] && echo "  ok   ${WORK}/full.hwpx"

echo
echo "── 3. format-handler 직접 편집·저장 ──"
BEFORE_HASH="$(sha256_file "${WORK}/editable.hwpx")"
BEFORE_VIEW="$("${OFFICECLI}" view "${WORK}/editable.hwpx" text)"
check "HWPX 직접 조회" "${BEFORE_VIEW}" "$(printf 'before\nsecond')"
"${OFFICECLI}" set "${WORK}/editable.hwpx" \
  '/document/section[1]/paragraph[1]/text[1]' \
  --prop 'text=after & saved' >/dev/null
"${OFFICECLI}" save "${WORK}/editable.hwpx" >/dev/null
"${OFFICECLI}" close "${WORK}/editable.hwpx" >/dev/null
AFTER_VIEW="$("${OFFICECLI}" view "${WORK}/editable.hwpx" text)"
check "HWPX 저장 후 재열기" "${AFTER_VIEW}" "$(printf 'after & saved\nsecond')"
check "HWPX 원본 hash 변경" \
  "$([[ "$(sha256_file "${WORK}/editable.hwpx")" != "${BEFORE_HASH}" ]] && echo yes || echo no)" \
  "yes"
"${OFFICECLI}" validate "${WORK}/editable.hwpx" >/dev/null
check "HWPX view가 형제 DOCX를 만들지 않음" \
  "$([[ ! -e "${WORK}/editable.docx" ]] && echo yes || echo no)" "yes"
"${OFFICECLI}" close "${WORK}/editable.hwpx" >/dev/null

cp "${WORK}/editable.hwpx" "${WORK}/editable.owpml"
"${OFFICECLI}" set "${WORK}/editable.owpml" \
  '/document/section[1]/paragraph[2]/text[1]' \
  --prop 'text=OWPML saved' >/dev/null
"${OFFICECLI}" save "${WORK}/editable.owpml" >/dev/null
"${OFFICECLI}" close "${WORK}/editable.owpml" >/dev/null
OWPML_VIEW="$("${OFFICECLI}" view "${WORK}/editable.owpml" text)"
check "OWPML 저장 후 재열기" "${OWPML_VIEW}" "$(printf 'after & saved\nOWPML saved')"
"${OFFICECLI}" validate "${WORK}/editable.owpml" >/dev/null
"${OFFICECLI}" close "${WORK}/editable.owpml" >/dev/null

echo
echo "── 4. dump-reader plugins lint ──"
LINT="$("${OFFICECLI}" plugins lint "${DUMP_PLUGIN}" --fixture "${WORK}/full.hwpx" --json 2>&1)"
UNKNOWN="$(printf '%s' "${LINT}" | python3 -c "
import json,sys
try: print(json.load(sys.stdin)['data']['unknown_prop_count'])
except Exception: print('parse-error')
")"
check "미지 prop 개수" "${UNKNOWN}" "0"

echo
echo "── 5. dump-reader JSONL → DOCX replay ──"
"${DUMP_PLUGIN}" dump "${WORK}/full.hwpx" >"${WORK}/full.jsonl"
python3 - "${WORK}/full.jsonl" "${WORK}/full.json" <<'PY'
import json
import pathlib
import sys

source = pathlib.Path(sys.argv[1])
target = pathlib.Path(sys.argv[2])
items = [json.loads(line) for line in source.read_text(encoding="utf-8").splitlines()
         if line.strip()]
target.write_text(json.dumps(items, ensure_ascii=False), encoding="utf-8")
PY
"${OFFICECLI}" create "${WORK}/full.docx" >/dev/null
"${OFFICECLI}" batch "${WORK}/full.docx" \
  --input "${WORK}/full.json" --stop-on-error >/dev/null
check "DOCX replay 산출물" "$([[ -f "${WORK}/full.docx" ]] && echo yes || echo no)" "yes"

echo
echo "── 6. 본문 내용 ──"
DOCX="${WORK}/full.docx"
check "제목"          "$(cell_text "${DOCX}" '/body/p[1]')" "분기 보고서"
check "혼합서식 문단" "$(cell_text "${DOCX}" '/body/p[2]')" "매출은 전년 대비 12% 증가했습니다."
# 문단 내 줄바꿈은 \v로 남고 문단이 쪼개지지 않아야 한다.
check "소프트 줄바꿈" "$(cell_text "${DOCX}" '/body/p[3]')" "$(printf '첫 번째 줄\v같은 문단 둘째 줄')"
# 문단 인덱스: p1 제목 / p2 혼합 / p3 줄바꿈 / [tbl1] / [tbl2] / p4 체크박스 /
#              [tbl3] / p5 이미지 / p6 내어쓰기 / p7 PUA / p8 엔티티 / p9 탭
# 표는 /body/tbl[N]으로 따로 인덱싱되므로 문단 번호를 건너뛰지 않는다.
check "엔티티 해제"   "$(cell_text "${DOCX}" '/body/p[8]')" '각주 & 참고 <자료> "인용"'
check "탭"            "$(cell_text "${DOCX}" '/body/p[9]')" "$(printf '왼쪽\t오른쪽')"

echo
echo "── 7. 문단 서식 ──"
check "가운데 정렬"   "$(cell_prop "${DOCX}" '/body/p[1]' align)"       "center"
check "굵게"          "$(cell_prop "${DOCX}" '/body/p[1]' bold)"        "true"
check "글자색"        "$(cell_prop "${DOCX}" '/body/p[1]' color)"       "#1F4E79"
check "글자크기 18pt" "$(cell_prop "${DOCX}" '/body/p[1]' size)"        "18pt"
check "글꼴"          "$(cell_prop "${DOCX}" '/body/p[1]' font.ea)"     "함초롬돋움"
check "문단뒤 여백"   "$(cell_prop "${DOCX}" '/body/p[1]' spaceAfter)"  "8pt"
check "양쪽 정렬"     "$(cell_prop "${DOCX}" '/body/p[2]' align)"       "justify"
check "왼쪽 들여쓰기" "$(cell_prop "${DOCX}" '/body/p[2]' indent)"      "20pt"
check "첫줄 들여쓰기" "$(cell_prop "${DOCX}" '/body/p[2]' firstLineIndent)" "10pt"
check "줄간격"        "$(cell_prop "${DOCX}" '/body/p[2]' lineSpacing)" "1.6x"

echo
echo "── 8. 표 (tbl[1]): 배경색 + 행 전체 가로병합 ──"
check "열 개수"   "$(cell_prop "${DOCX}" '/body/tbl[1]' cols)" "3"
check "행 개수"   "$(cell_prop "${DOCX}" '/body/tbl[1]' rows)" "3"
check "머리 셀"   "$(cell_text "${DOCX}" '/body/tbl[1]/tr[1]/tc[1]')" "구분"
check "배경색"    "$(cell_prop "${DOCX}" '/body/tbl[1]/tr[1]/tc[1]' fill)" "#EDEDED"
check "3열 병합"  "$(cell_prop "${DOCX}" '/body/tbl[1]/tr[3]/tc[1]' colspan)" "3"

echo
echo "── 9. 표 (tbl[2]): 세로병합 + 행 중간 가로병합 ──"
check "세로병합 첫칸"     "$(cell_prop "${DOCX}" '/body/tbl[2]/tr[1]/tc[1]' vmerge)" "restart"
check "세로병합 이음칸"   "$(cell_prop "${DOCX}" '/body/tbl[2]/tr[2]/tc[1]' vmerge)" "continue"
check "이음칸 텍스트 없음" "$(cell_text "${DOCX}" '/body/tbl[2]/tr[2]/tc[1]')" ""
check "행중간 가로병합"   "$(cell_prop "${DOCX}" '/body/tbl[2]/tr[1]/tc[2]' colspan)" "2"
# 병합 뒤 셀 인덱스가 당겨지는지 — 격자 열번호를 쓰면 여기서 깨진다.
check "병합 뒤 셀 위치"   "$(cell_text "${DOCX}" '/body/tbl[2]/tr[2]/tc[2]')" "좌"
check "병합 뒤 셀 위치2"  "$(cell_text "${DOCX}" '/body/tbl[2]/tr[2]/tc[3]')" "우"

echo
echo "── 10. 이미지 ──"
PIC="$("${OFFICECLI}" get "${DOCX}" '/body/p[5]' --depth 1 --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
kids=d['data']['results'][0].get('children',[]) if d.get('success') else []
p=[c for c in kids if c.get('type')=='picture']
if not p: print('none|||'); raise SystemExit
f=p[0].get('format',{})
print('|'.join([p[0]['type'], f.get('alt',''), f.get('width',''), f.get('height','')]))
")"
IFS='|' read -r PTYPE PALT PW PH <<< "${PIC}"
check "picture 요소"  "${PTYPE}" "picture"
check "대체 텍스트"   "${PALT}"  "매출 추이"
# 7200 HWPUNIT = 1 inch = 72pt = 2.54cm. 단위 없이 보내면 EMU로 읽혀 0.0cm가 된다.
check "그림 너비"     "${PW}"    "2.5cm"
check "그림 높이"     "${PH}"    "1.3cm"

echo
echo "── 11. 내어쓰기 (음수 hc:intent → hangingIndent) ──"
# HWP는 내어쓰기를 음수 intent로 표현한다. docx의 w:firstLine은 음수를 받지
# 않으므로 w:hanging 으로 나가야 한다. prop 이름만 맞으면 lint는 통과하므로
# OOXML 산출물을 직접 확인한다.
check "hangingIndent 값" "$(cell_prop "${DOCX}" '/body/p[6]' hangingIndent)" "85.7pt"
RAW="$("${OFFICECLI}" raw "${DOCX}" /word/document.xml 2>/dev/null)"
HANG="$(printf '%s' "${RAW}" | grep -c 'w:hanging="1714"' || true)"
NEGFIRST="$(printf '%s' "${RAW}" | grep -c 'w:firstLine="-' || true)"
check "w:hanging 생성"    "${HANG}"     "1"
check "음수 w:firstLine"  "${NEGFIRST}" "0"

echo
echo "── 12. 한컴 사용자 정의 영역(PUA) 문자 ──"
# 매핑을 추측하지 않는다. 문자는 그대로 보존하고 진단으로만 알린다.
check "PUA 문자 보존" \
  "$(cell_text "${DOCX}" '/body/p[7]' | python3 -c "
import sys
s=sys.stdin.read()
print('yes' if '\uF0854' in s and '\uF0855' in s else 'no')")" \
  "yes"
PUANOTE="$("${DUMP_PLUGIN}" dump "${WORK}/full.hwpx" 2>&1 >/dev/null | grep -c 'private-use' || true)"
check "PUA 진단 보고"  "${PUANOTE}" "1"

echo
echo "── 13. 폼 컨트롤 체크박스 ──"
# HWPX 양식 문서는 체크박스를 문자(☑)가 아니라 hp:checkBtn 폼 컨트롤로 넣는다.
# 문자로 바꾸면 Word에서 켜고 끌 수 없고 체크 안 된 상자는 사라진다.
CB="$("${OFFICECLI}" query "${DOCX}" formfield --json 2>/dev/null | python3 -c "
import json,sys
try: rs=json.load(sys.stdin)['data']['results']
except Exception: print('0||'); raise SystemExit
items=sorted((r['format'].get('name',''), r['format'].get('checked')) for r in rs)
print(str(len(rs)) + '|' + ','.join(n for n,_ in items) + '|' + ','.join(str(bool(c)).lower() for _,c in items))
")"
IFS='|' read -r CBCOUNT CBNAMES CBSTATES <<< "${CB}"
check "체크박스 개수"        "${CBCOUNT}"  "3"
check "체크박스 이름"        "${CBNAMES}"  "CBNest1,CBNest2,CBTop"
# CBNest1=false, CBNest2=true, CBTop=true (이름 정렬 순서)
check "체크 상태"            "${CBSTATES}" "false,true,true"
# 중첩표 안 체크박스는 평문 변환 때 유실됐던 경로다.
check "중첩표 체크박스 보존" "$(printf '%s' "${CBNAMES}" | grep -c 'CBNest1')" "1"

echo
if [[ "${FAIL}" -eq 0 ]]; then
  echo "ALL CHECKS PASSED"
else
  echo "SOME CHECKS FAILED" >&2
fi
exit "${FAIL}"
