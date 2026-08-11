# 확정 계약 (테스트가 검증하는 대상)

출처: `iOfficeAI/OfficeCLI` `plugins/plugin-protocol.md` v1 (final draft),
`schemas/help/docx/*`, wiki `command-batch.md` / `command-dump.md`.
아래 항목은 전부 `tests/protocol_contract.rs`에서 기계적으로 검증한다.

## C1. `--info` 매니페스트 (§4)

`<plugin> --info` → stdout에 **JSON 객체 1개**, exit 0.

필수 필드 (§4.1):

| 필드 | 타입 | 우리 값 |
|---|---|---|
| `name` | string, kebab-case | `officecli-hwpx` |
| `version` | string, SemVer | `0.1.0` |
| `protocol` | integer | `1` (불일치 시 메인이 exit 5로 거부) |
| `kinds` | array | `["dump-reader"]` |
| `extensions` | array, 점 포함 | `[".hwpx"]` |
| `idle_timeout_seconds` | object | `{"default":60,"verbs":{"dump":30}}` |
| `runtime` | string | `"rust"` |
| `target` | string | `"docx"` — dump-reader는 **필수**, `docx`/`xlsx`/`pptx` 중 하나 |

`idle_timeout_seconds` 규칙 (§4.2):
- `default`는 필수, 양의 정수
- 매니페스트에서 `0`은 **금지** (silent never-kill 방지)
- 권장값: `dump-reader.dump` = 30초

## C2. `dump` 서브커맨드 (§5.1)

```
<plugin> dump <source-file> [--media-dir <dir>]
```

- `<source-file>`: 절대경로
- `--media-dir`: 플러그인이 임시 파일을 둘 수 있는 스크래치 디렉터리 (선택)
- `--log-file <path>`, `--quiet`: 모든 서브커맨드가 받아야 함 (§5.4)
- `OFFICECLI_BIN` 환경변수: 중간 `.docx`를 만드는 플러그인용. 우리는 사용하지 않음.

## C3. 출력 형식 (§2.1, §5.1)

- JSONL: **한 줄에 JSON 객체 하나**, `\n`으로 종료, **행별 개별 flush**
- 최상위 JSON 배열(`[{...},{...}]`)은 `corrupt_batch`로 **거부됨**
- 각 줄의 스키마는 `officecli batch --commands` 항목 하나와 동일
- 진단 출력은 stderr 또는 `--log-file` (stdout 오염 금지)

## C4. 크로스런타임 규약 (§5.5) — MUST

- stdout/stderr는 **UTF-8, BOM 없음**
- 줄 구분자는 **`\n`** (Windows에서도 `\r\n` 금지)
- 모든 JSON 키는 **snake_case**
- 문서화된 종료코드만 반환

## C5. 종료코드 (§6.5)

| 코드 | 의미 | 우리 매핑 |
|---|---|---|
| 0 | 성공 | 정상 dump |
| 2 | Corrupt input file | ZIP 아님 / OWPML 파싱 실패 / 필수 파트 없음 |
| 3 | Feature unsupported in this build | (예약) |
| 5 | Protocol mismatch | (메인이 판정) |
| 6 | Idle timeout | **호스트가 부과. 플러그인이 직접 내지 않는다** |

## C6. BatchItem 필드 (wiki `command-batch.md`)

| 필드 | 필수 | 용도 |
|---|---|---|
| `command` | 예 | `add` / `set` / `remove` / `move` / `get` / `query` / `raw-set` … |
| `parent` | - | add의 부모 경로 |
| `type` | - | add의 요소 타입 |
| `path` | - | set/remove/move의 대상 경로 |
| `index` | - | 삽입 위치 |
| `props` | - | 객체 `{"k":"v"}` 또는 `["k=v"]` 배열 |

## C7. 대상 어휘 — docx (`schemas/help/docx/*`)

**경로 세그먼트는 요소명이 아니라 축약형이다.** 프로토콜 문서 §5.1 예시는
`/body/paragraph[1]`로 적혀 있으나, 어휘의 single source of truth인 스키마는
`positional: /body/p[N]`이라고 명시한다. 스키마를 따른다.

| 요소 | `type` (alias) | 위치 경로 | 부모 |
|---|---|---|---|
| paragraph | `paragraph` (`p`) | `/body/p[N]` | body |
| run | `run` (`r`) | `/body/p[N]/r[N]` | paragraph |
| table | `table` (`tbl`) | `/body/tbl[N]` | body |
| row | `row` (`tr`) | `/body/tbl[N]/tr[R]` | table |
| cell | `cell` (`tc`) | `/body/tbl[N]/tr[R]/tc[C]` | row |
| picture | `picture` (`image`,`img`) | `/body/p[N]/r[N]` | paragraph |

우리가 사용하는 속성 (스키마에서 확인된 것만):

- paragraph: `text`, `align`, `style`, `indent`, `lineSpacing`,
  `spaceBefore`, `spaceAfter`, `bold`, `italic`, `size`, `color`, `font`, `underline`
- run: `text`, `bold`, `italic`, `underline`, `strike`, `color`, `size`, `font`,
  `superscript`, `subscript`, `highlight`
- table: `rows`, `cols` (add 시 — wiki `command-add-word.md` 확인),
  `colWidths`, `align`, `indent`, `caption`
- row: `cols`, `height`
- cell: `text`, `fill`, `align`, `valign`, `colspan`, `vmerge`, `hmerge`, `width`
- picture: `src` (파일경로/URL/data-URI), `alt`, `width`, `height`

## C8. Emit 전략 (wiki `command-dump.md` 모방)

네이티브 `officecli dump`의 규칙을 따른다:

- **단일 런 문단은 `add p` 한 줄로 병합**한다 (`props.text`에 텍스트).
- **다중 런 문단은 문단 + 런 자식 행으로 분리**한다.
- 표는 typed `add` 행으로 emit.
- 이미지는 `src=` prop에 **data URI로 인라인**.

## C9. 설치 경로 (§3)

메인의 탐색 순서. 첫 매치가 이긴다.

1. `$OFFICECLI_PLUGIN_DUMP_READER_HWPX` (실행파일 절대경로)
2. `~/.officecli/plugins/dump-reader/hwpx/plugin`
3. `<officecli 디렉터리>/plugins/dump-reader/hwpx/plugin`
4. PATH의 `officecli-dump-reader-hwpx` → `officecli-hwpx`

`<kind>`는 kebab-case, `<ext>`는 점 없는 확장자.

---

## 결정 기록 (ADR)

### ADR-1: `dump-reader`를 선택한다 (`format-handler` 아님)
프로토콜 §2.3/§4.5는 `.hwpx`를 format-handler 예시로 든다. 그러나 format-handler는
소스 파일을 read-write로 소유해야 한다(§2.3). HWPX **쓰기** 구현체가 없고
`unhwp`도 추출 전용이므로, 쓰기 없이 format-handler를 선언하면 계약 위반이다.
읽기 전용 마이그레이션은 dump-reader의 정의(§2.1)와 정확히 일치한다.
쓰기 능력을 확보하면 format-handler로 승격한다.

### ADR-2: `unhwp`를 파싱에 쓰지 않는다
`unhwp`의 출력(Markdown/text/JSON)은 런 단위 서식·셀 병합·색상을 표현하지 못한다.
dump-reader는 이 정보를 담은 명령을 emit해야 하므로, 손실 있는 중간표현을 거치면
안 된다. HWPX는 ZIP+XML이므로 `zip` + `quick-xml`로 직접 파싱한다.
`unhwp`는 `.hwp` 5.0 바이너리 지원이 필요해질 때 별도 플러그인에서 채택한다.

### ADR-3: 대상은 `docx`
HWPX는 워드프로세서 포맷이다. `target`은 `docx`/`xlsx`/`pptx` 중 하나여야 하며(§4.1),
문단·런·표 모델이 대응되는 것은 `docx`다.

### ADR-4: 자식 경로에 절대 인덱스 대신 `last()` 술어를 쓴다
처음에는 본문 문단·표를 세어 `/body/p[3]`, `/body/tbl[1]` 같은 절대 경로를 만들었다.
이건 깨진다. §2.1에 따르면 메인은 "blank `<target>` skeleton"을 만들고 배치를
재생하는데, **그 스켈레톤에 빈 문단이 하나라도 들어 있으면 우리가 센 인덱스가
전부 1씩 밀린다.** 우리는 스켈레톤 내용을 확인할 수 없고, 확인할 방법도 없다.

`last()` 술어는 이 의존을 제거한다:

- `add /body --type paragraph` 직후 그 문단은 항상 `/body/p[last()]`다.
- `add /body --type table` 직후 그 표는 항상 `/body/tbl[last()]`다.
- 표 안의 `tr[R]/tc[C]`는 우리가 방금 `rows`/`cols`로 만든 것이라 결정적이다.

근거:
- wiki `command-query-word.md`: "`p[last()]` selects the last paragraph"
- wiki `command-dump.md`: "Subtree emit uses `last()` xpath predicates **so the
  script is safe to replay onto non-blank documents**"

두 번째 인용이 결정적이다. 네이티브 dump가 같은 문제를 같은 방법으로 푼다.

부수 효과로 emitter에서 인덱스 카운터가 사라져 코드가 단순해졌다.
