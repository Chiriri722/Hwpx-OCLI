# 작업 계획 (2026-08-01)

> **역사 문서**: 이 문서는 HWPX 파서 P0/P1/P-1 계획과 그 근거를 보존한다.
> 현재 실행 계획은 `../task_plan.md`, 바이너리 HWP 결정은
> `04-hwp-support-plan.md`를 따른다. 아래의 과거 범위 제외 결정은 후속 ADR로
> 대체된 경우 최신 문서를 우선한다.
>
> **진행 상태**: P0(G1·G2)과 P1(G3·G4) 완료. P-1 회귀 코퍼스 도입 완료.
> 남은 것은 P2(G5 스타일 매핑)와 표본 확대.
> 4절의 각 항목 아래에 완료 기록을 덧붙였다.

실제 문서 5건으로 검증한 결과와 그로부터 도출한 다음 작업 계획.

## 0. 요약

| 항목 | 결과 |
|---|---|
| 검증 표본 | 실제 문서 **5건** (구청 공고·안내문·서식, 문학관 신청서) |
| 텍스트 커버리지 | **5/5 완전** (한컴 `PrvText.txt` 기준 대조) |
| 변환 실패 | **0건** (전부 exit 0, `plugins lint` 미지 prop 0) |
| 이번에 고친 버그 | **2건** (취소선 오적용 152건, 폰트 인덱스 노출 152건) |
| 남은 격차 | **5건** (P0 2건, P1 2건, P2 1건) |
| 우선순위 재조정 | 각주·수식·도형을 **후순위로 강등** (코퍼스에 0건) |

## 1. 표본 구성

| # | 문서 | 크기 | 출처 변환 | 특징 |
|---|---|---|---|---|
| d1 | 공시송달공고문(제2026-59호) | 53KB | **한컴 네이티브** | 문서 전체가 표 1칸 안. 중첩표 |
| d2 | 외식업소 공공배달 지원사업 모집안내문(2차) | 16KB | RHWP | 문단 70개, 표 1개, 다단 |
| d3 | 2026 대구문학관 공모전 신청서 P1 | 19KB | RHWP | 표 5개, 체크박스 8, 누름틀 4 |
| d4 | 같은 문서 P2 | 18KB | RHWP | 동일 계열 |
| d5 | AI활용 아이디어 제안서_서식 | 11KB | RHWP | 색상·글꼴 다양, 내어쓰기 |

**d1이 중요하다.** `META-INF/rhwp-hwp5-origin` 표식이 없는 유일한 파일로,
한컴 한글이 직접 저장한 네이티브 HWPX다. 이전 인수인계 문서에서
"위험 중간"으로 남겨둔 항목("한컴 직접 저장본 미검증")이 이것으로 해소됐다.

### 코퍼스가 실제로 담고 있는 내용 요소

정확한 태그 경계로 조사한 결과:

| 요소 | 등장 |
|---|---|
| 문단·런·서식 | 전부 |
| 표 (병합·배경색·중첩) | 전부 |
| 누름틀 (`hp:fieldBegin type="CLICK_HERE"`) | d3, d4 |
| 폼 체크박스 (`hp:checkBtn`) | d3, d4 |

**없는 것**: 이미지, 각주, 미주, 수식, 사각형, 글상자, 직선, 다각형, 타원,
곡선, 연결선, 묶음, 차트, OLE, 하이퍼링크, 책갈피, 콤보박스, 입력상자,
버튼, 라디오, 메모, 자동번호, 쪽번호, 머리말/꼬리말.

이 사실이 3절의 우선순위 재조정 근거다.

## 2. 이번에 고친 것

둘 다 **잘못된 데이터를 내보내던** 문제로, 픽스처로는 잡을 수 없었다.
내 픽스처가 스펙 추측에 기반했고 실제 한컴 출력과 달랐기 때문이다.

### F1. `hh:strikeout`은 `type`이 아니라 `shape`를 쓴다

```xml
<hh:strikeout shape="NONE" color="#000000"/>
```

5개 문서 241개 `charPr` **전부** `shape`만 갖고 `type`은 없다. 파서가 `type`을
읽고 없으면 `"SINGLE"` 기본값을 줬으므로 **모든 글자에 취소선이 붙었다**(152건).
렌더 이미지에서 제목과 소제목에 줄이 그어진 것으로 발견했다.

수정: `shape` 우선, `type` 폴백, **둘 다 없으면 끈 것으로 본다**.
취소선은 드문 서식이므로 모호할 때 끄는 쪽이 안전하다.
테스트: `strikeout_reads_shape_not_type`

### F2. `hh:fontRef`는 폰트 이름이 아니라 폰트 표의 인덱스다

```xml
<hh:fontfaces><hh:fontface lang="HANGUL"><hh:font id="0" face="한컴바탕"/>
<hh:charPr id="0"><hh:fontRef hangul="0" latin="0"/></hh:charPr>
```

파서가 `@hangul`을 이름으로 그대로 썼으므로 `font: "14"`, `font: "2"` 같은
값이 나갔다. 수정 후 `함초롬바탕`(78), `맑은 고딕`(24), `HY헤드라인M`(15) 등
실제 이름이 나온다.

- `hh:fontfaces` 사전 훑기로 `id → face` 표를 만든다.
- 언어별 `hh:fontface`가 같은 id 공간을 쓰므로 **HANGUL을 우선**하고 나머지는
  빈 자리만 채운다.
- 표에 없는 순수 숫자는 **버린다**. `font: "99"`를 내보내는 것보다 낫다.
- 이름을 직접 쓰는 생성기도 있으므로 그 경우도 받아준다.

테스트: `font_ref_resolves_through_the_font_table`,
`font_ref_accepts_a_literal_name_when_not_an_index`

## 3. 남은 격차와 우선순위

### 우선순위 기준

1. **잘못된 출력을 내는가** — 없는 것보다 나쁘다. 최우선.
2. **실제 표본에서 몇 건 발생하는가** — 추측이 아닌 실측 빈도.
3. **사용자 목적(양식 작성)에 직접 기여하는가** — 이 프로젝트의 시작이
   "체크박스 조작이 힘들다"였다.
4. 구현 비용.

### P0 — 잘못된 출력을 내고 있다

#### G1. 음수 `firstLineIndent`가 유효하지 않은 OOXML을 만든다 — **완료**

**증거**: 코퍼스에 41건. HWP 내어쓰기(hanging indent)는 `hc:intent`가 음수다.
현재 그 값을 `firstLineIndent`에 그대로 넣는다. 실측 결과:

```
firstLineIndent=-500 → <w:ind w:left="1000" w:firstLine="-500"/>   ← w:firstLine은 음수 불가
hangingIndent=500    → <w:ind w:left="1000" w:hanging="500"/>      ← 올바름
```

`officecli`는 값을 그대로 통과시키므로 lint와 계약 테스트가 전부 통과한다.
d5 렌더에서 줄바꿈된 행의 들여쓰기가 어긋난 것으로 확인했다.

**작업**: `ParaStyle`에서 음수 `indent_first_twip`을 `hanging_twip`으로 분리하고
emitter에서 `hangingIndent`(양수)로 내보낸다. 0과 양수는 현행 유지.

**검증**: 단위 테스트 + `raw`로 `w:hanging` 확인 + d5 렌더 대조.
**비용**: 작음 (반나절)

**결과**: `ParaStyle::set_first_line_indent()`가 부호로 갈라 넣고 emitter가
`hangingIndent`(양수)로 내보낸다. 코퍼스 41건이 `w:hanging`으로 나가고
음수 `w:firstLine`은 0건. 테스트: `negative_first_line_indent_becomes_hanging`,
`parses_hanging_indent_from_negative_intent`,
`hanging_indent_uses_its_own_prop_never_a_negative_value`

#### G2. 셀 안 여러 문단이 하나로 뭉개진다 — **완료**

**증거**: 코퍼스 셀 151개 중 **25개**가 문단 2개 이상. 최댓값은 d1의 **14개**.
d1은 문서 본문 전체가 표 1칸 안에 있어 31개 문단이 셀 문단 1개가 됐다.
한국 공문서에서 흔한 레이아웃이다.

현재는 `\v`(soft break)로 이어 붙인다. 텍스트는 보존되지만 문단별 정렬·여백·
스타일이 전부 소실되고, 문서 구조(문단 수)가 왜곡된다.

**작업**: 셀의 첫 문단은 `set <cell>/p[1]`에 넣고, 두 번째부터는
`add <cell> --type paragraph`로 문단을 추가한다. 실측으로 셀에 문단 추가가
동작하는 것은 확인했다(`add paragraph into cell` → OK).

주의: 갓 만든 셀에는 빈 문단이 하나 있으므로 **첫 문단은 반드시 `set`**이어야
한다. 전부 `add`로 하면 맨 위에 빈 줄이 생긴다.

**검증**: 셀당 문단 수를 `get <cell> --depth 1`의 `childCount`로 대조.
d1의 셀 문단 수가 14가 되는지 확인.
**비용**: 중간 (하루)

**결과**: 셀당 최대 문단 수가 원본과 일치(9/13/13/1). d1 렌더에서 제목 가운데
정렬·굵게, 본문 내어쓰기, 문단 간격이 모두 복원됐다. 다중 서식 문단의 런은
`<cell>/p[last()]`에 붙는다. 테스트:
`many_paragraph_cell_preserves_count_and_per_paragraph_align`,
`mixed_style_cell_paragraph_emits_runs_to_the_right_paragraph`

### P1 — 사용자 목적에 직접 기여

#### G3. 누름틀(`CLICK_HERE`)을 입력 가능한 텍스트 폼필드로 — **완료**

**증거**: d3·d4에 8건. 구조:

```xml
<hp:fieldBegin type="CLICK_HERE" name="" editable="1">
  <hp:parameters><hp:stringParam name="Command">
    Clickhere:set:51:Direction:wstring:9:기재하지 마세요. HelpState:wstring:0:
  </hp:stringParam></hp:parameters>
</hp:fieldBegin>
... 본문 런 ...
<hp:fieldEnd beginIDRef="1520616239"/>
```

HWP의 "클릭해서 입력" 자리다. 즉 **양식의 입력란**이다. docx `formfield`가
`type=text`를 지원하므로(`text`/`name`/`enabled`) 체크박스와 같은 방식으로
매핑할 수 있다. 그러면 Word에서 탭으로 이동하며 채울 수 있는 양식이 된다.

이 프로젝트의 출발점이 "양식 조작이 힘들다"였으므로 체크박스 지원과 같은
계열의 성과다.

**작업**:
- 파서: `fieldBegin`/`fieldEnd`를 `beginIDRef`로 짝지어 구간을 인식한다.
  `Command` 문자열에서 `Direction:wstring:<len>:<안내문>`을 뽑아 안내 문구로 쓴다.
- 모델: `Inline::TextField { name, hint, editable }`
- emitter: `add <para> --type formfield --prop type=text --prop name=... --prop text=<안내문>`

**미확인**: `Command` 문자열 형식이 이 두 문서에서만 관찰됐다. 파싱은
관용적으로(실패 시 안내문 생략) 처리하고 원문을 버리지 않는다.

**검증**: `officecli query <f> formfield`로 개수·타입 대조.
**비용**: 중간 (하루)

**결과 및 계획 수정**: 실측에서 규칙을 바꿨다. 처음 계획은 모든 누름틀을
폼필드로 바꾸는 것이었는데, d3/d4가 **작성 완료된 제출본**이어서 에세이 815자와
시 본문이 누름틀 안에 있었다. 폼필드로 바꾸면 그 내용이 `w:fldChar` 결과로
들어가 **글자 서식을 잃고** `view text`·`get` 표면에서 덜 드러난다.

그래서 규칙을 나눴다:

| 누름틀 구간 | 판정 | 처리 |
|---|---|---|
| 내용 없음 | 빈 입력 슬롯 | `formfield type=text` (안내 문구를 초기값으로) |
| 내용 있음 | 문서 내용 | 서식 유지한 일반 텍스트 런 |

d3/d4 결과: text 폼필드 3개 + checkbox 8개, 에세이 815자는 `align`·`bold`·
`italic`·`color`·`size`·`font`를 유지한 채 셀 텍스트로 남는다.

안내 문구는 `Direction:wstring:<문자수>:<문구>`에서 뽑는다. **문자 단위**이지
바이트가 아니다(한글은 UTF-8 3바이트). 테스트:
`empty_click_here_field_becomes_a_text_field`,
`filled_click_here_field_stays_as_styled_text`,
`parses_direction_hint_from_command_string`,
`hint_length_is_counted_in_characters_not_bytes`

#### G4. 중첩표를 실제 중첩표로 — **완료**

**증거**: d1·d3·d4에 존재. 현재는 행마다 탭 구분 텍스트 문단으로 평탄화한다.
내용은 보존되지만(체크박스 포함) 표 구조가 사라진다.

이전에 평탄화를 택한 이유는 "`add cell` 아래에 `table`을 넣는 경로가 어휘
스키마에 명시돼 있지 않다"였다. 이제 실제 바이너리가 있으므로 **실측으로
확인할 수 있다**.

**작업**:
1. 먼저 실측한다: `add /body/tbl[1]/tr[1]/tc[1] --type table --prop rows/cols`가
   되는지, 그리고 그 안의 셀 경로가 어떻게 되는지.
2. 되면 중첩표를 그대로 만든다. 안 되면 현행 평탄화를 유지하고 그 사실을
   ADR로 기록한다.

**검증**: `get /body/tbl[1]/tr[1]/tc[1] --depth 2`로 중첩 table 자식 확인.
**비용**: 실측 결과에 따라 작음~중간

**결과**: 실측에서 **된다**는 것을 확인했다.
`add <cell> --type table --prop rows/cols` 가 동작하고, 경로는
`<cell>/tbl[last()]` 이며 진짜 중첩 `<w:tbl>`이 만들어진다.
`<cell>/tbl[last()]/tr[R]/tc[C]/p[1]` 로 중첩 셀도 채울 수 있다.

`/body/tbl[last()]`가 body의 **직속** 자식만 고르므로, 셀 안에 표를 추가해도
바깥 표를 가리키는 경로는 변하지 않는다. 그래서 경로 충돌이 없다.

모델을 `Cell.paragraphs: Vec<Paragraph>` → `Cell.blocks: Vec<Block>`로 바꿔
문단과 중첩표를 등장 순서대로 담는다. d1 렌더가 원본 표 구조를 복원했다.
테스트: `nested_table_becomes_a_real_nested_table`,
`preserves_checkboxes_inside_nested_tables`

### P2 — 구조 품질

#### G5. `styleIDRef`/`hh:heading` → docx `style` — **보류 (실측 근거)**

**증거**: 모든 문서에 `styleIDRef`가 있고 `hh:paraPr`에 `hh:heading` 요소가 있다.
현재 둘 다 무시한다. 그래서 변환 결과의 모든 문단이 `Normal`이다
(d1 실측: `Style Distribution: Normal: 17`).

제목 계층이 살아나면 문서 구조 파악(목차, 개요 보기, 접근성)에 크게 기여한다.
다만 코퍼스 5건은 대부분 시각적 서식(굵게·크기)으로 제목을 표현하고 있어
`hh:heading type="NONE"`이 많다. 즉 **효과가 문서에 따라 크게 다르다.**

**작업**: `Contents/header.xml`의 `hh:styles`에서 `id → name` 표를 만들고,
`hh:heading/@level`이 있으면 `Heading{level}`로, 없으면 스타일 이름을 그대로
`style` prop에 넣는다. docx에 없는 스타일 이름은 무시된다는 점을 확인해야 한다.

**검증**: `view <f>.docx stats`의 Style Distribution.
**비용**: 중간

**실측 결과 — 지금은 하지 않는다**

착수 전 실측에서 세 가지가 드러나 보류로 결정했다.

1. **본문 문단이 개요 스타일을 쓰지 않는다.** 문서별 `styleIDRef` 분포:

   | 문서 | 사용 스타일 |
   |---|---|
   | d1 | 바탕글(Normal) × 31 |
   | d2 | 바탕글(Normal) × 81, 도표제목 × 1 |
   | d3 | 바탕글(Normal) × 105 |
   | d4 | 바탕글(Normal) × 105 |
   | d5 | 바탕글(Normal) × 28 |

   `개요 1`~`개요 6`(Outline 1~6) 스타일은 `hh:styles`에 **정의만 되어 있고**
   본문에서 쓰이지 않는다. HWP가 기본 제공하는 스타일이라 모든 문서에 존재한다.
   즉 계획서에서 "효과가 문서에 따라 다르다"고 적은 것이 실측으로 확인됐다.
   이 코퍼스에서는 **효과가 0이다.**

2. **`style` prop은 정의 없는 스타일을 만들어내지 않는다.** 실측:

   ```
   add paragraph --prop style=Heading1  →  <w:pStyle w:val="Heading1"/>
   styles.xml 에 정의된 styleId        →  ["Normal"] 뿐
   ```

   `Heading1`, `도표제목` 모두 이름 그대로 통과하고 `validate`도 지나가지만
   **정의가 없으므로 서식도 개요 수준도 생기지 않는다.** 즉 지금 그대로
   `style`을 내보내면 **매달린 참조만 늘고 얻는 것이 없다.**

3. **제대로 하려면 스타일 정의부터 만들어야 한다.** `officecli help docx style`에
   `add /styles --type style`과 `outlineLvl`(0-9, "Drives TOC and Navigator")이
   있으므로 가능하다. 다만 그러면 작업 범위가
   "스타일 표 파싱 + 스타일 정의 생성 + 참조" 세 단계로 늘어난다.

**결론**: 우선순위 기준 2번("실측 표본에서 몇 건 발생하는가")에 따라 보류한다.
개요 스타일을 실제로 쓰는 문서(보고서·논문 계열)를 표본에 확보하면 그때
착수한다. 그때는 **스타일 정의 생성까지** 범위에 넣어야 한다.

지금 `style`을 내보내지 않는 것이 옳은 동작임을 확인한 것도 결과다.

#### G6. 한컴 사용자 정의 영역(PUA) 문자 — **부분 완료 (감지·보고)**

**증거**: d2에 `U+F0854`/`U+F0855` 한 쌍. 5개 문서 중 1건, 총 2자.

한글은 일부 특수문자를 유니코드 사용자 정의 영역에 저장한다. 한컴 글꼴이
그 코드포인트에 글리프를 매핑하므로 한글에서는 정상으로 보이지만, 다른
글꼴에서는 빈 사각형이 된다. d2 렌더에서 `『』`가 있어야 할 자리에 사각형이
나온 것으로 발견했다.

**매핑을 추측하지 않는다.** 해당 런의 `charPr`에 `fontRef`가 없어 어느 글꼴의
어느 글리프인지 특정할 수 없다. 문맥으로 `『』`라고 짐작할 수는 있지만,
이 프로젝트에서 추측이 틀린 사례가 이미 여럿이다(취소선 `type`,
폰트 인덱스, `vmerge` enum, 그림 크기 단위).

**한 것**: 문자는 **그대로 보존**하고 개수를 진단으로 보고한다.

```
$ officecli-dump-reader-hwpx dump 모집안내문.hwpx > /dev/null
dumped 193 batch items from 모집안내문.hwpx
note: 2 private-use character(s) passed through unmapped (Hancom-specific
glyphs; they may render as empty boxes outside Hancom fonts)
```

정보를 잃지 않으면서 사용자가 확인해야 할 지점을 알려준다.
테스트: `detects_private_use_area_codepoints`,
`counts_private_use_chars_including_nested_tables`,
`private_use_chars_are_reported_but_not_altered`

**남은 것**: 신뢰할 수 있는 한컴 PUA ↔ 유니코드 대응표를 확보하면 치환한다.
근거 없이 표를 만들지는 않는다.

### 후순위로 강등하는 항목

이전 인수인계 문서(`02-handover.md` 5절)의 로드맵을 **재조정한다.**

| 항목 | 이전 순위 | 새 순위 | 근거 |
|---|---|---|---|
| 각주/미주 | 3 | 보류 | 코퍼스 5건에 **0건**. "학술·공문서에서 빈도가 높다"는 내 추정이었고 실제 공문서 표본에는 없었다 |
| 목록 번호 매기기 | 4 | 보류 | 0건. 실제 문서는 `1.`, `가.`를 텍스트로 직접 쓴다 |
| 머리말/꼬리말 | 5 | 보류 | 0건 |
| 수식·도형·차트 | - | 보류 | 0건 |

**강등이 아니라 근거 없음 처리다.** 표본이 5건뿐이므로 "필요 없다"가 아니라
"필요하다는 증거가 아직 없다"가 맞다. 해당 요소가 있는 문서를 만나면 그때
올린다. 그때까지 추정으로 순서를 정하지 않는다.

## 4. 작업 순서와 방식

```
1주차  G1 음수 들여쓰기          (P0, 작음)
       G2 셀 다중 문단           (P0, 중간)
       → 코퍼스 5건 회귀 + 렌더 대조

2주차  G4 중첩표 실측 → 결정      (P1, 먼저 실측)
       G3 누름틀 폼필드          (P1, 중간)
       → 코퍼스 5건 회귀 + 렌더 대조

3주차  G5 스타일 매핑            (P2)
       프로세스 정비 (5절)
```

각 항목의 공통 절차:

1. **실측 먼저.** 어휘·값 형식을 문서에서 추측하지 않고 `officecli help`와
   최소 배치 실험으로 확인한다. F1·F2와 앞선 5개 버그가 모두 추측에서 나왔다.
2. **실패하는 테스트를 먼저 쓴다.** 실제 문서에서 관찰한 XML을 픽스처에 반영한다.
   추측한 형식이 아니라 **관찰한 형식**을 쓴다.
3. 구현.
4. `cargo test` + `clippy -D warnings`.
5. `scripts/verify-roundtrip.sh`.
6. **코퍼스 회귀 + 렌더 이미지 육안 확인** (5절).

## 5. 프로세스 정비 (이번 검증에서 얻은 교훈)

### 왜 필요한가

지금까지 나온 버그 12개 중 **7개**가 실제 문서에서만 드러났다. 그리고 그중
**4개**는 계약 테스트와 `plugins lint`가 전부 통과하는 상태에서
**렌더 이미지를 눈으로 보고** 찾았다.

`plugins lint`는 prop **이름**이 스키마에 있는지만 본다. 그 **값**이 의도한
효과를 내는지는 보지 않는다. 그래서 다음이 전부 통과했다:

- `width: "1440"` (twip 의도, EMU로 해석되어 0.0cm)
- `vmerge: "3"` (정수, 실제로는 enum)
- 셀 `text`의 `\n` (통과하지만 줄바꿈 안 됨)
- 모든 글자에 `strike: "true"`

### P-1. 회귀 코퍼스 도입 — **완료**

실제 문서 5건을 회귀 테스트 자산으로 고정한다.

- `tests/corpus/` 에 문서와 **기대 요약**(항목 수, 문단·표·폼필드 개수,
  텍스트 커버리지)을 저장한다.
- `scripts/verify-corpus.sh`가 전 문서를 변환하고 요약을 대조한다.
- **주의**: 원본 문서를 저장소에 넣을지는 배포 형태에 따라 결정해야 한다.
  공개 기관 문서지만 개인정보(성명·주소가 마스킹돼 있으나)가 포함된
  d1은 그대로 커밋하지 않는 편이 안전하다. 익명화한 축약본을 만들거나
  저장소 밖 경로를 환경변수로 받는다.

**결과**: `scripts/verify-corpus.py`. 원본은 **커밋하지 않고**
`HWPX_CORPUS` 환경변수로 경로를 받는다. 기대 요약만
`tests/corpus/expected.json`에 커밋한다.

```sh
HWPX_CORPUS=~/hwpx-corpus scripts/verify-corpus.py --update   # 기준선 생성
HWPX_CORPUS=~/hwpx-corpus scripts/verify-corpus.py            # 회귀 검증
```

요약에 담는 것: exit code, 배치 항목 수, lint 미지 prop, validate 통과 여부,
문단·표·셀·폼필드 개수, OOXML 지표(`w:br`, `w:hanging`, 음수 `w:firstLine`,
`w:vMerge`, `w:tbl`, `w:checkBox`, `FORMTEXT`), 한컴 `PrvText.txt` 대비 누락 문자 수.

좌표·ID처럼 불안정한 값은 넣지 않는다. 기준선과 무관하게 항상 검사하는 것:
exit 0, 최상위 배열 아님, raw 개행 없음, validate 통과, 음수 `w:firstLine` 없음.

현재 기준선: 5개 문서 전부 `validate=True`, `unknown=0`,
`prvtext_missing_chars=0`.

### P-2. 렌더 육안 확인을 절차로 명문화

`docs/02-handover.md`에 이미 추가했다. 계획 단계에서도 각 작업의 검증 항목에
포함한다. 자동화할 수 있는 부분은 자동화한다:

- `officecli view <f>.docx raw`로 OOXML을 직접 확인하는 어서션을 늘린다.
  (`<w:br/>` 개수, `<w:hanging>` 존재 등) — 이게 lint보다 강하다.
- 스크린샷은 자동 비교(픽셀 diff)까지 갈 필요는 없다. 새 기능마다 한 번
  눈으로 보는 것으로 충분하다.

### P-3. "값의 효과"를 검증하는 계층 추가

`scripts/verify-roundtrip.sh`가 이미 하고 있는 것을 확장한다.
prop을 넣고 **OOXML 산출물**을 확인하는 표를 유지한다.

| prop | 기대 OOXML | 현재 검증 |
|---|---|---|
| `align=center` | `<w:jc w:val="center"/>` | O (readback) |
| picture `width=72pt` | `extent Cx` = 914400 | O (readback 2.5cm) |
| cell `text` + `\v` | `<w:br/>` | O (raw 카운트) |
| `vmerge=restart` | `<w:vMerge w:val="restart"/>` | O (readback) |
| `hangingIndent` | `<w:ind w:hanging=...>` | **G1에서 추가** |

## 6. 리스크와 미결 질문

| # | 항목 | 대응 |
|---|---|---|
| R1 | 표본 5건은 여전히 적다. 문서 종류가 구청 공고·양식에 치우쳤다 | 종류를 넓힌다 (보고서, 논문, 표 중심 통계자료). 다만 추정으로 기능을 만들지는 않는다 |
| R2 | RHWP 변환본 4건 / 네이티브 1건. 비율이 역전돼야 한다 | 한컴 직접 저장본을 더 모은다. F1·F2는 네이티브(d1)에도 해당됐으므로 공통 문제였다 |
| R3 | `hp:switch`/`hp:case`/`hp:default` 구조를 우연히 잘 처리하고 있다 | 현재 마지막 값(=`hp:default`)이 이긴다. 의도한 동작이지만 테스트가 없다. G1 작업 시 테스트 추가 |
| R4 | 누름틀 `Command` 문자열 형식 근거가 2개 문서뿐 | 파싱 실패 시 안내문만 생략하고 필드는 만든다. 원문을 버리지 않는다 |
| R5 | 코퍼스 문서의 저장소 포함 여부 | P-1 참고. 익명화 또는 외부 경로 |
| R6 | Windows/Linux 실행 | GitHub Actions run `31572303544`에서 기존 HWPX 경로의 두 네이티브 job 성공. 새 HWP bridge·Windows Job Object·MSRV/discovery 변경의 첫 원격 결과는 대기 중 |

## 7. 하지 않을 것

범위를 지키기 위해 명시한다.

- **HWPX 쓰기.** dump-reader의 계약 밖이다. 확보되면 `format-handler`로
  승격하는 별도 과제다 (`01-protocol-contract.md` ADR-1).
- **바이너리 HWP 파서를 자체 구현하거나 벤더링하는 일.** 후속 결정에서 선택적
  RHWP→HWPX 프로세스 브리지를 구현했다. 남은 매니페스트·설치 discovery는
  `04-hwp-support-plan.md` H1에서 추적한다(정정된 ADR-2, ADR-5).
- **완벽한 시각 재현.** dump-reader의 목적은 AI 에이전트가 읽고 편집할 수 있는
  구조로 옮기는 것이다. 픽셀 단위 재현이 목표가 아니다.
- **추정에 기반한 기능 추가.** 3절 후순위 항목 참고.
