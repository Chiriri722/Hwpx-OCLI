//! `Contents/sectionN.xml` 본문 파싱.
//!
//! 요소 이름 근거: `unhwp-0.7.0/src/hwpx/section.rs`에서 확인된
//! `p`, `run`, `t`, `tbl`, `tr`, `tc`, `subList`, `cellSpan`, `colSpan`,
//! `rowSpan`, `pic`, `img`, `binaryItemIDRef`, `tab`, `charPrIDRef`,
//! `paraPrIDRef`, `styleIDRef`, `linesegarray`, `lineSeg`.
//!
//! ## 구조 차이 평탄화
//!
//! HWPX는 표를 `hp:p > hp:run > hp:tbl`로 **문단 안에** 넣는다.
//! docx는 표가 `/body` 직속 형제다. 따라서 문단 하나가 표를 품고 있으면
//! `[표 앞 문단, 표, 표 뒤 문단]`으로 쪼개서 내보낸다. 빈 문단은 버린다.

use quick_xml::events::Event;
use quick_xml::Reader;

use super::model::{
    Block, Cell, Equation, EquationMode, Image, Inline, Note, NoteKind, Paragraph, Table,
    TextField, TextRun,
};
use super::styles::{normalize_color, StyleTable};
use super::xml::{attr, attr_i64, attr_usize, local_name, resolve_entity};
use crate::error::{PluginError, Result};

/// 셀 안에 또 표가 나오는 등 재귀가 깊어질 때의 상한. 악의적 입력 방어.
const MAX_DEPTH: usize = 32;
const MAX_TABLE_ROWS: usize = 32_768;
const MAX_TABLE_COLS: usize = 512;
const MAX_TABLE_CELLS: usize = 100_000;
const MAX_TABLE_GRID_SLOTS: usize = 1_000_000;

pub fn parse_section(xml: &str, styles: &StyleTable) -> Result<Vec<Block>> {
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = true;

    let mut blocks = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name_owned = e.name();
                if local_name(name_owned.as_ref()) == "p" {
                    let owned = e.into_owned();
                    blocks.extend(parse_paragraph(&mut reader, &owned, styles, 0)?);
                }
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(blocks)
}

/// `hp:p` 하나를 읽어 블록들로 변환한다.
///
/// 표를 품고 있으면 여러 블록으로 쪼개진다. 그래서 반환형이 `Vec<Block>`이다.
fn parse_paragraph(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &StyleTable,
    depth: usize,
) -> Result<Vec<Block>> {
    let para_style = styles.para_style(attr(start, "paraPrIDRef").as_deref());

    let mut out: Vec<Block> = Vec::new();
    let mut current = Paragraph {
        style: para_style.clone(),
        inlines: Vec::new(),
    };
    // 현재 열린 run의 글자모양. run 밖의 텍스트는 기본 서식으로 본다.
    let mut run_style = None;
    // 열려 있는 누름틀. `(필드, 시작 시점의 인라인 개수, fieldBegin id)`
    //
    // 텍스트는 평소처럼 문단 인라인으로 흘려보낸다(글자 서식이 보존된다).
    // `fieldEnd`에서 그 사이에 아무것도 추가되지 않았다면 = 빈 입력 슬롯이므로
    // 그 자리에 폼필드를 넣는다. 내용이 있었다면 그 내용이 문서 내용이므로
    // 텍스트 그대로 둔다.
    let mut open_field: Option<(TextField, usize, Option<String>)> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name_owned = e.name();
                match local_name(name_owned.as_ref()).as_str() {
                    "run" => {
                        run_style = Some(styles.char_style(attr(&e, "charPrIDRef").as_deref()));
                    }
                    "t" => {
                        let style = run_style.clone().unwrap_or_default();
                        let text = read_text_until_end(reader, "t")?;
                        if !text.is_empty() {
                            push_text(&mut current, text, style);
                        }
                    }
                    "tab" => current.inlines.push(Inline::Tab),
                    "lineBreak" | "linebreak" => current.inlines.push(Inline::LineBreak),
                    "equation" => {
                        let owned = e.into_owned();
                        current
                            .inlines
                            .push(Inline::Equation(parse_equation(reader, &owned)?));
                    }
                    "footNote" | "endNote" if depth < MAX_DEPTH => {
                        let owned = e.into_owned();
                        current.inlines.push(Inline::Note(parse_note(
                            reader,
                            &owned,
                            styles,
                            depth + 1,
                        )?));
                    }
                    "footNote" | "endNote" => {
                        return Err(PluginError::corrupt(format!(
                            "note nesting exceeds the maximum depth of {MAX_DEPTH}"
                        )));
                    }
                    "tbl" if depth < MAX_DEPTH => {
                        // 표 앞까지의 문단을 먼저 확정한다.
                        validate_display_equation_placement(&current)?;
                        flush_paragraph(&mut out, &mut current, &para_style);
                        let owned = e.into_owned();
                        let table = parse_table(reader, &owned, styles, depth + 1)?;
                        out.push(Block::Table(table));
                    }
                    "pic" => {
                        let owned = e.into_owned();
                        if let Some(img) = parse_picture(reader, &owned)? {
                            current.inlines.push(Inline::Image(img));
                        }
                    }
                    // 누름틀 시작. HWP의 "클릭해서 입력" 자리 = 양식 입력란.
                    "fieldBegin" => {
                        let owned = e.into_owned();
                        let is_click_here = attr(&owned, "type")
                            .is_some_and(|t| t.eq_ignore_ascii_case("CLICK_HERE"));
                        let field_id = attr(&owned, "id");
                        // `parameters` 안의 Command 문자열을 읽어야 하므로 요소를 소비한다.
                        let command = read_field_command(reader)?;

                        if is_click_here {
                            let field = TextField {
                                name: attr(&owned, "name").filter(|s| !s.trim().is_empty()),
                                hint: parse_field_hint(&command),
                            };
                            open_field = Some((field, current.inlines.len(), field_id));
                        }
                    }
                    // 누름틀 끝. `beginIDRef`로 짝을 맞춘다.
                    "fieldEnd" => {
                        let end_ref = attr(&e, "beginIDRef");
                        let matches_open = match (&open_field, &end_ref) {
                            (Some((_, _, Some(a))), Some(b)) => a == b,
                            // id가 없으면 가장 최근에 열린 필드로 본다.
                            (Some(_), _) => true,
                            (None, _) => false,
                        };
                        if matches_open {
                            if let Some((field, start, _)) = open_field.take() {
                                // 구간에 내용이 없었으면 빈 입력 슬롯이다.
                                if current.inlines.len() == start {
                                    current.inlines.push(Inline::TextField(field));
                                }
                            }
                        }
                    }
                    // 폼 컨트롤 체크박스. 양식 문서는 체크박스를 문자가 아니라
                    // 이 요소로 넣는다. 무시하면 체크 안 된 상자가 사라진다.
                    "checkBtn" => {
                        let checked =
                            attr(&e, "value").is_some_and(|v| v.eq_ignore_ascii_case("CHECKED"));
                        let name = attr(&e, "name").filter(|s| !s.trim().is_empty());
                        current
                            .inlines
                            .push(Inline::CheckBox(super::model::CheckBox { name, checked }));
                        // 자식(formCharPr/sz/pos/outMargin)은 필요 없다.
                        skip_element(reader, "checkBtn")?;
                    }
                    // 렌더링 좌표 캐시. 본문이 아니므로 통째로 건너뛴다.
                    "linesegarray" => {
                        skip_element(reader, "linesegarray")?;
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name_owned = e.name();
                match local_name(name_owned.as_ref()).as_str() {
                    "run" => run_style = None,
                    "p" => {
                        // fieldEnd 없이 문단이 끝난 경우. 내용이 없었다면 슬롯으로 본다.
                        if let Some((field, start, _)) = open_field.take() {
                            if current.inlines.len() == start {
                                current.inlines.push(Inline::TextField(field));
                            }
                        }
                        break;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }

    validate_display_equation_placement(&current)?;

    // 마지막 조각. 표가 하나도 없었다면 빈 문단도 살려서 문서의 빈 줄을 보존한다.
    if out.is_empty() {
        out.push(Block::Paragraph(current));
    } else {
        flush_paragraph(&mut out, &mut current, &para_style);
    }

    Ok(out)
}

fn validate_display_equation_placement(paragraph: &Paragraph) -> Result<()> {
    let display_count = paragraph
        .inlines
        .iter()
        .filter(|inline| {
            matches!(
                inline,
                Inline::Equation(Equation {
                    mode: EquationMode::Display,
                    ..
                })
            )
        })
        .count();
    if display_count == 0 {
        return Ok(());
    }
    if display_count == 1 && paragraph.inlines.len() == 1 {
        return Ok(());
    }
    Err(PluginError::unsupported_feature(
        "a display equation mixed with other paragraph content cannot be ordered without loss",
    ))
}

/// `hp:equation`의 직접 자식 `hp:script`와 `hp:pos`를 읽고 수식을 변환한다.
fn parse_equation(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
) -> Result<Equation> {
    let mut script = None;
    let mut mode = None;
    let mut depth = 0usize;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(
                    "unexpected end of XML inside equation",
                ));
            }
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                if depth == 0 && name == "script" {
                    if script.is_some() {
                        return Err(PluginError::corrupt(
                            "equation contains more than one script element",
                        ));
                    }
                    script = Some(read_equation_script(reader)?);
                } else {
                    if depth == 0 && name == "pos" {
                        if mode.is_some() {
                            return Err(PluginError::corrupt(
                                "equation contains more than one pos element",
                            ));
                        }
                        let raw = attr(&event, "treatAsChar").ok_or_else(|| {
                            PluginError::corrupt("equation pos is missing treatAsChar")
                        })?;
                        mode = Some(match raw.trim().to_ascii_lowercase().as_str() {
                            "1" | "true" => EquationMode::Inline,
                            "0" | "false" => EquationMode::Display,
                            _ => {
                                return Err(PluginError::corrupt(format!(
                                    "equation pos has invalid treatAsChar value {raw:?}"
                                )));
                            }
                        });
                    }
                    depth = depth.saturating_add(1);
                }
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if depth == 0 && name == "equation" {
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        buf.clear();
    }

    let source =
        script.ok_or_else(|| PluginError::corrupt("equation is missing required script"))?;
    let mode = mode.ok_or_else(|| PluginError::corrupt("equation is missing required pos"))?;
    let mut formula = super::equation::to_latex(&source).map_err(|error| {
        if error.is_unsupported() {
            PluginError::unsupported_feature(error.to_string())
        } else {
            PluginError::corrupt(error.to_string())
        }
    })?;

    let text_color = attr(start, "textColor")
        .map(|raw| {
            normalize_color(raw.clone()).ok_or_else(|| {
                PluginError::corrupt(format!("equation has invalid textColor {raw:?}"))
            })
        })
        .transpose()?;
    if let Some(color) = text_color.filter(|color| color != "#000000") {
        formula = format!(r"\color{{{color}}}{{{formula}}}");
    }

    Ok(Equation { formula, mode })
}

fn read_equation_script(reader: &mut Reader<&[u8]>) -> Result<String> {
    let mut script = String::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(
                    "unexpected end of XML inside equation script",
                ));
            }
            Event::Text(text) => script.push_str(&text.decode()?),
            Event::CData(text) => script.push_str(&String::from_utf8_lossy(text.as_ref())),
            Event::GeneralRef(reference) => script.push_str(&resolve_entity(&reference)?),
            Event::Start(event) => {
                return Err(PluginError::corrupt(format!(
                    "equation script contains nested element {}",
                    local_name(event.name().as_ref())
                )));
            }
            Event::End(event) if local_name(event.name().as_ref()) == "script" => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(script)
}

/// `hp:footNote`/`hp:endNote`와 필수 `hp:subList` 본문을 읽는다.
///
/// 주석 요소 전체를 여기서 소비해야 바깥 문단 파서가 주석 본문의 텍스트를 본문
/// 런으로 오인하지 않는다. `subList` 안의 문단/표 순서는 일반 문단 파서에 맡긴다.
fn parse_note(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &StyleTable,
    depth: usize,
) -> Result<Note> {
    let tag = local_name(start.name().as_ref());
    let kind = match tag.as_str() {
        "footNote" => NoteKind::Footnote,
        "endNote" => NoteKind::Endnote,
        _ => {
            return Err(PluginError::corrupt(format!(
                "unexpected note element {tag}"
            )));
        }
    };

    let mut blocks = Vec::new();
    let mut saw_sub_list = false;
    let mut sub_list_depth = 0usize;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(format!(
                    "unexpected end of XML inside {tag}"
                )));
            }
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "subList" if sub_list_depth == 0 => {
                        if saw_sub_list {
                            return Err(PluginError::corrupt(format!(
                                "{tag} contains more than one subList"
                            )));
                        }
                        saw_sub_list = true;
                        sub_list_depth = 1;
                    }
                    "subList" => sub_list_depth += 1,
                    "p" if sub_list_depth == 1 => {
                        let owned = event.into_owned();
                        blocks.extend(parse_paragraph(reader, &owned, styles, depth)?);
                    }
                    _ if sub_list_depth == 0 => {
                        return Err(PluginError::corrupt(format!(
                            "{tag} contains {name} outside subList"
                        )));
                    }
                    _ => {}
                }
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if name == "subList" && sub_list_depth > 0 {
                    sub_list_depth -= 1;
                } else if name == tag {
                    if sub_list_depth != 0 {
                        return Err(PluginError::corrupt(format!(
                            "{tag} ended before its subList"
                        )));
                    }
                    break;
                }
            }
            _ => {}
        }
        buf.clear();
    }

    if !saw_sub_list {
        return Err(PluginError::corrupt(format!(
            "{tag} is missing its required subList"
        )));
    }
    // 한/글은 주석 본문 안의 주석을 표현하지 못하며 그런 파일을 열 때 오류를
    // 낸다. 하위 표 셀까지 검사해 잘못된 참조 그래프를 DOCX로 재생하지 않는다.
    if blocks_contain_note(&blocks) {
        return Err(PluginError::corrupt(format!(
            "{tag} contains a nested footnote or endnote"
        )));
    }

    Ok(Note {
        kind,
        number: attr_usize(start, "number"),
        instance_id: attr(start, "instId"),
        blocks,
    })
}

fn blocks_contain_note(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(paragraph) => paragraph
            .inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Note(_))),
        Block::Table(table) => table
            .cells
            .iter()
            .any(|cell| blocks_contain_note(&cell.blocks)),
    })
}

/// 텍스트가 있을 때만 문단을 확정하고, `current`를 새 문단으로 갈아끼운다.
fn flush_paragraph(out: &mut Vec<Block>, current: &mut Paragraph, style: &super::model::ParaStyle) {
    let done = std::mem::replace(
        current,
        Paragraph {
            style: style.clone(),
            inlines: Vec::new(),
        },
    );
    // 표 앞뒤로 생기는 빈 껍데기 문단은 버린다.
    if !done.inlines.is_empty() {
        out.push(Block::Paragraph(done));
    }
}

/// 같은 서식의 텍스트가 연달아 오면 하나의 런으로 합친다.
fn push_text(p: &mut Paragraph, text: String, style: super::model::CharStyle) {
    if let Some(Inline::Text(last)) = p.inlines.last_mut() {
        if last.style == style {
            last.text.push_str(&text);
            return;
        }
    }
    p.inlines.push(Inline::Text(TextRun { text, style }));
}

fn parse_table(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &StyleTable,
    depth: usize,
) -> Result<Table> {
    let declared_rows = attr_usize(start, "rowCnt");
    let declared_cols = attr_usize(start, "colCnt");

    let mut cells: Vec<Cell> = Vec::new();
    // cellAddr가 없는 문서를 위한 폴백 카운터.
    let mut row_cursor = 0usize;
    let mut col_cursor = 0usize;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name_owned = e.name();
                match local_name(name_owned.as_ref()).as_str() {
                    "tr" => col_cursor = 0,
                    "tc" => {
                        let owned = e.into_owned();
                        let mut cell = parse_cell(reader, &owned, styles, depth)?;
                        // cellAddr가 없었다면 커서로 채운다.
                        if cell.addr_missing {
                            cell.inner.row = row_cursor;
                            cell.inner.col = col_cursor;
                        }
                        col_cursor = checked_table_add(
                            cell.inner.col,
                            cell.inner.col_span.max(1),
                            "cell column span",
                        )?;
                        if cells.len() >= MAX_TABLE_CELLS {
                            return Err(table_limit(format!(
                                "cell count exceeds maximum {MAX_TABLE_CELLS}"
                            )));
                        }
                        cells.push(cell.inner);
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name_owned = e.name();
                match local_name(name_owned.as_ref()).as_str() {
                    "tr" => {
                        row_cursor = row_cursor
                            .checked_add(1)
                            .ok_or_else(|| table_limit("row cursor overflowed".to_string()))?;
                    }
                    "tbl" => break,
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }

    // 선언값이 없거나 실제보다 작으면 셀 주소에서 유도한다.
    let mut derived_rows = 0usize;
    let mut derived_cols = 0usize;
    for cell in &cells {
        derived_rows = derived_rows.max(checked_table_add(
            cell.row,
            cell.row_span.max(1),
            "cell row span",
        )?);
        derived_cols = derived_cols.max(checked_table_add(
            cell.col,
            cell.col_span.max(1),
            "cell column span",
        )?);
    }

    let rows = declared_rows.unwrap_or(0).max(derived_rows);
    let cols = declared_cols.unwrap_or(0).max(derived_cols);
    validate_table_dimensions(rows, cols, cells.len())?;

    // 열 너비는 병합 셀 제약까지 써서 유도한다 (`model::derive_col_widths`).
    let col_widths_twip = super::model::derive_col_widths(&cells, cols);

    Ok(Table {
        rows,
        cols,
        col_widths_twip,
        cells,
    })
}

fn checked_table_add(left: usize, right: usize, label: &str) -> Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| table_limit(format!("{label} overflowed")))
}

fn validate_table_dimensions(rows: usize, cols: usize, cells: usize) -> Result<()> {
    if rows > MAX_TABLE_ROWS {
        return Err(table_limit(format!(
            "row count {rows} exceeds maximum {MAX_TABLE_ROWS}"
        )));
    }
    if cols > MAX_TABLE_COLS {
        return Err(table_limit(format!(
            "column count {cols} exceeds maximum {MAX_TABLE_COLS}"
        )));
    }
    if cells > MAX_TABLE_CELLS {
        return Err(table_limit(format!(
            "cell count {cells} exceeds maximum {MAX_TABLE_CELLS}"
        )));
    }
    let slots = rows
        .checked_mul(cols)
        .ok_or_else(|| table_limit("row-by-column grid size overflowed".to_string()))?;
    if slots > MAX_TABLE_GRID_SLOTS {
        return Err(table_limit(format!(
            "grid size {slots} exceeds maximum {MAX_TABLE_GRID_SLOTS}"
        )));
    }
    Ok(())
}

fn table_limit(message: String) -> PluginError {
    PluginError::corrupt(format!("table resource limit exceeded: {message}"))
}

struct ParsedCell {
    inner: Cell,
    addr_missing: bool,
}

fn parse_cell(
    reader: &mut Reader<&[u8]>,
    _start: &quick_xml::events::BytesStart<'static>,
    styles: &StyleTable,
    depth: usize,
) -> Result<ParsedCell> {
    let mut cell = Cell {
        row_span: 1,
        col_span: 1,
        ..Default::default()
    };
    let mut addr_missing = true;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name_owned = e.name();
                match local_name(name_owned.as_ref()).as_str() {
                    "cellAddr" => {
                        if let Some(r) = attr_usize(&e, "rowAddr") {
                            cell.row = r;
                            addr_missing = false;
                        }
                        if let Some(c) = attr_usize(&e, "colAddr") {
                            cell.col = c;
                            addr_missing = false;
                        }
                    }
                    "cellSpan" => {
                        cell.row_span = attr_usize(&e, "rowSpan").unwrap_or(1).max(1);
                        cell.col_span = attr_usize(&e, "colSpan").unwrap_or(1).max(1);
                    }
                    "cellSz" => {
                        cell.width_twip = attr_i64(&e, "width").map(super::model::hwpunit_to_twip);
                    }
                    "fillBrush" | "windowBrush" => {
                        if let Some(c) = attr(&e, "faceColor").and_then(normalize_color) {
                            cell.fill = Some(c);
                        }
                    }
                    "p" if depth < MAX_DEPTH => {
                        let owned = e.into_owned();
                        // 중첩표를 평탄화하지 않고 블록으로 그대로 담는다.
                        // docx도 셀 안에 표를 넣을 수 있다(실측 확인).
                        cell.blocks
                            .extend(parse_paragraph(reader, &owned, styles, depth + 1)?);
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name_owned = e.name();
                if local_name(name_owned.as_ref()) == "tc" {
                    break;
                }
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(ParsedCell {
        inner: cell,
        addr_missing,
    })
}

fn parse_picture(
    reader: &mut Reader<&[u8]>,
    _start: &quick_xml::events::BytesStart<'static>,
) -> Result<Option<Image>> {
    let mut bin_id: Option<String> = None;
    let mut width_twip = None;
    let mut height_twip = None;
    let mut alt = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name_owned = e.name();
                match local_name(name_owned.as_ref()).as_str() {
                    "img" => {
                        if let Some(id) = attr(&e, "binaryItemIDRef") {
                            bin_id = Some(id);
                        }
                        if let Some(a) = attr(&e, "alt").filter(|s| !s.trim().is_empty()) {
                            alt = Some(a);
                        }
                    }
                    // hp:sz 또는 hp:curSz가 표시 크기를 담는다.
                    "sz" | "curSz" | "orgSz" => {
                        if width_twip.is_none() {
                            width_twip = attr_i64(&e, "width").map(super::model::hwpunit_to_twip);
                        }
                        if height_twip.is_none() {
                            height_twip = attr_i64(&e, "height").map(super::model::hwpunit_to_twip);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name_owned = e.name();
                if local_name(name_owned.as_ref()) == "pic" {
                    break;
                }
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(bin_id.map(|bin_item_id| Image {
        bin_item_id,
        width_twip,
        height_twip,
        alt,
        data: None,
        content_type: None,
    }))
}

/// `hp:fieldBegin` 요소를 소비하면서 `hp:stringParam` 내용을 모은다.
///
/// 구조: `fieldBegin > parameters > stringParam[@name="Command"]`
fn read_field_command(reader: &mut Reader<&[u8]>) -> Result<String> {
    let mut command = String::new();
    let mut in_string_param = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let owned = e.name();
                if local_name(owned.as_ref()) == "stringParam" {
                    in_string_param = true;
                }
            }
            Event::Text(t) if in_string_param => command.push_str(&t.decode()?),
            Event::CData(t) if in_string_param => {
                command.push_str(&String::from_utf8_lossy(t.as_ref()))
            }
            Event::GeneralRef(r) if in_string_param => command.push_str(&resolve_entity(&r)?),
            Event::End(e) => {
                let owned = e.name();
                match local_name(owned.as_ref()).as_str() {
                    "stringParam" => in_string_param = false,
                    "fieldBegin" => break,
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(command)
}

/// `Command` 문자열에서 안내 문구를 뽑는다.
///
/// 실측 형식: `Clickhere:set:51:Direction:wstring:9:기재하지 마세요. HelpState:wstring:0:`
/// `Direction:wstring:<문자수>:<문구>` 이므로 길이만큼 잘라낸다.
///
/// 이 형식은 실제 문서 2건에서만 관찰했다. 파싱에 실패하면 문구를 생략하되
/// **필드 자체는 만든다.** 안내 문구는 부가 정보다.
fn parse_field_hint(command: &str) -> Option<String> {
    const KEY: &str = "Direction:wstring:";
    let rest = command.split_once(KEY)?.1;
    let (len_str, body) = rest.split_once(':')?;
    let len: usize = len_str.trim().parse().ok()?;
    if len == 0 {
        return None;
    }
    let hint: String = body.chars().take(len).collect();
    let hint = hint.trim().to_string();
    if hint.is_empty() {
        None
    } else {
        Some(hint)
    }
}

/// `<hp:t>` 안의 텍스트를 모은다. 중간에 `hp:tab` 등이 섞여도 텍스트만 취한다.
fn read_text_until_end(reader: &mut Reader<&[u8]>, end: &str) -> Result<String> {
    let mut out = String::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Text(t) => out.push_str(&t.decode()?),
            Event::CData(t) => out.push_str(&String::from_utf8_lossy(t.as_ref())),
            // quick-xml 0.41은 `&amp;` 같은 엔티티를 별도 이벤트로 낸다.
            // 처리하지 않으면 문자가 조용히 사라진다.
            Event::GeneralRef(r) => out.push_str(&resolve_entity(&r)?),
            Event::Start(e) => {
                let name_owned = e.name();
                let n = local_name(name_owned.as_ref());
                if n == "tab" {
                    out.push('\t');
                } else if n == "lineBreak" {
                    out.push('\n');
                } else if n == end {
                    depth += 1;
                }
            }
            Event::End(e) => {
                let name_owned = e.name();
                if local_name(name_owned.as_ref()) == end {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// 여는 태그를 이미 읽은 상태에서 해당 요소를 닫는 태그까지 버린다.
fn skip_element(reader: &mut Reader<&[u8]>, name: &str) -> Result<()> {
    let mut depth = 0usize;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let owned = e.name();
                if local_name(owned.as_ref()) == name {
                    depth += 1;
                }
            }
            Event::End(e) => {
                let owned = e.name();
                if local_name(owned.as_ref()) == name {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_direction_hint_from_command_string() {
        // 실측 형식 (2026 대구문학관 참가신청서)
        let cmd = "Clickhere:set:51:Direction:wstring:9:기재하지 마세요. HelpState:wstring:0:  ";
        assert_eq!(parse_field_hint(cmd).as_deref(), Some("기재하지 마세요."));
    }

    #[test]
    fn hint_length_is_counted_in_characters_not_bytes() {
        // 한글은 UTF-8에서 3바이트다. 바이트로 세면 잘린다.
        let cmd = "Direction:wstring:5:가나다라마 뒤쪽은 무시";
        assert_eq!(parse_field_hint(cmd).as_deref(), Some("가나다라마"));
    }

    #[test]
    fn missing_or_malformed_hint_yields_none() {
        assert_eq!(parse_field_hint(""), None);
        assert_eq!(parse_field_hint("Clickhere:set:0:"), None);
        // 길이 0은 문구 없음
        assert_eq!(parse_field_hint("Direction:wstring:0:"), None);
        // 길이가 숫자가 아니면 포기
        assert_eq!(parse_field_hint("Direction:wstring:abc:내용"), None);
    }
}
