# 구현 계획: 직접 읽기 개선 및 리뷰 후속 수정

## 현재 추가 범위 — 2026-09-29

사용자 제공 `Pro.txt`와 공개 기능 설명을 검토하여 구조/원본 위치/편집 가능성
노출을 채택한다. 외부 구현·의존성은 도입하지 않는다. 기존 XML 이벤트 스캔과
writer를 재사용하고 [ADR-0017](../../docs/adr/0017-hwpx-source-aware-read-model.md)의
실패 계약·자원 예산·revision 조건을 적용한다. 같은 구조로 기존 줄 범위 조회의
원본 경로와 생략 분량을 공개한다. 명시적 텍스트 대상의 양식 채우기를 검증하고
필드명 자동 매칭·범용 diff·캐시 삭제·native 렌더는 근거가 필요한 후속 단계로 둔다.

검증은 실패 계약 → focused/전체 Rust → clippy/MSRV → .NET 계약 → 실제
호스트/플러그인 typed query/get/set/save/reopen 순으로 한다. 사용자에게 승인받은
플러그인 접목 범위에서 구형 dump-reader 설치를 기존 설치기로 승격하고 실제
CLI routing도 검사한다. 결과는 날짜별 리뷰에 기록한다. 원격 CI·한글 렌더링은
이 로컬 완료 기준과 구분하며 기존 릴리스 게이트를 바꾸지 않는다.
실제 CLI에서 추가 발견한 `JsonElement` 직렬화 실패는 공용 `AppJsonContext`의
누락된 타입 등록을 보완하고, get/query 출력 회귀 테스트로 검증한다.

## 후속 범위 — 2026-09-30

09-29 리뷰의 남은 항목을 이어 간다. Python ZIP writer 저장 거부는 writer의 header
재생성이 원인이므로 raw layout COW로 고치고 G3 비교를 raw header 바이트까지 넓힌다
([ADR-0018](../../docs/adr/0018-hwpx-byte-preserving-zip-cow.md)). 공개 Hancom 표본을
수용 자료로 삼아 그림을 읽기 전용으로 조회한다. 조판 캐시는 RHWP로 영향만 재현하고,
writer 변경은 실제 한글 비교 결과가 나올 때까지 보류한다. 한글 자동화는 09-30 bugcheck
이후 사용자 승인 전에는 실행하지 않는다. 원격 CI 대신 Linux 컨테이너와 격리 HOME
Windows에서 `test` job 단계를 재현한다.

## 이전 범위 — 2026-09-08

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
