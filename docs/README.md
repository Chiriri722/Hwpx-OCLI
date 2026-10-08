# 내부 문서 안내

최종 정리: 2026-09-30 · 검토 기준: `feat/hwpx-plugin` / `5431668a` 이후 작업

이 저장소는 OfficeCLI .NET 호스트와 Rust Hancom 플러그인을 함께 개발하는 포크다.
작업을 재개할 때는 아래 순서로 읽는다. 과거 검증 기록의 날짜·커밋을 현재 실행
결과와 혼동하지 않는다.

## 먼저 읽을 문서

1. [최신 비교 검토와 적용 결과](reviews/2026-09-29-pro-comparison.md): 구조/원본 위치
   조회, 호스트 JSON 출력 수정, 설치 승격, 검증과 남은 범위.
   [9월 8일 리뷰](reviews/2026-09-08-code-review.md)는 이전 수정의 근거다.
2. [개발 환경](../DEVELOPMENT.md): 도구 버전, Windows/Unix 실행법, 테스트 명령.
3. [Hancom 사용 안내](../plugins/hancom/README.md): 실제 지원 포맷과 설치·명령.
4. [통합 사양](../specs/001-hancom-unified/spec.md)과
   [정본 작업 계획](../specs/001-hancom-unified/task-plan.md): 요구사항과 완료 근거,
   새 근거가 있어야 재개할 항목.
5. [플러그인 프로토콜](../plugins/plugin-protocol.md)과
   [Hancom 계약 요약](../plugins/hancom/docs/01-protocol-contract.md).
6. [Spec-kit·보안·Linear·Sentry 연동](development-integrations.md)과
   [현재 구현 작업](../specs/001-hancom-unified/tasks.md).

## 현재 코드 지도

| 경로 | 책임 | 먼저 확인할 검증 |
|---|---|---|
| `src/officecli/Core/Plugins/` | discovery, manifest 검증, 자식 프로세스, JSONL 세션·dump 실행 | `tests/OfficeCli.Tests/Program.cs` 실행형 계약 harness |
| `src/officecli/Handlers/DocumentHandlerFactory.cs` | dump-reader 우선 해석, native sibling 선택, format-handler 연결 | 같은 host 계약 및 실제 CLI smoke |
| `src/officecli/Handlers/Word/` | Hancom projection에 필요한 스타일·주석·도형·차트의 DOCX 생성 | host 계약, `verify-roundtrip.sh` |
| `plugins/hancom/crates/hancom-core/` | 컨테이너 판별, 공용 모델·예산·진단, DOCX BatchItem emitter | core unit/contract 테스트 |
| `plugins/hancom/crates/hancom-hwp/src/` | HWPML, RHWP 브리지, HWPX 조회·편집, Cell/Show carrier | Rust integration 테스트 |
| `plugins/hancom/crates/hancom-hwp/src/owpml/` | package·본문·서식·수식, G0~G3, COW 편집 | `parse_owpml`, `owpml_editor_*`, `owpml_output_conformance` |
| `plugins/hancom/scripts/` | 설치·제거, fixture 생성, 코퍼스·왕복 검증 | `install_contract`, Python verifier 검사 |
| `.github/workflows/` | 3 OS host/Rust/MSRV/설치 smoke, action SHA 검사 | `hwpx-plugin.yml`, `action-pins.yml` |

Cargo 디렉터리 `hancom-hwp`의 package 이름은 `officecli-hwpx`다. 공식 진입점은
`officecli-hancom-{hwp,hwpx,cell,show}` 네 개이며, `officecli-dump-reader-hwpx`는
별도 기능이 아닌 호환 별칭이다. 활성 설치 경로는 여섯 곳이다.

코드 탐색은 `AGENTS.md`에 따라 codebase-memory MCP를 우선 사용한다. 이 checkout의
인덱스 이름은 `C-Users-White-Documents-GitHub-Hwpx-OCLI`다. SDK/cache 경로가 그래프에
섞일 수 있으므로 `src/officecli/` 또는 `plugins/hancom/crates/`로 범위를 좁힌다.

## 결정 기록

| 문서 | 고정하는 경계 |
|---|---|
| [ADR-0006](adr/0006-hancom-unified-plugin-boundaries.md) | 공용 core와 역할·target별 플러그인 분리 |
| [ADR-0007](adr/0007-hancom-section-stories-and-note-policy.md), [0008](adr/0008-hancom-paragraph-numbering-policy.md), [0009](adr/0009-hancom-named-style-policy.md) | 구역·주석, 목록, 이름 스타일 |
| [ADR-0010](adr/0010-hancom-shape-and-textbox-policy.md), [0011](adr/0011-hancom-chart-carrier-policy.md), [0012](adr/0012-hancom-private-use-character-policy.md) | 도형, 차트, PUA 보존 |
| [ADR-0013](adr/0013-hancom-package-preserving-editor-policy.md) | HWPX의 제한된 텍스트 편집과 G0~G3 |
| [ADR-0014](adr/0014-hancom-format-handler-install-promotion.md), [0015](adr/0015-hancom-format-handler-open-path-compatibility.md) | 설치 승격과 과거 호스트 lifecycle 호환 |
| [ADR-0016](adr/0016-hancom-v12-ooxml-carrier-bridge.md) | Cell 12.0300 / Show 12.0000 검증·복사 경로 |
| [ADR-0017](adr/0017-hwpx-source-aware-read-model.md) | HWPX 구조·그림·원본 위치·편집 가능성·revision 검증 |
| [ADR-0018](adr/0018-hwpx-byte-preserving-zip-cow.md) | HWPX 저장의 ZIP header 바이트 보존과 raw layout 경계 |
| [프로토콜 제안](proposals/plugin-multi-target-routing-and-export.md) | 아직 구현하지 않은 multi-target/export 제안 및 fork 확장 관계 |
| [공개 규격 출처](spec-sources.md) | 원문 리비전·URL·크기·SHA-256 |

## 이력 문서의 역할

기존 경로는 외부 참조를 보존하기 위해 유지한다. 같은 작업의 현재 상태를 여러 곳에서
독립적으로 관리하지 않는다. 현재 실행은 Spec-kit tasks.md, 전체 이력은 task-plan,
사용 계약은 plugin README와
프로토콜, 날짜별 실행 결과는 리뷰/인계 기록에 적는다.

| 문서/디렉터리 | 해석 방법 |
|---|---|
| [상세 인수인계](../plugins/hancom/docs/02-handover.md) | 기능별 검증 이력. 최신 리뷰와 함께 읽는다. |
| [시드 리뷰](../plugins/hancom/docs/00-seed-review.md) | 2026-07-31 초기 자료 복원 기록 |
| [기존 기능 계획](../plugins/hancom/docs/03-work-plan.md) | 최초 5문서 코퍼스와 후속 완료 이력 |
| [바이너리 HWP 계획](../plugins/hancom/docs/04-hwp-support-plan.md) | RHWP 도입 이력. 과거 대기 상태를 현재 blocker로 재사용하지 않는다. |
| [보안·호환 계획](../plugins/hancom/docs/05-security-compatibility-plan.md) | 과거 수정 근거와 당시 검증 수치 |
| [이전 task_plan](../plugins/hancom/task_plan.md), [notes](../plugins/hancom/notes.md) | 통합 계획 이전 작업과 실험 기록 |
| `.agents/brain/research/` | 날짜가 있는 정제된 조사 메모. 최신 ADR/사양과 대조한다. |
| `.agents/research/`, `.kiro/`, `.specify/`, `.agents/skills/` | 조사 원본·도구 설정·Codex 작업 도구. 제품 구현 상태의 정본이 아니다. |

개인 문서와 회귀 코퍼스는 `HWPX_CORPUS` 외부 경로에 유지한다. `.dotnet/`,
`.nuget/`, `plugins/hancom/{target,.officecli}/`는 재생성 가능한 로컬 산출물이다.
