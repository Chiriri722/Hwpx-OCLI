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
| `officecli plugins list`가 기존 HWPX 설치 경로를 발견 | 통과 |
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

Rust 1.88의 전체 테스트와 `cargo clippy --all-targets -- -D warnings`가 통과했다.
테스트 수는 플랫폼 전용 항목 때문에 OS별로 다르므로 명령 결과를 기준으로 본다.

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
| Windows / Linux 동작 | 부분 검증 | run `31700156231`의 양 OS H3 브리지·MSRV 1.88와 기존 HWPX discovery는 성공. H1 run `31890284597`, `32793306250`는 OfficeCLI 1.0.143 오류 처리에 가려 실패했다. 1.0.145 고정 자산은 로컬 양 OS smoke를 통과했으며 workflow 재실행이 필요 |
| 대용량 파일 성능 | 제한적 실측 | macOS arm64 합성 48MiB 표본 1회: 첫 출력 0.471초, 전체 0.540초, peak RSS 106.1MiB. 별도 host 계약에서 1초 idle budget보다 긴 프로세스가 반복 heartbeat로 완료되는 종단간 타이머 reset을 확인. 대형 binary HWP 자체는 미실측 |
| `officecli batch` 원자성 상호작용 | 낮음 | `view` 경로는 확인. `--best-effort` 없이 대량 실패 시 거동은 미확인 |

### 다음 사람이 가장 먼저 해야 할 일

H3 변환 경계와 H4/H5, 양 OS H3 네이티브 게이트 및 Windows의 기존
HWPX discovery는 run `31700156231`에서 끝났다. H1의
`[".hwpx", ".hwp"]` 매니페스트, 두 환경변수, 두 사용자 설치 경로는
로컬에 구현했다. 후속 run `31890284597`, `32793306250`는 실패했으며,
최신 run에서 Linux는 `plugins list` 뒤 HWP `view`, Windows는
`plugins list`에서 멈췄다. OfficeCLI 1.0.143의 `WriteError`가
`System.Private.Xml`을 로드하다 원래 예외를 가린 상태다. 체크섬을 고정한
1.0.145 workflow와 로컬 Windows·비-root Linux smoke는 정상이며, 새 Linux/Windows job의
실제 RHWP `officecli view <file.hwp> text` 성공 전에는 H1을 크로스 플랫폼
완료로 표시하지 않는다.

Phase 7의 host discovery·installer·공급망 하드닝은 로컬에서 완료했다.
Host 계약 35개, Windows installer 12개, 비-root Linux installer 21개와
Rust 전체 225개 회귀가 통과했다. Host는 후보 256개/전체 probe 30초/후보
manifest 1MiB/정상 manifest 합계 16MiB를 all-or-error로 제한하고, 이름 기반
`plugins info`의 재-probe identity가 바뀌면 `plugin_manifest_changed`로
거부한다. 명시적 실행 경로는 한 번만 probe한다.

Unix 설치기는 `hwpx/plugin` 하나만 실제 파일로 교체하고
`hwp/plugin -> ../hwpx/plugin` 상대 심볼릭 링크를 둔다. Windows는
staging한 두 복사본의 SHA-256과 `--info`를 확인한 뒤 순차 교체하며,
중간 실패 시 best-effort rollback한다. 강제 종료까지 포함한 두 경로
완전 원자성은 보장하지 않는다.

Unix rollback은 한 target 삭제가 실패해도 양쪽 백업 복원을 모두 시도한다.
검증된 새 설치 뒤 이전 백업 삭제만 실패하면 새 설치는 성공으로 유지하고
보존된 복구 백업 경로를 stderr 경고로 알린다.

설치와 제거는 절대 `HOME` 아래 `.officecli/plugins/dump-reader/{hwp,hwpx}`까지
기존 조상을 모두 사전 검사해 symlink/junction/reparse와 non-directory를
거부한다. 다만 같은 권한 주체의 검사 직후 경로 교체 경쟁은 handle 기반
installer가 아니므로 잔여 한계다.

OfficeCLI의 플러그인 목록은 정규화된 실행 경로별로 열거하므로 같은
매니페스트가 두 행으로 보일 수 있다. 전체 canonical manifest가 같으면
이름 해석은 첫 discovery 경로를 쓰고, 내용이 다르면 두 행에 경고한 뒤
이름 기반 `info`/`lint`를 거부한다. RHWP가 없을 때 `.hwp`가 exit 3을
반환하는 것은 의도한 선택 기능 계약이며 HWPX 경로에는 영향을 주지 않는다.

`scripts/verify-hwp-pairs.py`는 NFC 정규화로 동명 HWP/HWPX를 찾고 두 JSONL,
unknown prop, OfficeCLI batch/validate 및 문단·표·셀·폼필드 구조를 대조한다.
로컬의 독립 HWP/HWPX 1쌍은 34개 JSONL이 byte-for-byte 일치했고, RHWP 공식
HWP5 3종·HWP3 1종에서 만든 쌍도 19/48/712/467개 항목이 정확히 일치했다.
이는 브리지와 직접 HWPX 경로의 동등성 근거이며, 서로 독립 편집된 더 다양한
실문서 쌍을 계속 모아야 한다.

그 다음 장기 품질 작업은 실제 문서 표본 확대다.

**실제 문서를 더 모아서 돌린다.** 1건으로 버그 5개가 나왔다. 표본을 늘리는 것이
가장 효율이 높다.

```sh
officecli-dump-reader-hwpx dump 실제문서.hwpx | head -20   # 텍스트가 나오는지
officecli plugins lint officecli-hwpx --fixture 실제문서.hwpx --json
officecli view 실제문서.hwpx text
# 원본 옆에 실제문서.docx가 생길 수 있으므로 HWP 복사본으로 실행한다.
OFFICECLI_HWPX_CONVERTER=/absolute/path/to/rhwp officecli view 실제문서.hwp text
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
| `unhwp`로 파싱 | HWPX는 `zip` + `quick-xml` 직접 파싱, 바이너리 HWP는 선택적 RHWP→HWPX 브리지 | `unhwp`도 구조화 모델은 제공한다. 다만 폼 컨트롤 보존 경계를 검증하지 못했고 별도 매핑을 중복 유지해야 한다 (정정된 ADR-2, ADR-5) |
| `unhwp = "0.5"` | 최신은 0.7.0 | 확인 안 된 버전 |
| `edition = "2024"` | `2021` | unhwp 0.7이 2021. 2024로 올릴 이유 없음 |
| dump-reader (근거 없이) | dump-reader (근거 명시) | 프로토콜은 `.hwpx`를 format-handler 예시로 든다. 쓰기 구현체가 없어 dump-reader가 맞다 (ADR-1) |

시드 `main.rs`의 문제 9가지도 같은 문서 3절에 정리했다. 핵심은
`read_to_string`으로 ZIP을 읽으려 한 것, `TempDir`이 즉시 drop돼 테스트가
파일 부재로 실패한 것, 테스트 안에서 `cargo run`을 호출한 것이다.

### 설명만 보고 판단해서 틀린 다섯 번째 사례

Rust 쪽에서 `OsStr` 인자를 보존하면 Linux 비 UTF-8 HWP 경로도 안전하다고
생각했지만, 실제 RHWP v0.8.4는 `std::env::args()`로 UTF-8 `String`을 강제한다.
또 직접 child에 120초 제한을 두면 충분하다고 봤지만 background helper가 stderr를
상속하면 reader join이 무기한 남았다. 독립 리뷰와 실패 테스트로 둘 다 재현했다.

해결은 원본을 UTF-8 고정명의 private `source.hwp`로 복사해 RHWP에는 staging
경로만 넘기고, stderr drain을 bounded로 만들며 Unix process group과 Windows
Job Object로 자손까지 정리하는 것이다. API 표면의 설명이 아니라 실제 의존성
구현과 프로세스 트리를 확인해야 했던 사례다.

후속 검토에서는 상속된 `SIGCHLD=SIG_IGN`/blocked mask, 정상 종료 자손, 공개
scratch 권한까지 재현됐다. Unix는 signal-handler 비의존 `waitid(WNOWAIT)` polling과
`0700`/`0600`, Windows는 protected DACL의 atomic `NtCreateFile`, no-delete-share
handle, Job active-process drain으로 경계를 닫았다. 변환기 자체 경로도 RHWP의
`argv[0]` 수집 때문에 Unicode가 아니면 exit 3으로 거절한다.

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

1. 새 Linux/Windows `.hwp` discovery·RHWP `view` CI를 확인한다
   (`04-hwp-support-plan.md`).
2. 실제 한글 저장 파일 검증 (위 1번) — 장기 품질 작업의 전제
3. 스타일 이름 매핑 (`styleIDRef` → docx `style`). 제목 계층이 살아나므로
   문서 구조 파악에 가장 크게 기여
4. 각주/미주 — 학술·공문서에서 빈도가 높다
5. 목록 번호 매기기 (`numbering`)
6. 머리말/꼬리말
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
