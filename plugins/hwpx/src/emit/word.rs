//! 문서 모델 → OfficeCLI docx 어휘(BatchItem) 매핑.
//!
//! 어휘 근거: `schemas/help/docx/*.json`, `schemas/help/_shared/*.json`.
//! emit 전략 근거: wiki `command-dump.md` (`docs/01-protocol-contract.md` C8).
//!
//! ## 개행 처리 — 틀리기 쉬운 지점
//!
//! `schemas/help/_shared/paragraph.json`의 `text` 설명:
//!
//! > Newline semantics: `\n` is a PARAGRAPH boundary (the text is split into
//! > one paragraph per line...); `\v` is a soft line break within the
//! > paragraph (Shift+Enter). Do not also add a 'run' child with text on the
//! > same paragraph — they will duplicate.
//!
//! 따라서:
//! - HWPX `hp:lineBreak`(문단 내 줄바꿈)는 **`\v`**로 내보낸다. `\n`으로 보내면
//!   문단이 쪼개지고, 그 결과 이후 모든 `/body/p[N]` 인덱스가 어긋난다.
//! - 다중 런 문단은 `text` prop 없이 추가하고 런을 자식으로 붙인다. 둘을 같이
//!   쓰면 내용이 중복된다.

use base64::Engine;

use super::batch::BatchItem;
use crate::owpml::model::{
    Block, Cell, CharStyle, CheckBox, Document, Image, Inline, Paragraph, ParaStyle, Table,
    TextField, VertAlign,
};

/// 문단·런 `text` prop의 줄바꿈 문자 (Shift+Enter). `\n`과 혼동하면 안 된다.
const SOFT_BREAK: char = '\u{000B}';

/// 셀 내용은 셀의 `text` prop이 아니라 **셀 안 첫 문단**에 넣는다.
///
/// 실측으로 확인한 세 가지 (officecli v1.0.143):
///
/// 1. `set <cell> --prop text` 에 `\v`를 주면 거부된다:
///    "text contains XML-illegal control character U+000B at position 8.
///    Allowed control chars: `\t`, `\n`, `\r`."
/// 2. 같은 자리에 `\n`을 주면 통과하지만 **줄바꿈이 되지 않는다.**
///    `<w:t xml:space="preserve">첫줄\n둘째줄</w:t>` 처럼 리터럴 문자로 저장되고,
///    OOXML에서 `<w:t>` 안의 개행은 줄바꿈이 아니라 공백이다. 즉 조용히 유실된다.
/// 3. `set <cell>/p[1] --prop text` 에 `\v`를 주면 **진짜 `<w:br/>`가 생기고**
///    런도 줄 단위로 분리된다. 문단 핸들러에만 `\v` → `<w:br/>` 번역이 있다.
///
/// 그래서 셀 텍스트는 `p[1]` 경로로 보낸다. 갓 만든 셀에는 빈 문단이 하나
/// 있으므로 `p[1]`이 항상 존재하고, `add`가 아니라 `set`이라 빈 줄이 늘지 않는다.
/// `fill`/`colspan`/`vmerge`/`align`은 셀 속성이므로 셀 경로에 그대로 둔다.
const CELL_TEXT_PARAGRAPH: &str = "/p[1]";

/// docx 경로 세그먼트. `schemas/help/docx/*.json`의 `paths.positional`.
const SEG_TABLE: &str = "tbl";
const SEG_ROW: &str = "tr";
const SEG_CELL: &str = "tc";

/// `add`의 `type`. 스키마의 `element` 이름을 쓴다.
const TYPE_PARAGRAPH: &str = "paragraph";
const TYPE_RUN: &str = "run";
const TYPE_TABLE: &str = "table";
const TYPE_PICTURE: &str = "picture";
const TYPE_FORMFIELD: &str = "formfield";

/// `add` 직후 그 요소를 가리키는 경로.
///
/// 절대 인덱스(`/body/p[3]`)를 쓰지 않는 이유: 메인은 "blank `<target>`
/// skeleton"을 만들고 배치를 재생한다(§2.1). 그 스켈레톤에 빈 문단이 하나라도
///들어 있으면 우리가 센 인덱스가 전부 1씩 밀린다. 우리는 스켈레톤 내용을
/// 확인할 수 없다.
///
/// `last()` 술어는 이 의존을 없앤다. wiki `command-query-word.md`에
/// "`p[last()]` selects the last paragraph"로 문서화되어 있고,
/// `command-dump.md`는 네이티브 dump가 같은 이유로 이 기법을 쓴다고 밝힌다:
/// "Subtree emit uses `last()` xpath predicates so the script is safe to
/// replay onto non-blank documents."
const LAST_PARAGRAPH: &str = "/body/p[last()]";

/// 문서 전체를 BatchItem 목록으로 변환한다.
pub fn emit_document(doc: &Document) -> Vec<BatchItem> {
    let mut out = Vec::new();
    try_emit_document(doc, |item| {
        out.push(item);
        Ok::<(), std::convert::Infallible>(())
    })
    .expect("infallible BatchItem collector");
    out
}

/// 문서를 최상위 블록 단위로 변환하여 sink에 전달한다.
///
/// 전체 문서의 BatchItem을 미리 보관하지 않으므로 dump 경로는 첫 블록부터
/// JSONL을 내보낼 수 있다. 기존 `emit_document`는 골든/단위 테스트 호환성을
/// 위해 이 함수 위에 Vec 수집기로 유지한다.
pub fn try_emit_document<E>(
    doc: &Document,
    mut sink: impl FnMut(BatchItem) -> std::result::Result<(), E>,
) -> std::result::Result<usize, E> {
    let mut count = 0usize;
    for block in &doc.blocks {
        let mut items = Vec::new();
        match block {
            Block::Paragraph(p) => emit_paragraph(p, &mut items),
            Block::Table(t) => emit_table(t, "/body", &mut items),
        }
        for item in items {
            sink(item)?;
            count += 1;
        }
    }
    Ok(count)
}

fn emit_paragraph(p: &Paragraph, out: &mut Vec<BatchItem>) {
    let uniform = p.uniform_style();

    // 케이스 1: 서식이 균일하고 별도 자식이 필요 없으면 한 줄로 병합한다.
    if let (Some(style), false) = (uniform, p.needs_child_commands()) {
        let mut item = BatchItem::add("/body", TYPE_PARAGRAPH);
        item = apply_para_props(item, &p.style);
        item = item.prop("text", emit_text(p));
        item = apply_char_props(item, style);
        out.push(item);
        return;
    }

    // 케이스 2: 다중 서식 또는 이미지·체크박스 포함.
    // `text` prop을 주지 않는다 — 런 자식과 중복되기 때문이다.
    let mut para = BatchItem::add("/body", TYPE_PARAGRAPH);
    para = apply_para_props(para, &p.style);
    out.push(para);

    emit_paragraph_children(p, LAST_PARAGRAPH, out);
}

/// 문단 내용을 자식 명령들로 내보낸다.
///
/// `parent`는 대상 문단의 경로다. 본문 문단이면 `/body/p[last()]`,
/// 표 셀이면 `.../tc[N]/p[1]`. 두 경우 모두 `add`가 문단 끝에 덧붙으므로
/// 등장 순서가 그대로 보존된다 (실측 확인).
fn emit_paragraph_children(p: &Paragraph, parent: &str, out: &mut Vec<BatchItem>) {
    // 탭/줄바꿈은 바로 뒤 텍스트 런 앞에 붙인다. 뒤에 텍스트가 없으면
    // 별도 런으로 흘린다.
    let mut pending = String::new();

    /// 밀린 탭/줄바꿈을 런 하나로 흘린다.
    fn flush_pending(pending: &mut String, parent: &str, out: &mut Vec<BatchItem>) {
        if !pending.is_empty() {
            let text = std::mem::take(pending);
            out.push(
                BatchItem::add(parent, TYPE_RUN)
                    .prop("text", normalize_breaks(&text, SOFT_BREAK)),
            );
        }
    }

    for inline in &p.inlines {
        match inline {
            Inline::Tab => pending.push('\t'),
            Inline::LineBreak => pending.push(SOFT_BREAK),
            Inline::Text(r) => {
                let mut text = std::mem::take(&mut pending);
                text.push_str(&r.text);
                let mut item = BatchItem::add(parent, TYPE_RUN)
                    .prop("text", normalize_breaks(&text, SOFT_BREAK));
                item = apply_char_props(item, &r.style);
                out.push(item);
            }
            Inline::Image(img) => {
                flush_pending(&mut pending, parent, out);
                if let Some(item) = image_item(parent, img) {
                    out.push(item);
                }
            }
            Inline::CheckBox(cb) => {
                flush_pending(&mut pending, parent, out);
                out.push(checkbox_item(parent, cb));
            }
            Inline::TextField(tf) => {
                flush_pending(&mut pending, parent, out);
                out.push(text_field_item(parent, tf));
            }
        }
    }

    flush_pending(&mut pending, parent, out);
}

/// `hp:checkBtn` → docx 폼필드 체크박스.
///
/// 문자(`☑`/`☐`)로 바꾸지 않고 폼필드로 내보낸다. 그래야 Word에서 실제로
/// 켜고 끌 수 있다. 어휘 근거(`officecli help docx formfield`):
/// `type` enum에 `checkbox`, `checked` bool, `name` 문자열.
fn checkbox_item(parent: &str, cb: &CheckBox) -> BatchItem {
    let mut item = BatchItem::add(parent, TYPE_FORMFIELD).prop("type", "checkbox");
    // 이름은 안정 주소용이다. 없으면 생략한다 (호스트가 생성).
    if let Some(n) = cb.name.as_ref().filter(|n| is_safe_field_name(n)) {
        item = item.prop("name", n.clone());
    }
    item.flag("checked", cb.checked)
}

/// 누름틀 → docx 텍스트 폼필드.
///
/// HWP 누름틀은 양식의 입력란이다. 텍스트로 바꾸면 채울 수 없게 되므로
/// `type=text` 폼필드로 내보낸다. 어휘 근거(`officecli help docx formfield`):
/// `type` enum에 `text`, `text`(=초기값) 문자열, `name` 문자열.
fn text_field_item(parent: &str, tf: &TextField) -> BatchItem {
    let mut item = BatchItem::add(parent, TYPE_FORMFIELD).prop("type", "text");
    if let Some(n) = tf.name.as_ref().filter(|n| is_safe_field_name(n)) {
        item = item.prop("name", n.clone());
    }
    // 값이 있으면 값, 없으면 안내 문구. 한글이 클릭 전까지 문구를 보여주므로
    // 빈 양식에서는 문구를 넣는 편이 원본에 가깝다.
    if let Some(t) = tf.initial_text() {
        item = item.prop("text", normalize_breaks(&t, SOFT_BREAK));
    }
    item
}

/// 폼필드 이름 제약: 북마크 이름과 같다 (`officecli help docx formfield`).
/// 영숫자와 밑줄만 통과시킨다. 어긋나면 이름을 생략하는 편이 안전하다.
fn is_safe_field_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

/// 문단·런 `text` 값. 줄바꿈은 `\v`, 탭은 `\t`.
fn emit_text(p: &Paragraph) -> String {
    flatten_inlines(p, SOFT_BREAK)
}

/// 인라인들을 하나의 문자열로 편다. 줄바꿈 문자는 대상 표면에 맞게 호출자가 정한다.
fn flatten_inlines(p: &Paragraph, brk: char) -> String {
    let mut out = String::new();
    for inline in &p.inlines {
        match inline {
            Inline::Text(r) => out.push_str(&r.text),
            Inline::Tab => out.push('\t'),
            Inline::LineBreak => out.push(brk),
            // 별도 자식 명령으로 나가므로 텍스트에는 넣지 않는다.
            Inline::Image(_) | Inline::CheckBox(_) | Inline::TextField(_) => {}
        }
    }
    normalize_breaks(&out, brk)
}

/// `text` prop으로 나가는 모든 문자열이 반드시 통과하는 관문.
///
/// 줄바꿈으로 볼 수 있는 모든 형태(`\r\n`, `\r`, `\n`, `\v`)를 대상 표면이
/// 받아들이는 단일 문자로 정규화한다.
///
/// 두 가지 사고를 동시에 막는다:
///
/// 1. **문단에 raw `\n`** — 문단 경계로 해석되어 `add` 한 번에 문단이 여러 개
///    생기고, 이후 `p[last()]`가 엉뚱한 문단을 가리킨다.
/// 2. **셀에 `\v`** — XML 불법 문자로 거부되어 배치 전체가 롤백된다.
///
/// 파서는 문단 레벨 `hp:lineBreak`를 `Inline::LineBreak`로 분리하지만, 실제
/// 한컴 문서는 **`<hp:t>` 안에도 `<hp:lineBreak/>`를 넣는다**(실측). 그 경로는
/// 런 텍스트 문자열 안에 실려 온다. 출처를 하나하나 막는 대신 출구에서 한 번에
/// 정규화한다.
fn normalize_breaks(s: &str, brk: char) -> String {
    let needs_work = s
        .chars()
        .any(|c| matches!(c, '\n' | '\r' | SOFT_BREAK) && c != brk);
    if !needs_work {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // CRLF는 한 번의 줄바꿈으로 센다.
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(brk);
            }
            '\n' | SOFT_BREAK => out.push(brk),
            other => out.push(other),
        }
    }
    out
}

fn apply_para_props(mut item: BatchItem, s: &ParaStyle) -> BatchItem {
    if let Some(a) = s.align {
        item = item.prop("align", a.as_docx());
    }
    // 길이 prop은 twips 정수를 받는다 (`schemas/help/docx/paragraph.json`).
    if let Some(v) = s.indent_left_twip.filter(|v| *v != 0) {
        item = item.prop("indent", v.to_string());
    }
    // `firstLineIndent`와 `hangingIndent`는 상호 배타적이다.
    // 음수 `firstLineIndent`는 `w:ind/@firstLine`에 음수를 쓰는 유효하지 않은
    // OOXML을 만든다 (실측). 내어쓰기는 전용 속성으로 내보낸다.
    if let Some(v) = s.indent_first_twip.filter(|v| *v > 0) {
        item = item.prop("firstLineIndent", v.to_string());
    }
    if let Some(v) = s.indent_hanging_twip.filter(|v| *v > 0) {
        item = item.prop("hangingIndent", v.to_string());
    }
    if let Some(v) = s.space_before_twip.filter(|v| *v != 0) {
        item = item.prop("spaceBefore", v.to_string());
    }
    if let Some(v) = s.space_after_twip.filter(|v| *v != 0) {
        item = item.prop("spaceAfter", v.to_string());
    }
    if let Some(r) = s.line_spacing_ratio.filter(|r| (*r - 1.0).abs() > f64::EPSILON) {
        // `lineSpacing`은 배수 표기(`1.5x`)를 받는다 (`_shared/paragraph.json`).
        item = item.prop("lineSpacing", format!("{}x", trim_float(r)));
    }
    item
}

fn apply_char_props(mut item: BatchItem, s: &CharStyle) -> BatchItem {
    item = item.flag("bold", s.bold);
    item = item.flag("italic", s.italic);
    item = item.flag("underline", s.underline);
    item = item.flag("strike", s.strike);
    if let Some(c) = &s.color {
        item = item.prop("color", c.clone());
    }
    if let Some(c) = &s.highlight {
        item = item.prop("highlight", c.clone());
    }
    if let Some(sz) = s.size_pt {
        // `size`는 `12pt` 같은 단위 표기를 받는다.
        item = item.prop("size", format!("{}pt", trim_float(sz)));
    }
    if let Some(f) = &s.font {
        item = item.prop("font", f.clone());
    }
    match s.vert_align {
        Some(VertAlign::Superscript) => item = item.flag("superscript", true),
        Some(VertAlign::Subscript) => item = item.flag("subscript", true),
        None => {}
    }
    item
}

/// 이미지를 data URI로 인라인한다 (wiki `command-dump.md`: "Pictures |
/// Inlined as data URIs through the `src=` prop").
///
/// BinData에서 바이트를 못 찾았으면 `None`을 돌려 그 이미지를 건너뛴다.
/// 존재하지 않는 경로를 `src`로 내보내면 replay가 실패한다.
fn image_item(para_path: &str, img: &Image) -> Option<BatchItem> {
    let data = img.data.as_ref()?;
    if data.is_empty() {
        return None;
    }
    let ctype = img
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    let b64 = base64::engine::general_purpose::STANDARD.encode(data.as_ref());

    let mut item = BatchItem::add(para_path, TYPE_PICTURE).prop("src", format!("data:{ctype};base64,{b64}"));
    item = item.prop_opt("alt", img.alt.clone());
    // 반드시 단위를 붙인다. 스키마 경고:
    // "Always pass a unit (cm/in/pt) — a bare number is interpreted as raw EMU
    //  (914400 per inch), so width=5 renders an effectively invisible 5-EMU image."
    // 문단/표의 길이 prop과 달리 그림은 bare 숫자가 twip이 아니다.
    if let Some(w) = img.width_twip.filter(|v| *v > 0) {
        item = item.prop("width", twip_to_pt_string(w));
    }
    if let Some(h) = img.height_twip.filter(|v| *v > 0) {
        item = item.prop("height", twip_to_pt_string(h));
    }
    Some(item)
}

/// twip → `"36pt"`. 그림 크기 전용.
///
/// twip은 1/1440 inch, pt는 1/72 inch이므로 pt = twip / 20.
fn twip_to_pt_string(twip: i64) -> String {
    format!("{}pt", trim_float(twip as f64 / 20.0))
}

/// 표를 `parent` 아래에 만든다.
///
/// `parent`는 `/body` 또는 **셀 경로**다. 추가 직후 그 표를 가리키는 경로는
/// 항상 `{parent}/tbl[last()]` 이다.
///
/// 중첩표가 안전한 이유: `/body/tbl[last()]`는 body의 **직속** 자식 표만 고르므로,
/// 셀 안에 표를 추가해도 바깥 표를 가리키는 경로는 변하지 않는다.
/// 실측 확인: `add <cell> --type table` → `<cell>/tbl[1]` 로 접근 가능하고
/// 실제 중첩 `<w:tbl>`이 만들어진다.
fn emit_table(t: &Table, parent: &str, out: &mut Vec<BatchItem>) {
    if t.rows == 0 || t.cols == 0 {
        return;
    }

    // `officecli add <f> /body --type table --prop rows=2 --prop cols=2`
    // (wiki `command-add-word.md`).
    let mut add = BatchItem::add(parent, TYPE_TABLE)
        .prop("rows", t.rows.to_string())
        .prop("cols", t.cols.to_string());

    if t.col_widths_twip.len() == t.cols && t.col_widths_twip.iter().all(|w| *w > 0) {
        let widths: Vec<String> = t.col_widths_twip.iter().map(|w| w.to_string()).collect();
        add = add.prop("colWidths", widths.join(","));
    }
    out.push(add);

    let table_path = format!("{parent}/{SEG_TABLE}[last()]");
    let grid = build_occupancy_grid(t);

    // 행마다 왼쪽에서 오른쪽으로 훑는다.
    //
    // `colspan`은 셀을 실제로 합쳐서 그 행의 `tc` 개수를 줄인다(실측 확인:
    // 3열 행에 colspan=3을 주면 childCount가 3 → 1이 된다). 따라서 격자
    // 열번호를 그대로 `tc[C]`로 쓸 수 없다.
    //
    // 대신 **처리 순서상의 순번**을 쓴다. 왼쪽 그룹들을 이미 각각 하나의 `tc`로
    // 합쳐 놓았으므로, k번째 그룹의 경로는 항상 `tc[k]`다.
    for (r, row) in grid.iter().enumerate() {
        let mut tc_index = 0usize;
        let mut c = 0usize;
        while c < t.cols {
            let owner = row[c];

            // 같은 셀이 덮는 연속 열을 하나의 그룹으로 묶는다.
            // 빈 격자(None)는 묶지 않는다 — docx에서는 각각 별개 `tc`다.
            let mut width = 1usize;
            if owner.is_some() {
                while c + width < t.cols && row[c + width] == owner {
                    width += 1;
                }
            }

            tc_index += 1;

            if let Some(i) = owner {
                let cell = &t.cells[i];
                let path = format!("{table_path}/{SEG_ROW}[{}]/{SEG_CELL}[{tc_index}]", r + 1);
                let is_origin = cell.row == r;

                // 셀 속성 (배경색·병합·정렬)
                if let Some(item) = cell_props_item(&path, cell, is_origin, width) {
                    out.push(item);
                }
                // 셀 내용은 셀 안 첫 문단으로 (CELL_TEXT_PARAGRAPH 주석 참고).
                // 세로 병합의 이음 칸에는 내용을 넣지 않는다.
                if is_origin {
                    emit_cell_content(&path, cell, out);
                }
            }

            c += width;
        }
    }
}

/// 격자의 각 칸을 덮는 셀의 인덱스를 채운다.
///
/// HWPX는 병합에 가려진 칸에 `hp:tc`를 만들지 않는다. docx는 `rows`/`cols`로
/// 만들면 모든 칸이 존재한다. 이 격자가 둘 사이를 잇는다.
fn build_occupancy_grid(t: &Table) -> Vec<Vec<Option<usize>>> {
    let mut grid = vec![vec![None; t.cols]; t.rows];
    for (i, cell) in t.cells.iter().enumerate() {
        // 격자 밖을 가리키는 셀은 버린다. 손상된 rowAddr/colAddr 방어.
        if cell.row >= t.rows || cell.col >= t.cols {
            continue;
        }
        let row_end = cell.row.saturating_add(cell.row_span.max(1)).min(t.rows);
        let col_end = cell.col.saturating_add(cell.col_span.max(1)).min(t.cols);

        for row in &mut grid[cell.row..row_end] {
            for slot in &mut row[cell.col..col_end] {
                // 먼저 온 셀이 이긴다. 잘못된 span이 겹쳐도 덮어쓰지 않는다.
                if slot.is_none() {
                    *slot = Some(i);
                }
            }
        }
    }
    grid
}

/// 셀 속성만 `set`으로 채운다. 내용은 `cell_text`가 별도 명령으로 나간다.
///
/// - `is_origin`: 이 행이 세로 병합의 첫 행인지.
/// - `group_width`: 이 행에서 이 셀이 덮는 열 수. `colspan`으로 나간다.
fn cell_props_item(
    path: &str,
    cell: &Cell,
    is_origin: bool,
    group_width: usize,
) -> Option<BatchItem> {
    let mut item = BatchItem::set(path);

    if is_origin {
        if let Some(f) = &cell.fill {
            item = item.prop("fill", f.clone());
        }
        // 첫 문단의 정렬을 셀 정렬로 올린다.
        if let Some(a) = cell.paragraphs().next().and_then(|p| p.style.align) {
            item = item.prop("align", a.as_docx());
        }
    }

    // 세로 병합. `vmerge`는 정수가 아니라 enum이다 (실측 확인):
    //   "'restart' marks the top cell of a vertical span; 'continue' marks
    //    subsequent merged cells in the same column."
    if cell.row_span.max(1) > 1 {
        item = item.prop("vmerge", if is_origin { "restart" } else { "continue" });
    }

    // 가로 병합. 이 행에서 실제로 덮는 열 수를 쓴다.
    if group_width > 1 {
        item = item.prop("colspan", group_width.to_string());
    }

    if item.has_props() {
        Some(item)
    } else {
        None
    }
}

/// 셀 내용을 채운다. HWPX 셀의 `subList` 문단들을 docx 셀 문단들로 옮긴다.
///
/// 갓 만든 셀에는 빈 문단이 하나 있으므로 **첫 문단은 `set`**으로 그 자리를
/// 채우고, 두 번째부터 `add`로 문단을 늘린다. 전부 `add`로 하면 맨 위에 빈 줄이
/// 생긴다.
///
/// 이전에는 모든 문단을 `\v`로 이어 하나로 만들었다. 텍스트는 보존되지만
/// 문단별 정렬·여백이 전부 소실됐다. 실측(코퍼스 5건): 셀 151개 중 25개가
/// 문단 2개 이상이고 최댓값은 14개였다. 공시송달공고문은 본문 전체가 표 1칸
/// 안에 있어 31개 문단이 1개로 뭉개졌다.
///
/// 경로 근거(실측): `set <cell>/p[1]`, `add <cell> --type paragraph`,
/// `add <cell>/p[last()] --type run` 이 모두 동작하고 문단별 `align`이 보존된다.
fn emit_cell_content(cell_path: &str, cell: &Cell, out: &mut Vec<BatchItem>) {
    let first_para = format!("{cell_path}{CELL_TEXT_PARAGRAPH}");
    // docx 문단 경로 세그먼트는 `p` (schemas/help/docx/paragraph.json).
    let last_para = format!("{cell_path}/p[last()]");

    // 셀의 기존 빈 문단을 아직 쓰지 않았는지. 첫 문단만 `set`으로 그 자리를 쓴다.
    let mut seed_available = true;

    for block in &cell.blocks {
        match block {
            Block::Table(nested) => {
                // 중첩표는 셀 아래에 그대로 만든다. 경로는 `<cell>/tbl[last()]`.
                emit_table(nested, cell_path, out);
            }
            Block::Paragraph(p) => {
                let uniform = p.uniform_style();
                // 한 줄로 병합할 수 있는지. 인라인이 텍스트뿐이고 서식이 균일해야 한다.
                let collapsible = uniform.is_some() && !p.needs_child_commands();

                let first = seed_available;
                let mut seed = if first {
                    seed_available = false;
                    BatchItem::set(&first_para)
                } else {
                    BatchItem::add(cell_path, TYPE_PARAGRAPH)
                };
                seed = apply_para_props(seed, &p.style);

                if collapsible {
                    seed = seed.prop("text", flatten_inlines(p, SOFT_BREAK));
                    if let Some(style) = uniform {
                        seed = apply_char_props(seed, style);
                    }
                }

                // `add`는 빈 문단이라도 내보내야 문단 수가 맞는다.
                // `set`은 넣을 것이 없으면 생략한다(그 문단은 이미 존재한다).
                if !first || seed.has_props() {
                    out.push(seed);
                }

                if !collapsible {
                    let target = if first { &first_para } else { &last_para };
                    emit_paragraph_children(p, target, out);
                }
            }
        }
    }
}


/// `10` → `"10"`, `10.5` → `"10.5"`. 불필요한 `.0`을 없앤다.
fn trim_float(v: f64) -> String {
    if (v.fract()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owpml::model::TextRun;

    fn text_run(t: &str, style: CharStyle) -> Inline {
        Inline::Text(TextRun {
            text: t.into(),
            style,
        })
    }

    fn bold() -> CharStyle {
        CharStyle {
            bold: true,
            ..Default::default()
        }
    }

    #[test]
    fn trims_float_representation() {
        assert_eq!(trim_float(10.0), "10");
        assert_eq!(trim_float(10.5), "10.5");
        assert_eq!(trim_float(1.6), "1.6");
    }

    #[test]
    fn uniform_paragraph_collapses_to_single_add() {
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![text_run("안녕하세요", CharStyle::default())],
            })],
        };
        let items = emit_document(&doc);
        assert_eq!(items.len(), 1, "single-run paragraph must be one item");
        assert_eq!(items[0].command, "add");
        assert_eq!(items[0].r#type, Some(TYPE_PARAGRAPH));
        assert_eq!(items[0].props["text"], "안녕하세요");
    }

    #[test]
    fn mixed_paragraph_splits_into_paragraph_plus_runs() {
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![
                    text_run("보통 ", CharStyle::default()),
                    text_run("굵게", bold()),
                ],
            })],
        };
        let items = emit_document(&doc);
        assert_eq!(items.len(), 3);
        // 문단에는 text가 없어야 한다 (런과 중복되므로).
        assert!(
            !items[0].props.contains_key("text"),
            "multi-run paragraph must not carry text prop"
        );
        assert_eq!(items[1].parent.as_deref(), Some("/body/p[last()]"));
        assert_eq!(items[1].props["text"], "보통 ");
        assert_eq!(items[2].props["text"], "굵게");
        assert_eq!(items[2].props["bold"], "true");
    }

    #[test]
    fn line_break_becomes_vertical_tab_not_newline() {
        // 핵심 회귀 테스트: \n은 문단 분리이므로 절대 나오면 안 된다.
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![
                    text_run("첫줄", CharStyle::default()),
                    Inline::LineBreak,
                    text_run("둘째줄", CharStyle::default()),
                ],
            })],
        };
        let items = emit_document(&doc);
        let text = items[0].props["text"].as_str().expect("text");
        assert_eq!(text, "첫줄\u{000B}둘째줄");
        assert!(!text.contains('\n'), "raw \\n would split the paragraph");
    }

    #[test]
    fn children_attach_via_last_predicate_not_absolute_index() {
        let doc = Document {
            blocks: vec![
                Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![
                        text_run("가", CharStyle::default()),
                        text_run("나", bold()),
                    ],
                }),
                Block::Table(Table {
                    rows: 1,
                    cols: 1,
                    col_widths_twip: vec![],
                    cells: vec![Cell {
                        row: 0,
                        col: 0,
                        row_span: 1,
                        col_span: 1,
                        width_twip: None,
                        fill: None,
                        blocks: vec![Block::Paragraph(Paragraph {
                            style: ParaStyle::default(),
                            inlines: vec![text_run("셀", CharStyle::default())],
                        })],
                    }],
                }),
                Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![
                        text_run("다", CharStyle::default()),
                        text_run("라", bold()),
                    ],
                }),
            ],
        };
        let items = emit_document(&doc);
        let run_parents: Vec<&str> = items
            .iter()
            .filter(|i| i.r#type == Some(TYPE_RUN))
            .map(|i| i.parent.as_deref().expect("parent"))
            .collect();
        // last()를 쓰므로 표가 중간에 끼어도, 스켈레톤에 기존 문단이 있어도
        // 런이 항상 방금 추가한 문단에 붙는다.
        assert_eq!(
            run_parents,
            vec![
                "/body/p[last()]",
                "/body/p[last()]",
                "/body/p[last()]",
                "/body/p[last()]"
            ]
        );
        let cell_paths: Vec<&str> = items
            .iter()
            .filter(|i| i.command == "set")
            .map(|i| i.path.as_deref().expect("path"))
            .collect();
        assert_eq!(
            cell_paths,
            vec!["/body/tbl[last()]/tr[1]/tc[1]/p[1]"],
            "cell text goes to the cell's first paragraph"
        );
    }

    #[test]
    fn table_emits_rows_cols_then_cell_sets() {
        let t = Table {
            rows: 2,
            cols: 2,
            col_widths_twip: vec![800, 1200],
            cells: vec![
                Cell {
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 2,
                    width_twip: Some(800),
                    fill: Some("#EEEEEE".into()),
                    blocks: vec![Block::Paragraph(Paragraph {
                        style: ParaStyle::default(),
                        inlines: vec![text_run("머리", CharStyle::default())],
                    })],
                },
                Cell {
                    row: 1,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    width_twip: Some(800),
                    fill: None,
                    blocks: vec![Block::Paragraph(Paragraph {
                        style: ParaStyle::default(),
                        inlines: vec![text_run("값", CharStyle::default())],
                    })],
                },
            ],
        };
        let doc = Document {
            blocks: vec![Block::Table(t.clone())],
        };
        let items = emit_document(&doc);

        assert_eq!(items[0].command, "add");
        assert_eq!(items[0].r#type, Some(TYPE_TABLE));
        assert_eq!(items[0].props["rows"], "2");
        assert_eq!(items[0].props["cols"], "2");
        assert_eq!(items[0].props["colWidths"], "800,1200");

        let (order, cells) = table_cells(t);
        assert_eq!(
            order,
            vec![
                "/body/tbl[last()]/tr[1]/tc[1]",
                "/body/tbl[last()]/tr[2]/tc[1]"
            ]
        );

        let head = &cells["/body/tbl[last()]/tr[1]/tc[1]"];
        assert_eq!(head["text"], "머리");
        assert_eq!(head["fill"], "#EEEEEE");
        assert_eq!(head["colspan"], "2");

        let body = &cells["/body/tbl[last()]/tr[2]/tc[1]"];
        assert_eq!(body["text"], "값");
        assert!(!body.contains_key("colspan"));
    }

    #[test]
    fn empty_table_emits_nothing() {
        let doc = Document {
            blocks: vec![Block::Table(Table::default())],
        };
        assert!(emit_document(&doc).is_empty());
    }

    /// 표 하나를 감싸 emit하고 `set` 항목만 (경로, props) 형태로 뽑는다.
    fn table_sets(t: Table) -> Vec<(String, serde_json::Map<String, serde_json::Value>)> {
        let doc = Document {
            blocks: vec![Block::Table(t)],
        };
        emit_document(&doc)
            .into_iter()
            .filter(|i| i.command == "set")
            .map(|i| (i.path.clone().expect("path"), i.props.clone()))
            .collect()
    }

    /// 셀 단위로 본 결과.
    ///
    /// 실제 emit은 셀 속성(`<cell>`)과 텍스트(`<cell>/p[1]`)를 두 명령으로
    /// 내보내지만, 테스트에서는 셀 하나로 합쳐 보는 편이 읽기 쉽다.
    /// 반환값은 (emit 순서의 셀 경로 목록, 경로별 병합 props).
    #[allow(clippy::type_complexity)]
    fn table_cells(
        t: Table,
    ) -> (
        Vec<String>,
        std::collections::HashMap<String, serde_json::Map<String, serde_json::Value>>,
    ) {
        let mut order: Vec<String> = Vec::new();
        let mut merged: std::collections::HashMap<
            String,
            serde_json::Map<String, serde_json::Value>,
        > = std::collections::HashMap::new();

        for (path, props) in table_sets(t) {
            let cell_path = path
                .strip_suffix(CELL_TEXT_PARAGRAPH)
                .unwrap_or(&path)
                .to_string();
            if !order.contains(&cell_path) {
                order.push(cell_path.clone());
            }
            let entry = merged.entry(cell_path).or_default();
            for (k, v) in props {
                entry.insert(k, v);
            }
        }
        (order, merged)
    }

    fn plain_cell(row: usize, col: usize, text: &str, rspan: usize, cspan: usize) -> Cell {
        Cell {
            row,
            col,
            row_span: rspan,
            col_span: cspan,
            width_twip: None,
            fill: None,
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![text_run(text, CharStyle::default())],
            })],
        }
    }

    #[test]
    fn horizontal_merge_mid_row_shifts_following_cell_index() {
        // 회귀 테스트. colspan은 셀을 실제로 합쳐 그 행의 tc 개수를 줄인다.
        // 격자 열번호를 그대로 쓰면 병합 뒤 셀이 존재하지 않는 인덱스를 가리킨다.
        //
        // 3열 표, 첫 두 열이 병합된 행:  [ A(2열) ][ B ]
        // A는 tc[1], B는 격자상 열 2지만 docx에서는 tc[2]다.
        let t = Table {
            rows: 1,
            cols: 3,
            col_widths_twip: vec![],
            cells: vec![
                plain_cell(0, 0, "A", 1, 2),
                plain_cell(0, 2, "B", 1, 1),
            ],
        };
        let (order, cells) = table_cells(t);
        assert_eq!(
            order,
            vec![
                "/body/tbl[last()]/tr[1]/tc[1]",
                "/body/tbl[last()]/tr[1]/tc[2]"
            ],
            "cell after a merge must use the collapsed index, not the grid column"
        );
        let a = &cells["/body/tbl[last()]/tr[1]/tc[1]"];
        assert_eq!(a["colspan"], "2");
        assert_eq!(a["text"], "A");
        let b = &cells["/body/tbl[last()]/tr[1]/tc[2]"];
        assert_eq!(b["text"], "B");
        assert!(!b.contains_key("colspan"));
    }

    #[test]
    fn vertical_merge_uses_restart_and_continue_enum() {
        // 회귀 테스트. vmerge는 정수가 아니라 enum이다:
        //   restart = 세로 병합의 첫 칸, continue = 이어지는 칸
        // rowSpan 정수를 보내면 값 검증에서 거부된다.
        let t = Table {
            rows: 2,
            cols: 2,
            col_widths_twip: vec![],
            cells: vec![
                plain_cell(0, 0, "세로병합", 2, 1),
                plain_cell(0, 1, "우상", 1, 1),
                plain_cell(1, 1, "우하", 1, 1),
            ],
        };
        let (_, by_path) = table_cells(t);

        let top = &by_path["/body/tbl[last()]/tr[1]/tc[1]"];
        assert_eq!(top["vmerge"], "restart");
        assert_eq!(top["text"], "세로병합");

        // 아래 칸은 HWPX에 tc가 없지만 docx 격자에는 있다. continue를 채워야 한다.
        let cont = &by_path["/body/tbl[last()]/tr[2]/tc[1]"];
        assert_eq!(cont["vmerge"], "continue");
        assert!(
            !cont.contains_key("text"),
            "continuation cell must not repeat the text"
        );

        // 병합과 무관한 오른쪽 칸들은 그대로.
        assert_eq!(by_path["/body/tbl[last()]/tr[1]/tc[2]"]["text"], "우상");
        assert_eq!(by_path["/body/tbl[last()]/tr[2]/tc[2]"]["text"], "우하");
    }

    #[test]
    fn combined_merge_keeps_colspan_on_continuation_rows() {
        // 가로+세로 동시 병합. 이어지는 행도 같은 폭을 유지해야 격자가 어긋나지 않는다.
        let t = Table {
            rows: 2,
            cols: 3,
            col_widths_twip: vec![],
            cells: vec![
                plain_cell(0, 0, "덩어리", 2, 2),
                plain_cell(0, 2, "우상", 1, 1),
                plain_cell(1, 2, "우하", 1, 1),
            ],
        };
        let (_, by_path) = table_cells(t);

        let top = &by_path["/body/tbl[last()]/tr[1]/tc[1]"];
        assert_eq!(top["colspan"], "2");
        assert_eq!(top["vmerge"], "restart");

        let cont = &by_path["/body/tbl[last()]/tr[2]/tc[1]"];
        assert_eq!(cont["colspan"], "2");
        assert_eq!(cont["vmerge"], "continue");

        // 오른쪽 열은 두 행 모두 tc[2]다.
        assert_eq!(by_path["/body/tbl[last()]/tr[1]/tc[2]"]["text"], "우상");
        assert_eq!(by_path["/body/tbl[last()]/tr[2]/tc[2]"]["text"], "우하");
    }

    #[test]
    fn holes_in_the_grid_still_consume_a_cell_slot() {
        // 격자에 빈 칸이 있으면(HWPX가 tc를 안 만든 경우) docx에는 여전히 tc가
        // 있으므로 인덱스를 소비해야 한다.
        let t = Table {
            rows: 1,
            cols: 3,
            col_widths_twip: vec![],
            cells: vec![
                plain_cell(0, 0, "첫", 1, 1),
                // 열 1은 비어 있음
                plain_cell(0, 2, "셋", 1, 1),
            ],
        };
        let (order, _) = table_cells(t);
        assert_eq!(
            order,
            vec![
                "/body/tbl[last()]/tr[1]/tc[1]",
                "/body/tbl[last()]/tr[1]/tc[3]"
            ],
            "the empty grid column must still occupy tc[2]"
        );
    }

    #[test]
    fn checkbox_becomes_interactive_formfield_not_a_character() {
        // 실측(2026 대구문학관 참가신청서): 양식 문서는 체크박스를 `☑` 문자가
        // 아니라 `hp:checkBtn` 폼 컨트롤로 넣는다. 문자로 바꾸면 Word에서
        // 켜고 끌 수 없고, 체크 안 된 상자는 아예 사라진다.
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![
                    text_run("264자 시  ", CharStyle::default()),
                    Inline::CheckBox(CheckBox {
                        name: Some("CheckBox2".into()),
                        checked: false,
                    }),
                    Inline::LineBreak,
                    text_run("815자 에세이 ", CharStyle::default()),
                    Inline::CheckBox(CheckBox {
                        name: Some("CheckBox3".into()),
                        checked: true,
                    }),
                ],
            })],
        };
        let items = emit_document(&doc);

        // 문단은 text prop 없이 나가고 내용은 자식으로 붙는다.
        assert_eq!(items[0].r#type, Some(TYPE_PARAGRAPH));
        assert!(!items[0].props.contains_key("text"));

        let fields: Vec<&BatchItem> = items
            .iter()
            .filter(|i| i.r#type == Some(TYPE_FORMFIELD))
            .collect();
        assert_eq!(fields.len(), 2, "both checkboxes must survive");

        assert_eq!(fields[0].props["type"], "checkbox");
        assert_eq!(fields[0].props["name"], "CheckBox2");
        assert!(
            !fields[0].props.contains_key("checked"),
            "unchecked must omit the flag (false is the default)"
        );

        assert_eq!(fields[1].props["name"], "CheckBox3");
        assert_eq!(fields[1].props["checked"], "true");

        // 순서 보존: 텍스트 → 체크박스 → 줄바꿈+텍스트 → 체크박스
        let kinds: Vec<&str> = items
            .iter()
            .skip(1)
            .map(|i| i.r#type.unwrap_or("?"))
            .collect();
        assert_eq!(kinds, vec![TYPE_RUN, TYPE_FORMFIELD, TYPE_RUN, TYPE_FORMFIELD]);
    }

    #[test]
    fn checkbox_inside_a_cell_targets_the_cell_paragraph() {
        let t = Table {
            rows: 1,
            cols: 1,
            col_widths_twip: vec![],
            cells: vec![Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width_twip: None,
                fill: None,
                blocks: vec![Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![
                        text_run("동의 ", CharStyle::default()),
                        Inline::CheckBox(CheckBox {
                            name: None,
                            checked: true,
                        }),
                    ],
                })],
            }],
        };
        let doc = Document {
            blocks: vec![Block::Table(t)],
        };
        let items = emit_document(&doc);
        let field = items
            .iter()
            .find(|i| i.r#type == Some(TYPE_FORMFIELD))
            .expect("formfield");
        assert_eq!(
            field.parent.as_deref(),
            Some("/body/tbl[last()]/tr[1]/tc[1]/p[1]"),
            "checkbox must attach to the cell's paragraph"
        );
        assert_eq!(field.props["checked"], "true");
        assert!(
            !field.props.contains_key("name"),
            "missing name must be omitted, not empty"
        );
    }

    #[test]
    fn click_here_field_becomes_a_fillable_text_formfield() {
        // 누름틀은 양식 입력란이다. 텍스트로 바꾸면 채울 수 없다.
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![
                    text_run("접수번호 ", CharStyle::default()),
                    Inline::TextField(TextField {
                        name: None,
                        hint: Some("기재하지 마세요.".into()),
                    }),
                ],
            })],
        };
        let items = emit_document(&doc);
        let field = items
            .iter()
            .find(|i| i.r#type == Some(TYPE_FORMFIELD))
            .expect("formfield");
        assert_eq!(field.props["type"], "text");
        // 한글은 클릭 전까지 안내 문구를 보여주므로 초기값으로 넣는다.
        assert_eq!(field.props["text"], "기재하지 마세요.");
        assert!(!field.props.contains_key("name"), "빈 이름은 생략");

        // 순서: 텍스트 런 → 폼필드
        let kinds: Vec<&str> = items.iter().skip(1).map(|i| i.r#type.unwrap_or("?")).collect();
        assert_eq!(kinds, vec![TYPE_RUN, TYPE_FORMFIELD]);
    }

    #[test]
    fn text_field_without_hint_still_emits_a_field() {
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![Inline::TextField(TextField {
                    name: Some("Field1".into()),
                    hint: None,
                })],
            })],
        };
        let field = emit_document(&doc)
            .into_iter()
            .find(|i| i.r#type == Some(TYPE_FORMFIELD))
            .expect("formfield");
        assert_eq!(field.props["type"], "text");
        assert_eq!(field.props["name"], "Field1");
        assert!(!field.props.contains_key("text"));
    }

    #[test]
    fn rejects_unsafe_form_field_names() {
        // 폼필드 이름은 북마크 이름과 같은 제약을 받는다.
        assert!(is_safe_field_name("CheckBox2"));
        assert!(is_safe_field_name("_field_1"));
        assert!(!is_safe_field_name(""));
        assert!(!is_safe_field_name("2starts_with_digit"));
        assert!(!is_safe_field_name("has space"));
        assert!(!is_safe_field_name("한글이름"));
        assert!(!is_safe_field_name(&"x".repeat(41)));
    }

    #[test]
    fn many_paragraph_cell_preserves_count_and_per_paragraph_align() {
        // 실측: 공시송달공고문은 본문 전체가 표 1칸 안에 있고 문단이 14개였다.
        // 예전 구현은 이것을 문단 1개로 뭉갰다.
        let aligns = [
            Some(crate::owpml::model::Align::Center),
            Some(crate::owpml::model::Align::Right),
            None,
            Some(crate::owpml::model::Align::Justify),
        ];
        let blocks: Vec<Block> = aligns
            .iter()
            .enumerate()
            .map(|(i, a)| {
                Block::Paragraph(Paragraph {
                    style: ParaStyle {
                        align: *a,
                        ..Default::default()
                    },
                    inlines: vec![text_run(&format!("문단{}", i + 1), CharStyle::default())],
                })
            })
            .collect();

        let t = Table {
            rows: 1,
            cols: 1,
            col_widths_twip: vec![],
            cells: vec![Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width_twip: None,
                fill: None,
                blocks,
            }],
        };
        let doc = Document {
            blocks: vec![Block::Table(t)],
        };
        let items = emit_document(&doc);

        // 셀 문단 명령만 골라낸다 (셀 속성 set 은 제외).
        let cell_paras: Vec<&BatchItem> = items
            .iter()
            .filter(|i| {
                (i.command == "set" && i.path.as_deref() == Some("/body/tbl[last()]/tr[1]/tc[1]/p[1]"))
                    || (i.command == "add"
                        && i.r#type == Some(TYPE_PARAGRAPH)
                        && i.parent.as_deref() == Some("/body/tbl[last()]/tr[1]/tc[1]"))
            })
            .collect();

        assert_eq!(cell_paras.len(), 4, "문단 수가 보존돼야 한다");
        // 첫 문단만 set, 나머지는 add (빈 문단이 앞에 생기지 않게)
        assert_eq!(cell_paras[0].command, "set");
        assert!(cell_paras[1..].iter().all(|i| i.command == "add"));

        let texts: Vec<&str> = cell_paras
            .iter()
            .map(|i| i.props["text"].as_str().expect("text"))
            .collect();
        assert_eq!(texts, vec!["문단1", "문단2", "문단3", "문단4"]);

        // 문단별 정렬이 각각 보존돼야 한다.
        assert_eq!(cell_paras[0].props["align"], "center");
        assert_eq!(cell_paras[1].props["align"], "right");
        assert!(!cell_paras[2].props.contains_key("align"));
        assert_eq!(cell_paras[3].props["align"], "both");
    }

    #[test]
    fn mixed_style_cell_paragraph_emits_runs_to_the_right_paragraph() {
        // 두 번째 문단이 다중 서식이면 런은 `p[last()]`(방금 추가한 문단)에
        // 붙어야 한다. `p[1]`에 붙으면 첫 문단이 오염된다.
        let t = Table {
            rows: 1,
            cols: 1,
            col_widths_twip: vec![],
            cells: vec![Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width_twip: None,
                fill: None,
                blocks: vec![
                    Block::Paragraph(Paragraph {
                        style: ParaStyle::default(),
                        inlines: vec![text_run("첫문단", CharStyle::default())],
                    }),
                    Block::Paragraph(Paragraph {
                        style: ParaStyle::default(),
                        inlines: vec![
                            text_run("보통", CharStyle::default()),
                            text_run("굵게", bold()),
                        ],
                    })],
            }],
        };
        let doc = Document {
            blocks: vec![Block::Table(t)],
        };
        let items = emit_document(&doc);
        let run_parents: Vec<&str> = items
            .iter()
            .filter(|i| i.r#type == Some(TYPE_RUN))
            .map(|i| i.parent.as_deref().expect("parent"))
            .collect();
        assert_eq!(
            run_parents,
            vec![
                "/body/tbl[last()]/tr[1]/tc[1]/p[last()]",
                "/body/tbl[last()]/tr[1]/tc[1]/p[last()]"
            ]
        );
    }

    #[test]
    fn nested_table_becomes_a_real_nested_table() {
        // 예전에는 중첩표를 탭 구분 텍스트로 평탄화했다. 실측으로
        // `add <cell> --type table` 이 동작하고 경로가 `<cell>/tbl[last()]` 인
        // 것을 확인했으므로 구조를 그대로 옮긴다.
        let inner = Table {
            rows: 1,
            cols: 2,
            col_widths_twip: vec![],
            cells: vec![plain_cell(0, 0, "내부A", 1, 1), plain_cell(0, 1, "내부B", 1, 1)],
        };
        let outer = Table {
            rows: 1,
            cols: 1,
            col_widths_twip: vec![],
            cells: vec![Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width_twip: None,
                fill: None,
                blocks: vec![
                    Block::Paragraph(Paragraph {
                        style: ParaStyle::default(),
                        inlines: vec![text_run("표 앞 문단", CharStyle::default())],
                    }),
                    Block::Table(inner),
                ],
            }],
        };
        let doc = Document {
            blocks: vec![Block::Table(outer)],
        };
        let items = emit_document(&doc);

        let tables: Vec<&BatchItem> = items
            .iter()
            .filter(|i| i.r#type == Some(TYPE_TABLE))
            .collect();
        assert_eq!(tables.len(), 2, "outer + nested");
        assert_eq!(tables[0].parent.as_deref(), Some("/body"));
        assert_eq!(
            tables[1].parent.as_deref(),
            Some("/body/tbl[last()]/tr[1]/tc[1]"),
            "nested table must be added under the cell"
        );

        // 중첩표 셀 경로는 셀 아래 tbl[last()] 기준이어야 한다.
        let inner_cells: Vec<&str> = items
            .iter()
            .filter_map(|i| i.path.as_deref())
            .filter(|p| p.contains("/tc[1]/tbl[last()]/"))
            .collect();
        assert!(
            inner_cells.contains(&"/body/tbl[last()]/tr[1]/tc[1]/tbl[last()]/tr[1]/tc[1]/p[1]"),
            "got {inner_cells:?}"
        );

        // 바깥 셀의 문단은 그대로 남아야 한다.
        let outer_text = items
            .iter()
            .find(|i| i.path.as_deref() == Some("/body/tbl[last()]/tr[1]/tc[1]/p[1]"))
            .expect("outer cell paragraph");
        assert_eq!(outer_text.props["text"], "표 앞 문단");
    }

    #[test]
    fn distribute_align_maps_to_its_own_value() {
        // `officecli help docx paragraph`: values include `distribute`
        assert_eq!(
            crate::owpml::model::Align::Distribute.as_docx(),
            "distribute"
        );
    }

    #[test]
    fn normalize_maps_every_newline_form_to_the_target_break() {
        // 회귀 테스트. 실제 한컴 문서는 `<hp:t>` 안에도 `<hp:lineBreak/>`를 넣어서
        // 런 텍스트 문자열 자체에 `\n`이 실려 온다.
        assert_eq!(normalize_breaks("가\n나", SOFT_BREAK), "가\u{000B}나");
        assert_eq!(normalize_breaks("가\r\n나", SOFT_BREAK), "가\u{000B}나");
        assert_eq!(normalize_breaks("가\r나", SOFT_BREAK), "가\u{000B}나");
        // CRLF는 두 번이 아니라 한 번이다.
        assert_eq!(normalize_breaks("가\r\n나", SOFT_BREAK).chars().count(), 3);
        // 탭은 건드리지 않는다.
        assert_eq!(
            normalize_breaks("가\t나\u{000B}다", SOFT_BREAK),
            "가\t나\u{000B}다"
        );
        assert_eq!(normalize_breaks("변경 없음", SOFT_BREAK), "변경 없음");
    }

    #[test]
    fn cell_text_goes_to_the_cell_paragraph_with_soft_breaks() {
        // 실측 회귀 테스트.
        //   `set <cell> --prop text` + `\v`  → XML 불법 문자로 거부
        //   `set <cell> --prop text` + `\n`  → 통과하지만 줄바꿈이 안 됨 (조용한 유실)
        //   `set <cell>/p[1] --prop text` + `\v` → 진짜 <w:br/>
        let t = Table {
            rows: 1,
            cols: 1,
            col_widths_twip: vec![],
            cells: vec![Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width_twip: None,
                fill: Some("#EEEEEE".into()),
                blocks: vec![
                    Block::Paragraph(Paragraph {
                        style: ParaStyle::default(),
                        inlines: vec![text_run("첫", CharStyle::default())],
                    }),
                    Block::Paragraph(Paragraph {
                        style: ParaStyle::default(),
                        inlines: vec![text_run("둘", CharStyle::default())],
                    })],
            }],
        };
        let sets = table_sets(t);

        // 셀 경로에는 속성만, text는 없어야 한다.
        let (props_path, props) = &sets[0];
        assert_eq!(props_path, "/body/tbl[last()]/tr[1]/tc[1]");
        assert_eq!(props["fill"], "#EEEEEE");
        assert!(
            !props.contains_key("text"),
            "cell path must not carry text (\\v rejected, \\n silently lost)"
        );

        // 문단 경로에 첫 문단 text가 나가야 한다.
        let (text_path, text_props) = &sets[1];
        assert_eq!(text_path, "/body/tbl[last()]/tr[1]/tc[1]/p[1]");
        assert_eq!(text_props["text"], "첫");
    }

    /// 줄바꿈 불변식을 검사한다.
    ///
    /// 모든 `text`는 문단·런 경로로 나가므로 `\v`가 유효하고, raw `\n`은 금지다
    /// (문단 경계로 해석되어 문단이 쪼개진다). 셀 속성 경로에는 text가 아예 없다.
    fn assert_break_invariants(items: &[BatchItem]) {
        for item in items {
            // 셀 속성 경로(`.../tc[N]`)는 text를 갖지 않아야 한다.
            if let Some(path) = &item.path {
                if path.contains("/tc[") && !path.ends_with(CELL_TEXT_PARAGRAPH) {
                    assert!(
                        !item.props.contains_key("text"),
                        "cell property path must not carry text: {path}"
                    );
                }
            }
            for (k, v) in &item.props {
                let Some(s) = v.as_str() else { continue };
                assert!(
                    !s.contains('\n'),
                    "prop {k} leaked raw newline (splits paragraph): {s:?}"
                );
                assert!(!s.contains('\r'), "prop {k} leaked CR: {s:?}");
            }
        }
    }

    #[test]
    fn no_emitted_prop_violates_break_invariants() {
        let doc = Document {
            blocks: vec![
                Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![
                        text_run("가", CharStyle::default()),
                        Inline::LineBreak,
                        text_run("나", bold()),
                    ],
                }),
                Block::Table(Table {
                    rows: 1,
                    cols: 1,
                    col_widths_twip: vec![],
                    cells: vec![Cell {
                        row: 0,
                        col: 0,
                        row_span: 1,
                        col_span: 1,
                        width_twip: None,
                        fill: None,
                        blocks: vec![
                            Block::Paragraph(Paragraph {
                                style: ParaStyle::default(),
                                inlines: vec![text_run("첫", CharStyle::default())],
                            }),
                            Block::Paragraph(Paragraph {
                                style: ParaStyle::default(),
                                inlines: vec![text_run("둘", CharStyle::default())],
                            })],
                    }],
                }),
            ],
        };
        assert_break_invariants(&emit_document(&doc));
    }

    #[test]
    fn newline_inside_run_text_never_reaches_output() {
        // 회귀 테스트. 실제 한컴 문서는 `<hp:t>` 안에 `<hp:lineBreak/>`를 넣어서
        // 런 텍스트 문자열 자체에 `\n`이 실려 온다. emitter가 반드시 막아야 한다.
        let doc = Document {
            blocks: vec![
                // 단일 서식 경로
                Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![text_run(
                        "참가와 관련한 \n개인정보 수집",
                        CharStyle::default(),
                    )],
                }),
                // 다중 런 경로
                Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![
                        text_run("앞\n뒤", CharStyle::default()),
                        text_run("굵게\n줄", bold()),
                    ],
                }),
                // 셀 경로 — 여기서는 `\v`가 금지, `\n`이 정상
                Block::Table(Table {
                    rows: 1,
                    cols: 1,
                    col_widths_twip: vec![],
                    cells: vec![Cell {
                        row: 0,
                        col: 0,
                        row_span: 1,
                        col_span: 1,
                        width_twip: None,
                        fill: None,
                        blocks: vec![Block::Paragraph(Paragraph {
                            style: ParaStyle::default(),
                            // 소스에 \v가 섞여 들어온 최악의 경우
                            inlines: vec![text_run("셀\u{000B}안", CharStyle::default())],
                        })],
                    }],
                }),
            ],
        };

        let items = emit_document(&doc);
        assert_break_invariants(&items);

        // 줄바꿈 자체는 보존되어야 한다 — 버리는 게 아니라 변환이다.
        assert_eq!(
            items[0].props["text"],
            "참가와 관련한 \u{000B}개인정보 수집"
        );
        let cell = items
            .iter()
            .find(|i| i.path.as_deref().is_some_and(|p| p.ends_with(CELL_TEXT_PARAGRAPH)))
            .expect("cell paragraph item");
        assert_eq!(cell.props["text"], "셀\u{000B}안");
    }

    #[test]
    fn image_without_data_is_skipped() {
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![Inline::Image(Image {
                    bin_item_id: "missing".into(),
                    width_twip: None,
                    height_twip: None,
                    alt: None,
                    data: None,
                    content_type: None,
                })],
            })],
        };
        let items = emit_document(&doc);
        // 문단은 나오지만 picture는 나오지 않아야 한다.
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].r#type, Some(TYPE_PARAGRAPH));
    }

    #[test]
    fn image_with_data_becomes_data_uri() {
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![Inline::Image(Image {
                    bin_item_id: "image1".into(),
                    width_twip: Some(1440),
                    height_twip: Some(720),
                    alt: Some("그림".into()),
                    data: Some(vec![0x89, 0x50, 0x4E, 0x47].into()),
                    content_type: Some("image/png".into()),
                })],
            })],
        };
        let items = emit_document(&doc);
        assert_eq!(items.len(), 2);
        let pic = &items[1];
        assert_eq!(pic.r#type, Some(TYPE_PICTURE));
        assert_eq!(pic.parent.as_deref(), Some("/body/p[last()]"));
        let src = pic.props["src"].as_str().expect("src");
        assert!(src.starts_with("data:image/png;base64,"), "got {src}");
        assert!(src.ends_with("iVBORw=="), "got {src}");
        assert_eq!(pic.props["alt"], "그림");
        // 그림 크기는 단위를 붙여야 한다. bare 숫자는 EMU로 해석되어
        // 1440 → 0.0016인치(사실상 보이지 않음)가 된다.
        assert_eq!(pic.props["width"], "72pt");
        assert_eq!(pic.props["height"], "36pt");
    }

    #[test]
    fn picture_dimensions_always_carry_a_unit() {
        // 회귀 테스트: 단위 없는 그림 크기는 실측에서 0.0cm로 렌더됐다.
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![Inline::Image(Image {
                    bin_item_id: "i".into(),
                    width_twip: Some(2880),
                    height_twip: Some(1000),
                    alt: None,
                    data: Some(vec![1, 2, 3].into()),
                    content_type: Some("image/png".into()),
                })],
            })],
        };
        let items = emit_document(&doc);
        let pic = items.iter().find(|i| i.r#type == Some(TYPE_PICTURE)).expect("picture");
        for key in ["width", "height"] {
            let v = pic.props[key].as_str().expect("string");
            assert!(
                v.ends_with("pt") || v.ends_with("cm") || v.ends_with("in"),
                "{key} must carry a unit, got {v:?}"
            );
        }
        assert_eq!(pic.props["width"], "144pt");
        assert_eq!(pic.props["height"], "50pt");
    }

    #[test]
    fn twip_to_pt_divides_by_twenty() {
        assert_eq!(twip_to_pt_string(1440), "72pt");
        assert_eq!(twip_to_pt_string(20), "1pt");
        assert_eq!(twip_to_pt_string(30), "1.5pt");
    }

    #[test]
    fn paragraph_style_maps_to_docx_props() {
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle {
                    align: Some(crate::owpml::model::Align::Center),
                    indent_left_twip: Some(400),
                    indent_first_twip: Some(200),
                    indent_hanging_twip: None,
                    space_before_twip: Some(100),
                    space_after_twip: Some(120),
                    line_spacing_ratio: Some(1.6),
                },
                inlines: vec![text_run("제목", CharStyle::default())],
            })],
        };
        let items = emit_document(&doc);
        let p = &items[0].props;
        assert_eq!(p["align"], "center");
        assert_eq!(p["indent"], "400");
        assert_eq!(p["firstLineIndent"], "200");
        assert_eq!(p["spaceBefore"], "100");
        assert_eq!(p["spaceAfter"], "120");
        assert_eq!(p["lineSpacing"], "1.6x");
    }

    #[test]
    fn hanging_indent_uses_its_own_prop_never_a_negative_value() {
        // 회귀 테스트. 음수 firstLineIndent는 <w:ind w:firstLine="-N"/> 이라는
        // 유효하지 않은 OOXML을 만든다. 코퍼스에 41건 있었다.
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle {
                    indent_left_twip: Some(1714),
                    indent_hanging_twip: Some(1714),
                    ..Default::default()
                },
                inlines: vec![text_run("내어쓰기 문단", CharStyle::default())],
            })],
        };
        let props = &emit_document(&doc)[0].props;
        assert_eq!(props["hangingIndent"], "1714");
        assert_eq!(props["indent"], "1714");
        assert!(
            !props.contains_key("firstLineIndent"),
            "hanging과 firstLine은 함께 나갈 수 없다"
        );
        for (k, v) in props {
            if let Some(s) = v.as_str() {
                assert!(!s.starts_with('-'), "prop {k} has a negative length: {s}");
            }
        }
    }

    #[test]
    fn char_style_maps_size_with_pt_unit() {
        let doc = Document {
            blocks: vec![Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![text_run(
                    "본문",
                    CharStyle {
                        size_pt: Some(10.0),
                        color: Some("#FF0000".into()),
                        font: Some("함초롬바탕".into()),
                        underline: true,
                        ..Default::default()
                    },
                )],
            })],
        };
        let p = &emit_document(&doc)[0].props;
        assert_eq!(p["size"], "10pt");
        assert_eq!(p["color"], "#FF0000");
        assert_eq!(p["font"], "함초롬바탕");
        assert_eq!(p["underline"], "true");
    }


    #[test]
    fn multi_paragraph_cell_becomes_multiple_paragraphs() {
        let doc = Document {
            blocks: vec![Block::Table(Table {
                rows: 1,
                cols: 1,
                col_widths_twip: vec![],
                cells: vec![Cell {
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    width_twip: None,
                    fill: None,
                    blocks: vec![
                        Block::Paragraph(Paragraph {
                            style: ParaStyle::default(),
                            inlines: vec![text_run("첫", CharStyle::default())],
                        }),
                        Block::Paragraph(Paragraph {
                            style: ParaStyle::default(),
                            inlines: vec![text_run("둘", CharStyle::default())],
                        })],
                }],
            })],
        };
        let items = emit_document(&doc);
        // 셀 안 두 문단은 각각 별개 문단으로 나가야 한다.
        // 예전에는 `\v`로 이어 하나로 만들어 문단별 서식이 소실됐다.
        let first = items
            .iter()
            .find(|i| i.path.as_deref() == Some("/body/tbl[last()]/tr[1]/tc[1]/p[1]"))
            .expect("first cell paragraph");
        assert_eq!(first.command, "set", "첫 문단은 기존 빈 문단을 채운다");
        assert_eq!(first.props["text"], "첫");

        let second = items
            .iter()
            .find(|i| {
                i.command == "add"
                    && i.r#type == Some(TYPE_PARAGRAPH)
                    && i.parent.as_deref() == Some("/body/tbl[last()]/tr[1]/tc[1]")
            })
            .expect("second cell paragraph");
        assert_eq!(second.props["text"], "둘");
    }
}
