# HWPX 보안·호환성 후속 계획

작성일: 2026-08-11  
최종 갱신: 2026-08-12

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
- [ ] 새 GitHub Actions workflow의 첫 Linux/Windows 네이티브 실행 결과를 확인한다.

Windows 구현은 `GetFileInformationByHandle`로 열린 source/log 핸들의 volume serial과 64-bit file index를 비교한다. 크로스 타깃 컴파일은 통과했으며 실제 NTFS 하드 링크 동작은 Windows CI가 검증한다.

## 검증 순서

1. 각 취약 동작을 가장 작은 단위 테스트로 재현한다.
2. 테스트가 기대 이유로 실패하는지 확인한다.
3. 최소 구현으로 통과시킨 뒤 우회 입력을 추가 확인한다.
4. 전체 Rust 테스트, 포맷, clippy를 실행한다.
5. 실제 OfficeCLI 왕복 43개 검사를 다시 실행한다.

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
