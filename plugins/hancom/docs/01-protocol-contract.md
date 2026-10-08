# 확정 계약 (테스트가 검증하는 대상)

2026-09-08 후속 수정: 직접 HWPX 읽기는 혼합 `hp:t`·CDATA와 run/text 안의
탭·줄바꿈을 보존하며 혼합 노드 편집은 거부한다. 네 가지 `//type` 별칭은 일반
타입 조회와 동일하다. 호스트는 잘못된 응답 형태·버전·Unicode를 `protocol_mismatch`로
차단하고 이후 command/save/close를 보내지 않는다. 정상 error 응답은 세션을 유지한다.

최종 계약 정리: 2026-09-29. 정본은 이 포크의
[`plugins/plugin-protocol.md`](../../plugin-protocol.md) v1이며, Cell/Show에는
fork-local `direct-native` / `byte-preserving` 확장이 적용된다.
기본 계약 출처: `iOfficeAI/OfficeCLI` `plugins/plugin-protocol.md` v1 (final draft),
`schemas/help/docx/*`, wiki `command-batch.md` / `command-dump.md`.
매니페스트·dump 계약은 `tests/protocol_contract.rs`, 설치·discovery
경로는 `tests/install_contract.rs`에서 기계적으로 검증한다.

## C1. `--info` 매니페스트 (§4)

`<plugin> --info` → stdout에 **JSON 객체 1개**, exit 0.

필수 필드 (§4.1):

| 필드 | 타입 | 우리 값 |
|---|---|---|
| `name` | string, kebab-case | `officecli-hancom-hwp` |
| `version` | string, SemVer | `0.1.0` |
| `protocol` | integer | `1` (불일치 시 메인이 exit 5로 거부) |
| `kinds` | array | `["dump-reader"]` |
| `extensions` | array, 점 포함 | `[".hwp", ".hml"]` |
| `idle_timeout_seconds` | object | `{"default":60,"verbs":{"dump":30}}` |
| `runtime` | string | `"rust"` |
| `target` | string | `"docx"` — dump-reader는 **필수**, `docx`/`xlsx`/`pptx` 중 하나 |

위 표는 `officecli-hancom-hwp`의 dump-reader 계약이다. 다른 진입점은 다음과 같다.

| 바이너리 | kind | extensions | target / stdout |
|---|---|---|---|
| `officecli-hancom-hwpx` | format-handler | `.hwpx`, `.owpml` | target 없음 / 세션 JSONL 응답 |
| `officecli-hancom-cell` | dump-reader | `.cell` | xlsx / 성공 시 stdout 0바이트 |
| `officecli-hancom-show` | dump-reader | `.show` | pptx / 성공 시 stdout 0바이트 |

`officecli-dump-reader-hwpx`는 HWP/HML dump-reader의 호환 별칭이다.
명시적으로 `dump file.hwpx`를 진단할 수 있지만 `.hwpx`를 manifest에 광고하지 않는다.

`idle_timeout_seconds` 규칙 (§4.2):
- `default`는 필수, 양의 정수
- 매니페스트에서 `0`은 **금지** (silent never-kill 방지)
- 권장값: `dump-reader.dump` = 30초

## C2. `dump` 서브커맨드 (§5.1)

이 절과 C3, C6~C8의 BatchItem 설명은 HWP/HML dump-reader에 적용한다.
Cell/Show의 별도 성공 계약은 C12를 따른다.

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
| 3 | Feature unsupported in this build | 바이너리 HWP인데 선택적 RHWP 변환기가 없거나 안전하게 실행할 수 없음 |
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

메인의 탐색 순서. 요청한 `(kind, ext)`별 첫 매치가 이긴다.

| 순위 | HWPX | HWP | OWPML | HML |
|---|---|---|---|---|
| 환경변수 | `$OFFICECLI_PLUGIN_FORMAT_HANDLER_HWPX` | `$OFFICECLI_PLUGIN_DUMP_READER_HWP` | `$OFFICECLI_PLUGIN_FORMAT_HANDLER_OWPML` | `$OFFICECLI_PLUGIN_DUMP_READER_HML` |
| 사용자 경로 | `~/.officecli/plugins/format-handler/hwpx/plugin` | `~/.officecli/plugins/dump-reader/hwp/plugin` | `~/.officecli/plugins/format-handler/owpml/plugin` | `~/.officecli/plugins/dump-reader/hml/plugin` |
| bundled 경로 | `<officecli 디렉터리>/plugins/format-handler/hwpx/plugin` | `<officecli 디렉터리>/plugins/dump-reader/hwp/plugin` | `<officecli 디렉터리>/plugins/format-handler/owpml/plugin` | `<officecli 디렉터리>/plugins/dump-reader/hml/plugin` |
| PATH | `officecli-format-handler-hwpx` → `officecli-hwpx` | `officecli-dump-reader-hwp` → `officecli-hwp` | `officecli-format-handler-owpml` → `officecli-owpml` | `officecli-dump-reader-hml` → `officecli-hml` |

Cell/Show는 각각 `OFFICECLI_PLUGIN_DUMP_READER_CELL` /
`OFFICECLI_PLUGIN_DUMP_READER_SHOW`, 사용자·bundled 경로의
`dump-reader/cell/plugin` / `dump-reader/show/plugin`을 사용한다.
Windows 실행 파일에는 `.exe`가 붙는다.

`<kind>`는 kebab-case, `<ext>`는 점 없는 확장자다. Unix 설치기는 HWP와 HWPX
canonical 경로에 역할별 실제 파일을 원자 교체하고, HML은 `../hwp/plugin`,
OWPML은 `../hwpx/plugin` 상대 심볼릭 링크를 둔다. Windows 설치기는 심볼릭 링크
권한에 의존하지 않고 역할별 바이너리를 네 경로에 복사한다. Cell/Show 전용
바이너리는 각 한 경로에 설치한다. 총 여섯 활성 경로를 staging·체크섬·`--info` 의미
검증한 뒤 순차 교체한다. 두 설치기 모두 활성 여섯 경로와 폐기할
`dump-reader/{hwpx,owpml}` 두 경로의 커밋 상태를 추적해 중간 실패 시 기존 상태를
역순으로 conflict-safe best effort 복원한다. 강제 종료까지 포함한 완전한 다중 경로
원자성은 보장하지 않는다. 세부 설치 계약은 [README](../README.md)의 설치 절을 따른다.

`plugins list`는 실행 경로별로 열거하므로 같은 매니페스트가 여러 행으로
보일 수 있다. 이는 `(kind, ext)`별 resolution 실패를 의미하지 않는다.
실제 확인에는 각 확장자의 샘플에 `officecli view <복사본> text`를 사용한다.
HWP/HML dump-reader는 같은 stem의 `.docx`를 만들 수 있으므로 원본이 아닌 복사본으로
실행한다. HWPX/OWPML format-handler는 원본을 직접 열고 형제 DOCX를 만들지 않는다.

## C10. HWPML 단일 XML 경계

HWPML은 대소문자가 정확한 비접두 `HWPML` 루트와 `Version`을 요구하고,
2.1/2.8/2.9/2.91의 보수적 공통 부분집합만 허용한다. 허용 목록은 각 리비전의
전체 문법 지원을 주장하지 않는다. 본문은 `BODY/SECTION/P/TEXT/CHAR`, 표 셀은
`TABLE/ROW/CELL/PARALIST/P` 부모 경로를 검증한다.

`TAB`과 `LINEBREAK`는 공용 inline으로, 묶음 빈 칸 `NBSPACE`는 U+00A0으로
보존한다. 코드 포인트를 단정할 수 없는 `HYPEN`/`FWSPACE`와 내용이 있는 미지원
컨트롤은 exit 3으로 실패한다. 잘못된 네임스페이스, XML 선언, 엔티티, 매핑 ID,
표 좌표는 exit 2다. DTD는 포맷 판별 후 엔티티 확장 없이 exit 3으로 거부한다.
파싱이 끝난 뒤에만 JSONL을 emit하므로 어떤 실패도 부분 stdout을 남기지 않는다.

## C11. HWPX/OWPML lifecycle 호환 경계

호스트의 규범 계약은 여전히 CLI `open <file>` 인자와 첫 JSONL 프레임의 최상위
`path`를 함께 보내는 것이다. Hancom format-handler는 릴리스 호스트 상호운용을
위해 **프레임에서 `path` 키가 완전히 빠진 경우에만** 이미 받은 CLI 경로를 사용한다.
키가 존재하면 문자열이어야 하며, canonical filesystem identity가 CLI 경로와
일치해야 한다. `null`이나 다른 타입, 다른 파일은 폴백하지 않고 거부한다.
`editable` 키가 완전히 빠지면 권한을 추론하지 않고 **읽기 전용(`false`)** 으로만
폴백한다. 키가 존재하면 Boolean이어야 하며, `null`이나 다른 타입은 거부한다.
또한 lifecycle 수정 이전 릴리스 호스트가 사용한 정확한 모양인
`args: {path: <string>, editable: <boolean>}`만 호환 입력으로 허용한다. 이 객체는
두 키를 모두 가져야 하고 다른 키가 없어야 하며, 최상위 `path`/`editable`과 섞이면
거부한다. 중첩 Boolean을 그대로 사용하므로 쓰기 권한을 추론하지 않는다.
같은 구형 호스트의 `save`는 정확히
`{"protocol":1,"msg_type":"command","command":"save","args":{}}`였으므로,
이미 `editable=true`로 열린 세션에서 이 네 필드와 명시적인 빈 `args`만 있는 경우에
한해 저장한다. `args` 누락/null/비어 있지 않음, `props`, 추가 필드는 모두 거부한다.
이 호환 규칙은 쓰기 권한을 만들지 않으며 규범 형식은 계속
`{"protocol":1,"msg_type":"save"}`다.
호스트의 규범 계약에서는 `path`와 Boolean `editable`이 모두 계속 필수이고,
`protocol`과 `msg_type=open`은 호환 폴백 없이 필수다. 근거와 비주장은
[ADR-0015](../../../docs/adr/0015-hancom-format-handler-open-path-compatibility.md)에 기록한다.

---

## C12. Cell/Show direct-native 계약

Cell 12.0300과 Show 12.0000의 검증된 OOXML carrier 부분집합만 지원한다.
매니페스트가 `direct-native`와 `byte-preserving`을 함께 선언하면 호스트는 매번
플러그인을 호출한다. 성공에는 exit 0, raw stdout 정확히 0바이트, 현재 source와
byte-identical한 non-reparse native sibling이 모두 필요하다. JSONL·공백·BOM은
이 모드에서 성공 출력이 아니다. 기존 sibling의 내용이 다르면 덮어쓰지 않는다.

플러그인은 ZIP/XML·관계 closure·허용 profile과 filesystem metadata를 검증한 뒤
no-clobber로 게시한다. 실패 후 호스트는 소유권을 증명하지 못하는 sibling을 삭제하지
않는다. 전체 범위와 한계는 [ADR-0016](../../../docs/adr/0016-hancom-v12-ooxml-carrier-bridge.md)을
따른다. 이는 일반 Cell/Show 변환기나 proprietary parser 계약이 아니다.

## C13. HWPX 구조 조회와 원본 참조

`structured-read`/`source-references` feature가 있는 format-handler는 `get`과
`query`에서 `table`, `cell`, `note`, `field`, `picture` 및 `//type` 별칭을 추가 지원한다.
기존 문단/텍스트 주소는 유지한다. 구조 주소의 숫자는 구역 내 종류별 XML 출현
순서(1부터)이며 셀의 행/열 번호가 아니다. 구조 노드와 문단은 구역의 평탄한
조회 목록이고, 실제 포함 관계는 아래 metadata를 따른다. `children`를 재배치하거나
같은 문단을 복제하지 않으므로 모든 반환 경로는 유일하다.

| `format` 항목 | 계약 |
|---|---|
| `parent_path` | 가장 가까운 인덱싱된 XML 조상. 없으면 구역 |
| `paragraph_paths`, `text_paths` | 포함된 문단/텍스트의 기존 경로. 필드는 검증된 범위만 연결 |
| `source` | `part`, `revision` (`sha256:` + 소문자 64자리), `byte_start`, `byte_end` |
| `self_closing` | 원본이 self-closing 요소인지 여부 |
| 표 `rows`, `cols` | 원문에 선언된 값만 공개 |
| 셀 `row`, `col`, `row_span`, `col_span` | 원문의 0부터 시작하는 주소 및 병합 값. 누락은 필드 생략; 추정·격자 확장 없음 |
| 주석 `note_kind`, `number`, `instance_id` | 각주/미주 종류와 존재하는 원문 속성 |
| 필드 `field_id`, `field_type`, `name`, `declared_editable` | 원문 마커 속성. 이름/선언은 쓰기 권한이나 유일성 보장이 아님 |
| 필드 `range_status`, `content_range` | 시작/끝 ID가 각각 유일하고 순서·subList 영역이 맞으면 `matched` 및 내용 반개구간. 그 외 `ambiguous`, `missing_id`, `missing_end`, `reversed`, `scope_mismatch` |
| 그림 `object_id`, `original_size`, `current_size`, `size`, `shape_comment` | `hp:pic`의 `id`와 직접 자식 `hp:orgSz`/`hp:curSz`/`hp:sz`의 원문 HWPUNIT `width`/`height` 객체. 같은 크기 요소가 반복되거나 값이 정수가 아니면 그 필드만 생략한다. `shape_comment`는 직접 `hp:shapeComment`가 정확히 하나이고 해석 가능할 때만. 단위 변환 없음. 중복 속성처럼 XML 자체가 잘못된 경우는 다른 구조 노드와 같이 거부한다 |
| 그림 `binary_item_id`, `binary_status` | 직접 core `hc:img`의 `binaryItemIDRef`. 상태는 `resolved`, `external`(`isEmbeded="0"`), `missing_manifest`, `unreadable_manifest`, `missing_item`, `ambiguous_item`, `missing_part`, `missing_reference`, `ambiguous_reference` |
| 그림 `binary_part`, `binary_size`, `media_type`, `embedded` | OPF `opf:manifest/opf:item` id가 유일하고 `href`가 실제 part일 때만 part와 선언된 압축 해제 크기. `media_type`/`embedded`는 원문 선언. 파일명 추측 없음 |
| `editable` | 현재 세션에서 이 노드 자체의 `set text`가 가능한지. 컨테이너는 항상 false |
| `editability` | `mode` (`text`, `text-targets`, `locked`), `reason`, 현재 세션의 편집 가능한 `target_paths`; 읽기 단계의 구조상 후보는 `text_candidate`/`candidate_target_paths` |

소스 범위는 압축 해제한 해당 XML part의 바이트 단위 `[start, end)`이며 요소
태그까지 포함한다. 필드의 `source`는 시작 마커 자체, `content_range`는 마커
사이 범위다. part revision은 현재 세션의 staged 편집도 반영한다. ZIP 전체의
revision이나 영구 식별자가 아니므로 `set` 후 다시 조회한다. 스냅샷 이후 외부
파일 변경은 기존 쓰기/저장 검증에서 거부한다.

쓰기 속성은 `text`와 선택적 문자열 `expected_revision`이다. 후자가 해당
part의 현재 revision과 다르면 `invalid_argument`이고 pending 변경은 생기지
않는다. revision만 보내거나 타입이 잘못된 경우도 거부한다. `text`와 미지원
속성을 섞으면 부분 적용 없이 거부한다. 미지원 속성만 보낸 기존 요청은
`unsupported_properties`를 반환한다. 여러 개의 `set`을 하나의 트랜잭션으로
만드는 계약은 아니며, 앞서 성공한 변경을 나중의 실패가 취소하지 않는다.

paired 빈 `hp:t`는 기존 writer로 채울 수 있다. `<hp:t/>`는 읽기 목록과 ordinal에
포함하지만 확장은 미지원이다. 혼합/중첩 내용의 잠금과 G0~G3 쓰기 경계는 유지한다.
원문 `declared_editable`은 정보를 표시할 뿐 기존 writer 권한을 확장하지 않는다.
호스트 resident는 읽기 전용으로 시작하고 첫 수정에서 editable로 승격하므로,
사전 조회에서는 `candidate_target_paths`로 후보를 조사할 수 있다. 후보는 XML
구조상 허용 범위를 뜻하며 strict editable 패키지 검증이나 쓰기 권한의 보증이 아니다.

`view outline`/`view issues`는 `unsupported_feature`를 반환한다. `validate`는
패키지 검증이며 제목 분석·레이아웃 검사를 대신하지 않는다. `view text` JSON의
기존 `text`/`lines`에 `paths`, `total_lines`, `omitted_before`, `omitted_after`를
추가해 범위 조회에서 생략한 분량과 원본 연결을 확인한다.

구역별 XML 요소 100,000개, 깊이 128, 관계 링크 200,000개를 넘으면
`unsupported_feature`로 실패한다. 기존 ZIP/XML 바이트 예산도 계속 적용한다.
이 계약은 레이아웃/자동 필드명 치환/표 구조 편집 지원을 뜻하지 않는다.
설계 근거는 [ADR-0017](../../../docs/adr/0017-hwpx-source-aware-read-model.md)이다.

## C14. HWPX 저장의 ZIP 바이트 보존

편집 저장은 바뀌지 않은 ZIP entry의 local header·payload·data descriptor와 central
record를 바이트 그대로 복사한다. 교체 entry는 CRC-32와 두 크기 필드만 바뀌고 나머지
header 바이트(생산자 `version made by`, `version needed`, flag, 시간, 내부/외부 속성,
이름, 주석)는 원본과 같다. 달라질 수 있는 것은 local header 위치와 end record의 central
directory 위치뿐이다. no-op 저장 후보는 원본과 바이트가 같다. G3와 저장 직전 TOCTOU
검사는 이 raw header 바이트까지 비교한다.

ZIP64, 다중 디스크, entry 사이나 end record 뒤의 여분 바이트, 물리/central directory 순서
불일치, 암호화, stored/deflate 외 압축, local/central 불일치, 인식하지 못한 data
descriptor가 있으면 editable open이 `unsupported_feature`로 실패한다. 교체할 part에
ZIP extra field가 있으면 저장이 `unsupported_feature`로 실패한다. G0~G2 판정은 바뀌지
않는다. 근거는 [ADR-0018](../../../docs/adr/0018-hwpx-byte-preserving-zip-cow.md)이다.

## 결정 기록 (ADR)

### ADR-1: `dump-reader`를 선택한다 (`format-handler` 아님) — superseded
프로토콜 §2.3/§4.5는 `.hwpx`를 format-handler 예시로 든다. 그러나 format-handler는
소스 파일을 read-write로 소유해야 한다(§2.3). HWPX **쓰기** 구현체가 없고
`unhwp`도 추출 전용이므로, 쓰기 없이 format-handler를 선언하면 계약 위반이다.
읽기 전용 마이그레이션은 dump-reader의 정의(§2.1)와 정확히 일치한다.
쓰기 능력을 확보하면 format-handler로 승격한다.

이 판단은 당시 구현에는 맞았지만 package-preserving writer를 확보해
[ADR-0013](../../../docs/adr/0013-hancom-package-preserving-editor-policy.md)으로
뒤집혔다. 현재 `.hwpx`/`.owpml`은 format-handler이고, `.hwp`/`.hml`만
dump-reader다. 설치 승격의 원자적 경계는 ADR-0014에 기록한다.

### ADR-2: `unhwp`를 파싱에 쓰지 않는다
초기 기록은 `unhwp`가 구조화된 모델을 노출하지 않는다고 잘못 전제했다.
결론은 유지하지만 근거를 정정한다. HWPX는 ZIP+XML을 직접 읽어 폼 컨트롤과
HWPUNIT 값을 이미 정확히 보존한다. 바이너리 HWP에서는 이 프로젝트의 핵심인
폼 컨트롤 보존을 `unhwp` 경계로 검증하지 못했고, 별도 중간모델을 추가하면 같은
매핑을 두 번 유지해야 한다. 따라서 HWPX는 `zip` + `quick-xml`로 직접 파싱하고,
바이너리 HWP는 ADR-5의 RHWP→HWPX 브리지로 같은 파이프라인을 재사용한다.

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

### ADR-5: 바이너리 HWP는 선택적 RHWP 프로세스 브리지로 읽는다
RHWP v0.8.4+의 `export-hwpx`를 선택적 외부 변환기로 사용한다. 라이브러리를
링크하거나 HWP 파서를 새로 만들지 않아 기존 HWPX 검증·자원 예산·emitter를
그대로 재사용할 수 있다. 변환기가 없으면 exit 3을 유지한다.

외부 프로세스는 shell 없이 고정 subcommand와 두 경로 인자를 분리해 받는다.
RHWP v0.8.4의 UTF-8 argv 제약 때문에 원본은 private scratch의 `source.hwp`로
복사하며, `converted.hwpx` 산출물을 매직 바이트로 재판별한 뒤 파싱한다. 원본
옆에는 쓰지 않고 RAII로 scratch를 정리하며 staging copy를 256MiB로 제한한다.
Unix scratch/source는 `0700`/`0600`, Windows scratch는 protected owner+SYSTEM
DACL과 원자 create+handle 경계로 보호한다. 총 120초 제한, bounded stderr,
Unix process group, Windows Job Object와 active-process drain을 적용한다.
이 경계의 최신 Linux/Windows 네이티브 검증은 통합 진입점·HWPML 강화 커밋
`17b65ea5`의 GitHub Actions run
[`33170785021`](https://github.com/Chiriri722/Hwpx-OCLI/actions/runs/33170785021)에서
성공했다. RHWP가 없으면 `.hwp`는 exit 3(`unsupported_feature`)을 반환하고
XML 기반 직접 경로는 계속 동작한다.
Windows의 `--media-dir`은 신뢰할 수 없는 junction이 될 수 있어 바이너리
HWP staging에는 쓰지 않고 사용자별 OS 임시 루트를 사용한다.
