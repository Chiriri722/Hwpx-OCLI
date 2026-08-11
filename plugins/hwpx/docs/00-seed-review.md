# 시드 재검토 결과 (2026-07-31)

원본: `Monday-chan (임시)/hwpx 확장자 번들 제작 계획서 작성 가능할까요_.zip`

## 0. 압축파일 복구 메모

zip의 중앙 디렉터리에는 2개 엔트리(`main.rs`, `Cargo.toml`)만 등록되어 있었으나
로컬 파일 헤더는 6개 존재했다. 중앙 디렉터리를 무시하고 로컬 헤더를 직접 파싱해
나머지 4개를 복구했다.

| 복구된 파일 | 내용 |
|---|---|
| `main.rs` | TDD 중이던 Rust 코드 |
| `Cargo.toml` | 의존성 선언 |
| `HWPX 포맷 지원 번들/스탠드얼론 제작 계획서.md` | 본 계획서 (시드) |
| `OfficeCLI 및 HWPX 지원 개발을 위한 추천 스킬.md` | 스킬 추천 문서 |
| `SKILL.md` | `python-development` 스킬 (이 프로젝트와 무관) |

`SKILL.md`는 Python 스킬 문서로 이 프로젝트와 관련이 없다. 혼입된 것으로 보인다.

## 1. 계획서 검증 — 맞은 것

| 주장 | 검증 결과 |
|---|---|
| OfficeCLI는 .NET 기반, 단일 바이너리, 플러그인 아키텍처 | **사실.** `iOfficeAI/OfficeCLI`, Apache-2.0, 23.7k stars |
| 플러그인은 독립 사이드카 프로세스 | **사실.** `plugins/plugin-protocol.md` v1 |
| `dump-reader` 종류가 외부 포맷을 네이티브로 변환 | **사실.** §2.1 |
| dump-reader: 단기 실행 / 소스 읽기전용 / JSONL stdout | **사실.** §2.1 표와 일치 |
| 어휘는 메인의 `<target>` 어휘, 플러그인 확장 불가 | **사실.** §2.1, §7.2 |
| 출력은 `<source-stem>.<target>` 형제 파일 | **사실.** §2.1 |
| HWPX = ZIP + XML(OWPML) 개방형 포맷 | **사실.** `mimetype` = `application/hwp+zip` |
| `unhwp`은 Rust, HWP/HWPX 지원, MIT | **사실.** crates.io 등재 |

## 2. 계획서 검증 — 틀린 것 / 수정한 것

### 2.1 `unhwp = "0.5"` → 실제 최신은 `0.7.0`
crates.io: `0.1.17` … `0.5.3`, `0.6.0`, `0.7.0`. 0.7.0은 `edition 2021`,
`rust-version 1.87`, features `[async, default, ffi, hwp3, hwp5, hwpx]`.

### 2.2 `edition = "2024"` + `unhwp 0.5` 조합의 툴체인 요구를 확인하지 않았음
edition 2024는 rustc 1.85+, unhwp 0.7은 1.87+ 필요. 이 환경에는 Rust가
아예 설치되어 있지 않았다(설치로 해결).

### 2.3 "`unhwp`의 .NET 바인딩으로 OfficeCLI와 통신" — **프로토콜과 어긋남**
계획서 4.1은 ".NET 바인딩을 활용하여 OfficeCLI 메인 바이너리와 통신"이라고
적었지만, dump-reader의 IPC는 **없다**. 프로토콜 §2.1:

> IPC | None — plugin writes JSONL (one `BatchItem` per line) to stdout and exits

즉 바인딩·FFI·Rustler가 전혀 필요 없다. 계약은 그냥 **stdout에 JSONL을 쓰고
exit 0**. 계획서 4.2.1의 4번 항목(.NET 바인딩 생성)은 삭제 대상이다.

### 2.4 "플러그인 디스커버리 메커니즘을 구현" — **메인이 이미 함**
계획서 4.2.2는 디스커버리를 우리가 구현/확장한다고 했지만, §3에 고정된
탐색 순서가 이미 정의되어 있다. 우리는 정해진 위치에 바이너리를 놓기만 하면 된다.

1. `$OFFICECLI_PLUGIN_DUMP_READER_HWPX`
2. `~/.officecli/plugins/dump-reader/hwpx/plugin`
3. `<officecli 디렉터리>/plugins/dump-reader/hwpx/plugin`
4. PATH의 `officecli-dump-reader-hwpx` 또는 `officecli-hwpx`

### 2.5 `dump-reader` vs `format-handler` — 근거를 보강해야 함
프로토콜 §2.3은 `format-handler`를 설명하며 **예시로 `.hwpx`를 직접 든다**.
§4.5에도 `officecli-hwpx` format-handler 예시 매니페스트가 있다. 즉 프로토콜
저자의 1순위 상정은 format-handler다. 계획서는 이를 검토하지 않고 dump-reader를 골랐다.

**그럼에도 dump-reader가 맞다**는 결론은 유지한다. 근거:

- format-handler는 소스 파일을 **read-write**로 소유해야 한다(§2.3 표).
  HWPX 쓰기가 필수라는 뜻이다.
- HWPX 쓰기 구현체가 없다. `unhwp`는 이름 그대로 **추출 전용**
  ("extracting HWP/HWPX documents into structured Markdown").
- 쓰기 없이 format-handler를 선언하면 계약 위반이다. 읽기만 되는 것은
  dump-reader가 정확한 종류다.
- 계획서 5절의 "향후 HWPX 쓰기 기능"이 생기면 그때 format-handler로 승격한다.

이 판단은 `docs/01-protocol-contract.md`에 결정 기록으로 남긴다.

### 2.6 **`unhwp`를 파싱에 쓰는 것은 이 용도에 부적합** (핵심 변경)
계획서는 `unhwp`로 HWPX를 파싱하자고 했다. 그러나 `unhwp`의 출력은
Markdown / plain text / JSON(메타데이터)이다. dump-reader가 emit해야 하는 것은
**구조화된 문서 명령**(문단 + 런별 bold/italic/색/크기, 표의 행·열·병합, 이미지)이다.

HWPX → Markdown → BatchItem 경로는 중간 단계에서 서식을 파괴한다. Markdown에는
런 단위 서식, 셀 병합, 색상, 폰트 크기를 표현할 방법이 없다. 즉 **손실 있는
중간표현을 거쳐 손실 없는 출력을 만들려는 설계 오류**다.

대안: HWPX는 ZIP + XML이므로 `zip` + `quick-xml`로 **직접 파싱**한다. 의존성이
더 가볍고, 서식을 온전히 통제할 수 있다.

`unhwp`의 가치는 `.hwp` 5.0(바이너리 OLE 포맷)에 있다. 그건 별도 확장자이므로
`dump-reader/hwp` 플러그인으로 나중에 분리 대응한다.

## 3. 시드 `main.rs` 문제점

```rust
let hwpx_content = fs::read_to_string(hwpx_file_path)?;
if hwpx_content.contains("Hello, HWPX!") {
    println!("{{\"type\":\"text\",\"content\":\"Hello, HWPX!\"}}");
}
```

| # | 문제 | 설명 |
|---|---|---|
| 1 | **가짜 구현** | 파싱이 없다. 입력 문자열 매칭 후 하드코딩된 상수를 출력한다. |
| 2 | **`read_to_string`이 원리적으로 틀림** | HWPX는 ZIP 바이너리다. UTF-8 문자열로 읽으면 실패한다. |
| 3 | **픽스처가 HWPX가 아님** | `<hwpx><body><p>...</p></body></hwpx>`는 존재하지 않는 포맷이다. 실제는 `hp:p`/`hp:run`/`hp:t` (OWPML). |
| 4 | **출력 스키마가 프로토콜과 무관** | `{"type":"text","content":...}`. 실제 계약은 `{"command":"add","parent":"/body","type":"p","props":{...}}`. |
| 5 | **`--info` 미구현** | 매니페스트 없이는 메인이 플러그인을 인식조차 못 한다(§4). |
| 6 | **`dump` 서브커맨드 미구현** | 계약은 `<plugin> dump <source>`인데 `<plugin> <source>`로 받는다. |
| 7 | **테스트가 실행 자체로 실패** | `create_temp_hwpx_file`에서 `TempDir`이 함수 종료 시 drop되며 디렉터리가 삭제된다. 반환된 경로는 이미 존재하지 않는다. |
| 8 | **테스트 안에서 `cargo run`** | 테스트 하네스가 cargo를 재귀 호출한다. 파일 락 경합/데드락 위험, 느림, CI 취약. |
| 9 | **RED가 아니라 위장된 GREEN** | 주석은 "RED (실패 예상)"인데 구현이 어서션 문자열에 맞춰져 있다. 7번 때문에 실제로는 그냥 실패한다. 어느 쪽이든 검증력이 0이다. |

7번과 8번이 합쳐지면 "테스트가 왜 실패하는지 알 수 없는" 상태가 된다.
파싱 실패인지, 임시파일 소멸인지, cargo 락인지 구분이 안 된다.

## 4. 재구축 방침

| 항목 | 시드 | 재구축 |
|---|---|---|
| 플러그인 종류 | dump-reader | dump-reader (근거 문서화, §2.5) |
| 파싱 | `unhwp` → Markdown → 명령 | OWPML 직접 파싱 (`zip` + `quick-xml`) |
| `--info` | 없음 | 프로토콜 §4 필수필드 전부 |
| CLI | `<plugin> <file>` | `<plugin> dump <file> [--media-dir] [--log-file] [--quiet]` |
| 출력 | 임의 JSON | `BatchItem` JSONL, 행별 flush |
| 픽스처 | 가짜 XML 문자열 | 코드로 생성하는 실제 ZIP+OWPML |
| 테스트 실행 | 테스트 내 `cargo run` | 라이브러리 직접 호출 + `assert_cmd` 통합테스트 |
| 종료코드 | 항상 0 | §6.5 매핑 (0 / 2 / 3 / 5) |

## 5. 검증 근거 목록

- `plugins/plugin-protocol.md` (909행, v1 final draft) — 계약 원본
- `schemas/help/docx/*.json`, `schemas/help/_shared/*.json` — 대상 어휘
- wiki `command-batch.md` — `BatchItem` 필드 스키마
- wiki `command-dump.md` — 네이티브 dump의 emit 전략 (우리가 모방할 기준)
- wiki `command-add-word.md` — 표 add 문법 (`rows`/`cols`)
- `unhwp-0.7.0/src/hwpx/{container,section,styles}.rs` — OWPML 요소명 교차검증
