# Feature Specification: 한컴오피스 통합 호환 플러그인

**Feature Branch**: `001-hancom-unified`
**Created**: 2026-08-28
**Status**: P3 진행 중 — P0~P2 완료, package-preserving HWPX editor와 출력 검증 게이트 구현 중
**Task Plan**: `./task-plan.md` (정본 작업 목록)
**Research**: `../../.agents/brain/research/hancom-unified-20260828.md` (정제된 조사·결정 기록)
**Official Sources**: `../../docs/spec-sources.md` (한컴 원문 URL·리비전·바이트·SHA-256)

**Input**: 이 레포를 한컴오피스 독자 규격(HWP, HWPX부터 한셀, 한쇼까지) 통합 호환
플러그인으로 확장하기 위해 필요한 작업과 도구를 조사하고 task-plan을 만든다.

## 문제

OfficeCLI는 `.docx`/`.xlsx`/`.pptx`만 네이티브로 다룬다. 한국 공공기관·학교·법원의
사실상 표준인 한컴오피스 문서는 플러그인 없이는 AI 에이전트가 손댈 수 없다.
현재 이 포크에는 `.hwpx`(+실험적 `.hwp`) 읽기 전용 dump-reader 하나만 있고,
한셀(`.cell`)·한쇼(`.show`)는 전혀 지원되지 않는다.

## 사용자 시나리오

### User Story 1 — 한글 문서를 에이전트가 읽고 편집 (Priority: P1)

사용자가 `.hwpx`/`.hwp`/`.owpml`/`.hml` 문서를 주면 에이전트가 내용을 구조적으로
읽고, 표·이미지·각주·수식까지 손실 없이 파악하고, 편집한 결과를 받는다.

**Why P1**: 한컴오피스 사용량의 대부분이 한글이다. 이미 부분 구현되어 있어 완성까지가
가장 짧고, 공개 스펙(R1)과 오픈소스 선행 기술이 풍부해 확실히 달성 가능하다.

**Independent Test**: `officecli view 문서.hwpx text` / `outline` 이 실제 내용을 반환하고,
`view issues`·`validate`가 통과하며, 편집 후 재열기에서 변경이 유지된다.

**Acceptance Scenarios**:

1. **Given** 각주·수식·머리말이 있는 `.hwpx`, **When** `officecli view ... annotated`,
   **Then** 각주·수식·머리말이 모두 출력에 나타난다 (현재는 누락).
2. **Given** `.hwp` 바이너리 문서, **When** 변환기 없이 열기, **Then** exit 3과 원인 명시.
3. **Given** `.hwpx`, **When** 지원 편집 후 `save`, **Then** package 안전·보존,
   UTF-8/XML 안전성, container/HPF topology, 별도 reader 재열기, semantic delta,
   unchanged-part hash, durable replacement 게이트를 모두 통과한다. 특정 DVC 정책을
   제품이 채택한 경우에만 그 이름·commit·정책 SHA에 대한 별도 smoke를 통과한다.

---

### User Story 2 — 한셀/한쇼 문서를 읽기 (Priority: P2)

사용자가 `.cell`/`.show` 문서를 주면 에이전트가 시트 데이터 / 슬라이드 내용을 읽는다.

**Why P2**: 수요는 분명하지만 **공개 스펙·오픈소스 선행 기술이 전무**하다(조사 §4, §6).
GitHub 검색 0건. 비용·불확실성이 P1보다 훨씬 크므로 뒤에 둔다.

**Independent Test**: `officecli view 장부.cell text`가 셀 값을 반환한다.

**Acceptance Scenarios**:

1. **Given** `.cell` 파일, **When** 파서 미완성 상태로 열기, **Then** exit 3 +
   "한셀 내부 구조 미지원" 원인 명시. **조용히 틀린 데이터를 내지 않는다.**
2. **Given** 한컴오피스가 설치된 Windows + 변환기 환경변수 설정, **When** 열기,
   **Then** `.xlsx`로 변환되어 정상 열린다.
3. **Given** `.cell` 파일, **When** 어떤 경로로든 열기, **Then** 원본 해시·mtime 불변.

---

### User Story 3 — 설치가 한 번에 끝난다 (Priority: P3)

사용자가 설치 스크립트를 한 번 실행하면 6개 이상의 한컴 확장자가 모두 인식된다.

**Why P3**: 기능이 없으면 의미가 없으므로 P1/P2 뒤. 다만 확장자별 디스커버리 경로가
따로 필요한 호스트 구조(제약 4) 때문에 별도 설계가 필요하다.

**Acceptance Scenarios**:

1. **Given** 새 머신, **When** 설치 스크립트 실행, **Then** `.hwpx .owpml .hml .hwp .cell
   .show` 전부가 `officecli view`로 실제 해석된다.
2. **Given** 설치 중 실패, **When** 롤백, **Then** 기존 설치본이 복원된다.

## 요구사항

### 기능 요구사항

- **FR-1** 확장자를 신뢰하지 않고 매직바이트로 컨테이너를 판별한다 (`.hwp`인데 실제로는
  HWPX인 파일이 흔하다 — 기존 구현이 이미 이 원칙을 따른다).
- **FR-2** 한글→docx, 한셀→xlsx, 한쇼→pptx로 매핑한다 (호스트 제약 1에 부합).
- **FR-3** 미지원 기능은 exit 3 + 원인 명시. 추측 파싱 금지.
- **FR-4** 손상 입력은 exit 2. 원본 파일은 모든 경로에서 불변.
- **FR-5** stdout은 JSONL 전용, 진단은 stderr/`--log-file`. UTF-8 no BOM, `\n` 개행.
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

1. 한글 계열 4개 확장자가 각주·수식·머리말·번호·스타일까지 docx로 왕복한다.
2. `.hwpx` 편집 결과가 ADR-0013의 필수 G0~G3와 `save` durability를 통과하고,
   독립 OWPML/native oracle에서 상호운용된다. DVC는 채택한 named policy에만 적용한다.
3. `.cell`/`.show`가 최소한 정직하게 실패하거나, 변환기 경유로 열린다.
4. `plugins lint`에서 미지원 prop 0건.
5. 3개 OS 네이티브 러너에서 전 확장자 디스커버리 성공.

## 범위 밖

한셀/한쇼 쓰기, 서식·테마 파일(`.hwt`/`.hcdt`/`.hpt`/`.hsdt`/`.htheme`/`.nxt`),
한컴 상용 SDK 통합, 자체 렌더링, 애니메이션/전환.

## 미해결 질문

`./task-plan.md` Key Questions Q1–Q7 참조. 착수 차단 요인은 Q1(`.cell`/`.show` 컨테이너
타입)과 R2(실제 표본 부재)다.
