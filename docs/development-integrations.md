# Spec-kit · Codex Security · Linear · Sentry

2026-09-08 연결 기준. 비밀 값은 저장하지 않고 식별자만
[development-integrations.json](../.specify/development-integrations.json)에 기록한다.

## Spec-kit

설치된 Specify CLI `1.0.2.dev0`의 공식 `integration install/use/uninstall`로
기본 진입점을 Codex로 전환했다. Copilot 전용 `.github/skills/speckit-*`는
중복 관리 대신 `.agents/skills/speckit-*`로 옮겼다. 공유 스크립트·템플릿의
명령 표기는 `$speckit-*`이며 `specify integration status`가 OK여야 한다.
헌장과 기존 명세·과거 작업 기록은 유지한다.

```powershell
$env:SPECIFY_FEATURE_DIRECTORY = 'specs/001-hancom-unified'
& .specify/scripts/powershell/check-prerequisites.ps1 -Json -RequireTasks -IncludeTasks
```

Codex에서는 `$speckit-analyze`, `$speckit-implement`를 사용할 수 있다.
새 스킬 목록이 보이지 않으면 프로젝트를 다시 열어 로드한다.
현재 실행 입력은 [plan.md](../specs/001-hancom-unified/plan.md)와
[tasks.md](../specs/001-hancom-unified/tasks.md), 전체 이력은
[task-plan.md](../specs/001-hancom-unified/task-plan.md)다.
`.specify/feature.json`은 로컬 상태여서 새 체크아웃에서는 위 환경변수를 지정한다.
현재 등록된 extension hook은 없다.

## Linear와 보안 검토

[Hwpx-OCLI 프로젝트](https://linear.app/david-lee-722/project/hwpx-ocli-c1f60602c6f4)는
David Lee 팀에 연결되어 있다. 같은 결함은 기존 이슈를 갱신하고 중복 생성하지 않는다.

| 리뷰 | Linear | 검증 대상 |
|---|---|---|
| R-1 | DAV-21 | 혼합 HWPX 텍스트 읽기·편집 경계 |
| R-2 | DAV-22 | 잘못된 JSONL 응답 이후 세션 차단 |
| R-3 | DAV-23 | `//type` 선택자 별칭 |

Codex Security `fix-finding` 절차로 R-2를 처리했다. 수정 전 독립 경계 분석,
수정 후 별도의 우회/회귀 검토, 추가 발견의 재현과 회귀 테스트를 수행한다.
이슈에는 실패 원인·영향 경로·대조군·검증 결과를 기록한다.
보안 전체 스캔·외부 CI·릴리스 검증과 이 범위의 수정 검증을 구분한다.

## Sentry

조직 `the-voltex-club`, 프로젝트 `hwpx-ocli`는 사용자가 생성했다.
사용자가 제공한 로컬 토큰으로 공식 API 조회에 성공했다. 2026-09-08 확인에서
`prod`, 최근 24시간, `is:unresolved` 결과는 빈 배열이었다.
이는 인증·조회 성공이며 앱 이벤트 수집이나 운영 무오류의 증거는 아니다.

조회는 설치된 Sentry 플러그인의 `sentry_api.py`를 사용하는
[sentry-issues.ps1](../scripts/sentry-issues.ps1)로 반복할 수 있다.

```powershell
# SENTRY_AUTH_TOKEN을 현재 셸에 설정한 경우
& scripts/sentry-issues.ps1 -Limit 20

# 사용자가 별도 보관한 토큰 파일을 이번 프로세스에서만 사용하는 경우
& scripts/sentry-issues.ps1 -TokenFile '.\조직 토큰.txt' -Limit 3
```

다른 설치 경로는 `-SentryApi` 또는 `SENTRY_API`, Python 경로는 `-Python`으로 지정한다.
조회는 GET만 사용하고 플러그인의 기본 PII 제거를 유지한다. 토큰은 출력하지 않고
자식 프로세스 실행 뒤 원래 환경변수를 복원한다. 읽기 전용 토큰은
`project:read`, `event:read`, `org:read` 권한을 사용한다.

`조직 토큰.txt`는 Git 제외 대상이다. 이 연동은 개발자의 이슈 조사 흐름이며,
제품 SDK/DSN 또는 문서·로그·사용자 데이터의 자동 전송은 추가하지 않는다.
실제 운영 이벤트가 생기면 issue ID·재현 조건을 확인한 뒤 대응 Linear 이슈에
민감한 원문 없이 링크와 원인·검증 결과를 연결한다.
