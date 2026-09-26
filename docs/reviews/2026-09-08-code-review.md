# 2026-09-08 코드 리뷰와 작업 인계

검토 기준: `feat/hwpx-plugin` / `3fb8616a69e93bf0d77c0c98c960c815069963a6`.
작업 시작 시 HEAD는 `17e02a35`였고 검토 도중 조사 자료·도구 설정을 추가한
`3fb8616a`가 들어왔다. 두 커밋 사이의 제품 코드 차이는 없다.

**후속 상태: R-1~R-3 코드 수정과 로컬 회귀 검증 완료.**
아래 결함 설명·행 번호·초기 테스트 수는 수정 전 리뷰 기록이다. 이번 후속 작업에서
제품 코드와 회귀 테스트를 추가했으며 커밋·푸시·사용자 Hancom 플러그인 재설치는
수행하지 않았다. 외부 CI와 실제 corpus 검증은 별도 후속 게이트다.

## 후속 수정 결과

| 항목 | 수정과 검증 | 추적 |
|---|---|---|
| R-1 | 읽기용 문단 값과 텍스트별 쓰기 허용 판단 분리. 혼합 `hp:t`, CDATA, 주석 주변 텍스트, namespace를 확인한 탭/줄바꿈 보존. 혼합 노드는 읽기 전용. run 제어문자는 plain set/save/reopen 후에도 보존. | DAV-21 |
| R-2 | 응답 object·protocol·필수 필드·중복 envelope 키·Unicode 검증. open capabilities가 잘못되면 실패하고 빈 commands는 모든 명령을 거부. 전송/종료 잠금을 통한 broken 상태 재검사. | DAV-22 |
| R-3 | `//document`, `//section`, `//paragraph`, `//text`와 bare 타입 결과 일치. 절대 경로 보존, 일반 XPath 등 지원 밖 선택자는 오류. | DAV-23 |

테스트를 먼저 추가해 R-1/R-3에서 기존 16개 통과·새 3개 실패, R-2에서 실제
`InvalidOperationException`을 확인했다. 보안 경계 독립 분석 후 패치를 작성했고,
별도 독립 재검토가 찾은 escaped lone surrogate 우회도 회귀 테스트로 닫았다.
정상 JSON result 7종(한글·이모지 포함), 정상 plugin error, 빈 capability 대조군,
대기 중인 save가 broken 상태를 다시 검사하는 동시성 사례를 포함한다.

| 최종 로컬 검증 | 결과 |
|---|---|
| `cargo test --workspace --locked --all-targets` | **646 passed** (HWPX format-handler 19개 포함) |
| `cargo clippy --workspace --locked --all-targets -- -D warnings` | 통과 |
| `cargo +1.88.0 check --workspace --locked --all-targets` | 통과 |
| .NET SDK 10.0.302 host build | 성공; 기존 Excel `SheetShift.cs:538` CS8602 경고 1개 |
| 실행형 `OfficeCli.Tests` | **59 passed** |
| 현재 host assembly → 실제 Rust HWPX 세션 | 합성 fixture open/view/query/set/save/close/reopen/validate 통과 |
| Spec-kit Codex integration / prerequisites | status OK; `plan.md`·`tasks.md` 읽기 성공 |
| Sentry 실제 GET + 조회 wrapper | 인증 성공, `prod` 최근 24시간 미해결 이슈 `[]` |

Windows sandbox의 파일 교체/SDK 실행 권한 제한은 정상 사용자 권한 재실행으로
구분했다. 한글·이모지 대조군에서는 가짜 플러그인의 stdout도 명세대로 UTF-8로
설정했다. 로그는 로컬 `%TEMP%/hwpx-fix-{rust,clippy,msrv,build,host-final,native-smoke}.log`다.
인증 정보·개인 문서는 로그나 Git에 포함하지 않는다.

Spec-kit은 Codex 스킬 디렉터리로 전환했다. Linear 프로젝트/이슈 및 Sentry
조직·프로젝트·조회 명령은 [연동 안내](../development-integrations.md)를 따른다.
Sentry 조회 성공은 SDK 이벤트 수집이 구성되었다는 뜻이 아니다.

남은 범위: Linux/macOS 및 새 GitHub CI, `HWPX_CORPUS`와 native oracle,
기존 사용자 dump-reader 설치의 세대 전환, 릴리스/배포. 일반 XPath, 미지원 문서
구조 편집, 호스트의 모든 verb별 payload 검증·server-pushed event 처리는 이 수정
범위에 포함하지 않는다. 이번 R-2 수정은 정상 reply envelope와 open 경계를 다룬다.

## 검토 범위와 방법

- 루트 지침, 개발/사용 문서, 정본 spec/task-plan, Hancom 계약·인계·이력 문서와
  ADR-0013~0016의 저장·설치·호환 경계를 대조했다.
- codebase-memory MCP로 인덱스를 만들고 심볼·호출 관계를 탐색했다. 구체적 판단에는
  현재 파일의 함수 본문과 테스트를 사용했다. 파일 패턴은 `*format_handler.rs`처럼
  glob으로 좁히며, SDK/cache가 포함된 전체 graph 수치는 제품 규모로 사용하지 않는다.
- `format_handler.rs`의 open/view/get/query/set/save, `owpml/editor.rs`의 COW 검증,
  Cell/Show의 package·관계·GIF 검증과 retained-file commit 경로를 살폈다.
- 호스트의 discovery, dump/native sibling 분기, 프로세스 reader drain, 세션 lifecycle,
  그리고 관련 Word projection 계약과 CI/설치 경계를 확인했다.
- 아래 결함은 합성 문서와 작은 가짜 플러그인으로 재현했다. upstream OfficeCLI의 모든
  Excel/PPTX 기능에 대한 전수 행 단위 감사나 별도 침투 테스트를 수행한 것은 아니다.

## R-1 · P1 · 편집 대상 필터가 읽을 텍스트까지 제거한다

위치: [`format_handler.rs`](../../plugins/hancom/crates/hancom-hwp/src/format_handler.rs)
1335~1535행, 특히 1470~1500행의 `active.plain` 검사와 텍스트 인덱스 삽입.
`paragraph_summaries`(739행)가 이 인덱스만 연결하므로 `view text` / `annotated`,
get/query도 누락된 텍스트를 복구할 수 없다.

`hp:t` 안에서 자식 태그·CDATA·주석 등을 만나면 `active.plain=false`로 바꾸고,
닫는 태그에서 해당 텍스트 전체를 인덱스에 추가하지 않는다. 편집을 거부할 조건이
읽기에서도 적용되어, 정상 주변 텍스트를 빈 문자열로 성공 반환한다. `hp:run` 아래
탭·줄바꿈은 별도 값으로 기록하지 않아 양쪽 텍스트가 합쳐진다.

현재 debug 바이너리에 LF JSONL을 직접 보내 확인했다. 각 문서는 첫 문단과 `TAIL`
문단으로 구성했다. 두 경로 모두 exit 0이며 직접 조회 응답은 `msg_type=ok`였다.

| 첫 run의 XML | 기존 명시적 HWPX dump의 첫 문단 | 직접 format-handler의 `view text` |
|---|---|---|
| `<hp:t>LEFT RIGHT</hp:t>` | `LEFT RIGHT` | `LEFT RIGHT\nTAIL` |
| `<hp:t>LEFT<hp:tab/>RIGHT</hp:t>` | `LEFT\tRIGHT` | `\nTAIL` |
| `<hp:t>LEFT<hp:lineBreak/>RIGHT</hp:t>` | `LEFT\u000bRIGHT` | `\nTAIL` |
| `<hp:t><![CDATA[LEFT RIGHT]]></hp:t>` | `LEFT RIGHT` | `\nTAIL` |
| `<hp:t>LEFT</hp:t><hp:tab/><hp:t>RIGHT</hp:t>` | `LEFT\tRIGHT` | `LEFTRIGHT\nTAIL` |
| `<hp:t>LEFT</hp:t><hp:lineBreak/><hp:t>RIGHT</hp:t>` | `LEFT\u000bRIGHT` | `LEFTRIGHT\nTAIL` |

**영향**: 기본 HWPX/OWPML 열기가 format-handler로 승격된 환경에서 내용이 사라진
조회 결과를 정상 문서로 받아들이게 된다. 원본 바이트 손상을 재현한 것은 아니다.

**수정 방향**: 읽기 표현과 `editable` 판단을 분리한다. 읽을 수 있는 mixed content는
그 순서·탭·줄바꿈을 보존하고, 편집할 수 없는 노드는 읽기 전용으로 노출한다. 지원 밖
내용은 의미를 추측해 비우지 말고 명시적 진단/실패 정책을 적용한다. 안전한 plain-text
쓰기 범위를 넓히는 변경과 혼합하지 않는다.

**필수 회귀**: 위 여섯 문서의 read-only/editable open, view/get/query 일관성,
plain text set/save, mixed-content set 거부, 편집하지 않은 entry hash 보존.
`parse_owpml`의 탭·줄바꿈 테스트가 통과해도 이 별도 인덱스 경로는 검증되지 않는다.

## R-2 · P2 · 잘못된 세션 응답 뒤에도 save를 전송한다

위치: [`FormatHandlerSession.cs`](../../src/officecli/Core/Plugins/FormatHandlerSession.cs)
265~297행, `SendRaw`의 `JsonNode.Parse(line)?.AsObject()`와 필드 `GetValue<string>()`.

파싱 블록은 `JsonException`만 잡는다. JSON 배열이나 타입이 잘못된 필드는
`InvalidOperationException`을 던져 `_broken=true` 처리를 지나친다. 이후 `Save()`는
`EnsureUsable()`을 통과해 같은 세션에 저장 요청을 보낸다. 또한 응답의 `protocol`
값을 검사하지 않는다. 이는 프로토콜 §6.7의 malformed reply → broken 경계와 다르다.

현재 빌드한 host assembly의 실제 `FormatHandlerSession`을 reflection으로 호출하고,
정상 open 핸드셰이크 이후 아래 응답을 반환하는 가짜 플러그인으로 확인했다.

| get 응답 | 호스트 결과 | `IsBroken` | 후속 save |
|---|---|---|---|
| `[]` | `InvalidOperationException` | false | 플러그인에 전달됨 |
| `{"protocol":1,"msg_type":42}` | `InvalidOperationException` | false | 플러그인에 전달됨 |
| `{"protocol":1,"msg_type":"error","error":[]}` | `InvalidOperationException` | false | 플러그인에 전달됨 |
| `{"protocol":2,"msg_type":"ok","result":"accepted"}` | 성공 `accepted` | false | 플러그인에 전달됨 |

**영향**: 잘못된 응답을 낸 플러그인을 계속 사용하며, 제어된 `protocol_mismatch` 대신
일반 예외가 노출된다. 가짜 플러그인으로 프로토콜 경계 실패를 재현한 것이며, 정상
Hancom 바이너리가 위 응답을 보낸다는 주장은 아니다.

**수정 방향**: root object, protocol=1, msg_type 타입/값, error envelope의 타입을
변환 전에 확인하고 모든 envelope 위반에서 세션을 broken으로 만든다. 정상적인
plugin error 응답과 프로토콜 자체가 깨진 응답을 구분한다.

**필수 회귀**: 각 malformed 응답의 오류 코드와 broken 상태, 이후 get/save가 자식에게
전송되지 않는지 검사한다. 기존 정상 open/save/close 및 지원하지 않는 명령 계약은 유지한다.

## R-3 · P2 · `//text` 정규화가 절대 경로 분기에 가려진다

위치: [`format_handler.rs`](../../plugins/hancom/crates/hancom-hwp/src/format_handler.rs)
647~662행, `HwpxSession::query`.

`strip_prefix("//")`로 타입 선택자를 정규화한 뒤, 먼저 원본 문자열의
`starts_with('/')`를 검사한다. 따라서 `//text`는 절대 노드 경로와 비교되어 항상
빈 배열을 반환한다. 같은 두 문단 fixture에서 현재 host session → 실제 Rust 경로로
`text`가 2개 노드를 반환하고 `//text`는 `[]`를 반환함을 확인했다.

**수정 방향**: `//` 별칭을 타입 선택자로 먼저 처리하거나, 이를 지원하지 않을 정책이면
해당 정규화를 제거하고 명시적 invalid_argument를 반환한다. 현재 문서가 권하는
`text` 선택자는 동작한다. bare type·별칭·존재하는 절대 경로·잘못된 선택자를 함께
테스트해 빈 검색 결과와 문법 오류를 구분한다.

## 이번 검증 결과

Windows x64, Rust stable 1.97.1, MSRV 1.88.0, .NET SDK 10.0.302, Python 3.13 환경.
테스트 수는 이 OS에서 실제 출력한 결과이며 다른 OS 수치로 일반화하지 않는다.

| 검증 | 이번 결과 |
|---|---|
| `cargo test --workspace --locked --all-targets` | **643 passed, 0 failed** |
| `cargo clippy --workspace --locked --all-targets -- -D warnings` | 통과 |
| `cargo +1.88.0 check --workspace --locked --all-targets` | 통과 |
| `dotnet build src/officecli/officecli.csproj --nologo -p:NuGetAudit=false` | 오류 0, 경고 1 |
| `dotnet run --project tests/OfficeCli.Tests/OfficeCli.Tests.csproj -p:NuGetAudit=false` | **55 PASS**, 실패 없음 |
| `python plugins/hancom/scripts/test_executable_paths.py` | **6 passed** |
| `python scripts/test-workflow-action-pins.py` | **17 passed** |
| 실제 host session → 실제 Rust의 open/set/save/dispose/reopen/view/validate | `REVIEW & SAVED` 재열기 확인, validate `[]` |
| 위 R-1 / R-2 / R-3의 별도 최소 재현 | 모두 현재 동작에서 확인 |
| 변경 문서의 로컬 링크 / `git diff --check` | 링크 63개 유효 / 공백 오류 없음 |

Rust suite 세부 수: core lib 99 + core contract 5 + HWP lib 205 + core compat 1 +
golden 3 + format-handler 16 + installer 22 + carrier 46 + COW 11 + G3 6 +
output conformance 14 + HWPML 44 + OWPML 107 + protocol 64 = 643.

빌드 경고는 기존 [`ExcelHandler.SheetShift.cs`](../../src/officecli/Handlers/Excel/ExcelHandler.SheetShift.cs)
538행 CS8602다. upstream 비교에서 해당 파일의 fork 변경은 없으며, 이번 검토에서는
실제 null 입력으로 재현하지 않아 확정 결함으로 승격하지 않았다.

제한된 실행 토큰에서는 Rust DACL 검사, Python 3.13 임시 폴더 검사, Rust 1.88
실행이 접근 거부로 막혔다. 일반 사용자 권한 재실행은 모두 통과했다. 최초 .NET
restore도 sandbox 네트워크 제한으로 실패했으나 같은 SDK의 일반 실행에서 복원·빌드가
완료됐다. 환경 실패를 제품 테스트 실패로 합산하지 않는다. `NuGetAudit=false` 검증은
의존성 취약점 감사 결과가 아니다.

## 로컬 설치와 검증의 한계

사용자 프로필의 `.officecli/plugins/dump-reader/{hwp,hml,hwpx,owpml}/plugin.exe`는
승격 전 manifest를 반환한다. 모두 이름/버전은 `officecli-hancom-hwp` / `0.1.0`이며
`.hwpx,.owpml,.hml,.hwp`를 함께 광고한다. 현재 레포의 역할별 manifest와 다르다.

`DocumentHandlerFactory`는 dump-reader를 먼저 찾는다. 실제 기본 CLI smoke에서 이
설치가 선택되어 합성 HWPX 옆에 DOCX가 생겼다. 따라서 그 실행을 새 format-handler의
조회 성공 근거로 사용하지 않았다. 사용자 설치는 변경하지 않았다. 위 저장 smoke는
discovery를 거치지 않고 현재 host session에 현재 Rust 실행 파일을 직접 지정했다.
전체 설치→CLI→resident→플러그인 검증을 대체하지 않는다.

이번에 다시 수행하지 않은 범위:

- Linux/macOS 네이티브 실행 및 새 GitHub Actions run. 문서에 이미 기록된
  `33349242319` 등의 과거 CI 증거는 당시 결과이며 이번 HEAD의 새 실행이 아니다.
- 실제 RHWP 바이너리 변환과 private corpus sweep. `HWPX_CORPUS`가 설정되지 않았고
  RHWP도 현재 PATH에서 발견되지 않았다. converter 가짜 프로세스 계약은 Rust suite에 포함된다.
- 전체 `verify-roundtrip.sh`, 한컴 native open/render/save/reopen, 독립 OWPML oracle,
  release publish와 제품 설치. 지원/배포 범위를 넓히기 전에 해당 gate를 다시 확인한다.

직접 format-handler의 `outline`과 `issues`도 현재 빈 결과를 반환하는 구현이다.
이를 모든 문서의 개요 없음·문제 없음의 증거로 사용하지 않는다. 기능 확장을 재개할 때는
DOCX projection의 기능 표와 직접 조회의 기능 표를 따로 검증해야 한다.

## 재현 자료와 다음 작업

현재 checkout의 무시된 디렉터리 `plugins/hancom/.officecli/review-20260908/`에 합성
fixture와 두 재현 프로그램을 남겼다. 개인 문서가 없으며 정식 테스트 suite에 편입한
상태는 아니다. 아래 경로는 로컬 준비 자료로, 새 clone에는 제공되지 않는다.

```powershell
# repository root; configure the pinned SDK as described in DEVELOPMENT.md
python plugins/hancom/.officecli/review-20260908/repro_hwpx_view.py
dotnet run --project plugins/hancom/.officecli/review-20260908/protocol/ProtocolReview.csproj
```

`repro_hwpx_view.py`는 여섯 합성 fixture와 비교 JSON을 해당 디렉터리에만 기록한다.
`ProtocolReview`는 현재 Windows debug host assembly를 참조하며 실제 사용자 문서를
열지 않는 가짜 플러그인 검사다. 최초 재현 결과는 `%TEMP%/hwpx-review-20260908-*.log`
및 위 디렉터리의 `hwpx-view-results.json`에 있다. 정식 수정 시에는 이 문서의 최소
입력과 기대 동작을 versioned `hwpx_format_handler.rs` / host harness로 옮긴다.

1. R-1의 실패 회귀를 추가하고 읽기·편집 인덱스 경계를 수정한다.
2. R-2의 malformed envelope와 후속 명령 차단 회귀를 추가한다.
3. R-3의 selector 별칭 정책을 확정하고 분기/테스트를 정리한다.
4. 전체 로컬 gate 후, 격리된 설치에서 current host의 실제 여섯 확장자 경로를 확인하고
   영향 OS의 CI 및 필요한 corpus/native oracle을 수행한다.
5. 새 표본/제품 정책이 필요한 legacy Cell/Show, 외부 변환기, DVC 정책, crate 분리는
   기존 task-plan의 evidence/structure gate가 열릴 때 재개한다.

문서 정리 내역: [내부 지도](../README.md), [개발 환경](../../DEVELOPMENT.md),
[통합 사양](../../specs/001-hancom-unified/spec.md),
[정본 계획](../../specs/001-hancom-unified/task-plan.md),
[계약 요약](../../plugins/hancom/docs/01-protocol-contract.md)에 현재 상태를 연결했다.
이전 인계·계획 문서는 이력 표시를 붙이고 기존 경로를 보존했다.
