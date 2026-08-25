# HWPX 보안·호환성 후속 계획

작성일: 2026-08-11  
최종 갱신: 2026-08-15

## 기준선

- 기존 기능 계획 P0/P1/P-1은 재검증 결과 완료 상태다.
- Rust 테스트 184개, `clippy -D warnings`, OfficeCLI 1.0.143 왕복 43개 검사가 모두 통과했다.
- 정식 보안 스캔 `d9803646-7e1b-4939-b9ff-c713dcb3e72c`에서 높음 2건, 중간 3건, 낮음 2건을 확인했다.

## P0 — 문서 자원 경계

- [x] ZIP 엔트리 수, 엔트리별 확장 크기, 문서 누적 확장 크기를 제한한다.
- [x] 실제 읽은 바이트를 제한하고 초과 시 제어된 `corrupt_input` 오류를 반환한다.
- [x] 표의 행·열·격자 슬롯 수와 셀 수를 검증한다.
- [x] 셀 주소·span 덧셈 및 행×열 곱셈에 checked arithmetic을 적용한다.
- [x] manifest/spine/section 수를 제한하고 중복 섹션을 최초 1회만 처리한다.

완료 조건: 악성 경계값 회귀 테스트가 수정 전 실패하고 수정 후 통과하며, 기존 코퍼스가 동일하게 통과한다.

## P1 — 이미지 및 출력 증폭

- [x] BinData 이름 인덱스를 패키지 열기 시 1회 구축한다.
- [x] 고유 BinData를 공유 바이트(`Arc<[u8]>`)로 캐시해 반복 압축 해제를 막는다.
- [x] 이미지 참조 수를 512개로 제한한다. 따라서 고유 이미지 수도 같은 상한 이하다.
- [x] 참조별 누적 원본 바이트를 64MiB로 제한해 반복 data URI 출력도 유한하게 묶는다.
- [x] 전체 문서 `Vec<BatchItem>` 대신 최상위 블록 단위로 생성·flush한다.

완료 조건: 같은 BinData의 반복 참조가 1회만 읽히며 예산 초과 입력은 제어된 오류로 종료한다.

## P2 — 진단·플랫폼 호환성

- [x] 진단에 포함되는 경로의 개행·제어 문자를 한 줄 이스케이프한다.
- [x] Unix에서 device/inode, Windows에서 volume/file index를 비교해 source의 하드 링크를 출력 전에 거부한다.
- [x] 로그 열기/쓰기 실패를 stdout 오염 없이 stderr에 알린다.
- [x] 진입점에서 `args_os`를 사용하고 비 UTF-8 Unix 경로 인자를 손실 없이 `PathBuf`로 보존한다.
- [x] Linux 비 UTF-8 실제 파일과 Windows 하드 링크를 네이티브 CI 테스트로 추가한다.
- [x] Windows PowerShell 설치 스크립트와 설치 후 매니페스트 검증을 CI에 추가한다.
- [x] 파싱 동안 10초 주기 heartbeat를 보내고 emitter를 블록 단위 JSONL 출력으로 바꾼다.
- [x] 새 GitHub Actions workflow의 첫 Linux/Windows 네이티브 실행 결과를 확인한다.
- [x] Windows에서 실제 OfficeCLI `plugins list`로 기존 HWPX 설치의 discovery를 확인한다.
- [x] Linux/Windows에서 선언 MSRV 1.88 전용 job의 첫 결과를 확인한다.
- [x] macOS 합성 48MiB 표본 1회의 wall-time/RSS와 실제 OfficeCLI watchdog을 실측한다.

Windows 구현은 `GetFileInformationByHandle`로 열린 source/log 핸들의 volume serial과 64-bit file index를 비교한다. 크로스 타깃 컴파일은 통과했으며 실제 NTFS 하드 링크 동작은 Windows CI가 검증한다.

## P3 — 선택적 HWP discovery 활성화

- [x] 매니페스트가 `[".hwpx", ".hwp"]`를 선언한다.
- [x] 두 환경변수와 두 사용자 extension 경로를 설치·제거 대상으로 관리한다.
- [x] Unix는 HWPX 실파일 하나와 HWP 상대 심볼릭 링크를 사용한다.
- [x] Windows는 두 복사본을 사전 검증하고 중간 실패 시 best-effort rollback한다.
- [x] H1 변경에 대한 로컬 전체 회귀와 실제 OfficeCLI HWP smoke를 완료한다.
- [ ] 새 Linux/Windows CI에서 실제 `.hwp` discovery와 RHWP `view`를 확인한다.

Windows의 두 대상은 순차 교체되므로 프로세스 강제 종료까지 포함한 완전한
두 경로 원자성을 보장하지 않는다. `plugins list`도 실행 경로별로 같은
매니페스트를 두 행 표시할 수 있다. 따라서 목록 행 수가 아니라
`extensions` 필드와 실제 `.hwp` resolution을 검증한다. `officecli view
<file.hwp> text`는 입력 옆에 `.docx` 형제 파일을 만들 수 있으므로 CI와
수동 검증 모두 원본이 아닌 복사본을 사용한다.

HWP discovery가 성공해도 RHWP가 없는 런타임에서는 `.hwp`가 exit
3(`unsupported_feature`)을 반환한다. HWPX 직접 경로에는 이 선택 의존성이
없다.

## P4 — Host discovery와 공급망 후속 하드닝

- [ ] 프로토콜과 맞게 상대 `OFFICECLI_PLUGIN_*` 실행파일 경로를 host에서 거부한다.
- [ ] 같은 매니페스트가 여러 extension 경로에 있을 때 `plugins list`의 identity/dedup 정책을 정한다.
- [ ] 사용자 디렉터리 외 `.hwp` PATH alias를 설치·지원할지 결정한다.
- [ ] installer ancestor reparse 정책과 Windows 악성 junction 제거 회귀를 네이티브로 검증한다.
- [ ] workflow의 외부 action tag를 검증된 전체 commit SHA로 고정한다.

현재 설치기의 `--print-env`/`-PrintEnv`는 절대경로만 출력한다. 그러나
OfficeCLI host는 프로토콜이 절대경로를 요구하는 것과 달리 사용자가 직접
지정한 상대 환경변수 경로도 후보로 받는다. 이 host 전역 문제는 H1 플러그인
변경과 섞지 않고 실패 테스트를 갖춘 별도 원자 변경으로 수정한다. H1 CI가
다운로드하는 OfficeCLI·RHWP·fixture 자체는 버전, 전체 commit URL, SHA-256으로
고정한다.

## 검증 순서

1. 각 취약 동작을 가장 작은 단위 테스트로 재현한다.
2. 테스트가 기대 이유로 실패하는지 확인한다.
3. 최소 구현으로 통과시킨 뒤 우회 입력을 추가 확인한다.
4. 전체 Rust 테스트, 포맷, clippy를 실행한다.
5. 실제 OfficeCLI 왕복 43개 검사를 다시 실행한다.

대용량 실측은 `scripts/verify-large-file.py`로 독립 생성한 저장형 ZIP HWPX를
사용한다. 플러그인의 첫 JSONL 지연·wall-time·peak RSS·원본 불변을 기록하고,
같은 파일을 OfficeCLI 30초 idle watchdog 아래 `plugins lint`로 다시 통과시킨다.

## 2026-08-11 검증 결과

- `cargo test --locked`: 199 passed.
- 릴리스 모드 `oversized_cell_span_returns_error_instead_of_panicking`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.
- OfficeCLI 1.0.143 실제 왕복: 43 checks passed.
- `cargo fmt --check`: 저장소 기준선의 기존 포맷 차이로 실패. 이번 작업과 무관한 전체 재포맷은 하지 않았다.
- macOS/APFS는 잘못된 UTF-8 파일명 생성을 `Illegal byte sequence`로 거부했다. 원시 `OsString` 보존 단위 테스트는 통과했고 실파일 검증은 Linux로 이관한다.

## 2026-08-12 Phase 5 검증 결과

- RED: 파일 ID 비교, 블록 sink, heartbeat helper가 구현 부재로 각각 컴파일 실패하는 것을 확인했다.
- `cargo test --locked`: 202 passed (lib 125 + binary 1 + golden 3 + parser 34 + protocol 39).
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo check --locked --target x86_64-pc-windows-gnu --all-targets`: passed.
- Windows target `cargo clippy --locked --target x86_64-pc-windows-gnu --all-targets -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- OfficeCLI 1.0.143 실제 왕복: 43 checks passed.
- GitHub Actions workflow YAML syntax과 `git diff --check`: passed.
- PowerShell/Windows 및 Linux 비 UTF-8 실파일의 네이티브 실행 결과는 workflow가 원격에서 처음 실행된 뒤 확정한다.

## 2026-08-13 Phase 5 원격 완료 확인

- 브랜치 HEAD와 `origin/feat/hwpx-plugin`이 `2d8b909de3311e9b44b42fd88788e208732db977`로 일치했다.
- GitHub Actions `HWPX plugin` run `31572303544`가 `completed / success`로 종료됐다.
- Linux x64의 test, clippy, release build가 성공했다. Linux 전용 비 UTF-8 실파일 테스트도 이 test 단계에 포함됐다.
- Windows x64의 test, clippy, release build와 네이티브 설치 검증이 성공했다. Windows 전용 하드 링크 회귀도 이 test 단계에 포함됐다.
- 따라서 P2의 네이티브 플랫폼 게이트는 완료 처리한다. 다만 실제 host discovery와 대용량/idle 실측은 Phase 5 잔여 게이트로 유지한다.

## 2026-08-13 잔여 호환성 감사

- Windows job은 설치 위치와 설치 바이너리의 `--info`를 검증했지만 `officecli plugins list`는 실행하지 않았다.
- `rust-version = "1.87"`에서 `cargo +1.87.0 check --locked --all-targets`를 실행하자 `zip 8.6.0 requires rustc 1.88`로 실패했다.
- ZIP 보안 경계 의존성을 낮추는 대신 선언 MSRV를 1.88로 정정하고 전용 CI job을 추가한다.
- Windows 설치 job에는 SHA-256을 고정한 OfficeCLI 1.0.143을 내려받아 실제 `plugins list` discovery를 검사하는 단계를 추가한다.
- 당시에는 새 CI 정의의 첫 원격 결과와 대용량/idle 통합 실측 전이라 Phase 5 전체를 완료로 표시하지 않았다. 대용량/idle 실측은 아래에서, 새 CI 확인은 2026-08-14에 완료했다.

## 2026-08-13 대용량/idle 제한적 실측

- `scripts/verify-large-file.py`로 섹션 4개 × 텍스트 12MiB, 실제 파일 48.0MiB의 저장형 ZIP HWPX를 생성했다.
- macOS arm64 단일 실행의 릴리스 플러그인: exit 0, 첫 출력 0.471초, wall-time 0.540초, peak RSS 106.1MiB, JSONL 4행/48.0MiB, 원본 hash·mtime 불변.
- OfficeCLI 1.0.143: `OFFICECLI_PLUGIN_IDLE_TIMEOUT_SECONDS=30` 아래 `plugins lint`가 0.771초에 성공했고 unknown prop은 0개였다.
- 전체 실행이 heartbeat 주기 10초보다 빨라 이 표본에서는 heartbeat 프레임이 발생하지 않았다. heartbeat 프레임 자체는 단위 테스트로 별도 검증한다.
- `BufferedReader.read(1MiB)`가 첫 바이트 시각을 늦게 기록하던 측정 결함은 `os.read`로 수정했다. 이 값은 합성 표본 1회 결과이지 일반 성능 보증이 아니다.
- 따라서 경계 크기 합성 표본 게이트만 닫는다. 느린 실제 dump의 heartbeat-host reset과 대형 binary HWP 실측은 계속 대기한다. MSRV/Windows discovery CI는 2026-08-14에 완료했다.

## 2026-08-13 HWP 변환기 보안 후속 리뷰

- RHWP v0.8.4는 `std::env::args()`를 사용해 Linux 비 UTF-8 argv를 거부한다. 원본과 변환 출력은 UTF-8 고정명의 private scratch에 staging하고, 비 UTF-8 `--media-dir`이면 시스템 temp로 안전하게 fallback한다.
- 원본 경로를 변환기에 직접 전달하지 않으며 copy 전후 hash·mtime 불변과 scratch 정리를 계약 테스트로 고정했다.
- 매직 헤더만 정상인 초대형 입력이 scratch 복사로 디스크를 소진하지 않도록 staging copy를 256MiB로 제한하고 성장 중인 파일도 제한된 reader로 재확인한다.
- 직접 child만 kill하고 stderr reader를 join하면 background helper의 상속 pipe 때문에 무기한 멈출 수 있었다. 실패 테스트로 재현한 뒤 stderr drain에 deadline을 추가했다.
- Unix는 전용 process group, Windows는 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` Job Object와 `TerminateJobObject`를 사용해 자손을 정리한다. Windows 표준 `Command::spawn`과 Job 할당 사이의 짧은 race는 남으므로 악성 실행파일의 완전한 sandbox라고 주장하지 않는다.
- Unix의 상속된 `SIGCHLD=SIG_IGN` crash와 blocked-mask hang을 재현해 signal-handler 기반 timeout을 제거했다. `waitid(WNOWAIT)`로 직접 child를 reap하지 않고 관찰한 뒤 group을 종료하므로 PGID 재사용 대상 오살도 막는다.
- scratch는 Unix `0700`/source `0600`을 강제한다. Windows는 owner+SYSTEM protected DACL을 상대 `NtCreateFile`의 create+handle 단계에서 원자 적용하고 delete-share 없는 root/child handle을 유지한다. Job active-process 0을 bounded wait한 뒤 scratch 삭제를 재시도한다.
- 공격자가 제어하는 Windows `--media-dir`의 junction retarget까지 경로 기반 RHWP argv에서 안전하게 보장할 수 없으므로, Windows binary HWP staging은 canonical user-temp root만 사용한다. `--media-dir`은 HWPX 직접 경로에는 원래 필요하지 않다.
- RHWP는 `argv[0]`도 UTF-8 `String`으로 수집하므로 명시/PATH/사용자 설치 후보 중 비 Unicode converter 경로는 선택하지 않고, 명시 설정은 exit 3으로 거절한다.
- 핵심 fake converter 성공/실패/출력 재검증 테스트를 양 OS에서 실행하고, Linux 비 UTF-8 staging과 Windows 실제 descendant 종료 테스트를 네이티브 CI에 고정했다. 첫 원격 결과는 GitHub Actions run `31700156231`에서 모두 성공했다.

## 2026-08-14 Phase 6 원격 완료와 검증기 이식성

- 브랜치 HEAD `d910b40d66707127e4fcff7811a8bc1b1329b23d`의 GitHub Actions run `31700156231`이 전체 성공했다.
- Linux x64는 220 tests, clippy, release build를 통과했고 Rust 1.88 MSRV check도 성공했다.
- Windows x64는 210 tests, clippy, release build, 네이티브 설치, OfficeCLI 1.0.143 `plugins list` discovery를 통과했고 Rust 1.88 MSRV check도 성공했다.
- 로컬 Windows에서 검증기 기본 경로가 확장자 없는 실행파일만 찾아 `.exe`를 놓치는 결함을 재현했다. 공통 탐색기로 Windows `.exe`와 Linux 확장자 없는 이름만 자동 선택하도록 수정했다. 명시 경로는 계속 허용한다.
- 같은 checkout에서 Windows release가 남은 채 Linux `CARGO_TARGET_DIR`를 따로 쓰면 검증기가 오래된 `.exe`를 선택하는 결함도 재현했다. 반대 OS 이름의 자동 fallback을 제거하고 `CARGO_TARGET_DIR/release`를 반영했다.
- 실행파일 탐색 단위 테스트 6개와 `verify-large-file.py --skip-officecli --sections 1 --text-mib 1` smoke를 Linux/Windows workflow에 추가했다. Windows 로컬 기본 경로 smoke도 exit 0, JSONL 1행/1.0MiB, 원본 불변으로 통과했다.
- Codex 제한 토큰에서만 `%TEMP%` 재접근이 거부된 현상은 샌드박스 밖의 원본 Windows protected-DACL 테스트가 통과해 제품 결함이 아닌 실행 환경 제약으로 분리했다.

## 2026-08-15 H3 원격 게이트 확인과 H1 로컬 구현

- GitHub Actions run `31700156231`에서 H3 브리지, Linux 비 UTF-8 staging,
  Windows Job Object process-tree 종료, 양 OS MSRV 1.88, Windows의 기존
  HWPX OfficeCLI discovery가 성공했다. 이 결과로 위 2026-08-13 H3 대기
  항목은 닫았다.
- 이 run은 H1 이전 커밋을 검증했다. 후속 H1은 매니페스트·두 환경변수·두
  사용자 설치 경로를 로컬 구현했다. macOS의 227개 Rust 테스트, 43개 HWPX
  왕복 검사, OfficeCLI 1.0.143과 공식 RHWP 0.8.4 HWP smoke는 통과했지만 새
  Linux/Windows `.hwp` discovery·RHWP `view` CI는 아직 실행하지 않았다.
- Unix의 한 실파일과 상대 심볼릭 링크는 양 확장자의 버전을 일치시킨다.
  Windows는 staging·SHA-256·`--info` 검증과 best-effort rollback을
  사용하지만 강제 종료를 포함한 완전한 두 경로 트랜잭션은 아니다.
- 목록의 중복 행 가능성, RHWP 부재 시 exit 3, 대형 binary HWP와 느린
  변환기의 heartbeat-host 통합 미실측은 잔여 제한으로 유지한다.
