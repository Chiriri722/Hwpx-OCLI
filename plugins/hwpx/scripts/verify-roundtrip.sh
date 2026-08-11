#!/usr/bin/env bash
#
# 실제 officecli 바이너리로 왕복 검증한다.
#
# 단위/통합 테스트는 우리 코드 안에서만 도는 반면, 이 스크립트는 진짜 메인
# 바이너리가 플러그인을 찾아 실행하고 그 출력을 docx로 재생하는 전 경로를 본다.
#
#   scripts/verify-roundtrip.sh            # PATH나 캐시의 officecli 사용
#   scripts/verify-roundtrip.sh --download # 없으면 공식 릴리즈 다운로드
#
# officecli는 .NET 없이 도는 단일 바이너리다(zero install). 릴리즈 자산을
# 받아 SHA256을 대조한 뒤 실행한다.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_DIR="${HOME}/.local/officecli-verify"
OFFICECLI_VERSION="${OFFICECLI_VERSION:-v1.0.143}"
DO_DOWNLOAD=0

[[ "${1:-}" == "--download" ]] && DO_DOWNLOAD=1

# ── officecli 찾기 ──
OFFICECLI=""
if command -v officecli >/dev/null 2>&1; then
  OFFICECLI="$(command -v officecli)"
elif [[ -x "${CACHE_DIR}/officecli" ]]; then
  OFFICECLI="${CACHE_DIR}/officecli"
elif [[ "${DO_DOWNLOAD}" -eq 1 ]]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)  ASSET="officecli-mac-arm64" ;;
    Darwin-x86_64) ASSET="officecli-mac-x64" ;;
    Linux-aarch64) ASSET="officecli-linux-arm64" ;;
    Linux-x86_64)  ASSET="officecli-linux-x64" ;;
    *) echo "unsupported platform: $(uname -s)-$(uname -m)" >&2; exit 69 ;;
  esac
  BASE="https://github.com/iOfficeAI/OfficeCLI/releases/download/${OFFICECLI_VERSION}"
  mkdir -p "${CACHE_DIR}"
  echo "downloading ${ASSET} (${OFFICECLI_VERSION})..."
  curl -fsSL "${BASE}/SHA256SUMS" -o "${CACHE_DIR}/SHA256SUMS"
  curl -fsSL "${BASE}/${ASSET}"   -o "${CACHE_DIR}/officecli"
  EXPECTED="$(awk -v a="${ASSET}" '$2==a || $2=="*"a {print $1}' "${CACHE_DIR}/SHA256SUMS")"
  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "${CACHE_DIR}/officecli" | awk '{print $1}')"
  else
    ACTUAL="$(shasum -a 256 "${CACHE_DIR}/officecli" | awk '{print $1}')"
  fi
  if [[ -z "${EXPECTED}" || "${EXPECTED}" != "${ACTUAL}" ]]; then
    echo "checksum mismatch — refusing to run" >&2
    echo "  expected: ${EXPECTED:-<not found in SHA256SUMS>}" >&2
    echo "  actual:   ${ACTUAL}" >&2
    rm -f "${CACHE_DIR}/officecli"
    exit 70
  fi
  echo "checksum OK"
  chmod +x "${CACHE_DIR}/officecli"
  OFFICECLI="${CACHE_DIR}/officecli"
else
  echo "officecli not found." >&2
  echo "re-run with --download, or put officecli on PATH." >&2
  exit 69
fi

echo "using officecli: ${OFFICECLI} ($("${OFFICECLI}" --version))"

# ── 플러그인이 설치되어 있어야 한다 ──
PLUGIN="${HOME}/.officecli/plugins/dump-reader/hwpx/plugin"
if [[ ! -x "${PLUGIN}" ]]; then
  echo "plugin not installed. running scripts/install.sh ..." >&2
  "${REPO_ROOT}/scripts/install.sh"
fi

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

echo
echo "── 1. 플러그인 디스커버리 ──"
DISCOVERED="$("${OFFICECLI}" plugins list 2>&1 | grep -c 'officecli-hwpx' || true)"
check "officecli가 플러그인을 찾는다" "${DISCOVERED}" "1"

echo
echo "── 2. 픽스처 생성 ──"
python3 "${REPO_ROOT}/scripts/make_fixture.py" "${WORK}/full.hwpx" >/dev/null
[[ -f "${WORK}/full.hwpx" ]] && echo "  ok   ${WORK}/full.hwpx"

echo
echo "── 3. plugins lint (모든 prop이 대상 스키마에 선언되어 있는지) ──"
LINT="$("${OFFICECLI}" plugins lint officecli-hwpx --fixture "${WORK}/full.hwpx" --json 2>&1)"
UNKNOWN="$(printf '%s' "${LINT}" | python3 -c "
import json,sys
try: print(json.load(sys.stdin)['data']['unknown_prop_count'])
except Exception: print('parse-error')
")"
check "미지 prop 개수" "${UNKNOWN}" "0"

echo
echo "── 4. dump-reader 경유 변환 ──"
( cd "${WORK}" && "${OFFICECLI}" view full.hwpx text > text.out 2>&1 )
check "형제 docx 생성" "$([[ -f "${WORK}/full.docx" ]] && echo yes || echo no)" "yes"

echo
echo "── 5. 본문 내용 ──"
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
echo "── 6. 문단 서식 ──"
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
echo "── 7. 표 (tbl[1]): 배경색 + 행 전체 가로병합 ──"
check "열 개수"   "$(cell_prop "${DOCX}" '/body/tbl[1]' cols)" "3"
check "행 개수"   "$(cell_prop "${DOCX}" '/body/tbl[1]' rows)" "3"
check "머리 셀"   "$(cell_text "${DOCX}" '/body/tbl[1]/tr[1]/tc[1]')" "구분"
check "배경색"    "$(cell_prop "${DOCX}" '/body/tbl[1]/tr[1]/tc[1]' fill)" "#EDEDED"
check "3열 병합"  "$(cell_prop "${DOCX}" '/body/tbl[1]/tr[3]/tc[1]' colspan)" "3"

echo
echo "── 8. 표 (tbl[2]): 세로병합 + 행 중간 가로병합 ──"
check "세로병합 첫칸"     "$(cell_prop "${DOCX}" '/body/tbl[2]/tr[1]/tc[1]' vmerge)" "restart"
check "세로병합 이음칸"   "$(cell_prop "${DOCX}" '/body/tbl[2]/tr[2]/tc[1]' vmerge)" "continue"
check "이음칸 텍스트 없음" "$(cell_text "${DOCX}" '/body/tbl[2]/tr[2]/tc[1]')" ""
check "행중간 가로병합"   "$(cell_prop "${DOCX}" '/body/tbl[2]/tr[1]/tc[2]' colspan)" "2"
# 병합 뒤 셀 인덱스가 당겨지는지 — 격자 열번호를 쓰면 여기서 깨진다.
check "병합 뒤 셀 위치"   "$(cell_text "${DOCX}" '/body/tbl[2]/tr[2]/tc[2]')" "좌"
check "병합 뒤 셀 위치2"  "$(cell_text "${DOCX}" '/body/tbl[2]/tr[2]/tc[3]')" "우"

echo
echo "── 9. 이미지 ──"
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
echo "── 10. 내어쓰기 (음수 hc:intent → hangingIndent) ──"
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
echo "── 11. 한컴 사용자 정의 영역(PUA) 문자 ──"
# 매핑을 추측하지 않는다. 문자는 그대로 보존하고 진단으로만 알린다.
check "PUA 문자 보존" \
  "$(cell_text "${DOCX}" '/body/p[7]' | python3 -c "
import sys
s=sys.stdin.read()
print('yes' if '\uF0854' in s and '\uF0855' in s else 'no')")" \
  "yes"
PUANOTE="$("${PLUGIN}" dump "${WORK}/full.hwpx" 2>&1 >/dev/null | grep -c 'private-use' || true)"
check "PUA 진단 보고"  "${PUANOTE}" "1"

echo
echo "── 12. 폼 컨트롤 체크박스 ──"
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
