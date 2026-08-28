# Task Plan: HWPX 후속 개발

## Goal
기존 HWPX 플러그인 계획의 완료 상태를 재검증하고, 확인된 보안·호환성 위험을 우선순위에 따라 수정하여 회귀 검증까지 마친다.

## Phases
- [x] Phase 1: 기존 작업 계획·Git 상태·검증 기준 재확인
- [x] Phase 2: 보안 및 OfficeCLI/플랫폼 호환성 위험 분석
- [x] Phase 3: 확인된 보안 위험에 대한 실패 테스트와 구현
- [x] Phase 4: 전체 회귀·왕복 검증 및 결과 문서화
- [x] Phase 5: 타 플랫폼·대용량 호환성 실측
  - [x] Windows volume/file ID 기반 하드 링크 차단 구현 및 크로스 타깃 컴파일
  - [x] Linux 비 UTF-8 실파일 및 Windows 하드 링크 CI 검증 추가
  - [x] Windows 네이티브 플러그인 설치 스크립트와 설치 무결성 검증 추가
  - [x] 대용량 파싱 heartbeat와 블록 단위 JSONL 출력
  - [x] 로컬 전체 회귀 및 실제 OfficeCLI 왕복 재검증
  - [x] GitHub Linux/Windows 러너의 첫 네이티브 실행 결과 확인
  - [x] 실제 Windows OfficeCLI `plugins list` 디스커버리 CI 첫 결과 확인
  - [x] Linux/Windows 선언 MSRV 1.88 전용 CI 첫 결과 확인
  - [x] macOS 합성 48MiB 표본 1회 wall-time/RSS 및 실제 OfficeCLI watchdog 실측
- [ ] Phase 6: 바이너리 HWP 선택적 지원
  - [x] H3a: RHWP v0.8.4 CLI 계약과 배포 체크섬 재검증
  - [x] H3b: 변환기 탐색·실행·임시 산출물 정리 경계를 TDD로 구현
  - [x] H3c: 변환 실패·출력 누락·비 HWPX 출력·원본 불변 보안 회귀 검증
  - [x] H3d: UTF-8 staging, bounded stderr, Unix process group, Windows Job Object 보강
  - [x] H3e: Linux/Windows 네이티브 브리지·프로세스 트리 CI 첫 결과 확인
  - [ ] H1: H3 완료 뒤에만 `.hwp` 디스커버리와 설치 경로를 활성화
    - [x] H1a: 매니페스트·두 환경변수·두 사용자 설치 경로의 로컬 구현
    - [x] H1b: Unix/PowerShell 설치·제거 계약과 로컬 OfficeCLI smoke
    - [ ] H1c: Linux/Windows 네이티브 `.hwp` discovery와 실제 RHWP `view` CI
    - [ ] H1d: 새 원격 증거를 반영한 공개 문서의 정식 지원 승격
  - [x] H4: 실제 HWP/HWPX 쌍과 공식 4표본 동등성 및 OfficeCLI 왕복 검증
  - [x] H5: README·ADR·인수인계 문서 갱신
- [x] Phase 7: OfficeCLI host discovery·공급망 하드닝을 별도 원자 변경으로 진행
  - [x] 상대 `OFFICECLI_PLUGIN_*` 경로를 host에서 거부하는 회귀 테스트와 구현
  - [x] 동일 매니페스트의 다중 설치 경로를 `plugins list`에서 다룰 identity/dedup 정책
  - [x] `.hwp` PATH alias 지원 여부와 설치 범위 결정
  - [x] installer ancestor reparse 정책과 Windows 악성 junction 제거 회귀를 네이티브로 검증
  - [x] GitHub Actions 외부 action 참조를 검증된 commit SHA로 고정

## Key Questions
1. 기존 `docs/03-work-plan.md`에서 실제로 남은 기능은 무엇인가?
2. 신뢰하지 않는 HWPX 입력이 자원 고갈, 경로 처리, XML/ZIP 처리 문제를 일으킬 수 있는가?
3. 최신 OfficeCLI와 macOS 외 플랫폼에서 계약이나 설치 방식이 달라질 위험은 무엇인가?

## Decisions Made
- 정식 작업 위치는 이 포크의 `plugins/hwpx`로 유지한다.
- 기존 기능 계획이 완료로 표시되어도 테스트와 실제 OfficeCLI 왕복 검증으로 다시 확인한다.
- 새 작업은 보안·호환성 위험을 별도 계획으로 기록하고, 재현 테스트가 가능한 항목부터 수정한다.
- P0/P1과 P-1은 완료로 확인했다. G5 스타일 매핑과 G6 PUA 치환은 실패가 아니라 실측 근거 부족에 따른 의도적 보류로 유지한다.
- 정식 보안 스캔에서 7건(높음 2, 중간 3, 낮음 2)을 확인했다. 우선 압축 자원 예산, 표 크기·산술 검증, 중복 섹션 제거를 같은 입력 검증 경계에서 수정한다.
- 반복 이미지 증폭은 고유 BinData 캐시와 참조·출력 예산을 함께 적용해야 완결된다. 진단 경로 이슈와 Windows 설치 경로는 그 다음 호환성 묶음으로 처리한다.
- 정식 스캔 7건은 먼저 macOS/Unix 실행 경로의 입력 경계·이미지 캐시·진단 경계에서 닫았다. Phase 5에서 Windows 하드 링크 식별 구현도 추가했고 네이티브 Windows runner에서 확인했다.
- 진입점은 `args_os`를 사용하며 비 UTF-8 경로 바이트를 `PathBuf`로 보존한다. macOS 파일시스템은 잘못된 UTF-8 파일명 생성을 거부하므로 실제 파일 왕복은 Linux 러너에서 추가 확인한다.
- Phase 5 착수 시 남은 호환성 작업은 Linux 비 UTF-8 실파일, Windows 네이티브 설치/하드 링크 식별, 30초 pre-output idle timeout이었다. 기존 네이티브 플랫폼 검증과 macOS 합성 48MiB 1회 실측에 이어 discovery/MSRV CI의 첫 결과도 확인했다.
- OfficeCLI는 stderr의 `{"heartbeat":true}` 행을 소비하면서 idle 타이머만 갱신한다. 파싱 단계에는 10초 주기 heartbeat를 적용하고 emitter는 문서 전체가 아니라 최상위 블록 단위로 출력한다.
- Windows 파일 동일성은 열린 핸들의 volume serial과 64-bit file index를 비교한다. 안정화되지 않은 Rust `MetadataExt` 대신 `windows-sys`의 `GetFileInformationByHandle`을 사용하고 Windows 크로스 타깃으로 전체 target을 컴파일한다.
- Linux/Windows 네이티브 런타임 테스트는 `.github/workflows/hwpx-plugin.yml`에 고정했다. GitHub Actions run `31572303544`에서 두 job과 Windows 네이티브 설치 검사가 모두 성공해 플랫폼 하위 게이트를 완료 처리한다.
- 기존 Windows job은 설치 경로와 `--info`만 확인했으며 실제 OfficeCLI discovery는 아니었다. 체크섬을 고정한 OfficeCLI 1.0.143으로 `plugins list`까지 실행하도록 후속 검증을 추가한다.
- 선언 MSRV 1.87은 잠금된 `zip 8.6.0`의 요구사항(Rust 1.88)과 충돌했다. 보안 입력 경계 의존성을 내리지 않고 MSRV를 1.88로 정정해 전용 CI에서 고정한다.
- Phase 6의 첫 구현은 `docs/04-hwp-support-plan.md`의 H3 변환기 경계다. 변환 결과를 다시 HWPX로 판별하고 원본 옆에는 파일을 쓰지 않으며, 외부 프로세스는 shell 없이 OS 원시 인자를 전달한다.
- H3가 실제 RHWP와 회귀 검증을 통과하기 전에는 매니페스트·설치 스크립트에 `.hwp`를 광고하지 않는다.
- macOS arm64에서 48MiB 저장형 HWPX를 1회 실측해 첫 출력 0.471초, 전체 0.540초, peak RSS 106.1MiB, 원본 불변을 확인했다. OfficeCLI 1.0.143의 30초 watchdog 아래 `plugins lint`도 0.771초/미지 prop 0으로 통과했다. 단일 합성 표본이며 실행이 10초보다 빨라 heartbeat 프레임 자체는 관찰되지 않았다.
- RHWP v0.8.4 공식 macOS arm64 자산 SHA-256을 `SHA256SUMS.txt`와 대조했다. 공식 HWP5 3종과 HWP3 1종은 새 브리지에서 19/712/48/467 JSONL 행으로 성공했다.
- RHWP v0.8.4가 비 UTF-8 argv를 받지 못하므로 원본을 UTF-8 고정명의 private scratch에 복사하고 그 staging 경로만 전달한다. 원본 hash·mtime은 유지한다.
- 외부 변환기는 shell 없이 세 인자를 분리하고 256MiB staging copy 예산, 120초 총 제한, 8KiB stderr tail, HWPX 재판별을 적용한다. Unix process group과 Windows Job Object로 자손 프로세스를 정리하며 stderr drain도 bounded다.
- `scripts/verify-hwp-pairs.py`로 실제 동명 HWP/HWPX 1쌍은 JSONL byte-for-byte 일치, unknown prop 0, OfficeCLI 구조/스키마 검증 일치를 확인했다. 공식 HWP5 3종·HWP3 1종의 RHWP 생성 쌍도 같은 검증을 통과했다.
- GitHub Actions run `31700156231`에서 `d910b40d`의 Linux/Windows 네이티브 test·clippy·release와 양 OS Rust 1.88 MSRV job 4개가 모두 성공했다. Windows 설치 뒤 실제 OfficeCLI `plugins list`도 통과했으므로 Phase 5와 H3e 원격 게이트를 완료 처리한다.
- Sol Pro 계획의 우선순위 중 구현 브랜치 `d910b40d`를 기준선으로 삼고 H1 `.hwp` 공개 디스커버리를 시작한다는 판단은 현재 코드·CI와 일치한다. 기본 브랜치 병합/PR은 15,184행 규모의 별도 원자 검토가 필요하므로 H1 구현과 섞지 않는다.
- H1 완료는 로컬 계약, 실제 OfficeCLI resolution, Linux/Windows 네이티브 RHWP view, 공개 문서 승격으로 나눠 판정한다. 사용자 경로별 열거 때문에 `plugins list`에 같은 매니페스트가 두 행 보일 수 있으므로 목록의 정확한 행 수는 resolution 증거로 사용하지 않는다.
- 프로토콜은 환경변수 플러그인 경로를 절대경로로 요구하지만 현재 OfficeCLI host는 상대경로도 후보로 받는다. 설치기가 출력하는 경로는 절대경로로 유지하고, host absolute-path enforcement와 목록 dedup/PATH alias 정책은 H1 변경과 분리한 후속 보안·호환성 변경으로 다룬다.
- H1 workflow의 OfficeCLI·RHWP·HWP fixture는 버전·전체 commit URL·SHA-256으로 고정한다. `actions/checkout`과 toolchain action의 commit SHA 고정은 기능 변경과 분리해 Phase 7 공급망 하드닝으로 추적한다.
- 양 extension 설치는 백업 여부와 새 target commit 여부를 따로 추적해야 한다. install/uninstall 모두 `HOME` 아래 `.officecli`부터 extension 디렉터리까지 기존 조상을 사전 검사해 symlink/junction/reparse와 non-directory를 fail-closed로 거부한다. Unix rollback은 한 target 정리가 실패해도 나머지 복원을 계속하고, 검증된 설치 뒤 백업 정리만 실패하면 성공 상태와 복구 백업을 보존한다. 스크립트 검사 직후 같은 권한 주체가 경로를 바꾸는 경쟁은 handle 기반 설치기가 아니므로 잔여 한계다.
- Python 검증기는 공통 실행파일 탐색기를 사용한다. Windows는 `.exe`, Linux는 확장자 없는 이름만 자동 선택하고, `CARGO_TARGET_DIR`가 있으면 해당 release 디렉터리를 우선한다. 실행파일 탐색 단위 테스트와 양 OS 1MiB large-file smoke를 workflow에 고정한다.
- Phase 7의 목록 identity는 정규화된 실행 경로다. 같은 안정 이름과 같은 전체 `--info` JSON의 여러 경로는 행을 유지하고 이름 해석만 첫 discovery 우선순위로 결정한다. 알려지지 않은 필드까지 포함한 canonical JSON fingerprint가 다르면 두 행에 경고하고 이름 기반 `info`/`lint`를 `plugin_name_ambiguous`로 거부한다.
- `plugins list`와 이름 기반 `info`/`lint`도 환경변수·사용자·bundled·PATH의 네 discovery class를 일관되게 본다. 환경변수와 PATH 디렉터리는 fully-qualified 경로만 허용하며, PATH에서는 모든 `officecli-<kind>-<ext>` 후보를 모든 short alias보다 먼저 찾는다.
- `.hwp`는 수동 PATH 별칭 `officecli-dump-reader-hwp`와 `officecli-hwp`를 모두 지원한다. HWPX 플러그인 설치기는 더 높은 우선순위의 사용자 extension 경로만 관리하고 전역 PATH 별칭은 만들지 않는다.
- 저장소의 외부 GitHub Action 참조 25곳은 2026-08-28에 공식 ref와 대조한 전체 commit SHA로 고정한다. 전용 workflow는 CPython 3.13과 PyYAML 6.0.3 wheel hash를 고정하고 workflow/reusable job 및 저장소 전역 composite action의 실행 위치만 의미적으로 순회해 symbolic ref 재도입을 막는다. PR에서는 read-only `pull_request_target`이 base의 trusted checker로 candidate를 데이터로만 검사하며 경로 filter를 두지 않는다. 입력은 non-symlink regular file, 1MiB, YAML depth/event/alias/node/merge-expansion 예산 안에서만 읽고 파싱한다.
- 전체 host 열거는 후보 256개, 후보별 stdout 1MiB, 정상 매니페스트 합계 16MiB, probe 합계 30초를 all-or-error 경계로 둔다. stderr는 모두 drain하되 16KiB만 보존하며 디렉터리 스캔은 root별 4,096개로 제한한다.
- 이름 기반 `plugins info`의 두 번째 probe는 최초 열거 identity와 달라지면 `plugin_manifest_changed`로 거부한다. 명시적 실행 경로는 raw future field와 typed manifest가 같은 snapshot이 되도록 한 번만 probe한다.

## 2026-08-28 Final Local Validation
- Windows Rust 전체 회귀 225개, `cargo clippy --locked --all-targets -- -D warnings`, release build, Rust 1.88 all-target check가 통과했다.
- Windows installer 계약 12/12와 고정 digest 비-root Linux 컨테이너 installer 계약 21/21이 통과했다.
- 로컬 OfficeCLI host 계약 35/35가 통과했다. 무한 stdout/stderr는 외부 8초 process-tree kill로 격리하고, 실제 `plugins list/info`, 16MiB 합계 예산, snapshot 변경, heartbeat reset, 후보 256개 및 비정상 사용자 프로필 경계를 포함한다.
- 실행파일 탐색 Python 6/6, action pin 17/17, workflow YAML 8개 parse, PowerShell parse, Bash 구문, 두 shell script LF 고정, `git diff --check`가 통과했다.
- 공식 OfficeCLI 1.0.145 양 OS 자산과 RHWP 0.8.4로 로컬 Windows 및 고정 digest 비-root Linux 컨테이너에서 실제 `english.hwp` discovery/view, 기대 텍스트, DOCX 생성, 원본 hash/mtime 불변, 중간 HWPX 미생성과 직접 HWPX 회귀를 확인했다.
- 로컬 Windows에는 고정 SDK 10.0.302 대신 10.0.400만 있다. 10.0.400 직접 MSBuild로 기본 solution과 계약 harness 35개를 빌드·실행했고, 공식 Linux SDK 10.0.302 이미지(`sha256:7a91ccecc26d71bf7688c627a6b5eae2e27bb2cd1e37e8abe738348904245692`)에서는 프로필 경계 추가 전 host 계약 34/34를 통과했다. 최신 35개 정확한 양 OS 검증은 새 CI job에 고정했다.

## Errors Encountered
- 2026-08-28 최종 공급망 감사에서 action pin 검사가 정규식으로 YAML을 흉내 내 flow-style과 escape된 `uses` 키를 놓치고, `run` 문자열 내부 가짜 키와 folded scalar를 오탐하는 RED를 확인했다. 후속 재감사에서는 임의 디렉터리/mixed-case composite action 누락, `$/` self-reference 오탐, 작은 merge-anchor 폭탄, 명시 mapping 폭을 숨긴 aggregate merge 폭탄, symlink 무제한 선읽기와 fork checkout opt-in 누락을 RED로 재현했다. 해시 고정 PyYAML의 의미 파싱 앞에 regular-file·bounded-read·문서별 및 전체 구조 복잡도 예산을 두고, 저장소 전역 action metadata, immutable Docker digest 및 base-trusted data-only PR 검사를 17개 계약으로 고정했다.
- 2026-08-28 외부 AI 교차 검토에서 `GetFolderPath(UserProfile)`의 빈 결과가 current-directory 상대 plugin root로 바뀌어 `FindFor`와 목록 경로가 어긋나는 P0를 확인했다. 프로필 문자열을 독립적으로 정규화하는 helper와 empty/whitespace/relative/drive-relative/root-relative/absolute 계약을 RED→GREEN으로 추가하고 두 discovery 경로가 invalid root를 함께 생략하도록 고쳤다.
- `Chiriri722/Hwpx-OCLI`의 `main`에는 현재 branch protection과 CODEOWNERS가 없다. base-trusted `pull_request_target`은 현재 PR이 checker를 no-op으로 바꾸는 우회는 막지만, maintainer가 gate 약화를 승인·merge하는 governance 위험까지 대신하지는 않는다. tamper-resistant 정책이 필요하면 default branch에 도달한 뒤 protected required workflow 또는 별도 신뢰 주체의 CODEOWNERS 필수 승인을 저장소 설정에서 구성해야 하며, 단독 소유자의 정상 gate 갱신을 잠그는 자동 byte-equality 검사는 적용하지 않는다.
- 2026-08-28 최종 diff 감사에서 Unix 재설치 완료 뒤 백업 삭제 실패가 유효한 설치를 실패로 보고하고, rollback의 첫 target 삭제 실패가 뒤의 백업 복원을 중단하는 경계를 찾았다. 두 실패를 명령 wrapper로 주입해 RED를 확인한 뒤, 정리 실패는 경고와 백업 보존으로 낮추고 rollback은 모든 target의 복원을 독립적으로 끝까지 시도하도록 고쳤다. 고정 digest 비-root Linux 계약 21개가 통과했다.
- 2026-08-28 재감사에서 H1이 포함된 GitHub Actions push run `31890284597`와 `32793306250`가 모두 실패한 사실을 확인했다. 최신 run의 Rust test·Clippy·release·large-file smoke와 양 OS MSRV job은 통과했다. Linux는 설치·manifest·`plugins list --json` 뒤 실제 HWP `view`에서, Windows는 설치·manifest 뒤 `plugins list --json`에서 실패했다. 두 표면 오류는 OfficeCLI 1.0.143의 `WriteError`가 `System.Private.Xml, Version=10.0.0.0`을 로드하지 못한 것이어서 최초 예외를 가린다. H1c는 실패 상태이며 정상 배포 자산으로 다시 검증해야 한다.
- 로컬 SDK는 저장소가 고정한 10.0.302 대신 10.0.400만 있어 일반 repo root 빌드가 거부됐다. 10.0.400의 MSBuild를 직접 호출하고 sandbox가 Roslyn compiler-server pipe를 거부하는 경로는 `UseSharedCompilation=false`로 우회해 기본 solution과 무의존 host 계약 harness를 빌드했다. 공식 Linux SDK 10.0.302 컨테이너에서도 harness 34개를 통과했으며, 정확한 Windows SDK 증거는 CI에 남아 있다. Windows NuGet audit endpoint는 sandbox 네트워크 제한으로 경고했으나 restore 산출물의 빌드·실행은 성공했다.
- 기존 작업 문서명이 `task_plan.md`가 아니라 `docs/03-work-plan.md`였다. 이 파일을 새 활성 계획으로 만들고 기존 문서를 이력/근거로 유지한다.
- 저장소 전체 `cargo fmt --check`는 이번 변경 전부터 존재하던 광범위한 포맷 차이 때문에 실패한다. 관련 없는 대규모 재포맷은 피했고 `clippy -D warnings`와 `git diff --check`를 통과시켰다.
- Rust stable의 Windows `MetadataExt::volume_serial_number/file_index`는 `windows_by_handle` unstable 오류로 크로스 컴파일에 실패했다. 열린 파일 핸들에 대한 Win32 API 호출로 교체했다.
- Windows 타깃 첫 다운로드와 전용 crate 다운로드는 sandbox 쓰기/DNS 제한으로 실패했고 승인된 범위에서 재실행해 완료했다.
- heartbeat worker의 첫 구현은 Clippy `while_let_loop`에 걸렸다. 동일한 종료 의미를 보존하는 `while let`으로 단순화해 `-D warnings`를 통과했다.
- 로컬에는 PowerShell과 실행 중인 Linux Podman VM이 없어 Windows 설치 및 Linux 비 UTF-8 실파일 테스트를 직접 실행할 수 없었다. 두 검증은 실제 OS GitHub Actions job으로 추가했다.
- `gh workflow list`는 기본 브랜치에 아직 없는 브랜치 전용 workflow를 표시하지 않았다. 커밋 check-runs와 run ID `31572303544`를 직접 조회해 실제 실행 결과를 확인했다.
- 첫 converter 구현은 직접 child 종료 뒤 stderr reader를 무기한 join해, background helper가 pipe를 상속하면 멈췄다. 실패 테스트로 재현한 뒤 process tree 종료와 bounded drain으로 수정했다.
- 후속 리뷰에서는 converter가 stderr를 닫고 정상 종료하면 조용한 자손이 process group에 남는 경로도 재현됐다. 정상/실패/timeout 모든 종료 경로에서 Unix process group 또는 Windows Job을 닫도록 회귀 테스트와 정리를 보강했다.
- 쓸 수 없는 `--media-dir`에서 private scratch 생성 실패가 입력 손상(exit 2)으로 오분류됐다. 안전한 변환 실행 기반이 없는 런타임 문제로 분류해 `unsupported_feature`(exit 3)로 고정했다.
- 상속된 `SIGCHLD` disposition/mask와 reap 후 PGID 재사용 경쟁을 피하려고 Unix timeout을 `waitid(WNOWAIT)` polling으로 교체했다. scratch는 Unix `0700`/`0600`, Windows atomic protected-DACL create+handle과 Job drain으로 보호한다.
- Windows의 임의 media root는 mutable junction 위험 때문에 binary HWP staging에 사용하지 않고 canonical user-temp root만 사용한다.
- RHWP v0.8.4는 `env::args()`를 사용해 Linux 비 UTF-8 원본·media 경로를 직접 받을 수 없다. UTF-8 private staging과 Linux 전용 회귀 테스트를 추가했다.
- 참조 ChatGPT 대화의 966행 첨부 `task-plan.md` 본문은 `read_thread` 응답과 로컬 파일시스템에 노출되지 않았다. 대화에 남은 핵심 판단·Phase 흐름·SHA-256을 현재 저장소 근거와 대조했고, 실행 세부 사항은 이 활성 계획에 기록한다.
- 로컬 `gh` 인증 토큰은 만료됐고 sandbox 네트워크도 차단됐다. GitHub 앱은 PR-triggered run만 반환해 push CI를 놓쳤으므로 공개 GitHub REST API의 전체 commit SHA와 Actions run jobs를 사용해 원격 결과를 검증했다.
- H1 Unix rollback의 첫 구현은 기존 HWP backup 이동이 실패하면 아직 교체하지 않은 HWP까지 삭제했다. 실패 주입 테스트로 재현한 뒤 target별 commit flag로 수정했다.
- uninstall은 extension 디렉터리가 외부 symlink/junction일 때 child `plugin`을 따라가 삭제할 수 있었다. Unix 실파일 RED와 Windows 계약 RED를 추가하고 제거 전에 symlink/reparse guard를 실행한다.
- Codex Windows 제한 토큰은 일반 사용자 SID 외에 sandbox SID의 ACL 교집합도 요구해 `%TEMP%`의 새 디렉터리를 즉시 다시 열지 못할 수 있다. 이 현상은 샌드박스 밖의 동일 DACL 테스트가 통과하는 것으로 환경 문제임을 확인했으며 제품 ACL 변경 사유로 사용하지 않는다.

## Status
**Phase 7 locally complete; Phase 6 / H1 remote gate pending** - H1a/H1b 로컬 구현·회귀·실제 OfficeCLI/RHWP smoke와 Phase 7 host/installer/공급망 하드닝은 완료했다. H1c의 두 push run은 OfficeCLI 1.0.143 오류 처리 중 원래 예외가 가려져 실패했다. 공식 1.0.145 자산·체크섬으로 workflow를 고쳤고 로컬 Windows와 비-root Linux 컨테이너 HWP view는 통과했지만 새 양 OS 원격 run 전까지 H1c/H1d와 Phase 6을 완료하지 않는다.

## Next Action Plan
1. 2026-08-28에 받은 publication 승인으로 현재 원자 변경을 commit/push하고 새 Linux/Windows workflow를 실행한다.
2. 양 OS의 실제 `.hwp` discovery·RHWP `view`·원본 불변·직접 HWPX 회귀가 모두 성공하면 run 링크를 기록하고 H1c/H1d와 Phase 6을 완료한다.
3. 현재 브랜치는 `upstream/main`보다 22커밋 뒤이므로, H1 게이트와 섞지 말고 별도 승인·변경으로 최신 upstream 통합과 전체 회귀를 수행한다.
