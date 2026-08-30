# Task Plan: 한컴오피스 통합 호환 플러그인 (HWP·HWPX·한셀·한쇼)

작성 2026-08-28 · 최근 갱신 2026-08-30 · P0 완료 커밋 `e77fb77c`
(`feat/hwpx-plugin`) · spec-kit feature `001-hancom-unified`
근거 문서: `.agents/brain/research/hancom-unified-20260828.md` (정제된 조사·결정 기록),
`docs/spec-sources.md` (한컴 공식 원문 URL·리비전·바이트·SHA-256)

## Goal

`plugins/hancom`의 HWP/HML dump-reader와 package-preserving HWPX/OWPML
format-handler를 기반으로, 근거가 확보된 한컴오피스 독자 규격을 정직한 실패
경계와 함께 다루는 플러그인 스위트로 확장한다.

| 계열 | 확장자 | 목표 kind | 목표 target | 현재 |
|---|---|---|---|---|
| 한글 | `.hwpx`, `.owpml` | format-handler | 원본 직접 편집 | package-preserving plain-text subset 완료 |
| 한글 | `.hwp` | dump-reader | docx | RHWP 경유 (Phase 6 완료) |
| 한글 | `.hml` | dump-reader | docx | HWPML 공통 부분집합 직접 지원 |
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
    officecli-hancom-hwp/   dump-reader · target=docx · exts .hwp .hml
    officecli-hancom-hwpx/  format-handler · exts .hwpx .owpml
    officecli-hancom-cell/  target=xlsx  · exts .cell .nxl
    officecli-hancom-show/  target=pptx  · exts .show
```

근거: 제약 5를 회피하는 가장 단순하고 검증 가능한 방법. 대안으로 (a) argv[0]/부모 디렉터리
이름으로 target을 바꿔 보고하는 단일 바이너리는 동작하지만 디스커버리 경로에 암묵 의존해
취약하다. (b) 프로토콜에 `targets: {ext: target}` 맵을 추가하는 업스트림 변경은 P7로 별도
제안한다. 세 바이너리는 공용 코어를 공유하므로 중복 로직은 없다.

### A2. 한글 계열은 역할별 두 바이너리로 승격 (완료)

HWPX/OWPML은 ADR-0013의 package-preserving closed text-edit subset과 durable save
경계를 확보해 format-handler로 전환했다. 같은 변경에서 두 확장자의 dump-reader
선언과 설치 경로를 제거한다. HWP/HML은 쓰기 대상이 아니므로 별도
`officecli-hancom-hwp` dump-reader로 남는다. 설치 승격과 rollback 경계는
ADR-0014에 고정한다.

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

### 조건부 필요

| # | 항목 | 용도 | 라이선스 | 판단 |
|---|---|---|---|---|
| R5 | **`hancom-io/dvc`** (HWPX Document Validation Checker, C++) | 이름·해시를 고정한 JSON 정책의 선택적 의미 smoke | 한컴 공식 | 범용 ZIP/XSD/KS 적합성 검증기가 아니다. 정책을 실제 제품 계약으로 채택할 때만 Windows 선택 게이트로 도입 |
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
- [x] T1-3 · `officecli-hancom-hwp` 바이너리로 기존 기능 이관 + `.owpml`·`.hml` 확장자 추가
      (`.owpml`은 HWPX와 동일 컨테이너 → 판별기만 확장. `.hml`은 단일 XML → 신규 리더).
      커밋 `7681206f`에서 통합 진입점·4확장자 매니페스트·HWPML 리더를 추가하고,
      `86ffc6fd`에서 stable lint를 정리했으며, `17b65ea5`에서 공식 HWPML 2.8 문법을
      기준으로 XML 선언/인코딩/DTD/엔티티/네임스페이스/부모 경로/매핑 ID/표 좌표와
      자원예산을 fail-closed로 강화했다. 2.1/2.8/2.9/2.91은 전체 문법이 아닌 공통
      부분집합 상호운용 허용 목록이다. canonical/legacy 진입점과 변환기 성공·실패
      바이트 동등성도 고정했다. 로컬 workspace 테스트 297개, Rust 1.88 check,
      stable clippy, release build를 통과했고,
      [HWPX plugin run 33170785021](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33170785021)과
      [action pin run 33170784965](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33170784965)이
      Linux/Windows·MSRV·host·실제 설치/조회/제거까지 모두 성공했다.
- [x] T1-4 · 설치 스크립트를 네 확장자 경로로 일반화. 커밋 `bc3d273c`에서 Unix는
      HWPX 실파일과 HWP/OWPML/HML 상대 링크, Windows는 네 검증 복사본을 설치하도록
      바꾸고, 확장자별 커밋 상태·역순 롤백·uninstall 전 symlink/reparse 가드를 계약
      테스트와 실제 OfficeCLI `view` CI로 고정했다.
- [x] T1-5 · 하위 호환: 커밋 `449e1c8c`에서 기존 HWPX 단독 설치본의 네 경로
      마이그레이션 성공·실패 복원·재시도·멱등성을 양 OS 계약 테스트로 고정했다.
      [HWPX plugin run 33172696561](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33172696561)의
      Linux/Windows 전체 회귀·MSRV·host·네 확장자 실제 설치/조회/제거와
      [action pin run 33172696668](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33172696668)이
      모두 성공했다.

### P2 — 한글 계열 커버리지 완성 (target=docx)

R1 스펙 확보 후 착수. 현재 미지원 목록이 곧 작업 목록이다.

- [x] T2-1 · 각주/미주 (`hp:footNote`/`hp:endNote`) → docx footnote/endnote.
      커밋 `6b78e1f1`에서 필수 단일 `subList`, 여러 문단·런 서식·표·이미지,
      본문 내 참조 순서와 종류별 번호를 보존하고 손상·중첩 주석은 fail-closed로 처리했다.
      로컬 전체 회귀·Clippy·`plugins lint`(unknown prop 0)와 실제 OfficeCLI
      각주/미주·표 DOCX 검증을 통과했으며,
      [HWPX plugin run 33234858972](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33234858972)와
      [action pin run 33234858790](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33234858790)이
      모두 성공했다. 실제 한컴 각주/미주와 사용자 표식·구역별 번호 정책은 T2-3에서
      생성·실측해 보존 또는 fail-closed 경계를 확정했다.
- [x] T2-2 · 수식 (`hp:equation`) → OMML/LaTeX. 공식 수식 형식 r1.3과 공개
      `SimpleEquation.hwpx`를 근거로 커밋 `d62155df`에서 inline/display·색상과 분수·근호·
      첨자·주요 연산자/함수·적분·행렬·cases/pile/alignment·장식을 구현했다. 고정 RHWP
      MIT 파서에 완전 소비·자원 한계·손실 명령 거부를 덧대어 근사 변환과 부분 stdout을
      막았다. 로컬 457개 테스트·Clippy·release, `plugins lint`(unknown prop 0), 실제
      OfficeCLI OMML DOCX 검증을 통과했고,
      [HWPX plugin run 33236634179](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33236634179)와
      [action pin run 33236634150](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33236634150)이
      모두 성공했다. 다양한 실제 한컴 수식 코퍼스는 여전히 필요하다.
- [x] T2-3 · 머리말/꼬리말 → docx header/footer. 커밋 `393785ab`에서 `content.hpf`
      구역 spine과 `BOTH`/`ODD`/`EVEN`/first story, 동적 `PAGE`/`NUMPAGES`, 구역별
      각주/미주 번호·재시작·시작·배치와 접두/접미·위첨자를 보존했다. 중간 활성화·겹치는
      story·사용자 주석 표식은 부분 출력 없이 거부한다. DOCX에 대응하지 않는 active
      `noteLine`/`noteSpacing`은 exit 3, dormant 정책은 원본 값을 담은 필수 구조화 경고로
      처리한다. 로컬 486개 Rust 테스트·38개 host 계약·Clippy·release·고정 .NET build와
      실제 한컴/공개 HWPX replay·`plugins lint`(unknown prop 0)를 통과했고,
      [HWPX plugin run 33241159576](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33241159576)와
      [action pin run 33241159629](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33241159629)이
      모두 성공했다. 결정 경계는 ADR-0007에 고정했다.
- [x] T2-4 · 목록 번호 (`numbering` 구조). 커밋 `025418bc`와 RHWP 호환성 수정
      `2289ca60`에서 `hh:heading`의
      NUMBER/BULLET과 구역별 OUTLINE을 typed model로 연결하고 OfficeCLI
      `abstractNum`/`level`/`num` 및 문단 `numId`/`numLevel`로 동적 보존했다.
      공식 토큰·형식과 한컴 네이티브로 확인한 배치 프로필만 허용하고, 활성 이미지·체크형
      표식·모호한 ID·손실 형식/배치는 JSONL 전에 fail-closed로 거부한다. dormant HWP
      level 10은 활성 level 1을 막지 않으며 실제 RHWP `english.hwp`로 검증했다. 로컬 502개 Rust
      테스트·Clippy·release·호스트 build, 실제 DOCX validate, 49개 공개 코퍼스 47개 성공과
      `plugins lint` unknown prop 0을 통과했다.
      [HWPX plugin run 33243420505](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33243420505)와
      [action pin run 33243420494](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33243420494)도
      양 OS에서 성공했다. 결정 경계는 ADR-0008에 고정했다.
- [x] T2-5 · 스타일 이름 (`styleIDRef` → docx `style`). 커밋 `50adcc31`에서
      본문·표·주석·머리말/꼬리말이 실제 참조하는 PARA 스타일과 비자기
      `nextStyleIDRef` 폐쇄만 strict하게 물질화했다. 정확한 숫자 ID·기본 이름,
      문단/런 속성, 다음 스타일, 개요 수준과 NUMBER/BULLET/구역 OUTLINE 번호를
      보존하며 직접 `paraPrIDRef`/런 서식은 별도 override로 유지한다. 한컴 네이티브
      오라클에 따라 `lockForm`은 검증만 하고 DOCX `locked`로 추측하지 않으며,
      `outlineShapeIDRef=0`의 암묵 기본 개요만 좁게 지원한다. 한 스타일이 구역마다
      다른 개요 번호를 요구하거나 활성 참조·정의가 손상되면 JSONL 전에 실패한다.
      로컬 513개 Rust 테스트·40개 host 계약·Clippy·release·정확한 SDK build,
      공개 281개 코퍼스 기준선(226 성공/23 corrupt/32 unsupported), 실제 224항목
      replay·DOCX validate 오류 0과 `plugins lint` unknown prop 0을 통과했다.
      결정 경계는 ADR-0009에 고정했다.
- [x] T2-6 · 도형·글상자 (`hp:rect`, `hp:textart` 등) → docx shape/textbox.
      커밋 `b379b4d3`에서 공식 HWPML r1.2, 한컴 2020 네이티브 DOCX, 공개
      281개 코퍼스를 교차검증해 축 정렬 `hp:rect`와 전체 `hp:ellipse`의 폐쇄
      부분집합을 구현했다. inline/페이지 floating, LEFT/CENTER, wrap/flow side와
      거리, z-order·겹침, 무채움/불투명 단색, SOLID/NONE 선, 0~50 둥근 모서리,
      `drawText`의 구조적 문단·표·이미지·주석·번호·스타일, `shapeComment` 설명을
      보존한다. 회전·뒤집기·그룹·보호·하이퍼링크·caption과 line/polygon/curve/
      connector/container/OLE/textart/arc/video는 근사하지 않고 stdout 전에
      실패한다. 로컬 526개 Rust 테스트·42개 host 계약·locked Clippy/release·
      정확한 SDK build, `plugins lint` unknown prop 0, 실제 replay/OpenXML validate,
      공개 코퍼스 173 success/22 corrupt/86 unsupported/0 other를 통과했다.
      결정 경계는 ADR-0010에 고정했다.
- [x] T2-7 · 차트 → docx chart. 커밋 `cd110588`에서 공식 차트 형식 r1.2,
      OWPML 모델 커밋 `1453388472c703a4b299a0834f425cdac16644b9`, 한컴 2020
      네이티브 DOCX와 공개 30개 chart part를 교차검증했다. 관계가 없고 자체완결인
      `c:chartSpace`와 native-verified `SQUARE/BOTH_SIDES`, floating
      `COLUMN/PARA TOP/LEFT`, zero-offset 프레임만 raw chart carrier로 보존한다.
      기본 프로필은 schema-valid XML만 무변경 수용하고, 한컴 parser가 명시하는
      `hwpxChartOrderRepairV1`만 구조화 SDK 오류가 정확히 일치하는 catAx/valAx/view3D
      순서를 SDK particle metadata로 재배치한 뒤 오류 0을 요구한다. 외부 관계·caption·
      미검증 배치·dateAx/serAx repair는 stdout 전에 실패한다. 29개 파일은 28 success/
      0 corrupt/1 unsupported/0 other였고, 지원 차트 28/28이 OfficeCLI replay·OpenXML
      validate·구조 fingerprint·관계 topology 검사를 통과했다. 로컬 533개 Rust 테스트,
      44개 host 계약, Clippy/release와 `plugins lint` unknown prop 0을 통과했으며 결정은
      ADR-0011에 고정했다.
- [x] T2-8 · 보류 항목 재평가. G5 스타일 매핑은 T2-5에서 완료했다. G6 PUA는
      public-domain Hanyang old-Hangul 5,660개 표를 찾았지만 글꼴 한정 표이고, 현재
      모델이 USER/SYMBOL을 포함한 7개 font slot을 한 필드로 축약해 적용 대상을 증명할
      수 없다. 281개 코퍼스에는 30파일·25,759회·85종 PUA가 있으며 25,649회가
      supplementary PUA다. 한컴 2020 native `exam_kor` DOCX도 원본의 BMP 44회와
      supplementary 83회를 그대로 보존하고 Jamo로 치환하지 않았다. 따라서 현행
      무변경 보존+개수 진단을 최종 기본 정책으로 유지한다. 향후에는 7개 source slot,
      exact-font allowlist, 글꼴별 native/PDF oracle이 모두 있을 때만 opt-in profile로
      재검토한다. 결정은 ADR-0012에 고정했다.
- [x] T2-9 · 각 P2 어휘 변경마다 실제 `plugins lint`를 재실행했다. T2-7 최종
      대표 dump도 4개 BatchItem, unknown prop 0을 확인했다.

### P3 — HWPX 쓰기 / format-handler 승격 (A2 실행)

ADR-1(읽기 전용) 을 **뒤집는** 변경이므로 새 ADR로 명시 기록한다.

- [x] T3-1 · R6/R8 writer 의미, R5 실제 범위, 호스트 `save` 계약을 조사하고
      package-preserving closed-subset 설계를 ADR-0013으로 확정했다.
- [x] T3-2a · 필수 G0~G2 출력 게이트 도입: ZIP/CRC·예산·경로·alias·link,
      byte-exact first/stored mimetype, UTF-8/XML 안전성, container/HPF/header/section
      토폴로지와 참조 무결성을 `validate_output_package` 및 14개 회귀로 검증한다.
- [x] T3-2b · G3 저장 검증 도입: strict package reader 재열기, ordered entry snapshot,
      압축 전/후 SHA-256·ZIP metadata, explicit mutation plan, requested exact semantic delta,
      모든 unchanged-part 동일성과 changed-part의 비내용 metadata 보존을 6개 회귀와
      49-entry native no-op probe로 검증한다.
      DOCX 변환 모델은 편집 subset이 완전히 표현될 때만 추가 oracle로 사용한다.
- [ ] T3-2c · 제품이 특정 DVC 정책을 실제로 약속할 때만 R5 commit·OWPML model·정책 JSON
      SHA-256·fixture를 고정한 Windows semantic-policy smoke를 추가한다. 일반 P3 완료 조건은 아니다.
- [x] T3-3 · package-preserving OWPML editor 구현. exact replacement hash/key set,
      source TOCTOU, raw-entry COW, central `version made by` 복원, strict G0~G3를 한 경로로
      묶고, paragraph ordinal(+선택적 id precondition)/text ordinal로 지정한 직접
      `hp:p/hp:run/hp:t` inner bytes만 바꾸는 11개 회귀를 통과한다. 49-entry native
      `exam_kor.hwpx`의 실제 한-node edit에서도 나머지 48개 entry snapshot 동일을 확인했다.
- [x] T3-4 · format-handler 프로토콜 구현. 분리된 `officecli-hancom-hwpx`가 bounded JSONL
      open 핸드셰이크와 vocabulary/capability 스냅샷, view/get/query/validate/raw,
      직접 plain `hp:p/hp:run/hp:t` set, 정규적 save/close를 구현한다. add/remove/move/
      copy/raw_set/add_part는 광고하지 않고 `unsupported_command`로 실패해 성공 no-op을
      만들지 않는다. 관대한 읽기 세션과 strict editable gate도 분리했다.
- [x] T3-5 · `.hwpx`/`.owpml`의 dump-reader 선언 제거와 format-handler 전환을 **같은 커밋**으로
      완료했다(제약 2). `.hwp`/`.hml` dump-reader와 HWPX/OWPML format-handler를 역할별
      두 바이너리로 분리하고, 설치기는 네 활성 경로를 검증·커밋한 뒤 두 legacy 경로를
      폐기한다. clean Linux current host에서 exact discovery와
      `set → save → close → reopen → validate`, source hash/XML delta, 형제 DOCX 부재를
      검증했고 Windows/Unix 설치 계약 18/26개를 통과했다. 결정은 ADR-0014에 고정했다.
- [x] T3-6 · 원본 손상 방지. 같은 디렉터리 임시 파일에 COW하고 G0~G3·별도 재열기·
      file flush/sync와 권한 복사를 모두 마친 뒤 Unix rename+directory sync 또는 Windows
      ReplaceFileW(WRITE_DAC 거부 시 이름이 그대로라는 조건에서 MoveFileExW write-through)
      로 한 번에 교체한다. 검증/TOCTOU/교체 실패는 원본을 건드리지 않으며 임시 파일을
      회수한다. 기본 사전 백업은 만들거나 보존하지 않는 정책으로 확정했다.

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
5. **Q5** JVM 의존(R6/R7)을 런타임에 허용할 것인가? 현재 판단은 **참조용만**이나, HWPX 쓰기를
   빨리 원하면 `hwpxlib` 사이드카가 가장 빠른 길이다. 단일 바이너리 원칙과 충돌한다.
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
| 불완전 semantic model의 역직렬화 | 조용한 OWPML 요소·서식·확장 손실 | 전체 문서 재생성 금지, source-preserving COW + 최소 subtree patch, 모르면 `unsupported_feature` |
| DVC/한컴 open을 표준 적합성으로 오인 | 검증 범위 과장과 잘못된 안전 보장 | 결과를 named policy/interoperability smoke로만 표기, KS/XSD 근거와 분리(ADR-0013) |

## Acceptance Gates (Phase 완료 판정)

- **모든 Phase 공통**: `cargo test --locked` + `cargo clippy --all-targets -- -D warnings` +
  `dotnet build src/officecli/officecli.csproj` 통과.
- **어휘를 건드린 모든 변경**: 실제 `officecli` 바이너리로 `plugins lint` — 미지원 prop 0건.
  (호스트 내장 대상 포맷 스키마 검증이므로 어휘 매핑의 기계적 보증 수단)
- **P2 이후**: `HWPX_CORPUS` 실문서 회귀 + 골든파일 무변경(의도적 변경은 diff 육안 검토).
- **P3 필수 G0~G3**: package 안전/보존 hash + XML well-formedness/비활성 파싱 +
  project package-topology profile v1 + 별도 reader 재열기/semantic delta + `save` durability.
- **P3 독립 oracle**: 고정 OWPML model 및 버전 기록 native 한글 open/render/save/reopen은
  상호운용성 증거다. R5 DVC는 채택한 named policy가 있을 때만 해당 정책 smoke다.
  어느 결과도 KS/XSD 적합성으로 표현하지 않는다.
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
11. Q6은 해소했다. 공식 HWPML 문법과 실제 2.91 공개 코퍼스를 확보했고 통합 바이너리의
    증분 비용이 작으므로 `.hml`을 P1에 유지한다. 미지원 컨트롤은 누락시키지 않고 exit 3으로
    실패한다.
12. T2-7은 관계 없는 자체완결 차트만 raw carrier로 보존한다. schema-order 허용은
    source-selected `hwpxChartOrderRepairV1`로 한정하고 기본 raw surface는 strict를 유지한다.
    세부 경계는 ADR-0011을 따른다.
13. G6 PUA는 한컴 native DOCX와 같이 무변경 보존한다. 글꼴 한정 표를 전역 적용하지 않으며,
    정확한 source font slot과 allowlist oracle 없이는 치환하지 않는다(ADR-0012).
14. R5 DVC는 사용자 JSON 정책의 제한된 의미 checker이며 범용 HWPX/OWPML validator가 아니다.
    필수 P3 게이트에서 제외하고, 이름·commit·정책 SHA·fixture가 고정된 선택 smoke로만 허용한다.
15. Q4는 해소했다. KS X 6101/XSD는 standards claim에 필요하고 DVC로 대체할 수 없다.
    구매 전에도 project profile로 편집기 구현은 진행하되 `schema-valid`/`KS 적합`을 주장하지 않는다.
16. HWPX 쓰기는 불완전 semantic model의 전체 직렬화가 아니라 source package를 보존하는
    closed-subset COW 편집기로 구현한다. 상세 게이트와 중단 조건은 ADR-0013을 따른다.

## Status

**P0·P1·P2·P3 완료.** 커밋 `e77fb77c`의 GitHub-hosted Linux/Windows HWPX plugin
run `33157787880`과 action pin run `33157787944`가 모두 성공해 T0-1~T0-6을 닫았다.
T1-1은 기존 Cargo target surface와 lockfile을 보존한 workspace 이동으로 구현했고, 로컬과
원격 양 OS 회귀·MSRV·host·공급망·설치 검증을 모두 통과했다.
T1-2는 공용 core와 기존 HWP 호환 re-export를 구현했고 run `33162799813`/`33162799808`로
같은 양 OS 게이트를 다시 통과했다.
T1-3은 `officecli-hancom-hwp`와 `.owpml`/`.hml` 직접 읽기, strict HWPML 경계를 구현했고
run `33170785021`/`33170784965`로 같은 게이트를 통과했다.
T1-4/T1-5는 네 확장자 설치 트랜잭션과 HWPX 단독 설치 마이그레이션을 구현했고 run
`33172696561`/`33172696668`로 양 OS 전체 회귀와 실제 네 확장자 경로를 통과했다.
T2-1은 각주/미주 본문 블록과 참조 순서를 구현했고 run `33234858972`/`33234858790`으로
같은 양 OS 게이트와 실제 설치·조회·제거를 통과했다.
T2-2는 공식 r1.3 기반의 엄격 수식→LaTeX 변환과 inline/display 배치를 구현했고 run
`33236634179`/`33236634150`으로 양 OS 전체 회귀·MSRV·host·실제 설치 검증을 통과했다.
T2-3은 구역 spine·머리말/꼬리말·페이지 필드와 구역별 주석 정책을 구현했고 run
`33241159576`/`33241159629`으로 양 OS 전체 회귀·MSRV·host·실제 설치 검증을 통과했다.
T2-4는 NUMBER/BULLET/OUTLINE을 동적 DOCX 목록으로 보존하고 한컴 네이티브 배치 오라클과
49개 공개 코퍼스에서 검증했다. 구현 커밋은 `025418bc`, RHWP level 10 호환성 수정은
`2289ca60`이며 수정 원격 게이트 `33243420505`/`33243420494`가 양 OS에서 성공했다.
T2-5는 활성 이름 스타일과 직접 서식을 분리해 보존하고, 이름 스타일이 소유한 목록·구역 개요를
연결했다. 구현 커밋 `50adcc31`은 281개 공개 코퍼스의 기존 성공/실패 기준선을 유지하면서
실제 OfficeCLI 224항목 replay·lint·validate를 통과했다. 숫자 ID, 선행 `next` 참조,
`lockForm`, 직접 NONE과 스타일 OUTLINE, 암묵 outline 0의 경계는 한컴 2020 네이티브
DOCX 오라클과 ADR-0009에 고정했다.
T2-6은 축 정렬 rectangle/rounded rectangle, 구조적 textbox, whole ellipse의 검증된
부분집합만 typed OfficeCLI drawing으로 내리고 나머지 활성 도형을 명시적으로 거부한다.
구현 커밋 `b379b4d3`은 실제 한컴 rect/ellipse/center/line 오라클, 두 실제 OfficeCLI
replay, 281개 공개 코퍼스와 ADR-0010으로 지원/실패 경계를 고정했다.
T2-7은 커밋 `cd110588`에서 28개 자체완결 chart part를 raw carrier로 보존하고,
관계·caption·미검증 배치와 일반화할 수 없는 schema repair는 fail-closed로 유지했다.
28/28 실제 replay·validate·fingerprint·topology, 533개 Rust 테스트, 44개 host 계약과
ADR-0011로 경계를 고정했다. T2-8은 PUA 대응표와 30파일 전수 분포, 한컴 native
`exam_kor` DOCX를 재검토한 결과 native도 BMP 44회·supplementary 83회를 무변경 보존하므로
현행 보존+진단을 최종 정책으로 확정했다(ADR-0012). T2-9 lint는 unknown prop 0이다.
T3-1은 R6/R8 writer와 R5 DVC를 고정 commit에서 대조해 ADR-0013의 package-preserving
closed-subset 설계로 닫았다. DVC는 정책 checker로 재범위화했다. T3-2a는 writer 전용
G0~G2 `validate_output_package`와 14개 회귀를 구현했으며, 관대한 기존 reader 계약과 분리했다.
T3-2b는 ordered package snapshot과 압축 전/후 SHA-256, explicit changed-part plan,
strict candidate 재열기, preserved ZIP metadata, exact known-semantic expectation을 6개 회귀로
고정했다. 네이티브
49-entry `exam_kor.hwpx` no-op raw copy도 모든 snapshot 동일성을 통과했다. unrelated polygon이
DOCX 변환 모델에 없다는 이유로 안전한 no-op을 막지 않도록 full conversion reader는 표현 가능한
mutation의 추가 semantic oracle로만 사용한다.
T3-3은 source snapshot TOCTOU와 exact part hash를 강제하는 raw-entry COW, 반복되는 Hancom
paragraph id를 ordinal로 분리하는 namespace/parent-chain aware `hp:t` patch, scoped text oracle을
11개 회귀로 구현했다. 네이티브 49-entry package의 한-node edit도 G0~G3를 통과했다. 공개 ZIP
API가 낮추는 central `version made by`는 원본 두 바이트로 복원하며, 재현할 수 없는 ZIP extra
field가 있는 mutation은 fail-closed 한다.
호스트 선행 조건은 커밋 `0429890a`에서 canonical open/save lifecycle 및 fail-closed capability로
수정했다. T3-4/T3-6은 별도 HWPX format-handler에서 읽기 전용 호환성과 strict editable gate를
분리하고, direct plain text set을 multi-target semantic expectation으로 검증한 뒤 sibling-temp
durable atomic replacement에 연결했다. 1 MiB JSONL 입력 상한은 초과 프레임을 추가 대용량
할당 없이 배수하고 다음 요청 경계를 보존한다. 프로토콜의 `max_lines`와 기존 호스트 철자도
호환하며,
호스트 자체는 규격 키로 수정했다. 580개 workspace Rust 테스트, Clippy, release 3-bin build와
47개 host contract가 통과했다.
T3-5는 dump-reader의 HWPX/OWPML 소유권을 제거하고 HWP/HML dump-reader와
HWPX/OWPML format-handler를 역할별 두 바이너리 및 네 활성 설치 경로로 분리했다.
active-first/retire-later 설치와 여섯 경로 conflict-safe rollback은 ADR-0014로 고정했다.
Windows/Unix 설치 계약 18/26개, clean Linux current-host exact discovery 및 실제
HWPX/OWPML `set → save → close → reopen → validate`, source hash/escaped XML delta가
통과했으며 형제 DOCX는 생성되지 않았다. GitHub-hosted Linux/Windows workflow도 같은
current-host 검증을 수행하도록 갱신했다.
P4/P5는 별도로 R2(실제 `.cell`/`.show` 표본)가 들어오기 전까지 착수할 수 없다.

## Next Action Plan

1. P6에서 현재 두 역할 바이너리의 공개 문서·보안/자원 회귀와 native CI 범위를 먼저
   정리한다. `.cell`/`.show` 바이너리를 요구하는 설치 매트릭스 항목은 R2 이후 확장한다.
2. 현재 브랜치의 upstream 지연은 기능 변경과 섞지 않고 별도 통합 변경으로 처리한다.
3. P4/P5는 R2 표본과 Q3가 해소될 때까지 중단한다. Q5 JVM은 참조 구현 조사에만 허용하고
   runtime에는 넣지 않는 기존 단일 바이너리 결정을 유지한다.
4. R2가 확보되면 T4-1 컨테이너 판별 스파이크를 최우선으로 돌려 Q1을 해소하고,
   그 결과로 P4/P5의 실제 규모를 재산정한다.
