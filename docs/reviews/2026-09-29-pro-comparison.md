# Pro 비교 검토와 독립 구현 결과

2026-09-29 · `feat/hwpx-plugin` · 시작점 `5431668a` · 사용자 제공 `Pro.txt` ·
후속 작업 2026-09-30([아래](#후속-작업-결과-2026-09-30))

## 판단과 출처

가장 효과적인 적용 대상은 원본을 편집할 때 필요한 구조·주소·허용 범위를 먼저
드러내는 것이다. 기존의 G0~G3 저장 검증과 제한된 쓰기 정책을 유지하면서
직접 HWPX 조회를 확장했다. 다른 포맷의 파서나 변환 엔진은 추가하지 않았다.

기능 아이디어는 [kordoc의 공개 README](https://github.com/chrisryugj/kordoc/blob/main/README-EN.md)의
구조화 읽기·원본 연결·양식 작업과 비교했다. 실제 한글을 통한 확인 방법은
[jkf87/hwp-mcp README](https://github.com/jkf87/hwp-mcp)를 참고했다. 동명인
다른 hwp-mcp 저장소와 혼동하지 않았다. 외부 저장소의 성능·버전·호환성을 이
프로젝트에서 실측한 것으로 인용하지 않는다. 공개 문서는 2026-09-29 열람 기준이다.

외부 소스를 복사·번역하거나 runtime dependency로 넣지 않았다. 사용자 분석에
포함된 구현 설명을 읽었으므로 인적 분리가 있는 법적 clean-room 인증을 주장하지
않는다. 독립 구현의 근거는 기존 namespace-aware XML 스캔, 저장소 OWPML
parser/fixtures, [공개 규격 출처](../spec-sources.md)다.

## 제안별 적용 여부

| Pro 제안 | 결정 및 실제 결과 |
|---|---|
| P0 호스트/플러그인 조합 | 소스 빌드와 typed host proxy를 넘어 실제 CLI까지 검사. 아래 JSON 출력 결함을 발견해 공용 formatter 수정. 사용자 설치의 구형 HWPX dump-reader를 기존 설치기로 승격 |
| P0 outline/issues 의미 | 빈 성공을 제거하고 `unsupported_feature` 반환. 패키지 `validate`와 구분 |
| P1 표/셀/주석/필드 읽기 | 네 타입의 `get`/`query` 추가. 병합·빈 셀·중첩 구조를 원문 좌표와 기존 문단 경로로 연결 |
| P1 원본/편집 가능성 | part SHA-256, 요소 바이트 범위, 포함 경로, 세션별 편집 가능 텍스트 및 잠금 이유 공개 |
| P1 제한된 양식 채우기 | 기존 명시적 text 경로 재사용. paired 빈 텍스트/분할 런의 정확한 대상 치환·저장 검증. revision 조건과 혼합 미지원 속성의 부분 적용 거부 추가. 이름 기반 자동 매칭·bulk fill은 도입하지 않음 |
| P1 캐시/렌더 | 기존 writer는 지정한 텍스트 바이트만 바꿈. `linesegarray` 삭제·레이아웃 재계산은 실제 한글 비교 근거가 없어서 보류. 09-30: RHWP로 영향만 재현했고 한글 비교는 승인 대기 |
| P2 분할 읽기 | 기존 줄 범위 조회의 JSON에 경로·전체 줄 수·앞뒤 생략량 추가 |
| P2 내용/구조 diff | 원본 연결 기반을 마련했으나 별도 diff 엔진은 보류. 서로 다른 문서에서 같은 ordinal을 동일 객체로 간주하지 않음 |
| P2 이미지 직접 조회 | 09-30: 공개 표본을 수용 자료로 `//picture` 추가. manifest id가 유일하고 part가 있을 때만 연결 |
| 후순위 HWP 직접 패치/광범위 구조 편집 | 수요·회귀 자료·native oracle 확보 전 보류. 기존 RHWP 경로 재사용 |

## 구현 경계

`format_handler/source_index.rs`는 기존 읽기 루프의 XML 이벤트를 관찰한다.
변환용 모델은 셀 주소 보정과 XML 위치 손실이 있어 원본 주소로 역변환하지 않는다.
기존 문단/텍스트 경로를 유지하고 구조 객체는 구역 내 종류별 ordinal로 식별한다.
`parent_path`, `paragraph_paths`, `text_paths`가 실제 연결이며 `children`의 평탄한
조회 목록을 XML 계층으로 오해하지 않아야 한다.

필드의 이름과 원문 `editable` 속성은 쓰기 권한이 아니다. ID가 중복되거나 끝
마커가 없거나 순서/subList 영역이 맞지 않으면 미해결 상태를 공개한다. 셀 좌표가
없으면 채워 넣지 않는다. XML 깊이 128, 구역별 요소 100,000, 관계 200,000의
정확한 허용/거부 경계를 검사한다. 원본 해시는 part 단위이며 편집 후 갱신된다.

실제 resident는 첫 수정 전까지 읽기 전용이므로, 권한과 후보를 구분했다.
`text_candidate`/`candidate_target_paths`는 구조상 후보이며 `editable`/`target_paths`는
현재 세션 권한이다. 후보만 보고 strict editable 패키지 검증을 통과했다고 판단하지 않는다.

`set`은 새 인덱스가 완성된 뒤에만 pending 변경을 게시한다. 오래된 revision,
잘못된 guard, 미지원 속성이 섞인 텍스트 수정은 적용하지 않는다. 여러 `set`을
자동으로 묶는 트랜잭션은 추가하지 않았다. 이미 성공한 변경은 후속 실패로 취소되지
않는다. self-closing 텍스트·혼합 내용·구조 편집은 기존 쓰기 경계 밖이다.

## 실제 CLI에서 추가로 발견한 결함

typed proxy의 `DocumentNode.Format`에 들어온 값은 `JsonElement`다. 호스트의
`AppJsonContext`에 이 타입의 source-generation metadata가 없어 `get/query --json`이
`internal_error`로 실패했다. 단순 세션/반사 기반 JSON 출력만으로는 잡히지 않았다.
`FormatNode`/`FormatNodes` 호출자를 CLI, resident, batch까지 추적한 뒤 공용
컨텍스트에 타입 등록 한 줄을 추가했다. 중첩 객체·배열·Boolean·숫자·null을
실제 플러그인 역직렬화 방식으로 읽고 다시 출력하는 회귀 테스트를 남겼다.

## 검증 기록

- 구조 조회 테스트는 수정 전 `//table` 등 `invalid_argument`로 실패했다.
- 호스트 출력 회귀는 수정 전 `JsonElement` metadata 누락으로 실패했다.
- Rust 1.98.0: 전체 workspace 652개 통과; clippy `-D warnings` 통과.
- Rust 1.88.0: 최종 workspace `check --locked` 통과.
- .NET 10.0.302: 최종 호스트 계약 60개 통과. 기존 Excel `SheetShift.cs:538`의
  nullable 경고 CS8602는 남아 있으며 이번 변경에서 해당 코드는 수정하지 않았다.
- 기존 코드의 Windows 보호 DACL/원자 교체 검사는 샌드박스에서 접근 거부됐다.
  실제 사용자 권한으로 재실행해 통과했으며 검사를 완화하지 않았다.
- debug host + 실제 Rust 바이너리: typed 구조 query/get, revision guard,
  set/save/read-only reopen, validate 통과.
- release 네 바이너리 빌드 완료. 기존 설치기로 여섯 활성 경로를 등록하고
  `dump-reader/{hwpx,owpml}` 두 경로를 폐기했다. 변경 전 네 실행 파일은
  `plugins/hancom/.officecli/review-20260929/install-backup/`에 별도 보관했다.
- 저장소 apphost `src/officecli/bin/Debug/net10.0/officecli.exe` + 설치된 release:
  공개 CLI discovery, resident open/query/get, 후보 조회, 조건부 set, 오래된 revision 거부,
  outline 미지원, save/close/read-only reopen/validate 통과. DOCX sidecar가 생기지 않음도 확인했다.
- `git diff --check` 및 새 문서 링크 검사, 코드 그래프 갱신 완료. 변경은 미커밋 상태다.

실행 로그는 `%TEMP%/hwpx-pro-{rust-tests,clippy,msrv,host-tests,host-red,real-host,release,install,cli}-20260929.log`에 있다.
재현 harness와 합성 fixture는 Git에서 제외된
`plugins/hancom/.officecli/review-20260929/`에 있다. 제품 회귀 테스트는
`hwpx_format_handler.rs`, `source_index.rs`, `tests/OfficeCli.Tests/Program.cs`에 남는다.

## 후속 작업 결과 (2026-09-30)

아래 네 후속 항목을 이어서 진행했다. 첫 시도는 시스템 오류(bugcheck 0x1E)로 재부팅되어
다시 시작했다.

### 1. 실제 한글 비교: 도구 준비, 실행은 승인 대기

- 09:54~09:55 프로브는 한컴오피스 2020(`Hwp.exe` 11.0.0.9136)을 새 자동화 인스턴스로
  띄웠다. 생성한 임시 HWPX를 열고 한글을 정상 종료한 뒤 09:55:04에 끝났다. 보안 모듈을
  등록하지 않아도 `Open(..., "HWPX", "forceopen:true")`가 성공했다. WPF 창 하나가
  나타났지만 호출을 막지 않았고, 감시기는 아무 버튼도 누르지 않았다.
- 09:58 재부팅 기록에는 bugcheck 0x1E(`0xc0000005`)가 남았다. 덤프는 관리자 권한으로만
  읽을 수 있다. 전날에는 한글 자동화 없이 0xD1이 있었다. 인과를 확인하지 못했으므로
  승인 전에는 한글 자동화를 다시 실행하지 않았다.
- `native_cache_oracle.py`는 기존 한글에 연결하지 않고 임시 복사본만 연다. 보안 모듈은
  등록하지 않고, 대화상자에서는 거부·취소만 누른다. 아래 세 사례의 모든 변형에 대해
  쪽수, PDF, 한글 재저장본의 `hp:lineseg` 수를 기록한다.
  `--i-accept-hangul-automation` 없이는 실행을 거부함을 확인했다.

### 2. 조판 캐시 영향: RHWP로만 재현

RHWP v0.8.4는 한글이 아닌 독립 렌더러다. 공개 Hancom 표본의 대상 텍스트를 실제
format-handler로 길게 바꾼 뒤, 캐시를 지운 두 변형을 만들어 비교했다.

| 사례 | 캐시 유지(현재 writer) | 편집 문단 캐시만 삭제 | 구역 전체 캐시 삭제 |
|---|---|---|---|
| 표 셀 긴 텍스트 (`basic-table-01`) | 1줄로 셀 밖에 넘침 | 10줄, 표가 늘어남 | 10줄 |
| 병합 셀 (`table-text`, colSpan 4) | 1줄로 표 밖에 넘침 | 4줄, 행 높이는 그대로라 다음 행과 겹침 | 4줄 |
| 본문 문단 (`footnote-01`, 원래 6쪽) | 15줄, 7쪽 | 15줄, 7쪽 | 15줄, 6쪽 |

편집하지 않은 원본도 캐시를 모두 지우면 RHWP에서 6쪽이 5쪽이 됐다. 캐시 삭제는 중립적인
처리가 아니므로 writer는 바꾸지 않았다. 삭제 여부와 범위는 한글 결과로 정한다.

### 3. Python ZIP writer 저장 거부: 원인과 수정

- 원인은 검증이 아니라 writer였다. 기존 COW는 `zip` crate로 모든 entry header를 다시
  만들었고, 이때 Unix mode에 regular-file 비트가 더해졌다(`0o600`→`0o100600`). G3가
  이를 올바르게 거부했다.
- 같은 재생성은 G3가 보지 않던 필드도 바꿨다. 09-29 빌드로 공개 표본 8개를 편집·저장하면
  모두 성공으로 보고됐지만, 91개 entry의 header 필드 273개가 바뀌었다(`version needed`
  20→10, DOS 속성 `0x20` 및 flag `0x4` 제거).
- `owpml::zip_layout`은 바뀌지 않은 entry와 header 바이트를 그대로 복사한다. 교체 entry는
  CRC와 크기만 바꾼다. G3와 TOCTOU 검사는 raw header 바이트까지 비교한다. 보존을 증명할 수
  없는 layout은 editable open에서 거부하며, G0~G2 기준은 완화하지 않았다
  ([ADR-0018](../adr/0018-hwpx-byte-preserving-zip-cow.md),
  [C14](../../plugins/hancom/docs/01-protocol-contract.md#c14-hwpx-저장의-zip-바이트-보존)).
- 새 COW 테스트 4개는 수정 전 커밋에서 실패하고 수정 후 통과한다. Python writer 세
  방식이 모두 저장된다. 공개 표본 8개의 저장 후 header 차이는 0이다. 2개는 기존 strict
  검사가 편집 open에서 거부한다(manifest의 `D:\다운로드\` 경로, 압축된 `mimetype`).

### 4. 그림 직접 조회

- `//picture`는 `hp:pic`의 직접 `hc:img` 참조, 원문 HWPUNIT `orgSz`/`curSz`/`sz`, 하나뿐인
  `shapeComment`, 원본 위치를 반환한다. binary part는 OPF manifest id가 유일하고 part가
  실제로 있을 때만 연결한다. 외부 연결·누락·중복은 상태 값으로 공개하고, DOCX reader의
  파일명 추측은 쓰지 않는다. 그림은 읽기 전용이다.
- 수용 자료는 rhwp 커밋 `496333b2`의 공개 표본 12개다(Hancom 10~13 생성, git blob SHA-1
  확인). `test-image` 5개, 표 셀 안의 `tb-img-03` 1개, `paper_anchor_infront_pic` 1개가
  해결된다. `issue1891_external_bindata_link`는 외부 연결 4개와 내장 2개로 구분된다.

### 5. 보류를 유지한 항목

- 필드 이름 기반 자동 채우기: 표본의 필드 52개 중 7개는 이름이 없었다. 이름 있는 45개 중
  18개는 다른 필드와 이름이 같았다. 명시 경로 편집을 유지한다.
- 내용/구조 diff: 이번 자료로는 문서 간 객체 대응 기준을 정할 수 없어 보류한다.

### 6. 설치기와 CI 재현

- `install.ps1`은 Windows PowerShell 5.1에 없는 `[IO.Path]::IsPathFullyQualified`를 써서,
  파일을 바꾸기 전 첫 경로 검사에서 실패했다. README는 5.1 실행을 안내하고 있었다.
  `#Requires -Version 7.0`, README, 계약 테스트를 추가했다.
- 원격 CI는 커밋·push가 필요해 실행하지 않았다. 대신 `test` job 단계를 작업 트리
  스냅샷으로 로컬 재현했다.
  - Linux: `rust:latest`(Rust 1.97.1) 컨테이너 + .NET 10.0.302 runtime. 테스트 686개, verifier
    discovery, clippy, release 빌드, 대용량 smoke, 설치·`plugins list` 등록 6개, RHWP HWP
    보기(원본 hash·mtime 불변), HWPX/OWPML/HML 보기, 편집 저장·재열기·validate, Cell/Show
    carrier, 제거까지 통과했다. writestr 패키지 저장도 추가로 확인했다.
  - Windows: 설치기는 격리된 `HOME`에 설치하고 호스트는 `OFFICECLI_PLUGIN_*`로 연결했다.
    테스트 667개, verifier discovery, clippy, release 빌드, 대용량 smoke, 설치본과 release
    hash 일치, HWP/HWPX/OWPML/HML 보기, 편집 저장·재열기·validate, writestr 저장, Cell/Show
    carrier, 제거까지 통과했다. 실제 사용자 플러그인 파일 6개의 hash는 전후 같았다.
    격리 HOME은 호스트의 사용자 경로 열거에 보이지 않으므로 `plugins list` 단계는 Linux에서만
    재현했다.
  - 재현하지 못한 것: macOS job.
- CI MSRV job 결함: `plugins/hancom/rust-toolchain.toml`의 `stable`이 `rustup default 1.88.0`보다
  우선한다. 그래서 job의 plain `cargo check`는 Rust 1.88이 아닌 최신 stable로 검사하고 있었다.
  원격 job을 그대로 재현하면 stable을 설치한 뒤 `rustc 1.98.1`로 성공한다. MSRV job을 추가한
  `d910b40d`(08-13) 때부터 job 작업 디렉터리에 이 파일이 있었다. 따라서 원격 "MSRV 통과"
  기록은 모두 1.88 검사가 아니다. 로컬에서 `cargo +1.88.0`으로 명시한 검사는 유효하다.
  단계를 `cargo +1.88.0`으로 바꿨고, 고친 단계를 Linux 컨테이너에서 그대로 실행해 `rustc 1.88.0` check가 통과했다.
  Windows의 `cargo +1.88.0 check`도 통과했다. 고친 job의 원격 결과는 push 후 확인한다.

### 검증 기록 (2026-09-30)

- Rust 1.98.0: workspace 667개 통과, clippy `-D warnings`, rustfmt check(59개 파일).
  Rust 1.88.0 `check --locked` 통과.
- 별도 에이전트의 읽기 전용 코드 검토에서 결함 1건이 나왔다. 서명 없는 12바이트 data
  descriptor의 CRC가 서명 값(`PK\x07\x08`)과 같으면, 교체 저장 중 길이가 맞지 않아 panic이
  날 수 있었다. descriptor 형식을 저장된 길이로 판별하도록 고치고 회귀 테스트를 추가했다.
  나머지 두 건(공개 `verify_candidate`가 exact hash 강제를 호출자에게 맡기는 점, `set`의
  디스크 재읽기 창)은 이번 변경 전부터 있던 설계다. 현재 저장 경로는 exact hash 계획만 쓰고
  저장 직전에 원본을 다시 비교하므로 바꾸지 않았다.
- .NET 10.0.302: 호스트 빌드와 계약 테스트 60개 통과.
- release 빌드를 기존 설치기(pwsh 7)로 설치했다. 설치 전 파일은
  `review-20260930/install-backup/`에 있다. 저장소 apphost와 설치 release로 실제 CLI
  smoke를 통과했다. 09-29 구조/revision 흐름, Python writestr·native 모양 패키지의
  set/save/reopen/validate(비교 header 필드 84개 불변), `//picture`와 중첩 크기 객체 출력을
  확인했다.
- 로그는 `%TEMP%/hwpx-pro-{rust-tests,clippy,msrv,msrv-linux,msrv-ci-exact,fmt,host-build,host-tests,release,install,cli-run,ci-linux,ci-windows,zip-red}-20260930.log`에 있다.
  harness와 공개 표본은 Git에서 제외된 `plugins/hancom/.officecli/review-20260930/`에 있다.
  CI 재현용 작업 트리 스냅샷은 정리했다. `make-snapshot.ps1`로 다시 만들 수 있다.

## 남은 검증과 후속 우선순위 (2026-09-30 갱신)

1. 실제 한글 비교: 사용자 승인 후 `native_cache_oracle.py`를 실행한다. 먼저 관리자 권한으로
   `C:\Windows\Minidump\093026-11093-01.dmp`를 분석(`!analyze -v`)하는 것을 권한다.
   결과로 `hp:linesegarray` 유지 또는 편집 문단만 삭제를 정한다. 그때까지 긴 치환 뒤에는
   한글에서 결과를 확인해야 한다.
2. 원격 CI: 커밋·push 승인 후 Linux/Windows/macOS test와 MSRV job을 실행한다. 고친 MSRV
   단계(`cargo +1.88.0`)의 첫 원격 결과를 확인한다.
3. strict 검사가 거부한 Hancom 표본 2개(manifest 비이식 경로, 압축된 mimetype)는 읽기
   전용이다. 편집 허용은 별도 근거가 필요하다.
4. 내용/구조 diff와 필드 이름 기반 자동 채우기는 보류한다.

상세 wire 계약은 [C13](../../plugins/hancom/docs/01-protocol-contract.md#c13-hwpx-구조-조회와-원본-참조)과
[C14](../../plugins/hancom/docs/01-protocol-contract.md#c14-hwpx-저장의-zip-바이트-보존),
설계 결정은 [ADR-0017](../adr/0017-hwpx-source-aware-read-model.md)과
[ADR-0018](../adr/0018-hwpx-byte-preserving-zip-cow.md)을 따른다.
