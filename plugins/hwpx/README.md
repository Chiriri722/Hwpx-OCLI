# officecli-hwpx

[OfficeCLI](https://github.com/iOfficeAI/OfficeCLI)용 **HWPX(한글 문서) dump-reader
플러그인**. `.hwpx` 파일을 읽어 OfficeCLI의 docx 명령(JSONL)으로 변환한다.

Rust 단일 바이너리이며 HWPX 경로에는 런타임 의존성이 없다. 바이너리 HWP는
선택적으로 [RHWP](https://github.com/edwardkim/rhwp) v0.8.4+를 변환기로 사용한다.

## 무엇을 하는가

```
.hwpx  ──[이 플러그인]──▶  BatchItem JSONL  ──[officecli]──▶  .docx
.hwp   ──[RHWP, 선택]──▶  임시 .hwpx ──[이 플러그인]──────▶  .docx
```

OfficeCLI가 `.hwpx` 파일을 열면 이 플러그인을 `dump` 서브커맨드로 실행하고,
표준출력으로 흘러나오는 명령을 재생해 원본 옆에 `.docx` 형제 파일을 만든다.
편집은 그 `.docx`에 대해 이뤄진다. 원본 `.hwpx`는 읽기 전용이다.

## 설치

```bash
scripts/install.sh
```

`~/.officecli/plugins/dump-reader/hwpx/plugin`에 설치한다
(프로토콜 §3 탐색 순서 2순위).

```bash
scripts/install.sh --no-build    # 이미 빌드된 바이너리 사용
scripts/install.sh --uninstall   # 제거
scripts/install.sh --print-env   # 환경변수 방식 안내 (1순위 경로)
```

확인:

```bash
officecli plugins list
```

Windows PowerShell에서는 네이티브 `.exe`를 사용자 플러그인 경로에 설치한다.

```powershell
.\scripts\install.ps1
.\scripts\install.ps1 -NoBuild
.\scripts\install.ps1 -Uninstall
.\scripts\install.ps1 -PrintEnv
```

설치 위치는 `$HOME\.officecli\plugins\dump-reader\hwpx\plugin.exe`다.

## 포맷 판별

확장자를 믿지 않고 매직 바이트로 판별한다. `.hwp`인데 실제로는 HWPX인 파일이
흔하고 반대도 있다.

| 포맷 | 처리 |
|---|---|
| HWPX (ZIP + OWPML) | 직접 읽는다 |
| HWP 5.x (CFB) | RHWP가 있으면 임시 HWPX로 변환 후 처리, 없으면 exit 3 |
| HWP 3.0 | RHWP가 있으면 임시 HWPX로 변환 후 처리, 없으면 exit 3 |
| 그 밖 (`.docx` 등) | exit 2와 원인 명시 |

바이너리 HWP 지원 계획은 `docs/04-hwp-support-plan.md`.

RHWP를 PATH에 두거나 절대 실행파일 경로를 지정한다. 현재 매니페스트는 H3
브리지의 크로스 플랫폼 CI가 끝날 때까지 `.hwpx`만 광고하므로, `.hwp` 파일은
플러그인을 직접 실행해 검증한다.

```bash
rhwp --version  # v0.8.4 이상
export OFFICECLI_HWPX_CONVERTER=/absolute/path/to/rhwp
officecli-dump-reader-hwpx dump 문서.hwp
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
officecli-dump-reader-hwpx --info

# 변환 결과 보기
officecli-dump-reader-hwpx dump /path/to/문서.hwpx

# 조용히 (진단 출력 없이)
officecli-dump-reader-hwpx dump 문서.hwpx --quiet

# 진단을 파일로
officecli-dump-reader-hwpx dump 문서.hwpx --log-file /tmp/plugin.log
```

표준출력은 JSONL 전용이다. 진단은 stderr 또는 `--log-file`로 나간다.

## 커버리지

| HWPX | docx 매핑 |
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
| 폼 체크박스 (`hp:checkBtn`) | `add --type formfield --prop type=checkbox --prop checked=...` |
| 누름틀 (`hp:fieldBegin type="CLICK_HERE"`) | 빈 슬롯 → `formfield type=text` / 내용 있으면 서식 유지 텍스트 |
| 내어쓰기 (음수 `hc:intent`) | `hangingIndent` (docx는 음수 `firstLine`을 허용하지 않음) |
| 중첩표 | `add <cell> --type table` — 실제 중첩 `<w:tbl>` |
| 한컴 PUA 문자 | 그대로 보존 + 개수를 진단으로 보고 (매핑 추측 안 함) |
| 셀 안 여러 문단 | 첫 문단은 `set <cell>/p[1]`, 이후는 `add <cell> --type paragraph` |
| 다중 섹션 | `content.hpf` spine 순서로 연결 |

### 아직 안 되는 것

- 각주/미주 (`hp:footNote` / `hp:endNote`)
- 수식 (`hp:equation`)
- 도형·글상자 (`hp:rect`, `hp:textart` 등)
- 머리말/꼬리말
- 목록 번호 매기기 (텍스트는 살지만 `numbering` 구조는 미매핑)
- 스타일 이름 (`styleIDRef` → docx `style`)
- HWPX **쓰기** (그래서 `format-handler`가 아니라 `dump-reader`다 — `docs/01-protocol-contract.md` ADR-1)

## 개발

```bash
cargo test                          # 전체(플랫폼별 전용 검사 포함)
cargo test --lib                    # 단위 검사
cargo test --test parse_owpml       # OWPML 파싱 34개
cargo test --test protocol_contract # 프로토콜 계약 E2E + 플랫폼별 전용 검사
cargo test --test golden            # 골든파일 회귀 3개
cargo clippy --all-targets -- -D warnings
cargo build --release
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
src/
  main.rs           진입점, 종료코드 반환
  format.rs         입력 포맷 판별 (매직 바이트)
  lib.rs            인자 파싱 + 명령 디스패치
  manifest.rs       --info 매니페스트 (§4)
  error.rs          에러 → 종료코드 매핑 (§6.5/§6.6)
  owpml/
    package.rs      ZIP 컨테이너, content.hpf spine, BinData 해석
    styles.rs       header.xml 글자/문단 모양 표
    section.rs      본문 파싱, 표/문단 구조 평탄화
    model.rs        중간 문서 모델 + 단위 변환
    xml.rs          quick-xml 헬퍼 (네임스페이스 무시, 엔티티 해제)
  emit/
    batch.rs        BatchItem 직렬화
    word.rs         문서모델 → docx 어휘
    mod.rs          JSONL 스트리밍 (행별 flush)
tests/
  common/mod.rs         실제 ZIP+OWPML 픽스처 빌더
  parse_owpml.rs        파서 통합 테스트
  protocol_contract.rs  플러그인 바이너리 실행 계약 검증
  golden.rs             전체 파이프라인 회귀
  golden/canonical.jsonl
scripts/
  install.sh            디스커버리 경로에 설치
  install.ps1           Windows 사용자 플러그인 경로에 plugin.exe 설치
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

파서와 emitter는 중간 문서 모델로 분리되어 있어 각각 독립적으로 테스트된다.

## 라이선스

MIT
