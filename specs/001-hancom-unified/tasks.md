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
