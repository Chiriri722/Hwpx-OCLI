# officecli-hancom-hwp

[OfficeCLI](https://github.com/iOfficeAI/OfficeCLI)용 **한글 문서 dump-reader
플러그인**. `.hwpx`·`.owpml` ZIP과 legacy `.hml` 단일 XML은 직접 읽고,
바이너리 `.hwp`는 선택적 RHWP 변환기를 거쳐 OfficeCLI의 docx 명령(JSONL)으로
변환한다.

Rust 단일 바이너리이며 HWPX/OWPML/HWPML 경로에는 런타임 의존성이 없다.
바이너리 HWP만 선택적으로 [RHWP](https://github.com/edwardkim/rhwp) v0.8.4+를
변환기로 사용한다.

## 한컴 공개 문서 표기

본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.

공개 문서의 원본 URL과 검증된 SHA-256은
[`../../docs/spec-sources.md`](../../docs/spec-sources.md)에 기록한다. PDF 원본은 이
저장소에서 재배포하지 않는다.

## 무엇을 하는가

```
.hwpx/.owpml ──[이 플러그인]──▶ BatchItem JSONL ──[officecli]──▶ .docx
.hml          ──[이 플러그인]──▶ BatchItem JSONL ──[officecli]──▶ .docx
.hwp          ──[RHWP, 선택]──▶ 임시 .hwpx ──[이 플러그인]──────▶ .docx
```

OfficeCLI가 `.hwpx`, `.owpml`, `.hml`, 또는 `.hwp` 파일을 열면 이 플러그인을 `dump`로
실행하고, 표준출력의 명령을 재생해 원본 옆에 `.docx` 형제 파일을
만든다. 편집은 그 `.docx`에 대해 이뤄지며 원본 한글 문서는 읽기
전용으로 취급한다.

## 설치

설치 스크립트는 `.hwpx`·`.hwp`·`.owpml`·`.hml` 네 OfficeCLI 사용자 경로를 한
트랜잭션으로 관리한다. 기존 HWPX 경로만 설치된 환경도 같은 명령으로 네 경로
구성으로 마이그레이션되며, 실패하면 기존 설치본을 복원한다.

```bash
scripts/install.sh
```

Unix에서는 `~/.officecli/plugins/dump-reader/hwpx/plugin`에 실제 바이너리를
설치하고 `hwp`·`owpml`·`hml` 경로에는 각각 `../hwpx/plugin` 상대 심볼릭 링크를
만든다(프로토콜 §3 탐색 순서 2순위). 네 확장자 경로를 항상 함께 설치·제거하며
별도의 포맷 선택 옵션은 없다.

```bash
scripts/install.sh --no-build    # 이미 빌드된 바이너리 사용
scripts/install.sh --uninstall   # 제거
scripts/install.sh --print-env   # 환경변수 방식 안내 (1순위 경로)
```

`--print-env`는 같은 바이너리를 가리키는
`OFFICECLI_PLUGIN_DUMP_READER_HWPX`, `OFFICECLI_PLUGIN_DUMP_READER_HWP`,
`OFFICECLI_PLUGIN_DUMP_READER_OWPML`, `OFFICECLI_PLUGIN_DUMP_READER_HML` 네
설정을 출력한다.

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

설치 위치는 다음 네 곳이다.

```text
$HOME\.officecli\plugins\dump-reader\hwpx\plugin.exe
$HOME\.officecli\plugins\dump-reader\hwp\plugin.exe
$HOME\.officecli\plugins\dump-reader\owpml\plugin.exe
$HOME\.officecli\plugins\dump-reader\hml\plugin.exe
```

Windows에서는 심볼릭 링크 권한에 의존하지 않고 같은 바이너리를 네 경로에
복사한다. 네 임시 복사본의 SHA-256과 `--info`를 먼저 검증한 뒤 교체하고,
중간 실패 시 확장자별 커밋 상태를 역순으로 되돌려 기존 파일을 복원한다. 경로를
순차 교체하므로 프로세스 강제 종료까지 포함한 완전한 다중 경로 원자성을
보장하지는 않는다.

두 설치기는 절대 `HOME` 아래 `.officecli`부터 기존 관리 경로 조상을 먼저 검사하고,
symlink/junction/reparse point 또는 디렉터리가 아닌 component가 있으면 설치와
제거를 모두 중단한다. 같은 권한의 프로세스가 검사 직후 경로를 바꾸는 경쟁까지
없애는 handle 기반 설치기는 아니므로, 설치 중에는 `HOME`을 신뢰 경계로 둔다.
Unix 재설치의 각 target 복원은 앞선 정리 실패와 무관하게 끝까지 시도한다. 새
설치 검증이 끝난 뒤 이전 버전 백업만 지우지 못하면 유효한 설치는 성공으로
유지하고, 복구용 백업 위치를 경고로 남긴다.

## 포맷 판별

확장자를 믿지 않고 매직 바이트로 판별한다. `.hwp`인데 실제로는 HWPX인 파일이
흔하고 반대도 있다.

| 포맷 | 처리 |
|---|---|
| HWPX/OWPML (ZIP + OWPML) | 직접 읽는다 |
| HWPML (`.hml`, 단일 XML) | 문단·문자·기본 스타일·표 공통 부분집합을 직접 읽는다 |
| HWP 5.x (CFB) | RHWP가 있으면 임시 HWPX로 변환 후 처리, 없으면 exit 3 |
| HWP 3.0 | RHWP가 있으면 임시 HWPX로 변환 후 처리, 없으면 exit 3 |
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
# 매니페스트
officecli-hancom-hwp --info

# 변환 결과 보기
officecli-hancom-hwp dump /path/to/문서.hwpx
officecli-hancom-hwp dump /path/to/문서.hml

# 조용히 (진단 출력 없이)
officecli-hancom-hwp dump 문서.hwpx --quiet

# 진단을 파일로
officecli-hancom-hwp dump 문서.hwpx --log-file /tmp/plugin.log
```

표준출력은 JSONL 전용이다. 진단은 stderr 또는 `--log-file`로 나간다.

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

- 도형·글상자 (`hp:rect`, `hp:textart` 등)
- HWPX **쓰기** (그래서 `format-handler`가 아니라 `dump-reader`다 — `docs/01-protocol-contract.md` ADR-1)

## 개발

```bash
cargo test --workspace --locked --all-targets       # 공용 core + 플랫폼별 전용 검사
cargo test -p officecli-hwpx --test parse_owpml     # OWPML 파싱 89개
cargo test -p officecli-hwpx --test parse_hwpml     # HWPML 파싱 44개
cargo test -p officecli-hwpx --test protocol_contract # 프로토콜 계약 E2E 63개
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
scripts/verify-roundtrip.sh --download   # 공식 릴리즈 받아서 검증 (SHA256 대조)
scripts/verify-roundtrip.sh              # PATH/캐시의 officecli 사용
```

디스커버리 → `plugins lint` → 변환 → 서식·표·병합·이미지·체크박스·내어쓰기 왕복까지 43개 항목을
실제 `officecli` 바이너리로 확인한다. officecli는 .NET 없이 도는 단일 바이너리다.

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
    src/bin/officecli-hancom-hwp.rs 통합 진입점
    src/bin/officecli-dump-reader-hwpx.rs 하위 호환 진입점
    src/converter.rs    선택적 RHWP HWP→HWPX 변환 경계
    src/hwpml.rs        legacy HWPML 단일 XML 리더
    src/lib.rs          인자 파싱 + 명령 디스패치
    src/manifest.rs     --info 매니페스트 (§4)
    src/{format,error,emit}/ 공용 core 임시 호환 re-export
    src/owpml/
      package.rs        ZIP 컨테이너, content.hpf spine, BinData 해석
      equation/         수식 스크립트의 엄격 파싱·LaTeX 변환
      styles.rs         header.xml 글자/문단 모양·이름 스타일 표
      section.rs        본문 파싱, 표/문단 구조 평탄화
      model.rs          공용 model 임시 호환 re-export
      xml.rs            quick-xml 헬퍼 (네임스페이스 무시, 엔티티 해제)
    tests/              파서·프로토콜·설치·골든·호환 회귀
scripts/
  install.sh            Unix의 HWPX 실파일 + HWP/OWPML/HML 상대 링크 설치
  install.ps1           Windows 네 확장자 경로에 검증된 plugin.exe 복사
  make_fixture.py       전 기능 HWPX 생성 (Rust 코드와 독립)
  verify-roundtrip.sh   실제 officecli로 43개 항목 왕복 검증
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
