# Notes: HWPX 후속 개발

## Sources

### 기존 작업 계획
- Path: `docs/03-work-plan.md`
- 검토 예정: 완료 표시와 실제 코드·테스트·왕복 검증의 일치 여부

### 인수인계 문서
- Path: `docs/02-handover.md`
- 검토 예정: 알려진 한계, 미검증 플랫폼, 실제 문서 표본 위험

### OfficeCLI 플러그인 계약
- Path: `../plugin-protocol.md`
- 검토 예정: dump-reader 입력·출력·종료코드·idle timeout 계약

## Synthesized Findings

### 초기 상태
- 활성 브랜치: `feat/hwpx-plugin`
- 작업 트리: 계획 파일을 만들기 전에는 clean
- 원격 추적: `origin/feat/hwpx-plugin`

### 기존 계획 재확인
- P0(G1, G2), P1(G3, G4), P-1 회귀 코퍼스는 완료 상태다.
- G5 스타일 매핑은 코퍼스에서 실제 개요 스타일 사용이 0건이고, OfficeCLI가 참조 스타일 정의를 자동 생성하지 않아 보류됐다.
- G6 PUA 문자는 신뢰할 수 있는 대응표가 없어 보존 및 진단까지만 구현됐다.
- `docs/02-handover.md`의 테스트 167개 및 중첩표 평탄화 설명은 현재 코드보다 오래된 기록이다. 최신 기준은 `docs/03-work-plan.md`와 실제 테스트다.

### 2026-08-11 재검증
- `cargo test --quiet --locked`: 184 passed, 0 failed.
- `cargo clippy --quiet --locked --all-targets -- -D warnings`: exit 0.
- OfficeCLI 1.0.143 실제 왕복: 43 checks 모두 통과.

### 보안·호환성 분석
- 정식 스캔 ID: `d9803646-7e1b-4939-b9ff-c713dcb3e72c`
- 결과: 높음 2, 중간 3, 낮음 2.
- P0: ZIP 확장 크기 무제한, 표 차원 기반 밀집 할당.
- P1: 셀 span 산술 오버플로, 반복 이미지 증폭, 중복 spine 기반 반복 파싱.
- P2: 진단 경로 제어문자 주입, `--log-file`의 source alias 쓰기.
- 호환성 후속: 로그 실패 가시성, 30초 pre-output idle timeout, Windows 설치 경로, 비 UTF-8 Unix 인자.
- 중첩 표 재귀는 `MAX_DEPTH = 32`로 제한되어 별도 스택 고갈 취약점으로 보지 않는다.

### 구현 완료
- ZIP은 최대 4,096개 엔트리, 엔트리별 64MiB, 선언/실제 누적 확장 256MiB와 1,000:1 압축비 경계를 적용했다.
- XML은 항목별 16MiB, HPF는 4MiB, mimetype은 4KiB로 제한했다.
- 표는 32,768행, 512열, 100,000셀, 1,000,000 격자 슬롯과 checked arithmetic을 적용했다.
- manifest/spine/section 수를 제한하고 같은 section의 반복 spine 참조는 최초 한 번만 파싱한다.
- BinData stem 인덱스와 `Arc<[u8]>` 캐시를 추가했다. 이미지 참조는 512개, 참조별 누적 원본 바이트는 64MiB로 제한한다.
- 진단 문자열의 제어 문자를 이스케이프하고 source/log 동일 파일을 직접 경로·심볼릭 링크·Unix 하드 링크 기준으로 거부한다.
- 로그 열기/쓰기 실패는 변환 stdout을 오염시키지 않고 stderr 경고로 드러낸다.
- CLI 진입점은 `std::env::args_os`를 사용하며 경로 인자를 UTF-8 문자열로 강제 변환하지 않는다.

### 최종 검증
- `cargo test --locked`: 199 passed.
- 릴리스 모드 span 오버플로 회귀 테스트: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- OfficeCLI 1.0.143 실제 왕복: 43 checks passed.
- 코드 그래프 재인덱싱 및 호출 추적으로 입력 경계에서 새 검증 함수가 호출됨을 확인했다.
- `cargo fmt --check`는 기존 저장소 전반의 포맷 차이 때문에 실패한다. 대규모 무관 변경을 피하고 `git diff --check`를 통과시켰다.

### 남은 호환성 작업
- Linux 러너에서 비 UTF-8 실제 파일명 왕복 검증. macOS/APFS는 해당 파일명 생성을 `Illegal byte sequence`로 거부했다.
- Windows 네이티브 설치 경로와 Windows 하드 링크 파일 ID 검증.
- 대용량 문서에서 OfficeCLI 30초 pre-output idle timeout 실측 및 점진 출력 구조 검토.

### Phase 5 설계 확인
- OfficeCLI `PluginProcess.ReadStderr`는 `{"heartbeat":true}` 행을 소비하고 활동 시각만 갱신한다. 일반 stderr도 활동으로 보지만 사용자 진단으로 노출되므로 전용 heartbeat 행을 쓴다.
- 현재 `stream_document`는 `emit_document`가 문서 전체 `Vec<BatchItem>`을 만든 뒤에야 첫 줄을 쓴다. 기존 emitter API는 보존하되 top-level block 단위 sink API를 추가하면 출력 순서를 바꾸지 않고 최대 임시 목록을 단일 블록 범위로 줄일 수 있다.
- Windows 동일 파일 판정은 열린 파일 핸들의 volume serial과 file index 쌍으로 구현한다. Rust stable의 해당 `MetadataExt` 메서드는 아직 unstable이므로 `windows-sys`의 `GetFileInformationByHandle`을 사용한다.
- Linux/Windows 전용 동작은 GitHub Actions matrix에서 실제 파일시스템과 네이티브 바이너리로 검증한다.

### Phase 5 구현
- Windows source/log 판정은 경로 canonical 비교 뒤 두 파일을 열어 핸들 ID를 비교하고, 로그를 append로 연 뒤에도 source 핸들과 다시 비교한다.
- `try_emit_document`는 top-level block 하나의 명령만 임시 보관하고 sink가 실패하면 다음 블록을 생성하지 않는다. `stream_document`는 이 sink에서 행별 write/flush한다.
- 바이너리 진입점은 dump 실행 동안 10초마다 `{"heartbeat":true}`를 stderr에 쓰며 종료 시 worker를 즉시 join한다. `--quiet`/`--log-file`과 무관한 프로토콜 신호다.
- `scripts/install.ps1`은 Windows의 `$HOME\.officecli\plugins\dump-reader\hwpx\plugin.exe`에 설치하고 설치 전후 `--info`를 확인한다.
- `.github/workflows/hwpx-plugin.yml`은 Ubuntu와 Windows에서 test/clippy/release build를 실행한다. Linux는 비 UTF-8 실파일, Windows는 실제 하드 링크와 PowerShell 설치를 자동 검증한다.

### Phase 5 검증
- RED 테스트: `same_file_identity`, `try_emit_document`, `start_heartbeat` 부재로 예상된 컴파일 실패를 확인했다.
- `cargo test --locked`: 202 passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo check --locked --target x86_64-pc-windows-gnu --all-targets`: passed.
- Windows target `cargo clippy --locked --target x86_64-pc-windows-gnu --all-targets -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- OfficeCLI 1.0.143 실제 왕복: 43 checks passed.
- workflow YAML syntax 및 `git diff --check`: passed.
- 로컬에 PowerShell과 Linux Podman VM이 없어 두 OS의 네이티브 테스트는 실행하지 못했다. workflow 첫 실행 결과가 최종 플랫폼 증거다.

### 2026-08-13 Phase 5 플랫폼 게이트 완료
- HEAD와 `origin/feat/hwpx-plugin`은 `2d8b909de3311e9b44b42fd88788e208732db977`로 일치했고 worktree는 clean이었다.
- GitHub Actions `HWPX plugin` run `31572303544`가 성공했다.
- Linux x64 test/clippy/release build와 Linux 비 UTF-8 실파일 회귀가 성공했다.
- Windows x64 test/clippy/release build, 하드 링크 회귀, PowerShell 네이티브 설치 검증이 성공했다.
- 이 결과로 Phase 5의 네이티브 플랫폼 게이트를 닫았다.

### Phase 5 잔여 호환성 감사
- Windows workflow는 설치 위치와 `--info`만 확인했으며 실제 OfficeCLI discovery는 아니었다.
- `cargo +1.87.0 check --locked --all-targets`는 `zip 8.6.0 requires rustc 1.88`로 실패해 선언 MSRV가 사실과 달랐다.
- MSRV를 1.88로 바로잡아 전용 CI를 추가하고, Windows job은 체크섬 고정 OfficeCLI 1.0.143의 `plugins list`까지 확인하도록 확장한다.
- 합성 48MiB 문서와 실제 OfficeCLI 30초 watchdog의 빠른 경로는 아래에서 실측했다. 다만 10초를 넘기는 실제 dump가 heartbeat로 host idle timer를 갱신하는 통합 경로는 아직 미실측이다.
- 새 CI의 첫 원격 결과 전에는 Phase 5 전체를 완료로 승격하지 않는다.

### Phase 6 선택
- 다음 기능은 `docs/04-hwp-support-plan.md` H3의 선택적 RHWP HWP→HWPX 브리지로 정했다.
- 현재 포맷 판별과 `needs_conversion()` 분기점이 이미 있어 새 파서를 만들지 않고 기존 HWPX 파이프라인을 재사용할 수 있다.
- 먼저 H3 변환 경계를 구현하고, 성공 전에는 `.hwp` 확장자를 매니페스트·설치 경로에 광고하지 않는다.
- 보안 게이트: shell 금지, `OsStr` 인자 보존, 환경변수 경로 절대경로 제한, 임시 산출물 정리, 원본 불변, 변환 출력 HWPX 재판별, 변환기 부재 exit 3·변환 실패 exit 2.
- 기존 계획의 RHWP v0.8.2 근거는 최신 v0.8.4 CLI와 체크섬으로 재실측해야 한다. 아래 H3 기록에서 완료했다.

### Phase 5 대용량/idle 실측
- macOS arm64에서 독립 생성한 48.0MiB 저장형 HWPX 단일 실행: 첫 출력 0.471초, 전체 0.540초, peak RSS 106.1MiB, JSONL 4행/48.0MiB, 원본 불변.
- OfficeCLI 1.0.143의 30초 watchdog 아래 `plugins lint`: 0.771초, unknown prop 0, success.
- 10초 안에 종료돼 heartbeat frame은 관찰되지 않았다. helper 단위 테스트는 계속 통과한다.
- 첫 구현의 `read(1MiB)`가 첫 바이트가 아니라 버퍼 충전/EOF 시각을 기록하던 문제를 `os.read`로 수정했다. 합성 표본 1회 결과로 범위를 제한한다.

### Phase 6 H3 실행 경계
- RHWP v0.8.4 macOS arm64 공식 자산의 SHA-256 `6a5e6a7104a2ce40fd4235d1c95cc86b0291652f30f6d1bf1efd3708419ac176`이 `SHA256SUMS.txt`와 일치했다.
- CLI 계약은 `rhwp export-hwpx <입력> [출력] [--verify] [--verify-pages] [--json]`이다. `--help`는 지원하지 않고 사용법 오류(exit 2)와 함께 계약 문자열을 출력한다.
- 공식 HWP5 3종(영문/복합표/필드)과 HWP3 1종이 변환 및 플러그인 브리지에서 각각 19/712/48/467 JSONL 행으로 성공했다.
- `--verify`는 복합표 1종만 동일했고 나머지 3종은 RHWP 상류 IR 차이를 보고해 런타임 브리지에서는 사용하지 않는다.
- RED: 구성된 fake converter 성공 테스트가 구현 전 기존 exit 3으로 실패했다.
- GREEN: 변환기 부재 exit 3, 성공 JSONL, 원본 불변, private scratch 정리, 비정상 종료/출력 누락/비 HWPX exit 2, 절대 env path, timeout, stderr cap을 검증했다.
- 후속 리뷰에서 RHWP의 비 UTF-8 argv 제약과 background helper의 stderr pipe 상속 hang을 재현했다. UTF-8 고정명 staging, bounded drain, Unix process group, Windows Job Object로 보강했다.
- stderr를 닫은 정상 converter가 조용한 자손을 남기는 경로도 별도 RED 테스트로 재현했다. 직접 child의 성공 여부와 무관하게 Unix process group/Windows Job 전체를 정리하며, private scratch를 만들 수 없는 런타임은 exit 3으로 분류한다.
- 상속된 `SIGCHLD=SIG_IGN` crash와 blocked-mask hang을 RED로 고정하고 signal-handler 기반 runtime wait를 제거했다. Unix는 `waitid(WNOWAIT)`로 reap 전 group을 정리하고, Windows는 Job active-process 0을 기다린다.
- Unix scratch/source는 `0700`/`0600`, Windows는 owner+SYSTEM protected DACL의 상대 `NtCreateFile` create+handle과 no-delete-share로 원자 보호한다. 비 Unicode RHWP 실행파일 경로도 exit 3으로 거절한다.
- Windows의 임의 `--media-dir`은 mutable junction 위험 때문에 binary HWP staging에 쓰지 않고 canonical user-temp root로 한정한다.
- staging copy는 256MiB 예산을 두고 실제 읽기에도 `limit + 1` 상한을 적용해 파일 성장/디스크 소진 경계를 고정했다.
- 양 OS fake converter 계약, Linux 비 UTF-8 원본·media 경로, Windows descendant 종료 테스트를 추가했다. 새 네이티브 CI의 첫 결과는 대기 중이다.

### Phase 6 H4/H5
- `scripts/verify-hwp-pairs.py`를 추가했다. NFC 동명 쌍의 JSONL/요약, unknown prop, OfficeCLI batch/validate, 문단·표·셀·폼필드 구조와 원본 불변을 검사한다.
- 실제 독립 HWP/HWPX 1쌍은 34개 JSONL byte exact, unknown prop 0, OfficeCLI validate/구조 일치였다.
- RHWP 공식 HWP5 3종·HWP3 1종과 v0.8.4가 만든 HWPX 쌍은 19/48/712/467개 JSONL exact, unknown prop 0, OfficeCLI validate/구조 일치였다.
- README의 선택적 HWP 사용법, ADR-2 정정/ADR-5, 인수인계를 갱신했다. H1 `.hwp` 광고는 새 Linux/Windows 원격 CI가 성공한 뒤 진행한다.

### Phase 6 최종 로컬 검증
- `cargo +1.88.0 test --locked`: macOS 218 passed (lib 135 + binary 1 + golden 3 + parser 34 + protocol 45).
- `cargo +1.88.0 check --locked --all-targets`와 같은 toolchain의 Windows GNU all-target check: passed.
- stable `cargo clippy --locked --all-targets -- -D warnings`와 Windows GNU target clippy: passed.
- `cargo +1.88.0 build --release --locked`, workflow YAML parse, 두 Python verifier `--help`, `git diff --check`: passed.
- OfficeCLI 1.0.143 `verify-roundtrip.sh`: 43 checks passed.
- Windows/Linux 네이티브 테스트, 새 MSRV matrix, Windows OfficeCLI discovery는 변경을 원격에 올린 뒤 확인해야 한다.

### 2026-08-15 Sol Pro 계획 검증 및 H1 착수
- 참조 대화가 제시한 기준선 `d910b40d66707127e4fcff7811a8bc1b1329b23d`는 로컬 `HEAD`와 `origin/feat/hwpx-plugin`에 정확히 일치했고 worktree는 clean이었다.
- GitHub Actions run `31700156231`은 성공했다. Linux x64, Windows x64, Rust 1.88 MSRV Linux x64, Rust 1.88 MSRV Windows x64의 4개 job이 모두 success였다.
- 현재 `Manifest::default()`는 `.hwpx`만 선언하고 설치기도 `dump-reader/hwpx` 한 경로만 관리한다. OfficeCLI `PluginRegistry.CandidatePaths`는 요청 확장자별 환경변수·사용자 디렉터리를 탐색하고 `ManifestMatches`가 그 확장자를 매니페스트에서 다시 확인하므로 `.hwp`에는 별도 경로/환경변수와 양 확장자 매니페스트 선언이 모두 필요하다.
- Sol Pro 계획의 다음 단계 H1은 코드·호스트 discovery 계약과 일치한다. 반면 `main` 반영은 기능 브랜치가 main보다 4커밋, 51파일, 15,184행 추가된 별도 통합 작업이므로 H1 변경과 섞지 않는다.
- 참조 대화의 966행 첨부 파일은 `read_thread`나 로컬 파일 검색에서 회수되지 않았다. 대화의 요약·체크섬과 저장소의 활성 `task_plan.md`를 근거로 실행 계획을 보정했다.
- 로컬 `gh` 토큰 만료와 sandbox DNS 차단 때문에 공개 GitHub REST API 및 GitHub 앱의 run-job 조회로 원격 CI를 검증했다.

### 2026-08-15 H1 로컬 구현·실측
- RED에서 `--info`가 `.hwpx`만 선언했고 Unix 설치기가 `hwp` 사용자 경로와 환경변수를 관리하지 않아 새 매니페스트·설치 계약 6개가 실패하는 것을 확인했다.
- 매니페스트는 기존 순서를 보존한 `[".hwpx", ".hwp"]`를 선언하고, HWP는 선택적 RHWP 브리지라는 설명으로 정정했다.
- Unix 설치기는 HWPX 실행파일을 destination-local staging 뒤 교체하고 `hwp/plugin -> ../hwpx/plugin` 상대 링크를 관리한다. PowerShell 설치기는 두 복사본을 모두 staging·해시·`--info` 검증한 뒤 순차 교체하며 예외 시 best-effort rollback한다.
- Unix 설치 계약 6개와 매니페스트 계약이 GREEN이다. 설치/제거, 관련 없는 플러그인 보존, 상대 링크, HWP staging 실패 시 기존 HWPX 복구를 확인했다.
- 격리 임시 HOME에서 OfficeCLI 1.0.143과 RHWP 0.8.4로 `english.hwp`를 환경변수 plugin override 없이 실제 `view ... text`했다. exit 0, 기대 영문, 형제 DOCX, 원본 SHA-256·mtime 불변, 중간 HWPX 0개를 확인했다.
- 첫 smoke에 지정한 사용자 로컬 RHWP 경로는 확인 결과 v0.8.2여서 H1 v0.8.4 증거로 채택하지 않았다. 공식 v0.8.4 압축에서 추출한 실행파일로 같은 격리 smoke를 다시 실행해 위 결과를 확정했다.
- 두 사용자 경로를 열거하므로 `plugins list --json`에는 같은 매니페스트가 HWP/HWPX 경로별 두 행으로 표시됐다. 기존 왕복 스크립트는 정확히 1행 대신 최소 1행을 요구하도록 정정하고, 실제 `.hwp` view를 resolver 증거로 삼는다.
- 기존 HWP backup `mv` 실패를 주입하자 rollback이 아직 교체하지 않은 HWP를 삭제하는 RED가 재현됐다. 백업 여부와 새 target commit 여부를 분리한 뒤 기존 HWP/HWPX가 모두 보존되는 GREEN을 고정했다.
- uninstall에서 `hwp` extension 디렉터리가 외부 디렉터리 symlink이면 외부 `plugin`을 삭제하는 RED를 실파일로 재현했다. Unix는 제거 전 symlink를 exit 73으로 거부하고, PowerShell도 제거 전 reparse-point guard를 실행하도록 고정했다.
- H1 workflow는 `.hwp` smoke 뒤 RHWP가 명시적으로 만든 HWPX도 사용자 HWPX 경로로 `view`해 기존 직접 경로의 회귀를 양 OS에서 함께 검사한다. 같은 직접 HWPX 절차는 로컬 macOS에서도 통과했다.
- 프로토콜은 환경변수 plugin 경로를 절대경로로 요구하지만 OfficeCLI host는 상대경로도 후보로 받는 기존 보안 차이가 있다. H1 설치기가 출력하는 절대경로는 안전하게 유지하며 host enforcement·목록 dedup·HWP PATH alias 정책은 별도 원자 변경으로 남긴다.
- H1a/H1b는 로컬 완료다. 새 workflow의 Linux/Windows 네이티브 `.hwp` discovery와 실제 RHWP view는 커밋·푸시 뒤 첫 원격 run이 성공하기 전까지 미완료다.
- workflow는 OfficeCLI 1.0.143, RHWP 0.8.4 양 OS 자산과 전체 commit의 `english.hwp`를 하드코딩 SHA-256으로 검증한다. 외부 action의 tag 참조 SHA 고정은 별도 공급망 변경으로 남긴다.
- 최종 로컬 회귀는 Rust 1.88 기준 227개(lib 135 + binary 1 + golden 3 + installer 9 + parser 34 + protocol 45)가 통과했다. Rust 1.88 host/Windows GNU all-target check, stable host/Windows GNU Clippy `-D warnings`, release build, installer Bash 문법, workflow YAML/Linux block 문법, `git diff --check`도 통과했다.
- PowerShell 네이티브 실행과 새 Linux/Windows H1 workflow의 실제 다운로드·discovery·view 결과는 로컬에서 증명하지 않았으며 새 원격 run까지 H1c/H1d를 열어 둔다.
