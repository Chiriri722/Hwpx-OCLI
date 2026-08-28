# Task Plan: 한컴오피스 통합 호환 플러그인 (HWP·HWPX·한셀·한쇼)

작성 2026-08-28 · P0 완료 커밋 `e77fb77c` (`feat/hwpx-plugin`) · spec-kit feature `001-hancom-unified`
근거 문서: `.agents/brain/research/hancom-unified-20260828.md` (정제된 조사·결정 기록),
`docs/spec-sources.md` (한컴 공식 원문 URL·리비전·바이트·SHA-256)

## Goal

현재 `plugins/hancom/crates/hancom-hwp`(HWPX/HWP 읽기 전용 dump-reader 1종)를 한컴오피스 독자 규격 전체를
다루는 통합 호환 플러그인 스위트로 확장한다.

| 계열 | 확장자 | 목표 kind | 목표 target | 현재 |
|---|---|---|---|---|
| 한글 | `.hwpx`, `.owpml` | dump-reader → format-handler | docx | `.hwpx`만 dump-reader |
| 한글 | `.hwp` | dump-reader | docx | RHWP 경유 (Phase 6 완료) |
| 한글 | `.hml` | dump-reader | docx | 없음 |
| 한셀 | `.cell`, `.nxl` | dump-reader | **xlsx** | 없음 |
| 한쇼 | `.show` | dump-reader | **pptx** | 없음 |

## Non-Goals

- 한셀/한쇼 **쓰기**. 내부 구조가 미공개이므로 손상 위험이 크다. 읽기·변환만 한다.
- `.hwt`/`.hcdt`/`.hpt`/`.hsdt`/`.htheme`/`.nxt` 서식·테마 파일. 소속조차 미확정(F4).
- 한컴 상용 SDK 통합. 유료·비공개로 MIT 플러그인에 넣을 수 없다(조사 §5).
- 렌더링. HTML/PNG는 호스트가 변환된 docx/xlsx/pptx로 이미 처리한다.

---

## 확정된 호스트 제약 (코드 읽어 검증)

1. `PluginManifestExtensions.ResolveTargetFormat` (`src/officecli/Core/Plugins/PluginManifest.cs`
   ~149행) 은 dump-reader `target`을 `docx|xlsx|pptx`로 **하드 제한**하고 그 외는 throw 한다.
   → 한셀→xlsx, 한쇼→pptx 는 **호스트 수정 없이 합법**이다.
2. `DocumentHandlerFactory.TryOpenViaPlugin` (~313행) 은 **dump-reader를 format-handler보다
   먼저** 해석한다. → 같은 확장자에 두 kind를 동시 선언하면 format-handler는 영원히 선택되지
   않는다. HWPX 쓰기 전환은 dump-reader 선언 **제거**와 원자적으로 이뤄져야 한다.
3. format-handler는 이미 완전히 배선되어 있다(`FormatHandlerSession` + `FormatHandlerProxy`
   생성, ~367행). 소스의 "format-handler: not yet wired" 주석은 **오래된 오류**다.
4. 디스커버리는 `(kind, ext)` 단위 경로
   `~/.officecli/plugins/<kind>/<ext>/plugin(.exe)` 이다. 확장자마다 설치 경로가 따로 필요하다.
5. `--info` 매니페스트의 `target`은 **단일 문자열**이다. 한 바이너리를 `.hwpx`(→docx)와
   `.cell`(→xlsx) 경로에 함께 설치하면 같은 `--info`를 반환해 target이 충돌한다. → A1 결정 참조.

---

## 아키텍처 결정

### A1. 계열별 바이너리 3개 + 공용 코어 크레이트 (채택)

`plugins/hancom/` Cargo workspace 로 재편한다.

```
plugins/hancom/
  crates/
    hancom-core/      컨테이너 판별(CFB/ZIP/XML), CFB 리더, 공용 문서모델, 단위변환,
                      자원예산·보안경계, JSONL emitter, 진단/heartbeat
    hancom-hwp/       한글: OWPML(.hwpx/.owpml) + HWPML(.hml) + RHWP 브리지(.hwp)
    hancom-cell/      한셀: .cell/.nxl 파서
    hancom-show/      한쇼: .show 파서
  bins/
    officecli-hancom-hwp/   target=docx  · exts .hwpx .owpml .hml .hwp
    officecli-hancom-cell/  target=xlsx  · exts .cell .nxl
    officecli-hancom-show/  target=pptx  · exts .show
```

근거: 제약 5를 회피하는 가장 단순하고 검증 가능한 방법. 대안으로 (a) argv[0]/부모 디렉터리
이름으로 target을 바꿔 보고하는 단일 바이너리는 동작하지만 디스커버리 경로에 암묵 의존해
취약하다. (b) 프로토콜에 `targets: {ext: target}` 맵을 추가하는 업스트림 변경은 P7로 별도
제안한다. 세 바이너리는 공용 코어를 공유하므로 중복 로직은 없다.

### A2. 한글 계열은 dump-reader → format-handler 2단계로 승격

읽기는 지금처럼 dump-reader(→docx)로 유지하고, HWPX **쓰기**가 준비되면 `.hwpx`/`.owpml`만
format-handler로 전환한다. 제약 2 때문에 전환은 dump-reader 선언 제거와 같은 커밋이어야 한다.
`.hwp`/`.hml`은 쓰기 대상이 아니므로 영구히 dump-reader로 남는다.

### A3. 한셀/한쇼는 "판별 우선, 파싱 나중"

내부 구조가 미공개(조사 §4)이므로 먼저 컨테이너·매직바이트를 판별해 **정직한 실패**(exit 3
`unsupported_feature`, 원인 명시)를 반환하는 단계를 출시하고, 그 다음 스트림 해석을 점진적으로
올린다. 추측 파싱으로 조용히 틀린 데이터를 내보내지 않는다.

### A4. 한셀/한쇼 폴백 변환 경로

한컴오피스는 `.cell`→`.xlsx`, `.show`→`.pptx` 저장을 지원한다(조사 §2 ODF/OOXML 지원 언급).
자체 파서가 성숙하기 전까지는, 사용자가 이미 한컴오피스를 보유한 Windows 환경에서
외부 변환기를 지정하는 `OFFICECLI_HANCOM_CELL_CONVERTER` /
`..._SHOW_CONVERTER` 경계를 제공한다. `.hwp`의 RHWP 브리지와 **동일한 보안 계약**
(shell 미사용, private scratch staging, 산출물 재판별, 예산·타임아웃, 프로세스 트리 정리)을
재사용한다.


---

## 필수 레포·도구 (사용자 요구에 따른 명시)

### 반드시 필요 (없으면 해당 Phase 착수 불가)

| # | 항목 | 용도 | 획득 | 라이선스/비용 | 현재 |
|---|---|---|---|---|---|
| R1 | **한컴 공식 파일형식 스펙 5종** (HWP 5.0 r1.3, HWP3.0/HWPML r1.2, 배포용 r1.2, 수식 r1.3, 차트 r1.2) | 한글 계열 정본. 수식·차트 갭 해소의 유일한 근거 | `cdn.hancom.com/link/docs/...` (HTTP 200 검증) | 무료·열람자유, **표기의무 있음(T0-1)** | URL·SHA 고정 완료, PDF 미보관 |
| R2 | **실제 `.cell`/`.show` 표본 + 한컴오피스(또는 뷰어)** | 미공개 포맷의 유일한 ground truth. 컨테이너 판별·회귀 기준 | 사용자 제공 필요 | 한컴오피스 라이선스 | **없음 — 최대 블로커** |
| R3 | **Rust 1.88+**, **.NET 10 SDK** | 플러그인 빌드 / 호스트 빌드 | `scripts/bootstrap-dev.sh`, rustup | 무료 | 설치됨 |
| R4 | **`edwardkim/rhwp`** v0.8.4+ | `.hwp`(바이너리) → HWPX 변환기 | GitHub Releases (SHA-256 대조) | MIT | 통합됨 |
| R5 | **`hancom-io/dvc`** (HWPX Document Validation Checker, C++) | HWPX **쓰기** 산출물의 공식 적합성 검증. P3 게이트 | <https://github.com/hancom-io/dvc> | 한컴 공식 | 미도입 |

### 조건부 필요

| # | 항목 | 용도 | 라이선스 | 판단 |
|---|---|---|---|---|
| R6 | **`neolord0/hwpxlib`** | HWPX 쓰기 시맨틱 **참조 구현**. 쓰기+암호화 지원 확인됨 | Apache-2.0 (호환) | 참조용으로만. JVM 의존을 런타임에 넣지 않는다 |
| R7 | **`neolord0/hwplib`** | HWP 바이너리 쓰기/레코드 구조 참조 (Apache POI로 CFB 파싱) | Apache-2.0 (호환) | 참조용. RHWP 대체 후보 |
| R8 | **`hancom-io/hwpx-owpml-model`** | 한컴 **공식** OWPML 모델. 요소·속성 정본 교차검증 | Apache-2.0 (호환) | 강력 권장 |
| R9 | **KS X 6101:2011** 표준 본문 | OWPML 적합성·XSD 확인 | **유료** (KSSN/e나라표준인증) | P3 착수 시 구매 판단 |
| R10 | `hancom-io/hwpx-contents-extract`, `metatag-ex` | 공식 추출기 동작 비교 | 한컴 공식 | 선택 |
| R11 | 기존 `HWPX_CORPUS` 사설 코퍼스 | 실문서 회귀 (버그 14건 중 9건을 이것만 잡았음) | 사설 | 확장 필요 |

### 사용 불가로 판정

| 항목 | 판정 근거 |
|---|---|
| **한셀 SDK** (`hancom.com/product/sdk/hancellSdk`) | 계산엔진 임베딩용 **상용** SDK("한셀 SDK 구매"가 1단계). `.cell` 포맷 파서가 아니며 유료·비공개 → MIT 플러그인 불가 |
| **한쇼 SDK** | 한컴 SDK 목록에 **존재하지 않음** |
| 한컴오피스 SDK | UX 프레임 화이트라벨용, 포맷과 무관 |
| GPL/AGPL 계열 파서 | 플러그인 MIT · 호스트 Apache-2.0과 비호환 |

### 이미 있고 계속 쓰는 것

`crawl4ai`(조사), spec-kit 형식의 정제된 `specs/001-hancom-unified/` 계획, codebase-memory
(`hwpx-ocli` 지식 그래프), `gh`, GitHub Actions
(`.github/workflows/hwpx-plugin.yml`).

2026-08-28 신뢰 경계 감사에 따라 생성된 `.github/skills/speckit-*`와 실행 가능한
`.specify/scripts/`는 프로젝트 의존성으로 커밋하지 않는다. 계획 문서는 선언적 요구사항으로만
취급하며, 저장소 문서나 훅이 명령 실행 권한을 스스로 부여할 수 없다.


---

## Phases

기존 `plugins/hancom/task_plan.md`의 Phase 6(H1c/H1d 원격 게이트)·Phase 7(호스트 하드닝)은
아래 P0에 흡수했으며 커밋 `e77fb77c`의 원격 검증으로 완료했다.

### P0 — 기반 정리 및 컴플라이언스 (선행 필수, 확장과 무관하게 즉시)

- [x] **T0-1 · 한컴 스펙 저작권 표기 의무 이행** — 조사 시점에는 필수 문구가
      **0건**이었다. "본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여
      개발하였습니다."를 (a) `--info` 매니페스트/`--help` 출력=UI, (b) `plugins/*/README.md`
      =매뉴얼·도움말, (c) 소스 헤더 또는 `NOTICE`=소스 에 **모두** 기재.
      **법적 요구사항이며 다른 모든 작업보다 우선한다.** 2026-08-28에 네 표면 모두 반영하고
      실제 바이너리 계약 테스트로 고정했다.
- [x] T0-2 · R1 스펙 5종을 다운로드하고 SHA-256을 `docs/spec-sources.md`에 고정. 재배포는
      하지 않고 URL+해시만 커밋(무수정 원본 조건 준수). HTTP 상태·미디어 타입·PDF 매직·
      최종 URL·바이트 수도 함께 검증하고 임시 PDF는 삭제했다.
- [x] T0-3 · 기존 Phase 6 H1c/H1d 원격 CI 게이트 종료(`.hwp` 디스커버리 + RHWP `view`
      Linux/Windows 네이티브). [HWPX plugin run 33157787880](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33157787880)에서 양 OS host 계약 35개와 실제 HWP/HWPX 경로가 모두 성공했고, [action pin run 33157787944](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33157787944)도 통과했다.
- [x] T0-4 · 기존 Phase 7 호스트 하드닝 착수: 상대 `OFFICECLI_PLUGIN_*` 경로 거부,
      `plugins list` dedup 정책, installer ancestor reparse, Actions 외부 action SHA 고정.
- [x] T0-5 · `DocumentHandlerFactory`의 잘못된 "format-handler: not yet wired" 주석 수정
      (실제로는 배선되어 있음). 1줄 변경이지만 후속 설계 판단을 오염시키므로 먼저 고친다.
- [x] T0-6 · 이번 조사 결과를 `.agents/brain/`과 codebase-memory ADR에 기록.
      소스 ADR은 `docs/adr/0006-hancom-unified-plugin-boundaries.md`; graph의
      `adr_present=true`를 재확인했다.

### P1 — workspace 재편 (A1 실행)

- [x] T1-1 · `plugins/hwpx` → `plugins/hancom/` Cargo workspace 로 이동. **동작 변화 0**을
      목표로 하는 순수 구조 변경. 기존 테스트 전부(단위/`parse_owpml` 34개/`protocol_contract`/
      `golden` 3개/`install_contract`)가 그대로 통과해야 한다.
      기존 package/lib/bin 이름과 `Cargo.lock`을 유지한 채 `crates/hancom-hwp` 단일 member로
      옮겼다. 로컬에서 Rust 테스트 225개, Rust 1.88 all-target check, stable clippy, release build,
      네이티브 Windows host 계약 35개, 실행 경로 6개, action pin 17개, workflow YAML 8개,
      actionlint와 PowerShell/Bash 구문 검사를 모두 통과했다. 커밋 `6decf630`의
      [HWPX plugin run 33160363729](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33160363729)에서
      GitHub-hosted Linux/Windows 전체 회귀·MSRV·host·실제 HWP/HWPX 설치/조회/제거가 성공했고,
      [action pin run 33160363733](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33160363733)도 통과했다.
- [x] T1-2 · `hancom-core` 추출: 컨테이너 판별, 문서모델, 단위변환, 자원예산·보안경계,
      JSONL emitter, heartbeat, 진단. 기존 `format.rs`의 매직바이트 판별을 여기로.
      커밋 `da540e47`에서 공용 타입을 추출하고 기존 `officecli_hwpx` 공개 경로에는 호환
      re-export와 컴파일 계약을 유지했다. 로컬에서 workspace 테스트 232개, stable clippy,
      Rust 1.88 all-target check, release build와 .NET host build를 통과했다.
      [HWPX plugin run 33162799813](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33162799813)은
      Linux/Windows의 동일 회귀·MSRV·정확한 .NET 10.0.302 host 계약·실제 HWP/HWPX 설치/조회/제거를
      모두 통과했고,
      [action pin run 33162799808](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33162799808)도 성공했다.
- [ ] T1-3 · `officecli-hancom-hwp` 바이너리로 기존 기능 이관 + `.owpml`·`.hml` 확장자 추가
      (`.owpml`은 HWPX와 동일 컨테이너 → 판별기만 확장. `.hml`은 단일 XML → 신규 리더).
- [ ] T1-4 · 설치 스크립트를 다중 확장자·다중 바이너리로 일반화. 확장자별 커밋 플래그를
      개별 추적하고(기존 H1 교훈), uninstall 전 symlink/reparse 가드 유지.
- [ ] T1-5 · 하위 호환: 기존 `~/.officecli/plugins/dump-reader/hwpx/` 설치본 마이그레이션 경로.

### P2 — 한글 계열 커버리지 완성 (target=docx)

R1 스펙 확보 후 착수. 현재 미지원 목록이 곧 작업 목록이다.

- [ ] T2-1 · 각주/미주 (`hp:footNote`/`hp:endNote`) → docx footnote/endnote
- [ ] T2-2 · 수식 (`hp:equation`) → OMML/LaTeX. **R1의 "수식 형식 r1.3" 스펙이 필수**
- [ ] T2-3 · 머리말/꼬리말 → docx header/footer
- [ ] T2-4 · 목록 번호 (`numbering` 구조) — 현재 텍스트만 살아있음
- [ ] T2-5 · 스타일 이름 (`styleIDRef` → docx `style`)
- [ ] T2-6 · 도형·글상자 (`hp:rect`, `hp:textart` 등) → docx shape/textbox
- [ ] T2-7 · 차트 → docx chart. **R1의 "차트 형식 r1.2" 스펙이 필수**
- [ ] T2-8 · 보류 항목 재평가: G5 스타일 매핑, G6 PUA 치환 (실측 근거 확보 시)
- [ ] T2-9 · 각 항목마다 `plugins lint`(호스트 내장 docx 스키마 검증) 재실행 — 어휘 변경 시 필수

### P3 — HWPX 쓰기 / format-handler 승격 (A2 실행)

ADR-1(읽기 전용) 을 **뒤집는** 변경이므로 새 ADR로 명시 기록한다.

- [ ] T3-1 · R6/R8을 참조해 OWPML 쓰기 시맨틱 조사 → 설계 문서
- [ ] T3-2 · R5 `hancom-io/dvc`를 CI에 도입. **쓰기 산출물이 공식 검증기를 통과해야 한다**
- [ ] T3-3 · OWPML writer 구현 (읽기 왕복 무손실이 최소 기준)
- [ ] T3-4 · format-handler 프로토콜 구현: open 핸드셰이크, vocabulary 스냅샷,
      get/query/set/add/remove/move/copy/raw/raw_set/save/close, **정규적 `save` durability**
- [ ] T3-5 · `.hwpx`/`.owpml`의 dump-reader 선언 제거와 format-handler 전환을 **같은 커밋**으로
      (제약 2). `.hwp`/`.hml`은 dump-reader 유지 → 바이너리 분리 필요 여부 재확인
- [ ] T3-6 · 원본 손상 방지: 원자적 저장, 실패 시 롤백, 사전 백업 정책

### P4 — 한셀 `.cell` (target=xlsx) · A3 단계 적용

**R2(실제 표본)가 없으면 시작조차 불가.**

- [ ] T4-1 · **스파이크: 컨테이너 판별.** 표본의 매직바이트를 확인해 CFB(`D0CF11E0A1B11AE1`)
      /ZIP(`504B0304`)/기타를 판정하고 조사 §4의 UNKNOWN을 해소. 이 결과가 T4-3 이후 전체를 결정
- [ ] T4-2 · `.cell` 세대 판별 (넥셀 `.nxl` / 한셀 2010 / 한셀 2014 — 최소 2개 비호환 구조)
- [ ] T4-3 · 판별 전용 릴리스: 인식하면 exit 3 + 원인 명시, 손상 입력은 exit 2. 추측 파싱 금지
- [ ] T4-4 · A4 외부 변환기 경계(`OFFICECLI_HANCOM_CELL_CONVERTER`) — RHWP 브리지의 보안 계약
      재사용. 이것이 **먼저 실사용 가치를 낸다**
- [ ] T4-5 · 자체 파서: 시트/셀/값/수식 문자열 → xlsx 어휘
- [ ] T4-6 · 서식(셀 배경·테두리·숫자서식), 병합, 열 너비
- [ ] T4-7 · 수식 → xlsx. 한셀 함수명·인수 규약 차이 매핑표 필요
- [ ] T4-8 · 차트·피벗·조건부서식은 별도 판단(호스트 xlsx 어휘는 이미 풍부)
- [ ] T4-9 · `plugins lint`로 xlsx 어휘 검증 + 골든 회귀

### P5 — 한쇼 `.show` (target=pptx) · A3 단계 적용

P4의 컨테이너 판별 결과를 재사용한다(같은 시대 코드베이스일 가능성 LIKELY).

- [ ] T5-1 · 스파이크: 컨테이너·매직바이트 판별
- [ ] T5-2 · 판별 전용 릴리스 (exit 3 + 원인)
- [ ] T5-3 · A4 외부 변환기 경계(`OFFICECLI_HANCOM_SHOW_CONVERTER`)
- [ ] T5-4 · 자체 파서: 슬라이드/도형/텍스트프레임 → pptx 어휘
- [ ] T5-5 · 이미지·표·차트, 마스터/레이아웃
- [ ] T5-6 · 애니메이션·전환은 명시적 비목표로 두거나 별도 Phase
- [ ] T5-7 · `plugins lint` pptx 어휘 검증 + 골든 회귀

### P6 — 통합 배포·CI

- [ ] T6-1 · 3 바이너리 × 확장자 N개 설치 매트릭스 (Unix `install.sh` / Windows `install.ps1`)
- [ ] T6-2 · Linux/Windows/macOS 네이티브 CI에서 확장자별 실제 `officecli` 디스커버리 검증.
      `plugins list` 행 수는 경로별 열거 때문에 신뢰하지 말고 `view`로 실해석 확인(기존 교훈)
- [ ] T6-3 · 대용량·자원예산 실측을 계열별로 (기존 48MiB HWPX 실측 방식 재사용)
- [ ] T6-4 · 보안 회귀 통합: ZIP 폭탄, XML 폭탄, 경로 탈출, 하드링크/심볼릭 링크,
      CFB 순환참조(신규 — `.hwp`/`.cell`/`.show` 공통 위험)
- [ ] T6-5 · 공개 문서: 루트 README 포맷 표 갱신, SKILL.md, 플러그인 README

### P7 — 업스트림 프로토콜 제안 (선택)

- [ ] T7-1 · 매니페스트 `targets: {ext: target}` 맵 제안 → 단일 바이너리 다중 target 허용
      (A1 제약 5의 근본 해결)
- [ ] T7-2 · 같은 확장자에서 dump-reader/format-handler 우선순위를 플러그인이 선택하는 방법
- [ ] T7-3 · `exporter` kind로 docx/xlsx/pptx → hwpx/cell/show 역방향 내보내기 검토


---

## Key Questions (미해결 · 착수 전 반드시 답해야 함)

1. **Q1 (블로커)** `.cell`/`.show`의 컨테이너는 CFB인가 ZIP인가? → T4-1. R2 표본 필수.
2. **Q2** `.cell` 2010세대와 2014세대를 어떻게 구분하는가? 헤더 버전 필드가 있는가?
3. **Q3** 사용자는 한셀/한쇼를 **읽기만** 원하는가, 아니면 편집까지 원하는가? 읽기만이면
   A4 외부 변환기로 대부분 해결되고 P4-5/P5-4 이후를 미룰 수 있다.
4. **Q4** KS X 6101(R9, 유료) 구매가 필요한가? P3 쓰기 적합성에만 필요하다고 판단하나
   R5 `dvc` 검증기로 대체 가능할 수 있다.
5. **Q5** JVM 의존(R6/R7)을 런타임에 허용할 것인가? 현재 판단은 **참조용만**이나, HWPX 쓰기를
   빨리 원하면 `hwpxlib` 사이드카가 가장 빠른 길이다. 단일 바이너리 원칙과 충돌한다.
6. **Q6** `.hml`(단일 XML)의 실사용 빈도. 낮으면 P1에서 빼고 후순위로 내린다.
7. **Q7** 한글 계열이 format-handler로 승격되면 `.hwp`/`.hml`은 별도 바이너리로 분리해야
   하는가? (제약 2 + 제약 5 상호작용) → T3-5에서 확정.

## Risks

| 위험 | 영향 | 완화 |
|---|---|---|
| **R2 표본 부재** | P4/P5 전면 중단 | 사용자에게 즉시 요청. 그 전까지 P0~P3만 진행 |
| `.cell`/`.show` 완전 미공개 | 리버스엔지니어링 비용 예측 불가 | A3 단계 출시 + A4 폴백으로 가치를 먼저 낸다 |
| 한컴 표기의무 미이행 | **법적 리스크** | T0-1을 최우선 |
| 추측 파싱으로 조용한 데이터 오류 | 신뢰 붕괴 | A3 원칙: 모르면 exit 3, 절대 추측하지 않음 |
| 2014 구조 변경으로 표본 편향 | 특정 세대만 동작 | T4-2 세대 판별을 파서보다 먼저 |
| workspace 재편 중 회귀 | 기존 기능 손상 | T1-1은 "동작 변화 0" + 전체 테스트 통과를 게이트로 |
| 스펙 파생물 권리 조항 | 배타적 권리 주장 금지 | 스펙 재배포 안 함, URL+해시만, 표기 유지 |

## Acceptance Gates (Phase 완료 판정)

- **모든 Phase 공통**: `cargo test --locked` + `cargo clippy --all-targets -- -D warnings` +
  `dotnet build src/officecli/officecli.csproj` 통과.
- **어휘를 건드린 모든 변경**: 실제 `officecli` 바이너리로 `plugins lint` — 미지원 prop 0건.
  (호스트 내장 대상 포맷 스키마 검증이므로 어휘 매핑의 기계적 보증 수단)
- **P2 이후**: `HWPX_CORPUS` 실문서 회귀 + 골든파일 무변경(의도적 변경은 diff 육안 검토).
- **P3**: R5 `dvc` 공식 검증기 통과 + 왕복 무손실 + `save` durability(재열기 확인).
- **P4/P5**: 표본 대비 셀·슬라이드 수와 텍스트 일치, 원본 파일 mtime·해시 불변.
- **P6**: Linux/Windows/macOS 네이티브 러너에서 확장자별 `officecli view` 실해석 성공.

## Decisions Made

1. 한셀→xlsx, 한쇼→pptx는 **호스트 수정 없이** 가능하다(제약 1 검증). 호스트 변경을
   전제로 계획을 세우지 않는다.
2. 계열별 바이너리 3개 + 공용 코어(A1). 단일 바이너리 다중 target은 프로토콜 제약이므로
   P7 업스트림 제안으로 분리.
3. 한셀/한쇼는 읽기 전용. 쓰기는 비목표.
4. "모르면 실패한다"(A3). 추측 파싱 금지.
5. 외부 변환기 폴백(A4)이 자체 파서보다 **먼저** 실사용 가치를 낸다. 순서를 그렇게 잡는다.
6. 한컴 상용 SDK는 사용하지 않는다(유료·비공개·포맷 파서 아님).
7. 참조 구현(R6/R7)은 Apache-2.0으로 라이선스 호환이나 **런타임 JVM 의존은 넣지 않는다**
   (단일 바이너리 원칙). Q5에서 재확인.
8. T0-1 저작권 표기는 법적 요구사항이므로 확장 작업보다 우선한다.
9. spec-kit `specs/001-hancom-unified/`를 이 확장의 정본 위치로 삼고, 기존
   `plugins/hancom/task_plan.md`는 P0 이력과 검증 근거로 유지한다.
10. T1-1에서는 기존 `officecli-hwpx` package를 `crates/hancom-hwp`에 그대로 두고, 공용 core
    추출과 바이너리 이름 변경은 각각 T1-2/T1-3으로 분리한다. 구조 이동과 동작 변경을 한
    커밋에 섞지 않는다.

## Status

**P0 완료 · P1 진행 중(T1-1~T1-2 완료, T1-3 다음).** 커밋 `e77fb77c`의 GitHub-hosted Linux/Windows HWPX plugin
run `33157787880`과 action pin run `33157787944`가 모두 성공해 T0-1~T0-6을 닫았다.
T1-1은 기존 Cargo target surface와 lockfile을 보존한 workspace 이동으로 구현했고, 로컬과
원격 양 OS 회귀·MSRV·host·공급망·설치 검증을 모두 통과했다.
T1-2는 공용 core와 기존 HWP 호환 re-export를 구현했고 run `33162799813`/`33162799808`로
같은 양 OS 게이트를 다시 통과했다.
P4/P5는 별도로 R2(실제 `.cell`/`.show` 표본)가 들어오기 전까지 착수할 수 없다.

## Next Action Plan

1. T1-3에서 한글 계열 진입점과 `.owpml`·`.hml` 지원을 RED 테스트부터 구현한다.
2. T1-4/T1-5에서 다중 확장자 설치와 기존 설치본 마이그레이션을 계약 테스트로 고정한다.
3. 현재 브랜치의 upstream 지연은 workspace 재편과 섞지 않고 별도 통합 변경으로 처리한다.
4. P4/P5 착수 전 **사용자 확인 필요**: R2 표본 제공 가능 여부, Q3(읽기 전용 vs 편집), Q5(JVM 허용 여부).
5. R2가 확보되면 T4-1 컨테이너 판별 스파이크를 최우선으로 돌려 Q1을 해소하고,
   그 결과로 P4/P5의 실제 규모를 재산정한다.
