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
