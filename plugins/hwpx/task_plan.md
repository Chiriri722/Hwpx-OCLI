# Task Plan: HWPX 후속 개발

## Goal
기존 HWPX 플러그인 계획의 완료 상태를 재검증하고, 확인된 보안·호환성 위험을 우선순위에 따라 수정하여 회귀 검증까지 마친다.

## Phases
- [x] Phase 1: 기존 작업 계획·Git 상태·검증 기준 재확인
- [x] Phase 2: 보안 및 OfficeCLI/플랫폼 호환성 위험 분석
- [x] Phase 3: 확인된 보안 위험에 대한 실패 테스트와 구현
- [x] Phase 4: 전체 회귀·왕복 검증 및 결과 문서화
- [ ] Phase 5: 타 플랫폼·대용량 호환성 실측
  - [x] Windows volume/file ID 기반 하드 링크 차단 구현 및 크로스 타깃 컴파일
  - [x] Linux 비 UTF-8 실파일 및 Windows 하드 링크 CI 검증 추가
  - [x] Windows 네이티브 플러그인 설치 스크립트와 discovery 검증 추가
  - [x] 대용량 파싱 heartbeat와 블록 단위 JSONL 출력
  - [x] 로컬 전체 회귀 및 실제 OfficeCLI 왕복 재검증
  - [ ] GitHub Linux/Windows 러너의 첫 네이티브 실행 결과 확인

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
- 정식 스캔 7건은 먼저 macOS/Unix 실행 경로의 입력 경계·이미지 캐시·진단 경계에서 닫았다. Phase 5에서 Windows 하드 링크 식별 구현도 추가했으며, 실제 Windows 러너 증거만 남았다.
- 진입점은 `args_os`를 사용하며 비 UTF-8 경로 바이트를 `PathBuf`로 보존한다. macOS 파일시스템은 잘못된 UTF-8 파일명 생성을 거부하므로 실제 파일 왕복은 Linux 러너에서 추가 확인한다.
- Phase 5 착수 시 남은 호환성 작업은 Linux 비 UTF-8 실파일, Windows 네이티브 설치/하드 링크 식별, 30초 pre-output idle timeout이었다. 구현과 자동 검증 정의는 완료했고 다른 OS의 첫 CI 결과만 남겼다.
- OfficeCLI는 stderr의 `{"heartbeat":true}` 행을 소비하면서 idle 타이머만 갱신한다. 파싱 단계에는 10초 주기 heartbeat를 적용하고 emitter는 문서 전체가 아니라 최상위 블록 단위로 출력한다.
- Windows 파일 동일성은 열린 핸들의 volume serial과 64-bit file index를 비교한다. 안정화되지 않은 Rust `MetadataExt` 대신 `windows-sys`의 `GetFileInformationByHandle`을 사용하고 Windows 크로스 타깃으로 전체 target을 컴파일한다.
- Linux/Windows 네이티브 런타임 테스트는 `.github/workflows/hwpx-plugin.yml`에 고정했다. 현재 변경은 커밋·푸시하지 않았으므로 첫 CI 결과 확인 전에는 Phase 5 전체를 완료 처리하지 않는다.

## Errors Encountered
- 기존 작업 문서명이 `task_plan.md`가 아니라 `docs/03-work-plan.md`였다. 이 파일을 새 활성 계획으로 만들고 기존 문서를 이력/근거로 유지한다.
- 저장소 전체 `cargo fmt --check`는 이번 변경 전부터 존재하던 광범위한 포맷 차이 때문에 실패한다. 관련 없는 대규모 재포맷은 피했고 `clippy -D warnings`와 `git diff --check`를 통과시켰다.
- Rust stable의 Windows `MetadataExt::volume_serial_number/file_index`는 `windows_by_handle` unstable 오류로 크로스 컴파일에 실패했다. 열린 파일 핸들에 대한 Win32 API 호출로 교체했다.
- Windows 타깃 첫 다운로드와 전용 crate 다운로드는 sandbox 쓰기/DNS 제한으로 실패했고 승인된 범위에서 재실행해 완료했다.
- heartbeat worker의 첫 구현은 Clippy `while_let_loop`에 걸렸다. 동일한 종료 의미를 보존하는 `while let`으로 단순화해 `-D warnings`를 통과했다.
- 로컬에는 PowerShell과 실행 중인 Linux Podman VM이 없어 Windows 설치 및 Linux 비 UTF-8 실파일 테스트를 직접 실행할 수 없었다. 두 검증은 실제 OS GitHub Actions job으로 추가했다.

## Status
**Currently in Phase 5** - 구현과 로컬/크로스 컴파일 검증은 완료. 첫 Linux/Windows GitHub Actions 실행 결과만 남았다.
