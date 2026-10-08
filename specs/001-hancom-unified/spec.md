# Feature Specification: 한컴오피스 통합 호환 플러그인

**Feature ID**: `001-hancom-unified` · **Working Branch**: `feat/hwpx-plugin`
**Created**: 2026-08-28
**Status**: 2026-09-30 재검토 — P0~P3 구현과 검증된 Cell/Show carrier slice,
HWPX 구조·그림·원본 위치 조회, 호스트 JSON 출력 수정, ZIP header 바이트 보존 저장의
로컬 검증 완료.
새 리뷰 결함 R-1~R-3은 로컬 수정·회귀 검증 완료이며, legacy/다세대/외부 변환기는 evidence-gated deferred.
**Latest Review**: [2026-09-29 비교 검토와 직접 읽기 개선](../../docs/reviews/2026-09-29-pro-comparison.md)
**Task Plan**: `./task-plan.md` (정본 작업 목록)
**Research**: `../../.agents/brain/research/hancom-unified-20260828.md` (정제된 조사·결정 기록)
**Official Sources**: `../../docs/spec-sources.md` (한컴 원문 URL·리비전·바이트·SHA-256)

**Input**: 이 레포를 한컴오피스 독자 규격(HWP, HWPX부터 한셀, 한쇼까지) 통합 호환
플러그인으로 확장하기 위해 필요한 작업과 도구를 조사하고 task-plan을 만든다.

## 문제

OfficeCLI는 `.docx`/`.xlsx`/`.pptx`만 네이티브로 다룬다. 한국 공공기관·학교·법원의
사실상 표준인 한컴오피스 문서는 플러그인 없이는 AI 에이전트가 손댈 수 없다.
기획 시작 당시에는 HWPX 중심 읽기 전용 dump-reader만 있었다. 현재는 역할별 네
바이너리와 여섯 확장자 설치 경로가 있다. 다음 표는 구현된 부분집합이며 포맷 전체
지원이나 모든 내용의 무손실 변환을 뜻하지 않는다.

| 입력 | 현재 경로 | 쓰기 경계 |
|---|---|---|
| `.hwpx`, `.owpml` | 원본 직접 format-handler | G0~G3를 통과하는 plain text 치환만 |
| `.hml` | HWPML 공통 부분집합 → DOCX JSONL | 원본 읽기 전용 |
| 바이너리 `.hwp` | 선택적 RHWP → 임시 HWPX → DOCX JSONL | 원본 읽기 전용 |
| `.cell` 12.0300 | 검증된 OOXML → byte-identical XLSX sibling | 원본 읽기 전용 |
| `.show` 12.0000 | 검증된 OOXML → byte-identical PPTX sibling | 원본 읽기 전용 |
| legacy/기타 세대, `.nxl` | 근거 확보 전 명시적 미지원 | 미지원 |

## 사용자 시나리오

### User Story 1 — 한글 문서를 에이전트가 읽고 편집 (Priority: P1)

사용자가 `.hwpx`/`.hwp`/`.owpml`/`.hml` 문서를 주면 에이전트가 내용을 구조적으로
읽고, 지원 부분집합의 표·이미지·각주·수식을 조회하며, HWPX/OWPML에서는 허용된
텍스트를 편집한 결과를 받는다. DOCX projection reader와 직접 format-handler의
조회 능력은 동일하지 않다. 직접 조회는 `hp:t`의 혼합 텍스트·CDATA와 `hp:tab` /
`hp:lineBreak`를 보존한다. 혼합 텍스트 노드는 읽기 전용이며 plain text 쓰기 경계를
확장하지 않는다. `//document`, `//section`, `//paragraph`, `//text`는 타입 조회 별칭이다.
2026-09-29 확장에서는 표·셀·각주/미주·필드 마커와 XML 원본 위치·해시·편집 가능
텍스트 경로를 추가한다. `//table`, `//cell`, `//note`, `//field`도 동일하게 조회한다.
2026-09-30에는 `//picture`로 그림의 binary 참조와 원문 크기를 읽기 전용으로 공개한다.
기존 문단 경로를 재배치하지 않으며 중복 이름과 누락 좌표를 추측하지 않는다.
상세 계약은 [C13](../../plugins/hancom/docs/01-protocol-contract.md#c13-hwpx-구조-조회와-원본-참조)이다.

**Why P1**: 한컴오피스 사용량의 대부분이 한글이다. 이미 부분 구현되어 있어 완성까지가
가장 짧고, 공개 스펙(R1)과 오픈소스 선행 기술이 풍부해 확실히 달성 가능하다.

**Independent Test**: `officecli view 문서.hwpx text`와 구조 `query`가 원본 연결을
반환한다. 미구현 `outline`/`issues`는 명시적으로 실패하고 `validate`와 구분된다.
명시적 텍스트 편집·저장·재열기에서 변경이 유지되며 오래된 revision은 적용되지 않는다.

**Acceptance Scenarios**:

1. **Given** 각주·수식·머리말이 있는 `.hwpx`, **When** `officecli view ... annotated`,
   **Then** 지원 내용이 누락 없이 출력에 나타난다. 직접 format-handler의 본문 인덱스만으로
   이 시나리오 전체가 충족됐다고 보지 않으며, projection과 별도 조회 회귀로 확인한다.
2. **Given** `.hwp` 바이너리 문서, **When** 변환기 없이 열기, **Then** exit 3과 원인 명시.
3. **Given** `.hwpx`, **When** 지원 편집 후 `save`, **Then** package 안전·보존,
   UTF-8/XML 안전성, container/HPF topology, 별도 reader 재열기, semantic delta,
   unchanged-part hash, durable replacement 게이트를 모두 통과한다. 특정 DVC 정책을
   제품이 채택한 경우에만 그 이름·commit·정책 SHA에 대한 별도 smoke를 통과한다.

---

### User Story 2 — 한셀/한쇼 문서를 읽기 (Priority: P2)

사용자가 `.cell`/`.show` 문서를 주면 에이전트가 시트 데이터 / 슬라이드 내용을 읽는다.

**Why P2**: 관측한 modern 표본의 OOXML 구조를 활용할 수 있다. proprietary/legacy
세대는 규격·표본·oracle을 확보하지 못했으므로 current carrier 범위와 분리한다.

**Independent Test**: `officecli view 장부.cell text`가 셀 값을 반환한다.

**Acceptance Scenarios**:

1. **Given** 지원 profile 밖 `.cell`/`.show`, **When** 열기, **Then** exit 3과
   미지원 원인 명시. 손상 입력은 exit 2. **조용히 틀린 데이터를 내지 않는다.**
2. **Given** 검증된 Cell 12.0300 / Show 12.0000 OOXML carrier, **When** 열기,
   **Then** 원본과 byte-identical한 `.xlsx`/`.pptx` sibling이 생성되고 호스트가 읽는다.
   서로 다른 기존 sibling은 덮어쓰지 않는다.
3. **Given** `.cell` 파일, **When** 어떤 경로로든 열기, **Then** 원본 해시·mtime 불변.

외부 변환기 환경변수와 proprietary parser는 구현된 acceptance가 아니라
task-plan T4-4/T5-3 등의 보류 항목이다.

---

### User Story 3 — 설치가 한 번에 끝난다 (Priority: P3)

사용자가 설치 스크립트를 한 번 실행하면 6개 이상의 한컴 확장자가 모두 인식된다.

**Why P3**: 기능이 없으면 의미가 없으므로 P1/P2 뒤. 다만 확장자별 디스커버리 경로가
따로 필요한 호스트 구조(제약 4) 때문에 별도 설계가 필요하다.

**Acceptance Scenarios**:

1. **Given** 새 머신, **When** 설치 스크립트 실행, **Then** `.hwpx .owpml .hml .hwp .cell
   .show`가 각 지원 부분집합에서 `officecli view`로 실제 해석된다.
2. **Given** 설치 중 실패, **When** 롤백, **Then** 여섯 활성·두 폐기 경로를
   conflict-safe best effort로 복원하고 실패·백업 위치를 알린다. 다중 경로 전체의
   crash atomicity는 보장하지 않는다(ADR-0014/0016).

## 요구사항

### 기능 요구사항

- **FR-1** 확장자를 신뢰하지 않고 매직바이트로 컨테이너를 판별한다 (`.hwp`인데 실제로는
  HWPX인 파일이 흔하다 — 기존 구현이 이미 이 원칙을 따른다).
- **FR-2** HWP/HML dump는 DOCX, Cell/Show carrier는 XLSX/PPTX로 매핑한다.
  HWPX/OWPML format-handler는 원본을 직접 연다.
- **FR-3** dump/변환의 미지원 기능은 exit 3 + 원인 명시. 세션 명령은 JSONL error
  envelope로 응답한다. 추측 파싱과 내용 누락을 정상 성공으로 보고하는 동작은 금지한다.
- **FR-4** dump/변환의 손상 입력은 exit 2. 읽기 경로는 원본을 보존한다.
  사용자가 명시한 HWPX/OWPML 편집만 검증 후 저장할 수 있다.
- **FR-5** HWP/HML stdout은 BatchItem JSONL, format-handler stdout은 요청/응답 JSONL이다.
  Cell/Show direct-native 성공은 stdout 정확히 0바이트다. 진단은 stderr/지원하는 로그
  경로로 보낸다. JSONL은 UTF-8 no BOM, `\n` 개행이다.
- **FR-6** 한컴 공개 스펙 참조 표기를 UI·매뉴얼·도움말·소스에 모두 기재한다 (법적 의무).
- **FR-7** 외부 변환기는 shell 없이 실행하고 private scratch staging, 자원 예산,
  타임아웃, 프로세스 트리 정리를 적용한다 (기존 RHWP 브리지 계약 재사용).

### 비기능 요구사항

- **NFR-1** HWPX 경로는 런타임 외부 의존 0. 단일 정적 바이너리.
- **NFR-2** 대용량 입력에서 스트리밍 출력 + 10초 heartbeat로 호스트 watchdog 준수.
- **NFR-3** ZIP/XML 폭탄, 경로 탈출, 심볼릭/하드 링크, CFB 순환참조에 대한 회귀 테스트.
- **NFR-4** Linux/Windows/macOS 네이티브 CI 검증.
- **NFR-5** 편집은 source package를 보존하는 COW와 검증된 최소 XML subtree patch만 사용한다.
  전체 semantic model 역직렬화, 미입증 topology 변경, 성공 no-op mutation은 금지한다.
- **NFR-6** writer 검증 결과는 실제 범위대로 명명한다. KS X 6101/XSD 원문 없이
  `표준 적합`, `schema-valid`, `공식 validator`, `무손실 round-trip`을 주장하지 않는다.

## 성공 기준

1. HWP/HML 및 명시적 HWPX dump 진단 경로가 지원 부분집합을 DOCX로 투영한다.
   기본 HWPX/OWPML 열기는 원본 직접 조회·편집이며 DOCX를 생성하지 않는다.
2. `.hwpx` 편집 결과가 ADR-0013의 필수 G0~G3와 `save` durability를 통과하고,
   독립 OWPML/native oracle에서 상호운용된다. DVC는 채택한 named policy에만 적용한다.
3. `.cell`/`.show`가 검증된 carrier 부분집합에서 열리고, 지원 밖 입력은 명시적으로 실패한다.
4. `plugins lint`에서 미지원 prop 0건.
5. 3개 OS 네이티브 러너에서 전 확장자 디스커버리 성공.

## 범위 밖

한셀/한쇼 쓰기, 서식·테마 파일(`.hwt`/`.hcdt`/`.hpt`/`.hsdt`/`.htheme`/`.nxt`),
한컴 상용 SDK 통합, 자체 렌더링, 애니메이션/전환.

## 미해결 질문

`./task-plan.md` Key Questions와 최신 리뷰 R-1~R-3을 참조한다. 관측한 Cell/Show
profile의 컨테이너 판별은 완료했다. 다른 세대 표본, 외부 변환기 계약, named DVC 정책,
독립 native oracle의 추가 증거가 필요한 작업은 각각의 gate가 충족될 때만 재개한다.
