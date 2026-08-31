# OfficeCLI Hancom plugins

[OfficeCLI](https://github.com/iOfficeAI/OfficeCLI)용 한컴 문서 플러그인 모음이다.
역할·대상 포맷·쓰기 권한이 다른 네 바이너리를 제공한다.

- `officecli-hancom-hwpx`: `.hwpx`·`.owpml`을 직접 여는 `format-handler`.
  조회와 strict editable subset의 텍스트 수정·저장을 지원한다.
- `officecli-hancom-hwp`: `.hwp`·legacy `.hml`을 DOCX 명령으로 옮기는
  `dump-reader`. 바이너리 HWP만 선택적으로
  [RHWP](https://github.com/edwardkim/rhwp) v0.8.4+를 변환기로 사용한다.
- `officecli-hancom-cell`: 검증된 Cell 12.0300 OOXML carrier `.cell`을
  byte-identical `.xlsx` 형제로 만드는 읽기 전용 `dump-reader`.
- `officecli-hancom-show`: 검증된 Show 12.0000 OOXML carrier `.show`를
  byte-identical `.pptx` 형제로 만드는 읽기 전용 `dump-reader`.

## 한컴 공개 문서 표기

본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.

공개 문서의 원본 URL과 검증된 SHA-256은
[`../../docs/spec-sources.md`](../../docs/spec-sources.md)에 기록한다. PDF 원본은 이
저장소에서 재배포하지 않는다.

## 무엇을 하는가

```
.hwpx/.owpml ──[format-handler]──▶ view/get/query + text set/save
.hml          ──[dump-reader]────▶ BatchItem JSONL ──[officecli]──▶ .docx
.hwp          ──[RHWP, 선택]────▶ 임시 .hwpx ──[dump-reader]────▶ .docx
.cell 12.0300 ──[검증+byte copy]──────────────────────────────▶ .xlsx
.show 12.0000 ──[검증+byte copy]──────────────────────────────▶ .pptx
```

`.hwpx`와 `.owpml`은 형제 DOCX를 만들지 않고 원본 패키지를 직접 연다. 저장 가능한
범위는 plain `hp:p/hp:run/hp:t` 텍스트 노드의 치환이다. 저장은 같은 디렉터리의
copy-on-write 임시 파일에서 G0~G3 검증을 마친 뒤 원자 교체하며, 바뀌지 않은 ZIP
entry와 metadata는 그대로 보존한다. `.hwp`와 `.hml`은 읽기 전용 변환 경로다.
Cell/Show는 전체 ZIP·XML과 관측된 profile marker를 검증한 뒤 원본을 바꾸지
않고 native 형제를 만든다. marker는 지원 부분집합을 분류할 뿐 생산자나 provenance를
인증하지 않는다. 이 경로는 proprietary parser나 일반 변환기가 아니다.

## 설치

설치 스크립트는 다음 여섯 활성 경로와 두 폐기 경로를 하나의 rollback domain으로
관리한다.

| 확장자 | kind | 바이너리 |
|---|---|---|
| `.hwp`, `.hml` | `dump-reader` | `officecli-hancom-hwp` |
| `.hwpx`, `.owpml` | `format-handler` | `officecli-hancom-hwpx` |
| `.cell` | `dump-reader` (`target=xlsx`) | `officecli-hancom-cell` |
| `.show` | `dump-reader` (`target=pptx`) | `officecli-hancom-show` |

새 활성 경로 여섯 곳을 모두 검증·커밋한 뒤에만 이전
`dump-reader/{hwpx,owpml}` 경로를 폐기한다. 어느 단계든 실패하면 기존 여덟
관리 대상의 상태를 conflict-safe best effort로 복원하며, unrelated 플러그인은 건드리지
않는다. 설치 성공은 여섯 활성 경로가 한 suite version으로 postflight 검증되고 두 폐기
경로가 사라진 뒤에만 반환한다. 여덟 독립 경로 전체에 대한 filesystem/crash atomicity는
제공하지 않는다. 실패하거나 강제 종료된 실행은 partial layout과 backup을 남길 수 있으므로
같은 설치기가 성공할 때까지 Hancom plugin suite를 사용하지 않는다. rollback은 다른 actor가
바꾼 경로를 덮지 않으며 종료 이후의 완전 복구를 보장하지 않는다.
모든 역할의 name/version/protocol/kind/extensions/target을 staging 전에 확인하고,
Windows에서는 같은 로그인 세션과 plugin root를 공유하는 named mutex로, Unix에서는
같은 plugin root의 atomic lock directory로 install과 uninstall을 직렬화한다. 동시 host
실행이 한 immutable generation만 관측한다는 보장은 없다.
Unix 프로세스가 강제 종료되어 `.hancom-install.lock`이 남으면 실행 중인 설치기가
없는지 먼저 확인한 뒤 그 정확한 빈 디렉터리만 수동으로 제거한다.

```bash
scripts/install.sh
```

Unix에서는 `dump-reader/hwp/plugin`, `format-handler/hwpx/plugin`,
`dump-reader/cell/plugin`, `dump-reader/show/plugin`에 실제 바이너리를 설치한다.
`dump-reader/hml/plugin`은 `../hwp/plugin`, `format-handler/owpml/plugin`은
`../hwpx/plugin` 상대 심볼릭 링크다. 여섯 활성
경로를 항상 함께 설치·제거하며 별도의 포맷 선택 옵션은 없다.

```bash
scripts/install.sh --no-build    # 이미 빌드된 바이너리 사용
scripts/install.sh --uninstall   # 제거
scripts/install.sh --print-env   # 환경변수 방식 안내 (1순위 경로)
```

`--print-env`는 역할/확장자별 바이너리를 가리키는 다음 여섯 설정을 출력한다.

```bash
OFFICECLI_PLUGIN_DUMP_READER_HWP
OFFICECLI_PLUGIN_DUMP_READER_HML
OFFICECLI_PLUGIN_FORMAT_HANDLER_HWPX
OFFICECLI_PLUGIN_FORMAT_HANDLER_OWPML
OFFICECLI_PLUGIN_DUMP_READER_CELL
OFFICECLI_PLUGIN_DUMP_READER_SHOW
```

확인:

```bash
officecli plugins list
```

OfficeCLI는 정규화된 실행 경로별로 플러그인을 열거하므로 같은 매니페스트가
두 행으로 보일 수 있다. 내용까지 같은 등록은 이름 기반 명령에서 첫 discovery
경로를 쓰고, 같은 이름의 매니페스트 내용이 다르면 각 행에 경고한 뒤
`plugins info`/`lint <name>`을 모호성 오류로 거부한다. 이때는 목록의 절대
실행 경로를 명시한다. 이름 기반 `plugins info` 재-probe의 전체 매니페스트가
최초 snapshot과 달라져도 `plugin_manifest_changed`로 거부하며, 명시 경로는
한 번만 probe한다. 전체 열거는 후보 256개, 후보 manifest 1MiB, 정상 manifest
합계 16MiB, probe 합계 30초를 넘으면 부분 목록 없이 실패한다. 실제 확장자
해석은 아래의 `officecli view`로 검증한다.

Windows PowerShell에서는 네이티브 `.exe`를 사용자 플러그인 경로에 설치한다.

```powershell
.\scripts\install.ps1
.\scripts\install.ps1 -NoBuild
.\scripts\install.ps1 -Uninstall
.\scripts\install.ps1 -PrintEnv
```

설치 위치는 다음 여섯 곳이다.

```text
$HOME\.officecli\plugins\dump-reader\hwp\plugin.exe
$HOME\.officecli\plugins\dump-reader\hml\plugin.exe
$HOME\.officecli\plugins\format-handler\hwpx\plugin.exe
$HOME\.officecli\plugins\format-handler\owpml\plugin.exe
$HOME\.officecli\plugins\dump-reader\cell\plugin.exe
$HOME\.officecli\plugins\dump-reader\show\plugin.exe
```

Windows에서는 심볼릭 링크 권한에 의존하지 않고 역할별 바이너리를 해당 두 경로에
각각 복사하고 Cell/Show 전용 바이너리는 각 한 경로에 복사한다. 여섯 임시
복사본의 SHA-256과 `--info`의 name/protocol/exact kind/exact extensions/
exact target을 먼저 검증한 뒤 교체하고, 중간 실패 시 확장자별 커밋 상태를
역순으로 되돌린다. 경로를 순차 교체하므로 프로세스 강제 종료까지 포함한 완전한
다중 경로 원자성을 보장하지는 않는다.

두 설치기는 절대 `HOME` 아래 `.officecli`부터 기존 관리 경로 조상을 먼저 검사하고,
symlink/junction/reparse point 또는 디렉터리가 아닌 component가 있으면 설치와
제거를 모두 중단한다. 같은 권한의 프로세스가 검사 직후 경로를 바꾸는 경쟁까지
없애는 handle 기반 설치기는 아니므로, 설치 중에는 `HOME`을 신뢰 경계로 둔다.
각 target 복원은 앞선 정리 실패와 무관하게 끝까지 시도한다. rollback 전에 현재
파일의 hash/형태가 installer가 둔 값인지 다시 확인하므로, 동시 외부 변경을
덮어쓰지 않는다. 이 경우 복구용 백업 위치를 경고로 남긴다.
Windows 제거는 resident 종료 직후 남을 수 있는 image lock을 최대 20회 × 250ms
재시도한다. 각 시도마다 directory와 target의 reparse 경계를 다시 확인하며,
영구 권한 오류는 5초 안에 최종 실패한다.

## 포맷 판별

확장자를 믿지 않고 매직 바이트로 판별한다. `.hwp`인데 실제로는 HWPX인 파일이
흔하고 반대도 있다.

| 포맷 | 처리 |
|---|---|
| HWPX/OWPML (ZIP + OWPML) | 직접 읽는다 |
| HWPML (`.hml`, 단일 XML) | 문단·문자·기본 스타일·표 공통 부분집합을 직접 읽는다 |
| HWP 5.x (CFB) | RHWP가 있으면 임시 HWPX로 변환 후 처리, 없으면 exit 3 |
| HWP 3.0 | RHWP가 있으면 임시 HWPX로 변환 후 처리, 없으면 exit 3 |
| Cell 12.0300 (검증된 ZIP/OOXML profile) | 전체 package 검증 후 byte-identical `.xlsx` 형제 생성 |
| Show 12.0000 (검증된 ZIP/OOXML profile) | 전체 package 검증 후 byte-identical `.pptx` 형제 생성 |
| `.nxl`, CFB Cell/Show, 다른 생산자 build | exit 3; 구조를 추측하지 않음 |
| 그 밖 (`.docx` 등) | exit 2와 원인 명시 |

바이너리 HWP 지원 계획은 `docs/04-hwp-support-plan.md`.

RHWP를 PATH에 두거나 절대 실행파일 경로를 지정한다. RHWP가 없으면
`.hwp` 호출은 exit 3(`unsupported_feature`)으로 종료하지만 세 XML 기반 형식은
계속 외부 런타임 없이 동작한다.

통합 진입점과 HWPML 강화 커밋 `17b65ea5`는 GitHub Actions run
[`33170785021`](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33170785021)에서
Linux/Windows 테스트·clippy·release·MSRV 1.88·host 계약을 통과했다. 네 확장자의
실제 설치·조회·제거는
[`33172696561`](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33172696561)에서
모두 통과했다.

```bash
rhwp --version  # v0.8.4 이상
export OFFICECLI_HWPX_CONVERTER=/absolute/path/to/rhwp
officecli-hancom-hwp dump 문서.hwp
# 주의: 원본 옆에 문서.docx를 만든다. 복사본으로 검증할 것.
OFFICECLI_HWPX_CONVERTER=/absolute/path/to/rhwp officecli view 문서.hwp text
```

RHWP 바이너리는 [공식 릴리스](https://github.com/edwardkim/rhwp/releases)에서
받고 함께 제공되는 SHA-256 체크섬을 확인한다. 플러그인은 shell을 쓰지 않으며,
원본을 private scratch의 UTF-8 고정명으로 staging한 뒤 결과를 HWPX로 다시
판별한다. staging copy는 256MiB로 제한한다. 변환기 부재는 exit 3,
변환 실패·잘못된 산출물은 exit 2다. Unix scratch는 `0700`/staged source는
`0600`이고, Windows scratch는 owner와 LocalSystem만 허용하는 protected DACL로
원자 생성한다. RHWP v0.8.4 제약상 변환기 실행파일 경로도 Unicode여야 한다.
Windows에서는 공유 junction/재지정 공격을 피하기 위해 `--media-dir` 대신 사용자별
OS 임시 루트의 보호된 하위 디렉터리를 사용한다.

진단:

```bash
cargo run --release --example detect -- 문서1.hwp 문서2.hwpx
#   hwp5   문서1.hwp   version=5.1.0.1 compressed=true protection=none
#   hwpx   문서2.hwpx
```

## 직접 실행

```bash
# dump-reader 매니페스트와 format-handler 매니페스트
officecli-hancom-hwp --info
officecli-hancom-hwpx --info
officecli-hancom-cell --info
officecli-hancom-show --info

# dump-reader 변환 결과 보기
officecli-hancom-hwp dump /path/to/문서.hwp
officecli-hancom-hwp dump /path/to/문서.hml
officecli-hancom-cell dump /path/to/표본.cell
officecli-hancom-show dump /path/to/표본.show

# HWPX parser/emit 경로를 직접 진단할 때만 명시 실행
officecli-hancom-hwp dump 문서.hwpx --quiet

# 설치된 format-handler를 통한 직접 조회·텍스트 편집
officecli view 문서.hwpx text
officecli get 문서.hwpx '/document/section[1]/paragraph[1]/text[1]'
officecli set 문서.hwpx '/document/section[1]/paragraph[1]/text[1]' --prop 'text=새 텍스트'
officecli save 문서.hwpx
officecli close 문서.hwpx
officecli validate 문서.hwpx
```

`set`은 자동 resident 세션에 반영될 수 있다. 다른 프로그램이 디스크 파일을 즉시 읽어야
하면 `save`로 flush하고, 새 세션 재열기까지 확인하려면 위처럼 `close`한 뒤 다시 `view`한다.

네 플러그인의 표준출력은 각 프로토콜 전용이다. HWP/HML은 JSONL을 내보내고,
Cell/Show는 성공 시 stdout에 바이트를 하나도 쓰지 않고 native 형제를 직접
no-clobber 원자 커밋한다. 진단은 stderr로 나간다. 호스트는 매니페스트가
`direct-native`와 `byte-preserving`을 모두 선언한 경우 매번 플러그인을 호출하며,
exit 0 + raw stdout 0 bytes + 현재 source와 byte-identical인 non-reparse sibling만
성공으로 인정한다. BOM·공백·빈 줄·JSONL도 계약 위반이고, 서로 다른 기존 sibling은
플러그인 실행 전에 충돌로 거부한다. 실패한 실행 뒤에는 호스트가 소유권을 증명할 수
없는 sibling 경로를 삭제하지 않는다. 플러그인은 게시 전 private candidate만 정리하며,
이미 게시된 sibling은 보존된다.
`dump-reader`의 직접 HWPX 입력은 RHWP 브리지와 DOCX projection을 시험하기 위한
명시적 진단 경로일 뿐, 매니페스트에는 `.hwpx`/`.owpml`을 광고하지 않는다.

### Cell/Show modern OOXML carrier 경계

공개기관의 Cell 12.0300 한 개와 Show 12.0000 세 개만 지원 profile의 근거다.
이 marker들은 누구나 작성할 수 있으므로 한컴이 만들었다는 인증 수단이 아니다.
소스 512MiB, ZIP 4,096 entries, entry 64MiB, 누적 expanded 256MiB, XML 16MiB,
압축비 1,000:1과 XML event/name/attribute/namespace/depth 예산을 넘으면 실패한다.
local/central ZIP header와 실제 expanded length를 대조하고, 모든 entry CRC와
XML/rels를 끝까지 읽는다. 경로 충돌·symlink/special entry·암호화·미지원 압축·
DTD/PI·잘못된 선언/UTF-8/XML 1.0 문자·미선언 namespace를 거부한다. 모든 내부
relationship target은 존재해야 하며 전체 non-directory part가 root relationship에서
도달해야 한다. 이 closure의 분모에는 `[Content_Types].xml`, relationship part와 일반
internal part가 들어가고 directory entry와 external URI는 들어가지 않는다. 관측된
relationship/content-type만 허용하며 VBA/XLM macro part, ActiveX, OLE·embedded package,
external data/media 및 허용되지 않은 action/relationship class를 거부한다. Show의 bounded
HTTPS hyperlink는 보존한다. 따라서 arbitrary Cell formula·defined name이나 presentation
action을 inert하다고 인증하지 않으며, 소비자 보안 설정을 대신하지 않는다.

검증은 private finalized candidate의 retained file identity에서 파생한 같은 바이트로
수행한다. source primary stream의 hash/size/mtime 또는 경로 정체성이 copy부터 commit
사이 바뀌면 게시하지 않는다. fresh sibling에는 primary/default stream의 정확한 바이트와
source mtime을 보존한다. Windows는 read-only, `Zone.Identifier`, canonical DACL
rule/protected state를 복사·검증하고 EFS source와 그 외 ADS는 fail-closed한다. retained
Windows source handle은 read sharing만 허용해 이 path 기반 ADS/DACL 검사 동안 write/delete
open과 rename/replacement를 막는다. Linux는
retained descriptor에서 plugin credential로 보이고 읽을 수 있는 complete bounded xattr
set과 mode를 보존한다. macOS는 여기에 extended ACL을 별도 복사·검증한다. 열거·읽기·적용·
검증 실패는 게시 전에 fail-closed하고, cached sibling도 이 열거된 항목이 모두 맞아야
재사용한다. Windows owner/SACL/MIC, Unix UID/GID, process-invisible attribute, creation time,
hard-link identity, allocation/compression layout 등 file-object 전체 동일성을 보장하지 않는다.

Show 공개 표본의 잘못된 `0x5455` extended timestamp는 Show profile 후보에서
flags=2, 길이=13, 동일 timestamp 3회인 정확한 모양만 익명 검증 복사본에서
neutralize한다. 원본과 결과는 그 바이트를 그대로 유지한다. 상세 근거와 공개 표본
해시는 [ADR-0016](../../docs/adr/0016-hancom-v12-ooxml-carrier-bridge.md)에 있다.

## 커버리지

| HWPX/OWPML | docx 매핑 |
|---|---|
| 문단 (`hp:p`) | `add /body --type paragraph` |
| 글자 런 (`hp:run`) | `add /body/p[last()] --type run` |
| 굵게/기울임/밑줄/취소선 | `bold` / `italic` / `underline` / `strike` |
| 글자 크기·색·글꼴 | `size` (pt) / `color` (#RRGGBB) / `font` |
| 위·아래 첨자 | `superscript` / `subscript` |
| 문단 정렬 | `align` |
| 들여쓰기·문단 여백·줄간격 | `indent` / `firstLineIndent` / `spaceBefore` / `spaceAfter` / `lineSpacing` |
| 문단 내 줄바꿈 (`hp:lineBreak`) | `\v` (soft break) |
| 탭 (`hp:tab`) | `\t` |
| 표 (`hp:tbl`) | `add /body --type table --prop rows/cols` + 셀별 `set` |
| 셀 병합 (`hp:cellSpan`) | `colspan` / `vmerge` |
| 셀 배경색 | `fill` |
| 열 너비 (`hp:cellSz`) | `colWidths` (twip) |
| 이미지 (`hp:pic` + `BinData`) | `add --type picture --prop src=data:...` |
| 각주/미주 (`hp:footNote` / `hp:endNote`) | 참조 위치의 실제 `footnote` / `endnote` + 주석 본문 블록 |
| 구역별 각주/미주 정책 | 번호 형식·재시작·시작·배치 + 동적 참조의 접두/접미·위첨자 |
| 머리말/꼬리말 (`hp:header` / `hp:footer`) | 구역별 `header` / `footer` default·even·first story |
| 페이지 번호 (`hp:autoNum` PAGE/TOTAL_PAGE) | 동적 `PAGE` / `NUMPAGES` field |
| 목록 번호 (`hh:numbering` / `hh:bullet` / 구역 개요) | 동적 `abstractNum` + `num` + 문단 `numId`/`numLevel` |
| 이름 스타일 (`hh:styles` + `styleIDRef`) | 활성 `style` 정의 + 문단 `style`, 직접 서식 override 유지 |
| 수식 (`hp:equation`) | `equation` (`formula`=LaTeX, `mode`=inline/display) |
| 사각형·글상자·전체 타원 | 검증된 inline/floating `shape`/`textbox`, 구조적 글상자 본문·설명 |
| 자체완결 차트 (`hp:chart` + `Chart/*.xml`) | strict raw chart part + native-verified floating drawing |
| 폼 체크박스 (`hp:checkBtn`) | `add --type formfield --prop type=checkbox --prop checked=...` |
| 누름틀 (`hp:fieldBegin type="CLICK_HERE"`) | 빈 슬롯 → `formfield type=text` / 내용 있으면 서식 유지 텍스트 |
| 내어쓰기 (음수 `hc:intent`) | `hangingIndent` (docx는 음수 `firstLine`을 허용하지 않음) |
| 중첩표 | `add <cell> --type table` — 실제 중첩 `<w:tbl>` |
| 한컴 PUA 문자 | 그대로 보존 + 개수를 진단으로 보고 (매핑 추측 안 함) |
| 셀 안 여러 문단 | 첫 문단은 `set <cell>/p[1]`, 이후는 `add <cell> --type paragraph` |
| 다중 섹션 | `content.hpf` spine 순서로 연결 |

각주/미주는 필수 `hp:subList`의 여러 문단·런 서식·표·이미지를 구조적으로
보존한다. 중첩 주석이나 `subList` 밖의 내용은 조용히 버리지 않고 손상 입력으로
거부한다. 구역별 번호 형식·재시작·시작 번호·배치와 접두/접미 문자·위첨자는
동적 DOCX 참조로 보존한다. `userChar` 사용자 표식은 자동 번호 의미를 유지할 수 없어
exit 3으로 거부한다. DOCX에 대응 구역 속성이 없는 `noteLine`/`noteSpacing`은 정확히
파싱한다. 해당 종류의 실제 주석이 있으면 부분 stdout 없이 exit 3, 주석이 없으면 원본
값을 담은 필수 구조화 경고를 stderr로 낸 뒤 변환한다(`--quiet`도 이 경고는 숨기지 않는다).

머리말/꼬리말은 `BOTH`/`ODD`/`EVEN`, 첫 페이지 숨김, 여러 구역과 내부 문단·표·이미지·
동적 페이지 필드를 보존한다. 한 구역 안에서 본문 뒤에 활성화되거나 같은 슬롯이 겹치는
timeline은 DOCX 한 구역으로 넓히지 않고 exit 3으로 거부한다.

번호·글머리표·구역 개요는 표시 문자열로 고정하지 않고 한컴 정의를 동적 DOCX 목록으로
보존한다. 공식 `^n`/`^N`/`^1`~`^9` 토큰과 검증된 숫자·로마자·라틴·한글 형식만
허용한다. 활성 이미지/체크형 글머리표, 미지원 형식, 검증되지 않은 배치 값은 stdout 전에
exit 3으로 거부한다. PUA 표식은 G6 정책대로 치환을 추측하지 않고 그대로 보존·진단한다.

이름 스타일은 본문·표·주석·머리말/꼬리말이 실제 참조하는 PARA 스타일과 다음 스타일
의존성만 만든다. 숫자 ID와 기본 이름, 문단/런 속성, `nextStyleIDRef`, NUMBER/BULLET 및
구역 OUTLINE을 보존하면서 문단의 직접 `paraPrIDRef`·런 서식은 별도 override로 남긴다.
한컴 네이티브 결과에 따라 `lockForm`을 Word `locked`로 추측하지 않으며, 한 스타일이
구역마다 다른 개요 정의를 요구하는 경우에는 부분 출력 없이 exit 3으로 거부한다.
세부 경계는 [`../../docs/adr/0009-hancom-named-style-policy.md`](../../docs/adr/0009-hancom-named-style-policy.md)에 기록했다.

도형은 공식 r1.2와 한컴 native DOCX가 함께 증명한 축 정렬 rectangle/rounded rectangle,
구조적 textbox, whole ellipse만 보존한다. inline과 검증된 page-floating 배치, wrap/flow,
거리·z-order·겹침, 단색/무채움/선과 `shapeComment` 설명을 유지한다. line/custom path/
container/OLE 등은 근사하거나 누락하지 않고 exit 3으로 거부한다. 세부 경계는
[`../../docs/adr/0010-hancom-shape-and-textbox-policy.md`](../../docs/adr/0010-hancom-shape-and-textbox-policy.md)에 기록했다.

차트는 관계와 외부 자원이 없는 자체완결 UTF-8 `c:chartSpace`만 raw part로 보존한다.
OWPML frame은 native로 검증된 `SQUARE/BOTH_SIDES`, floating
`COLUMN/PARA TOP/LEFT`, zero-offset profile이어야 한다. 기본 raw carrier는 schema-valid
XML만 무변경 수용하며, 한컴 parser가 명시하는 `hwpxChartOrderRepairV1`만 제한된
catAx/valAx/view3D child-order 오류를 고친 뒤 validation 오류 0을 요구한다. 세부 경계는
[`../../docs/adr/0011-hancom-chart-carrier-policy.md`](../../docs/adr/0011-hancom-chart-carrier-policy.md)에 기록했다.

PUA는 글꼴별 표를 전역 mapping으로 추측하지 않고 한컴 native DOCX와 같이 그대로
보존하며 개수를 진단한다. exact source font identity와 glyph oracle이 없는 치환은 다른
문자로 의미를 바꿀 수 있다. 재평가 근거와 future opt-in 조건은
[`../../docs/adr/0012-hancom-private-use-character-policy.md`](../../docs/adr/0012-hancom-private-use-character-policy.md)에 기록했다.

수식은 공식 수식 형식 r1.3을 기준으로 분수·근호·첨자·주요 연산자와 함수·적분·행렬·
cases/pile/alignment·색상을 OfficeCLI LaTeX로 옮기며, OfficeCLI가 네이티브 OMML로 만든다.
인라인/표시 배치와 문단·표 셀 안의 형제 순서를 보존한다. 의미를 근사해야 하는
`LONGDIV`/`LADDER`/`SCALE` 계열과 호스트가 표현하지 못하는 일부 big/small 변형은 exit 3,
손상 구문·알 수 없는 명령·자원 한계 초과는 exit 2로 전체 변환을 중단한다. 어느 경우에도
앞선 문단의 부분 JSONL을 stdout에 남기지 않는다.

HWPML은 공식 2.8 문법의 공통 경로(`HWPML/BODY/SECTION/P/TEXT/CHAR`)와
2.1/2.8/2.9/2.91 상호운용 허용 목록을 사용한다. 이 목록은 각 버전의 전체 문법
지원을 뜻하지 않는다. `TAB`, `LINEBREAK`, `NBSPACE`, 기본 글자·문단 모양, 표는
보존한다. 의미를 안전하게 투영할 수 없는 문자 제어와 내용이 있는 미지원 컨트롤은
부분 출력을 만들지 않고 exit 3으로 실패한다. 잘못된 네임스페이스·ID·표 구조·XML은
exit 2이며 DTD는 엔티티를 확장하지 않고 exit 3이다.

### 아직 안 되는 것

- line/polygon/curve/connectLine/container/OLE/textart/arc/video, 회전·flip·group·보호·
  hyperlink·caption 등 검증되지 않은 도형 profile
- caption/TOP_AND_BOTTOM, 관계·외부 데이터·embedded workbook이 있는 차트와 검증되지
  않은 차트 배치
- plain 텍스트 치환을 넘는 HWPX 구조 편집(`add/remove/move/copy/raw-set/add-part`)
- 편집 gate를 통과하지 못한 비정규·불완전 HWPX의 저장(조회는 별도 관대한 경계)
- `.nxl`, CFB/legacy Cell·Show, Cell 12.0300/Show 12.0000 외 생산자 build
- Cell/Show source write-back·생성·정규화, 모든 세대 호환 또는 별도 export와의
  일반적 의미/바이트 동등성

## 개발

```bash
cargo test --workspace --locked --all-targets       # 공용 core + 플랫폼별 전용 검사
cargo test -p officecli-hwpx --test parse_owpml     # OWPML 파싱 107개
cargo test -p officecli-hwpx --test parse_hwpml     # HWPML 파싱 44개
cargo test -p officecli-hwpx --test protocol_contract # 프로토콜 계약 E2E 64개
cargo test -p officecli-hwpx --test hwpx_format_handler
cargo test -p officecli-hwpx --test ooxml_carrier
cargo test -p officecli-hwpx --test install_contract
cargo test -p officecli-hwpx --test golden          # 골든파일 회귀 3개
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo build --workspace --locked --release
```

### 실제 문서 코퍼스 회귀

```bash
HWPX_CORPUS=~/hwpx-corpus scripts/verify-corpus.py --update   # 기준선 생성
HWPX_CORPUS=~/hwpx-corpus scripts/verify-corpus.py            # 회귀 검증
```

실제 한글 문서로 회귀 검증한다. 합성 픽스처가 잡지 못하는 것을 잡는다 —
지금까지 나온 버그 14개 중 9개가 실제 문서에서만 드러났다.

문서 원본은 저장소에 넣지 않고 `HWPX_CORPUS` 경로로 받는다(개인정보·배포조건).
기대 요약만 `tests/corpus/expected.json`에 커밋한다.

### 실제 officecli로 왕복 검증

```bash
scripts/verify-roundtrip.sh                         # PATH의 current host 사용
OFFICECLI=/absolute/path/to/officecli scripts/verify-roundtrip.sh
```

두 kind와 여섯 활성 경로의 디스커버리, format-handler 조회·텍스트 저장·재열기·검증,
Cell/Show direct-native sibling의 byte/source-metadata 보존과
dump-reader의 `plugins lint` → JSONL → DOCX replay → 서식·표·병합·이미지·
체크박스·내어쓰기를 실제 current `officecli` 호스트로 확인한다. 승격 전
v1.0.145 릴리스는 필요한 format-handler lifecycle 계약이 없어 지원하지 않는다.

`plugins lint`는 우리가 emit한 모든 prop을 **바이너리에 내장된 대상 포맷 스키마**로
검사한다. 어휘 매핑을 기계적으로 보증하는 가장 강한 수단이므로, 어휘를 건드리면
반드시 다시 돌린다.

골든파일 갱신 (diff를 반드시 눈으로 검토):

```bash
UPDATE_GOLDEN=1 cargo test --test golden
```

### 구조

```
crates/
  hancom-core/
    src/container.rs    CFB/ZIP 매직바이트 판별
    src/model.rs        공용 문서모델 + 단위 변환
    src/emit/           BatchItem 직렬화 + docx JSONL emitter
    src/budget.rs       overflow-safe 공용 자원예산
    src/diagnostics.rs  단일행·터미널 안전 진단 경계
    src/heartbeat.rs    호스트 watchdog heartbeat
    src/error.rs        공용 에러 → 종료코드 매핑
    tests/core_contract.rs 공용 경계 계약 테스트
  hancom-hwp/
    src/bin/officecli-hancom-hwp.rs HWP/HML dump-reader 진입점
    src/bin/officecli-hancom-hwpx.rs HWPX/OWPML format-handler 진입점
    src/bin/officecli-hancom-cell.rs Cell 12.0300 OOXML carrier 진입점
    src/bin/officecli-hancom-show.rs Show 12.0000 OOXML carrier 진입점
    src/bin/officecli-dump-reader-hwpx.rs 하위 호환 진입점
    src/format_handler.rs bounded JSONL 세션과 command vocabulary
    src/converter.rs    선택적 RHWP HWP→HWPX 변환 경계
    src/hwpml.rs        legacy HWPML 단일 XML 리더
    src/ooxml_carrier.rs Cell/Show ZIP·profile 검증과 byte-copy commit
    src/lib.rs          인자 파싱 + 명령 디스패치
    src/manifest.rs     dump-reader --info 매니페스트 (§4)
    src/{format,error,emit}/ 공용 core 임시 호환 re-export
    src/owpml/
      package.rs        ZIP 컨테이너, content.hpf spine, BinData 해석
      conformance.rs    G0~G3 출력 패키지 검증
      editor.rs         raw-entry COW + strict text mutation
      equation/         수식 스크립트의 엄격 파싱·LaTeX 변환
      styles.rs         header.xml 글자/문단 모양·이름 스타일 표
      section.rs        본문 파싱, 표/문단 구조 평탄화
      model.rs          공용 model 임시 호환 re-export
      xml.rs            quick-xml 헬퍼 (네임스페이스 무시, 엔티티 해제)
    tests/              파서·프로토콜·설치·골든·호환 회귀
scripts/
  install.sh            Unix 실파일 네 개 + HML/OWPML 확장자 링크 설치
  install.ps1           Windows 여섯 활성 경로에 검증된 바이너리 복사
  generate-editable-fixture.py strict editable HWPX/OWPML smoke fixture
  generate-ooxml-carrier-fixture.py profile-compatible seed용 Cell/Show marker fixture
  make_fixture.py       전 기능 HWPX 생성 (Rust 코드와 독립)
  verify-roundtrip.sh   두 프로토콜과 DOCX projection 실제 host 검증
  verify-corpus.py      실제 한글 문서 코퍼스 회귀 검증
  verify-hwp-pairs.py   HWP/HWPX 쌍 JSONL·OfficeCLI 구조 동등성 검증
  verify-large-file.py  합성 대용량 HWPX wall-time/RSS/watchdog 실측
                        (Windows는 RSS를 null/unsupported로 보고)
docs/
  00-seed-review.md      시드 재검토 + 사실검증
  01-protocol-contract.md 확정 계약 + ADR
  02-handover.md         인수인계
  03-work-plan.md        실측 기반 작업 계획 + 진행 상태
  04-hwp-support-plan.md .hwp(바이너리) 지원 계획
```

`hancom-hwp`는 기존 공개 경로를 임시 re-export하면서 `hancom-core`의 공용 모델과
보안 경계를 사용한다. 파서와 emitter는 중간 문서 모델을 사이에 두고 독립적으로 테스트된다.

## 라이선스

MIT
