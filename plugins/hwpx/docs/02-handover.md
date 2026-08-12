# 인수인계

## 1. 검증한 것 / 검증하지 못한 것

정직하게 구분해 둔다. 이 경계를 모르면 다음 사람이 잘못된 가정 위에 쌓는다.

### 1-a. 실제 `officecli` 바이너리로 검증됨

`scripts/verify-roundtrip.sh`가 전 경로를 재현한다. **43개 항목 전부 통과.**

```sh
scripts/verify-roundtrip.sh --download   # 공식 릴리즈 받아서 검증
scripts/verify-roundtrip.sh              # PATH/캐시의 officecli 사용
```

officecli는 .NET 없이 도는 단일 바이너리다(zero install이 이 프로젝트의 셀링포인트).
스크립트가 릴리즈 자산을 받아 SHA256을 대조한 뒤 실행한다. 검증에 쓴 버전은
`v1.0.143`.

| 검증 항목 | 결과 |
|---|---|
| `officecli plugins list`가 플러그인을 발견 | 통과 |
| `officecli plugins lint` — emit한 모든 prop이 대상 스키마에 선언됨 | 통과, 미지 prop 0개 |
| `officecli view <f>.hwpx text` → 형제 `.docx` 자동 생성 | 통과 |
| 제목/혼합서식/엔티티/탭 텍스트 왕복 | 통과 |
| **문단 내 줄바꿈이 `\v`로 남고 문단이 쪼개지지 않음** | 통과 |
| 가운데·양쪽 정렬, 굵게, 색, 18pt, 글꼴, 문단 여백, 들여쓰기, 줄간격 | 통과 |
| 표 rows/cols/colWidths, 셀 텍스트, 배경색 | 통과 |
| 행 전체 가로 병합 (`colspan=3`) | 통과 |
| **세로 병합 (`vmerge=restart`/`continue`)** | 통과 |
| **행 중간 가로 병합 뒤 셀 인덱스가 당겨지는 것** | 통과 |
| 이미지 data URI 임베드, 대체 텍스트, 크기(2.5cm × 1.3cm) | 통과 |
| **폼 컨트롤 체크박스** (`type=checkbox`, `checked`, `name`) | 통과 |
| **중첩표 안 체크박스 보존** | 통과 |
| **실제 한컴 문서 왕복** (2026 대구문학관 참가신청서) | 통과 — 체크박스 8/8, 표 5개, 열 너비 복원 |

`plugins lint`는 우리가 추측한 게 아니라 **바이너리에 내장된 스키마**로 검사한다.
어휘 매핑을 기계적으로 보증하는 가장 강한 수단이다.

### 1-b. 우리 테스트로 검증됨

`cargo test` 202개, `cargo clippy --all-targets -- -D warnings` 무경고.

| 항목 | 방법 |
|---|---|
| 매니페스트가 §4.1 필수필드를 모두 갖춤 | `protocol_contract.rs::info_declares_required_fields` |
| `--info`가 단일 JSON 객체 + exit 0 | `info_exits_zero_and_prints_single_json_object` |
| JSONL이 한 줄에 객체 하나 / 최상위 배열 아님 | `dump_emits_one_json_object_per_line`, `dump_never_emits_top_level_array` |
| BOM 없음 / CR 없음 / snake_case 키 | `dump_stdout_has_no_bom_and_no_crlf`, `info_uses_snake_case_keys_only` |
| BatchItem 필드가 문서화된 것만 사용 | `dump_lines_use_documented_batch_item_fields` |
| 손상 입력 → exit 2 | `corrupt_input_exits_two`, `missing_file_exits_two`, `zip_without_sections_exits_two` |
| 호스트 예약 코드 6을 절대 내지 않음 | `never_exits_with_host_reserved_code_six` |
| stdout 오염 없음 (도움말·진단·에러) | `help_does_not_pollute_stdout`, `dump_keeps_diagnostics_off_stdout` |
| `--quiet` / `--log-file` / `--media-dir` | 각 동명 테스트 |
| OWPML 파싱 (문단·런·서식·표·병합·이미지·다중섹션·자원 경계) | `parse_owpml.rs` 34개 |
| 병합 격자 인덱싱 (가로/세로/복합/빈칸) | `word.rs::horizontal_merge_mid_row_*` 외 4개 |
| 단위 변환 (HWPUNIT→twip/pt, twip→pt) | `model.rs`, `word.rs::twip_to_pt_divides_by_twenty` |
| 엔티티 해제 (텍스트·속성 양쪽) | `preserves_korean_and_special_characters`, `unescapes_entities_in_attribute_values` |
| 전체 파이프라인 출력 고정 | `golden.rs` + `tests/golden/canonical.jsonl` |

### 1-c. 아직 검증 못함

| 항목 | 위험 | 비고 |
|---|---|---|
| 문서 종류 다양성 | 중간 | 실제 문서 **5건**으로 검증(버그 7건 발견). 구청 공고·양식에 치우쳤다. 보고서·논문·통계자료 계열은 미검증 |
| 이미지·각주·수식·도형·머리말 | 낮음~미상 | 코퍼스 5건에 **하나도 없었다**. 이미지는 합성 픽스처로만 검증됨. 나머지는 미구현 (`03-work-plan.md` 3절) |
| Windows / Linux 동작 | 낮음 | Windows 크로스 타깃 컴파일과 네이티브 CI 검사를 추가했다. 첫 원격 workflow 실행 결과 확인은 남음 |
| 대용량 파일 성능 | 낮음 | 섹션 단위로 읽고 행별 flush하므로 감시견에는 안전. 실측은 없음 |
| `officecli batch` 원자성 상호작용 | 낮음 | `view` 경로는 확인. `--best-effort` 없이 대량 실패 시 거동은 미확인 |

### 다음 사람이 가장 먼저 해야 할 일

**실제 문서를 더 모아서 돌린다.** 1건으로 버그 5개가 나왔다. 표본을 늘리는 것이
가장 효율이 높다.

```sh
officecli-dump-reader-hwpx dump 실제문서.hwpx | head -20   # 텍스트가 나오는지
officecli plugins lint officecli-hwpx --fixture 실제문서.hwpx --json
officecli view 실제문서.hwpx text
```

`plugins lint`가 미지 prop을 뱉거나 텍스트가 비면 그게 곧 수정 목록이다.

**렌더 이미지를 반드시 눈으로 본다.**

```sh
officecli view 실제문서.docx screenshot --out /tmp/check.png
```

3절 10~12번은 계약 테스트와 lint가 모두 통과하는 상태에서 렌더를 보고 찾았다.
`plugins lint`는 prop 이름이 스키마에 있는지만 확인하고, 그 값이 의도한 효과를
내는지는 보지 않는다.
`officecli batch <out.docx> --input <b.jsonl> --best-effort --json`으로 돌리면
어떤 항목이 거부되는지 개별로 볼 수 있다(기본 atomic 모드는 하나만 실패해도
전부 롤백해서 원인 파악이 어렵다).

## 2. 시드 계획서에서 바꾼 것

`docs/00-seed-review.md`에 전문. 요점만:

| 시드 | 실제 | 왜 |
|---|---|---|
| "unhwp의 .NET 바인딩으로 통신" | 바인딩 불필요 | dump-reader의 IPC는 **없음**. stdout JSONL + exit 0이 전부 (§2.1) |
| "플러그인 디스커버리 메커니즘 구현" | 구현 불필요 | 메인이 이미 함. 정해진 경로에 두면 끝 (§3) |
| `unhwp`로 파싱 | `zip` + `quick-xml` 직접 파싱 | `unhwp` 출력(Markdown)은 런 서식·셀 병합·색상을 표현 못 함. 손실 중간표현 (ADR-2) |
| `unhwp = "0.5"` | 최신은 0.7.0 | 확인 안 된 버전 |
| `edition = "2024"` | `2021` | unhwp 0.7이 2021. 2024로 올릴 이유 없음 |
| dump-reader (근거 없이) | dump-reader (근거 명시) | 프로토콜은 `.hwpx`를 format-handler 예시로 든다. 쓰기 구현체가 없어 dump-reader가 맞다 (ADR-1) |

시드 `main.rs`의 문제 9가지도 같은 문서 3절에 정리했다. 핵심은
`read_to_string`으로 ZIP을 읽으려 한 것, `TempDir`이 즉시 drop돼 테스트가
파일 부재로 실패한 것, 테스트 안에서 `cargo run`을 호출한 것이다.

## 3. 개발 중 실제로 잡힌 버그

테스트가 잡은 것들. 이게 TDD가 값을 낸 지점이다.

1. **XML 엔티티 소실.** quick-xml 0.41은 `&amp;` 같은 참조를
   `Event::GeneralRef`로 **따로** 발행한다. 처리하지 않으면 문자가 조용히
   사라진다. `한글 & <꺾쇠>` → `한글  꺾쇠`.
   → `section.rs::resolve_entity`. 테스트: `preserves_korean_and_special_characters`
2. **속성값 엔티티 소실.** 같은 문제가 속성에도 있었다. 원시 바이트를 그대로
   쓰면 `alt="A &amp; B"`가 문자열에 그대로 남는다.
   → `xml.rs::attr`에서 `normalized_value(XmlVersion::Explicit1_0)`.
3. **`\n`이 문단을 쪼갠다.** `_shared/paragraph.json`을 읽고 발견.
   OfficeCLI에서 `text` 안의 `\n`은 **문단 경계**이고 문단 내 줄바꿈은 `\v`다.
   HWPX `hp:lineBreak`를 `\n`으로 내보냈다면 문단 수가 늘어나 이후 모든 경로가
   어긋났을 것이다. → `SOFT_BREAK` 상수. 테스트:
   `line_break_becomes_vertical_tab_not_newline`, `no_emitted_prop_value_contains_raw_newline`
4. **절대 인덱스가 스켈레톤에 의존.** 골든 출력을 눈으로 검토하다 발견.
   `/body/p[4]`는 메인이 만드는 blank 스켈레톤에 빈 문단이 있으면 전부 밀린다.
   → `last()` 술어로 교체 (ADR-4).
5. **자기닫힘 `charPr` 누락.** `expand_empty_elements = false`였을 때
   `<hh:charPr id="0" .../>`는 End 이벤트가 없어 표에 등록되지 않았다.
   → `expand_empty_elements = true`.

### 실제 `officecli` 왕복 검증이 추가로 잡은 것

문서만 읽고는 못 잡았던 것들이다. 그래서 실측이 필요했다.

6. **그림 크기가 단위 없이 나갔다.** `width: "1440"`(twip)을 보냈는데
   `0.0cm`로 렌더됐다. 내장 스키마 설명:

   > width (extent.Cx). **Always pass a unit (cm/in/pt) — a bare number is
   > interpreted as raw EMU (914400 per inch)**, so width=5 renders an
   > effectively invisible 5-EMU image.

   문단 `indent`/`spaceAfter`나 표 `colWidths`는 bare 숫자가 twip으로 맞는데
   **그림만 EMU**다. 이 비대칭을 문서 통독만으로는 놓쳤다.
   → `twip_to_pt_string`. 테스트: `picture_dimensions_always_carry_a_unit`

7. **`vmerge`가 정수가 아니라 enum이었다.** rowSpan 정수(`vmerge: "3"`)를
   보내고 있었다. 실제 값은 `restart` / `continue`다:

   > 'restart' marks the top cell of a vertical span; 'continue' marks
   > subsequent merged cells in the same column.

   HWPX는 병합에 가려진 칸에 `hp:tc`를 만들지 않지만 docx 격자에는 그 칸이
   존재하므로, 가려진 칸마다 `continue`를 채워야 한다.

8. **`colspan`이 셀을 실제로 합쳐 이후 인덱스를 당긴다.** 3열 행에 `colspan=3`을
   주면 그 행의 `childCount`가 3 → 1이 된다. 격자 열번호를 그대로 `tc[C]`로 쓰면
   **행 중간에 병합이 있을 때** 존재하지 않는 인덱스를 가리킨다.
   `[A(2열)][B]`에서 B는 격자 열 2지만 docx에서는 `tc[2]`다.
   → `build_occupancy_grid` + 처리 순번 기반 인덱싱.
   테스트: `horizontal_merge_mid_row_shifts_following_cell_index`,
   `vertical_merge_uses_restart_and_continue_enum`,
   `combined_merge_keeps_colspan_on_continuation_rows`,
   `holes_in_the_grid_still_consume_a_cell_slot`

7·8번은 원래 인수인계 문서에 "위험 중간"으로 적어둔 항목이었다. 실측하니 둘 다
실제로 틀렸다. 추측을 위험으로 표시해두는 것만으로는 부족하고 결국 확인해야 한다.

### 실제 한컴 문서(2026 대구문학관 참가신청서)가 추가로 잡은 것

RHWP로 HWP → HWPX 변환한 실제 양식 문서. **우리가 만든 픽스처로는 하나도
못 잡았을 것들**이다.

9. **`<hp:t>` 안에 `<hp:lineBreak/>`가 들어 있다.** 파서는 문단 레벨 lineBreak만
   `Inline::LineBreak`로 분리했고, `<hp:t>` 내부의 것은 텍스트 문자열에 `\n`으로
   실려 나갔다. 그 `\n`은 문단 경계로 해석되어 문단을 쪼갠다.
   → `normalize_breaks`를 출구 관문으로 두어 출처와 무관하게 정규화.
   테스트: `newline_inside_run_text_never_reaches_output`

10. **셀 `text` prop은 `\v`를 거부하고, `\n`은 조용히 유실된다.**
    실제 왕복에서 배치가 통째로 롤백되며 드러났다:
    "text contains XML-illegal control character U+000B ... Allowed control
    chars: `\t`, `\n`, `\r`". `\n`으로 바꾸니 통과했지만 `<w:t>` 안 리터럴
    문자로 저장되어 **줄바꿈이 되지 않았다**(OOXML에서 `<w:t>` 내부 개행은 공백).
    렌더 이미지를 눈으로 보고서야 알았다.

    표면별 실측 결과:

    | 표면 | `\v` | `\n` | 줄바꿈 됨? |
    |---|---|---|---|
    | `add paragraph --prop text` | OK | OK | `\v` → `<w:br/>` |
    | `add run --prop text` | OK | OK | `\v` → `<w:br/>` |
    | `set <cell> --prop text` | **거부** | OK | **안 됨** |
    | `set <cell>/p[1] --prop text` | OK | OK | `\v` → `<w:br/>` |

    → 셀 내용을 `<cell>/p[1]` 경로로 보낸다. `fill`/`colspan`/`vmerge`/`align`은
    셀 속성이므로 셀 경로에 남긴다. 갓 만든 셀에는 빈 문단이 하나 있어
    `p[1]`이 항상 존재하고, `add`가 아니라 `set`이라 빈 줄이 늘지 않는다.

11. **체크박스는 문자가 아니라 `hp:checkBtn` 폼 컨트롤이다.** 원본에 `☑` 문자가
    3개, `hp:checkBtn`이 8개 있었다. 폼 컨트롤을 무시했더니 **체크 안 된 상자가
    전부 사라졌다.** docx `formfield`(`type=checkbox`, `checked`, `name`)로
    매핑하니 Word에서 실제로 켜고 끌 수 있는 체크박스가 됐다.
    테스트: `checkbox_becomes_interactive_formfield_not_a_character`

12. **중첩표를 평문으로 낮추면 체크박스·이미지가 사라진다.** 8개 중 4개가
    중첩표 깊이 2에 있었고, `plain_text()`로 평탄화하는 코드가 그 4개를 버렸다.
    → 중첩 `Table`을 셀 블록에 그대로 보존하고 OfficeCLI의 셀 아래 table 경로로
    실제 docx 중첩표를 만든다.
    테스트: `preserves_checkboxes_inside_nested_tables`

13. **열 너비를 첫 행에서만 유도하면 양식 문서에서 통째로 버려진다.** 그 문서의
    첫 행은 7열 전체를 병합한 제목 행이었다. `colSpan == 1`인 셀을 모두 훑어도
    0·1·6번 열만 커버됐다. 결과적으로 `colWidths`가 없어 7열이 균등 분배되고
    양식 비율이 깨졌다.
    → 병합 셀의 전체 너비를 제약으로 삼아 반복 해소한다. 미지 열이 가장 적은
    제약을 우선하고, 동점이면 span이 작은(더 국소적인) 제약을 쓴다.
    변환 후 `1592,1344,2211,721,1005,494,2258`(twip)로 실제 비율이 복원됐다.
    테스트: `derives_real_form_document_widths`

10·11·12번은 **렌더 이미지를 눈으로 확인**해서 찾았다. 계약 테스트와 lint는
전부 통과하는 상태였다. `plugins lint`는 prop 이름이 스키마에 있는지만 보고,
그 값이 의도한 효과를 내는지는 보지 않는다.

## 4. 알려진 설계 한계

- **중첩표는 실제 중첩표로 보존되지만 깊이 32를 넘으면 거부한다.** 악성 입력의
  재귀 스택 고갈을 막기 위한 의도적인 자원 경계다.
- **`color`/`size`가 거의 모든 항목에 붙는다.** HWPX는 charPr에 크기·색을
  항상 명시하므로 충실히 옮기면 그렇게 된다. 노이즈로 보이지만 정보 손실이
  없는 쪽을 택했다. docDefaults와 비교해 생략하는 최적화는 추후 과제.
- **`hp:cellSz`가 하나라도 없으면 `colWidths` 전체를 버린다.** 부분적인 열 너비는
  표를 더 망친다.
- **BinData를 못 찾은 이미지는 건너뛴다.** 존재하지 않는 `src`를 내보내면
  replay가 실패한다. 문단은 남으므로 위치는 보존된다.
- **`header.xml`이 없어도 진행한다.** 서식은 잃지만 텍스트는 살린다.
  아무것도 못 하는 것보다 낫다.

## 5. 다음 기능 우선순위 (제안)

1. 실제 한글 저장 파일 검증 (위 1번) — 다른 모든 것의 전제
2. 스타일 이름 매핑 (`styleIDRef` → docx `style`). 제목 계층이 살아나므로
   문서 구조 파악에 가장 크게 기여
3. 각주/미주 — 학술·공문서에서 빈도가 높다
4. 목록 번호 매기기 (`numbering`)
5. 머리말/꼬리말
6. `.hwp` 5.0 (바이너리 OLE) 지원. 이때는 `unhwp`를 쓰는 게 맞다.
   별도 `dump-reader/hwp` 플러그인으로 분리한다.
7. HWPX 쓰기 → 확보되면 `format-handler`로 승격 (ADR-1)

## 6. 참고 자료 위치

프로토콜과 어휘 스펙은 upstream에 있다. 인용한 파일:

- `iOfficeAI/OfficeCLI` `plugins/plugin-protocol.md` — 계약 원본 (909행)
- 같은 저장소 `schemas/help/docx/*.json`, `schemas/help/_shared/*.json` — docx 어휘
- OfficeCLI wiki `command-batch.md` — BatchItem 필드 표
- OfficeCLI wiki `command-dump.md` — 네이티브 dump의 emit 전략
- OfficeCLI wiki `command-add-word.md` — 표 add 문법
- OfficeCLI wiki `command-query-word.md` — `last()` 술어
- `unhwp` 0.7.0 `src/hwpx/{container,section,styles}.rs` — OWPML 요소명 교차검증

OWPML 요소 이름을 기억이나 추측으로 쓰지 말 것. 위 마지막 항목으로 확인했다.
