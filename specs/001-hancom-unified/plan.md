# 구현 계획: 2026-09-08 리뷰 후속 수정

기준 명세: [spec.md](spec.md). 이전 단계의 설계·완료 증거는
[task-plan.md](task-plan.md)에 보존한다. 이 파일과 [tasks.md](tasks.md)는
현재 Spec-kit 실행 입력이며 과거 완료 기록을 다시 구현하지 않는다.

## 기술 범위

- Rust `officecli-hwpx`: 읽기 인덱스에서 혼합 텍스트와 탭/줄바꿈을 보존한다.
  쓰기 허용 판단은 별도로 유지하고 기존 COW 저장기 범위를 확장하지 않는다.
- .NET 10 `FormatHandlerSession`: JSONL 응답 형태/버전을 검증하고 잘못된
  응답 이후 command/save/close를 차단한다. 정상 error와 임의 JSON result를 보존한다.
- 네 가지 HWPX 타입의 `//type` 별칭과 절대 경로를 구분한다.
- Codex Spec-kit 진입점, Linear 수정 추적, Sentry 조직/프로젝트 연결을 구성한다.

## 헌장 확인

프로토콜 호환성과 명시적 실패, 원본 보존, bounded execution, 실패 테스트 우선,
native 실행 증거, 개인정보/코퍼스 외부 보관 원칙을 적용한다.
보안 수정은 독립 경계 분석 → 구현 → 독립 우회/회귀 검토 순으로 수행한다.
검증 전 이슈 완료 또는 모든 OS 배포 가능을 주장하지 않는다.

## 검증

각 회귀 테스트 실패를 확인한 뒤 focused suite, 전체 Rust workspace 테스트와
clippy, .NET 빌드/실행형 계약 테스트를 수행한다. MSRV는 1.88.0이다.
새 CI/native oracle 및 실환경 Sentry 이벤트 수신은 로컬 단위 검증과 별개다.
