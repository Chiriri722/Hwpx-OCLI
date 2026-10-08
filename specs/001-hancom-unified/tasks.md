# 현재 실행 작업

이 파일은 Spec-kit 구현 입력이다. 전체 이전 단계 이력은
[task-plan.md](task-plan.md), 현재 설계는 [plan.md](plan.md)를 참조한다.

- [x] T001 Codex Spec-kit 통합 설치 및 prerequisite 검사
- [x] T002 Linear Hwpx-OCLI 프로젝트와 DAV-21/22/23 생성
- [x] T003 [R-1 / DAV-21] 혼합 텍스트 누락 실패 테스트와 읽기 보존 수정
- [x] T004 [R-2 / DAV-22] malformed 응답 실패 테스트와 세션 차단 수정
- [x] T005 [R-3 / DAV-23] //type 실패 테스트와 선택자 분기 수정
- [x] T006 보안 독립 재검토와 전체 로컬 회귀 검증
- [x] T007 Sentry the-voltex-club/hwpx-ocli 사용자 생성 및 실제 GET 연결 확인
- [x] T008 검증 결과, 연동 정보, 잔여 CI/배포 게이트 문서화

T003~T005는 테스트 실패 확인 후 구현한다. T006은 세 수정 후 실행하며,
T007의 로그인 대기는 코드 수정 작업을 막지 않는다.

검증 증거는 [후속 수정 결과](../../docs/reviews/2026-09-08-code-review.md)에 있다.
Linear는 커밋/머지 전 `In Review`로 둔다. OS CI·실제 corpus·설치 승격·릴리스는
이번 로컬 작업의 완료와 구분하며 전체 task-plan의 게이트를 유지한다.

# 2026-09-29 Pro 비교 후 직접 읽기 개선

범위/경계: [ADR-0017](../../docs/adr/0017-hwpx-source-aware-read-model.md).
이 절은 위의 이전 완료 기록을 대체하지 않는다.

- [x] P1 구조/원본 위치/편집 가능성 및 미구현 조회의 실패 계약을 테스트로 고정.
- [x] 기존 XML 스캔에 표·셀·각주/미주·필드 연결, 제한 예산, 소스 해시 추가.
- [x] 선택적 revision 검증과 기존 범위 조회의 생략 정보 구현.
- [x] P0 공용 호스트 JSON 출력의 플러그인 metadata 직렬화 결함 수정·검증.
- [x] P0 기존 설치기로 사용자 플러그인 승격 및 공개 CLI 경로 검증.
- [x] Windows Rust 전체/MSRV/clippy 및 호스트 계약·실제 바이너리 연동 확인.
- [x] 비교 제안별 채택/보류 근거, 실행 증거, native 렌더 및 원격 CI 한계 기록.

증거: [2026-09-29 비교 검토 결과](../../docs/reviews/2026-09-29-pro-comparison.md).
Rust 652개, 호스트 60개, 실제 설치/CLI 재열기 검증이 완료됐다. 원격 CI와 실제
한글 렌더 검증은 기존 릴리스 게이트에 남기며 이 목록의 로컬 완료와 구분한다.


# 2026-09-30 후속 작업

범위/경계: [ADR-0017](../../docs/adr/0017-hwpx-source-aware-read-model.md),
[ADR-0018](../../docs/adr/0018-hwpx-byte-preserving-zip-cow.md).

- [x] Python `zipfile.writestr` 저장 거부 재현·원인 확인. 수정 전 실패하는 COW 회귀 추가.
- [x] ZIP header를 재생성하지 않는 raw layout COW와 G3 raw header 비교 구현. G0~G2 유지.
- [x] 공개 Hancom 표본을 수용 자료로 `//picture` 읽기 전용 조회 구현.
- [x] 조판 캐시 영향을 RHWP로 재현(긴 셀·병합 셀·쪽 이동). writer 변경은 보류.
- [x] `install.ps1` PowerShell 7 요구를 명시하고 계약 테스트 추가.
- [x] Linux 컨테이너와 격리 HOME Windows에서 `test` job 단계 로컬 재현.
- [x] CI MSRV job이 `rust-toolchain.toml`의 stable로 검사하던 결함 수정(`cargo +1.88.0`),
  Linux 컨테이너에서 고친 단계 재현.
- [x] Rust 전체/MSRV/clippy/fmt, 호스트 계약, 설치 release 실제 CLI smoke.
- [ ] 실제 한글 비교(`native_cache_oracle.py`): 09-30 bugcheck 이후 사용자 승인 대기.
- [ ] 원격 GitHub Actions(Linux/Windows/macOS, MSRV): 커밋·push 승인 대기.

증거: [2026-09-30 후속 작업 결과](../../docs/reviews/2026-09-29-pro-comparison.md#후속-작업-결과-2026-09-30).
