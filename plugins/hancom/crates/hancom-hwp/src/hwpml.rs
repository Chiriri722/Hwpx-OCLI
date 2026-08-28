//! Legacy HWPML (`.hml`) single-XML reader.
//!
//! Hancom's public HWPML revision 1.2 defines the body as
//! `HWPML > BODY > SECTION > P > TEXT`, with character data normally wrapped
//! by `CHAR`. Compatible producers also emit character data directly in
//! `TEXT`, so both forms are accepted. Unsupported control subtrees are
//! skipped as a unit: metadata, scripts, binary payloads, and control
//! parameters must never leak into document text.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use officecli_hancom_core::budget::ResourceBudget;
use officecli_hancom_core::model::{
    derive_col_widths, hwpunit_to_point, hwpunit_to_twip, Align, Block, Cell, CharStyle, Document,
    Inline, ParaStyle, Paragraph, Table, TextRun, VertAlign,
};
use officecli_hancom_core::xml_encoding::decode_xml;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::error::{PluginError, Result};
use crate::owpml::styles::normalize_color;
use crate::owpml::xml::{local_name, resolve_entity};

const MAX_HWPML_BYTES: u64 = 100 * 1024 * 1024;
const MAX_XML_EVENTS: u64 = 4_000_000;
const MAX_XML_DEPTH: usize = 256;
const MAX_PARAGRAPHS: u64 = 200_000;
const MAX_INLINES: u64 = 2_000_000;
const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TEXT_NODE_BYTES: usize = 8 * 1024 * 1024;
const MAX_STYLES: u64 = 65_536;
const MAX_ATTRIBUTES_PER_ELEMENT: usize = 256;
const MAX_TABLES: u64 = 10_000;
const MAX_TABLE_CELLS: u64 = 100_000;
const MAX_TABLE_ROWS: usize = 32_768;
const MAX_TABLE_COLS: usize = 512;
const MAX_TABLE_GRID_SLOTS: usize = 1_000_000;

/// HWPML 파일을 열어 공용 문서 모델로 만든다.
pub fn read_document(path: &Path) -> Result<Document> {
    let file = File::open(path).map_err(|error| {
        PluginError::corrupt(format!("cannot open {}: {error}", path.display()))
    })?;
    read_document_from(BufReader::new(file))
}

/// 제한된 메모리 예산 안에서 HWPML을 읽는 리더.
///
/// UTF-16LE/BE를 안전하게 UTF-8로 정규화하기 위해 입력을 최대 100 MiB까지
/// 읽고, XML 이벤트·깊이·본문 출력에도 각각 독립적인 상한을 둔다. DTD는
/// 외부/내부 엔티티 경계를 없애기 위해 거부한다.
pub fn read_document_from<R: BufRead>(reader: R) -> Result<Document> {
    let mut bytes = Vec::new();
    reader.take(MAX_HWPML_BYTES + 1).read_to_end(&mut bytes)?;
    let decoded = decode_xml(&bytes, MAX_HWPML_BYTES)?;
    let mut xml = Reader::from_str(&decoded.text);
    let config = xml.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = true;

    let mut parser = HwpmlParser::default();
    let mut buf = Vec::new();
    loop {
        parser.events.consume(1)?;
        let event = xml.read_event_into(&mut buf)?;
        if xml.buffer_position() > MAX_HWPML_BYTES {
            return Err(limit_error(format!(
                "HWPML input exceeds {MAX_HWPML_BYTES} bytes"
            )));
        }

        match event {
            Event::Start(element) => parser.start(&element)?,
            Event::Empty(element) => {
                // `expand_empty_elements` normally turns this into Start+End,
                // but handle Empty as well so parser correctness is not tied to
                // that reader setting.
                let name = element.name().as_ref().to_vec();
                parser.start(&element)?;
                parser.end(&name)?;
            }
            Event::End(element) => parser.end(element.name().as_ref())?,
            Event::Text(text) => parser.text(&text.decode()?)?,
            Event::CData(text) => parser.text(&String::from_utf8_lossy(text.as_ref()))?,
            Event::GeneralRef(reference) => parser.text(&resolve_entity(&reference)?)?,
            Event::DocType(_) => {
                return Err(PluginError::corrupt(
                    "HWPML document type declarations are not supported",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    parser.finish()
}

struct HwpmlParser {
    depth: usize,
    root_seen: bool,
    root_closed: bool,
    body_seen: bool,
    head_depth: Option<usize>,
    body_depth: Option<usize>,
    font_face: Option<(usize, bool)>,
    hangul_fonts: HashMap<String, String>,
    fallback_fonts: HashMap<String, String>,
    char_styles: HashMap<String, CharStyle>,
    para_styles: HashMap<String, ParaStyle>,
    current_char_style: Option<(usize, String, CharStyle)>,
    current_para_style: Option<(usize, String, ParaStyle)>,
    paragraph_depth: Option<usize>,
    text_depth: Option<usize>,
    char_depth: Option<usize>,
    skip_depth: Option<usize>,
    table_depth: Option<usize>,
    cell_depth: Option<usize>,
    current_table: Option<Table>,
    current_cell: Option<Cell>,
    suspended_paragraph: Option<SuspendedParagraph>,
    active_text_style: CharStyle,
    current_paragraph: Option<Paragraph>,
    current_paragraph_is_table_trailing: bool,
    blocks: Vec<Block>,
    events: ResourceBudget,
    paragraphs: ResourceBudget,
    inlines: ResourceBudget,
    text_bytes: ResourceBudget,
    styles: ResourceBudget,
    tables: ResourceBudget,
    table_cells: ResourceBudget,
}

struct SuspendedParagraph {
    paragraph_depth: usize,
    text_depth: usize,
    active_text_style: CharStyle,
    trailing: Paragraph,
}

impl Default for HwpmlParser {
    fn default() -> Self {
        Self {
            depth: 0,
            root_seen: false,
            root_closed: false,
            body_seen: false,
            head_depth: None,
            body_depth: None,
            font_face: None,
            hangul_fonts: HashMap::new(),
            fallback_fonts: HashMap::new(),
            char_styles: HashMap::new(),
            para_styles: HashMap::new(),
            current_char_style: None,
            current_para_style: None,
            paragraph_depth: None,
            text_depth: None,
            char_depth: None,
            skip_depth: None,
            table_depth: None,
            cell_depth: None,
            current_table: None,
            current_cell: None,
            suspended_paragraph: None,
            active_text_style: CharStyle::default(),
            current_paragraph: None,
            current_paragraph_is_table_trailing: false,
            blocks: Vec::new(),
            events: ResourceBudget::new("HWPML XML event count", MAX_XML_EVENTS),
            paragraphs: ResourceBudget::new("HWPML paragraph count", MAX_PARAGRAPHS),
            inlines: ResourceBudget::new("HWPML inline count", MAX_INLINES),
            text_bytes: ResourceBudget::new("HWPML emitted text bytes", MAX_TEXT_BYTES),
            styles: ResourceBudget::new("HWPML style count", MAX_STYLES),
            tables: ResourceBudget::new("HWPML table count", MAX_TABLES),
            table_cells: ResourceBudget::new("HWPML table cell count", MAX_TABLE_CELLS),
        }
    }
}

impl HwpmlParser {
    fn start(&mut self, element: &BytesStart<'_>) -> Result<()> {
        validate_attributes(element)?;
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| limit_error("HWPML XML depth overflow"))?;
        if self.depth > MAX_XML_DEPTH {
            return Err(limit_error(format!(
                "HWPML XML depth {} exceeds maximum {MAX_XML_DEPTH}",
                self.depth
            )));
        }

        let raw_name = element.name();
        let name = local_name(raw_name.as_ref());

        if self.depth == 1 {
            if self.root_seen {
                return Err(PluginError::corrupt(
                    "HWPML XML contains more than one root element",
                ));
            }
            if !name.eq_ignore_ascii_case("HWPML") {
                return Err(PluginError::corrupt(format!(
                    "XML root <{name}> is not an HWPML document"
                )));
            }
            if let Some(version) = attr_ci(element, "Version")? {
                if !matches!(version.as_str(), "2.8" | "2.9" | "2.91") {
                    return Err(PluginError::unsupported_feature(format!(
                        "HWPML version {version:?} is not supported (expected 2.8, 2.9, or 2.91)"
                    )));
                }
            }
            self.root_seen = true;
            return Ok(());
        }

        if !self.root_seen || self.root_closed {
            return Err(PluginError::corrupt(
                "content appears outside the HWPML root element",
            ));
        }

        if self.skip_depth.is_some() {
            return Ok(());
        }

        if self.depth == 2 && name.eq_ignore_ascii_case("HEAD") {
            self.head_depth = Some(self.depth);
            return Ok(());
        }
        if self.depth == 2 && name.eq_ignore_ascii_case("BODY") {
            self.body_seen = true;
            self.body_depth = Some(self.depth);
            return Ok(());
        }

        if self.head_depth.is_some() {
            self.start_head(&name, element)?;
        } else if self.body_depth.is_some() {
            self.start_body(&name, element)?;
        }
        Ok(())
    }

    fn start_head(&mut self, name: &str, element: &BytesStart<'_>) -> Result<()> {
        if name.eq_ignore_ascii_case("FONTFACE") {
            let is_hangul = attr_ci(element, "Lang")?
                .is_some_and(|language| language.eq_ignore_ascii_case("Hangul"));
            self.font_face = Some((self.depth, is_hangul));
        } else if name.eq_ignore_ascii_case("FONT") {
            if let (Some((_, is_hangul)), Some(id), Some(font_name)) = (
                self.font_face,
                attr_ci(element, "Id")?,
                attr_ci(element, "Name")?,
            ) {
                if is_hangul {
                    self.hangul_fonts.insert(id, font_name);
                } else {
                    self.fallback_fonts.entry(id).or_insert(font_name);
                }
            }
        } else if name.eq_ignore_ascii_case("CHARSHAPE") {
            let id = attr_ci(element, "Id")?.unwrap_or_default();
            let mut style = CharStyle::default();
            if let Some(height) = attr_ci(element, "Height")?.and_then(|raw| parse_number(&raw)) {
                style.size_pt = Some(hwpunit_to_point(height));
            }
            if let Some(color) = attr_ci(element, "TextColor")?.and_then(hwpml_color) {
                style.color = Some(color);
            }
            if let Some(color) = attr_ci(element, "ShadeColor")?.and_then(hwpml_shade_color) {
                style.highlight = Some(color);
            }
            self.current_char_style = Some((self.depth, id, style));
        } else if name.eq_ignore_ascii_case("PARASHAPE") {
            let id = attr_ci(element, "Id")?.unwrap_or_default();
            let style = ParaStyle {
                align: attr_ci(element, "Align")?
                    .as_deref()
                    .and_then(Align::from_owpml),
                ..ParaStyle::default()
            };
            self.current_para_style = Some((self.depth, id, style));
        } else if name.eq_ignore_ascii_case("FONTID") && self.current_char_style.is_some() {
            let reference = attr_ci(element, "Hangul")?.or(attr_ci(element, "Latin")?);
            let font = reference.and_then(|id| self.resolve_font(&id));
            if let Some((_, _, style)) = self.current_char_style.as_mut() {
                style.font = font;
            }
        } else if let Some((_, _, style)) = self.current_char_style.as_mut() {
            if name.eq_ignore_ascii_case("BOLD") {
                style.bold = true;
            } else if name.eq_ignore_ascii_case("ITALIC") {
                style.italic = true;
            } else if name.eq_ignore_ascii_case("UNDERLINE") {
                style.underline = !attr_ci(element, "Type")?
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("None"));
            } else if name.eq_ignore_ascii_case("STRIKEOUT") {
                style.strike = !attr_ci(element, "Type")?
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("None"));
            } else if name.eq_ignore_ascii_case("SUPERSCRIPT") {
                style.vert_align = Some(VertAlign::Superscript);
            } else if name.eq_ignore_ascii_case("SUBSCRIPT") {
                style.vert_align = Some(VertAlign::Subscript);
            }
        } else if let Some((_, _, style)) = self.current_para_style.as_mut() {
            if name.eq_ignore_ascii_case("PARAMARGIN") {
                if let Some(value) = attr_ci(element, "Indent")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                {
                    style.set_first_line_indent(hwpunit_to_twip(value));
                }
                style.indent_left_twip = attr_ci(element, "Left")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                    .map(hwpunit_to_twip);
                style.space_before_twip = attr_ci(element, "Prev")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                    .map(hwpunit_to_twip);
                style.space_after_twip = attr_ci(element, "Next")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                    .map(hwpunit_to_twip);

                let spacing_type =
                    attr_ci(element, "LineSpacingType")?.unwrap_or_else(|| "Percent".to_string());
                if spacing_type.eq_ignore_ascii_case("Percent") {
                    style.line_spacing_ratio = attr_ci(element, "LineSpacing")?
                        .as_deref()
                        .and_then(parse_number)
                        .filter(|value| *value > 0)
                        .map(|value| value as f64 / 100.0);
                }
            }
        }
        Ok(())
    }

    fn start_body(&mut self, name: &str, element: &BytesStart<'_>) -> Result<()> {
        if self.current_table.is_some() {
            return self.start_table_body(name, element);
        }

        if name.eq_ignore_ascii_case("P") && self.current_paragraph.is_none() {
            return self.start_paragraph(element);
        }

        if self.current_paragraph.is_none() {
            if name.eq_ignore_ascii_case("SECTION") {
                return Ok(());
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML BODY element <{name}> is not supported yet; refusing to drop its content"
            )));
        }

        if name.eq_ignore_ascii_case("TEXT") && self.text_depth.is_none() {
            return self.start_text(element);
        }

        if self.text_depth.is_none() {
            if is_safe_ignored_control(name) {
                self.skip_depth = Some(self.depth);
                return Ok(());
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML control <{name}> beside TEXT is not supported yet; refusing to drop its content"
            )));
        }

        if name.eq_ignore_ascii_case("TABLE") {
            return self.start_table(element);
        }
        self.start_inline_content(name)
    }

    fn start_paragraph(&mut self, element: &BytesStart<'_>) -> Result<()> {
        self.paragraphs.consume(1)?;
        let style = attr_ci(element, "ParaShape")?
            .as_deref()
            .and_then(|id| self.para_styles.get(id))
            .cloned()
            .unwrap_or_default();
        self.current_paragraph = Some(Paragraph {
            style,
            inlines: Vec::new(),
        });
        self.current_paragraph_is_table_trailing = false;
        self.paragraph_depth = Some(self.depth);
        Ok(())
    }

    fn start_text(&mut self, element: &BytesStart<'_>) -> Result<()> {
        self.active_text_style = attr_ci(element, "CharShape")?
            .as_deref()
            .and_then(|id| self.char_styles.get(id))
            .cloned()
            .unwrap_or_default();
        self.text_depth = Some(self.depth);
        Ok(())
    }

    fn start_inline_content(&mut self, name: &str) -> Result<()> {
        if name.eq_ignore_ascii_case("CHAR") {
            if self.char_depth.is_some() {
                return Err(PluginError::corrupt("nested HWPML CHAR elements"));
            }
            self.char_depth = Some(self.depth);
            return Ok(());
        }
        if name.eq_ignore_ascii_case("TAB") {
            self.push_inline(Inline::Tab)?;
        } else if name.eq_ignore_ascii_case("LINEBREAK") {
            self.push_inline(Inline::LineBreak)?;
        } else if name.eq_ignore_ascii_case("HYPEN") {
            self.push_text("-")?;
        } else if name.eq_ignore_ascii_case("NBSPACE") {
            self.push_text("\u{00A0}")?;
        } else if name.eq_ignore_ascii_case("FWSPACE") {
            self.push_text("\u{3000}")?;
        } else if is_safe_ignored_control(name) {
            // Layout-only controls and non-text markers may be ignored without
            // deleting user-visible characters. Skip their whole subtree.
            self.skip_depth = Some(self.depth);
        } else {
            return Err(PluginError::unsupported_feature(format!(
                "HWPML control <{name}> is not supported yet; refusing to drop its content"
            )));
        }
        Ok(())
    }

    fn start_table(&mut self, element: &BytesStart<'_>) -> Result<()> {
        if self.current_table.is_some() {
            return Err(PluginError::unsupported_feature(
                "nested HWPML tables are not supported yet",
            ));
        }
        if self.char_depth.is_some() {
            return Err(PluginError::unsupported_feature(
                "HWPML TABLE nested inside CHAR is not supported",
            ));
        }
        self.tables.consume(1)?;
        let rows = attr_usize_ci(element, "RowCount")?.unwrap_or(0);
        let cols = attr_usize_ci(element, "ColCount")?.unwrap_or(0);
        validate_table_dimensions(rows, cols)?;

        let before = self
            .current_paragraph
            .take()
            .expect("table is inside paragraph");
        let trailing_style = before.style.clone();
        if paragraph_has_content(&before) {
            self.blocks.push(Block::Paragraph(before));
        }
        let paragraph_depth = self.paragraph_depth.take().expect("outer paragraph depth");
        let text_depth = self.text_depth.take().expect("table is inside TEXT");
        self.suspended_paragraph = Some(SuspendedParagraph {
            paragraph_depth,
            text_depth,
            active_text_style: self.active_text_style.clone(),
            trailing: Paragraph {
                style: trailing_style,
                inlines: Vec::new(),
            },
        });
        self.active_text_style = CharStyle::default();
        self.table_depth = Some(self.depth);
        self.current_table = Some(Table {
            rows,
            cols,
            col_widths_twip: Vec::new(),
            cells: Vec::new(),
        });
        Ok(())
    }

    fn start_table_body(&mut self, name: &str, element: &BytesStart<'_>) -> Result<()> {
        if name.eq_ignore_ascii_case("TABLE") {
            return Err(PluginError::unsupported_feature(
                "nested HWPML tables are not supported yet",
            ));
        }
        if name.eq_ignore_ascii_case("CELL") {
            if self.current_cell.is_some() {
                return Err(PluginError::corrupt("nested HWPML CELL elements"));
            }
            self.table_cells.consume(1)?;
            let row = attr_usize_ci(element, "RowAddr")?.unwrap_or(0);
            let col = attr_usize_ci(element, "ColAddr")?.unwrap_or(0);
            let row_span = attr_usize_ci(element, "RowSpan")?.unwrap_or(1).max(1);
            let col_span = attr_usize_ci(element, "ColSpan")?.unwrap_or(1).max(1);
            let width_twip = attr_ci(element, "Width")?
                .as_deref()
                .and_then(parse_hwpunit)
                .map(hwpunit_to_twip);
            self.current_cell = Some(Cell {
                row,
                col,
                row_span,
                col_span,
                width_twip,
                fill: None,
                blocks: Vec::new(),
            });
            self.cell_depth = Some(self.depth);
            return Ok(());
        }
        if name.eq_ignore_ascii_case("P")
            && self.current_cell.is_some()
            && self.current_paragraph.is_none()
        {
            return self.start_paragraph(element);
        }
        if self.current_paragraph.is_some() {
            if name.eq_ignore_ascii_case("TEXT") && self.text_depth.is_none() {
                return self.start_text(element);
            }
            if self.text_depth.is_some() {
                return self.start_inline_content(name);
            }
            if is_safe_ignored_control(name) {
                self.skip_depth = Some(self.depth);
                return Ok(());
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML table-cell control <{name}> is not supported yet; refusing to drop its content"
            )));
        }
        if is_safe_table_structure(name) {
            return Ok(());
        }
        Err(PluginError::unsupported_feature(format!(
            "HWPML table element <{name}> is not supported yet; refusing to drop its content"
        )))
    }

    fn end(&mut self, raw_name: &[u8]) -> Result<()> {
        if self.depth == 0 {
            return Err(PluginError::corrupt("unexpected closing XML element"));
        }
        let name = local_name(raw_name);

        if let Some(skip_depth) = self.skip_depth {
            if self.depth == skip_depth {
                self.skip_depth = None;
            }
            self.depth -= 1;
            return Ok(());
        }

        if self.head_depth.is_some() {
            if self
                .current_char_style
                .as_ref()
                .is_some_and(|(depth, _, _)| *depth == self.depth)
                && name.eq_ignore_ascii_case("CHARSHAPE")
            {
                let (_, id, style) = self.current_char_style.take().expect("checked");
                self.styles.consume(1)?;
                self.char_styles.insert(id, style);
            } else if self
                .current_para_style
                .as_ref()
                .is_some_and(|(depth, _, _)| *depth == self.depth)
                && name.eq_ignore_ascii_case("PARASHAPE")
            {
                let (_, id, style) = self.current_para_style.take().expect("checked");
                self.styles.consume(1)?;
                self.para_styles.insert(id, style);
            }

            if self.font_face.is_some_and(|(depth, _)| depth == self.depth)
                && name.eq_ignore_ascii_case("FONTFACE")
            {
                self.font_face = None;
            }
        }

        if self.body_depth.is_some() {
            if self.char_depth == Some(self.depth) && name.eq_ignore_ascii_case("CHAR") {
                self.char_depth = None;
            } else if self.text_depth == Some(self.depth) && name.eq_ignore_ascii_case("TEXT") {
                self.text_depth = None;
                self.active_text_style = CharStyle::default();
            } else if self.paragraph_depth == Some(self.depth) && name.eq_ignore_ascii_case("P") {
                let paragraph = self.current_paragraph.take().expect("paragraph depth set");
                if let Some(cell) = self.current_cell.as_mut() {
                    cell.blocks.push(Block::Paragraph(paragraph));
                } else if !self.current_paragraph_is_table_trailing
                    || paragraph_has_content(&paragraph)
                {
                    self.blocks.push(Block::Paragraph(paragraph));
                }
                self.current_paragraph_is_table_trailing = false;
                self.paragraph_depth = None;
                self.text_depth = None;
                self.char_depth = None;
                self.active_text_style = CharStyle::default();
            } else if self.cell_depth == Some(self.depth) && name.eq_ignore_ascii_case("CELL") {
                if self.current_paragraph.is_some() {
                    return Err(PluginError::corrupt(
                        "HWPML CELL closed before its paragraph",
                    ));
                }
                let cell = self.current_cell.take().expect("cell depth set");
                self.current_table
                    .as_mut()
                    .expect("cell belongs to table")
                    .cells
                    .push(cell);
                self.cell_depth = None;
            } else if self.table_depth == Some(self.depth) && name.eq_ignore_ascii_case("TABLE") {
                self.finish_table()?;
            }
        }

        if self.head_depth == Some(self.depth) && name.eq_ignore_ascii_case("HEAD") {
            self.head_depth = None;
        }
        if self.body_depth == Some(self.depth) && name.eq_ignore_ascii_case("BODY") {
            self.body_depth = None;
        }
        if self.depth == 1 && name.eq_ignore_ascii_case("HWPML") {
            self.root_closed = true;
        }

        self.depth -= 1;
        Ok(())
    }

    fn finish_table(&mut self) -> Result<()> {
        if self.current_cell.is_some() || self.current_paragraph.is_some() {
            return Err(PluginError::corrupt(
                "HWPML TABLE closed before its cell content",
            ));
        }
        let mut table = self.current_table.take().expect("table depth set");
        let inferred_rows = table
            .cells
            .iter()
            .map(|cell| cell.row.saturating_add(cell.row_span.max(1)))
            .max()
            .unwrap_or(0);
        let inferred_cols = table
            .cells
            .iter()
            .map(|cell| cell.col.saturating_add(cell.col_span.max(1)))
            .max()
            .unwrap_or(0);
        table.rows = table.rows.max(inferred_rows);
        table.cols = table.cols.max(inferred_cols);
        validate_table_dimensions(table.rows, table.cols)?;
        for cell in &table.cells {
            if cell.row.saturating_add(cell.row_span) > table.rows
                || cell.col.saturating_add(cell.col_span) > table.cols
            {
                return Err(PluginError::corrupt(format!(
                    "HWPML CELL ({},{}) span lies outside the declared table grid",
                    cell.row, cell.col
                )));
            }
        }
        table.col_widths_twip = derive_col_widths(&table.cells, table.cols);
        self.blocks.push(Block::Table(table));
        self.table_depth = None;

        let suspended = self
            .suspended_paragraph
            .take()
            .expect("table suspended an outer paragraph");
        self.paragraph_depth = Some(suspended.paragraph_depth);
        self.text_depth = Some(suspended.text_depth);
        self.active_text_style = suspended.active_text_style;
        self.current_paragraph = Some(suspended.trailing);
        self.current_paragraph_is_table_trailing = true;
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<()> {
        if text.len() > MAX_TEXT_NODE_BYTES {
            return Err(limit_error(format!(
                "HWPML text node has {} bytes (maximum {MAX_TEXT_NODE_BYTES})",
                text.len()
            )));
        }
        if self.root_closed && !text.trim().is_empty() {
            return Err(PluginError::corrupt(
                "non-whitespace content appears after the HWPML root element",
            ));
        }
        if self.skip_depth.is_none()
            && self.body_depth.is_some()
            && self.current_paragraph.is_some()
            && self.text_depth.is_some()
        {
            if self.char_depth.is_none() && text.trim().is_empty() {
                return Ok(());
            }
            self.push_text(text)?;
        }
        Ok(())
    }

    fn push_text(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.text_bytes
            .consume(u64::try_from(text.len()).unwrap_or(u64::MAX))?;
        let paragraph = self.current_paragraph.as_mut().expect("inside paragraph");
        if let Some(Inline::Text(previous)) = paragraph.inlines.last_mut() {
            if previous.style == self.active_text_style {
                previous.text.push_str(text);
                return Ok(());
            }
        }
        self.inlines.consume(1)?;
        paragraph.inlines.push(Inline::Text(TextRun {
            text: text.to_string(),
            style: self.active_text_style.clone(),
        }));
        Ok(())
    }

    fn push_inline(&mut self, inline: Inline) -> Result<()> {
        self.inlines.consume(1)?;
        self.current_paragraph
            .as_mut()
            .expect("inside paragraph")
            .inlines
            .push(inline);
        Ok(())
    }

    fn resolve_font(&self, id: &str) -> Option<String> {
        self.hangul_fonts
            .get(id)
            .or_else(|| self.fallback_fonts.get(id))
            .cloned()
            .or_else(|| {
                (!id.chars().all(|character| character.is_ascii_digit())).then(|| id.to_string())
            })
    }

    fn finish(self) -> Result<Document> {
        if !self.root_seen {
            return Err(PluginError::corrupt("XML input has no HWPML root element"));
        }
        if !self.root_closed || self.depth != 0 {
            return Err(PluginError::corrupt(
                "malformed xml: HWPML document ended before all elements were closed",
            ));
        }
        if !self.body_seen {
            return Err(PluginError::corrupt(
                "HWPML document does not contain a BODY element",
            ));
        }
        if self.current_table.is_some()
            || self.suspended_paragraph.is_some()
            || self.char_depth.is_some()
        {
            return Err(PluginError::corrupt(
                "malformed xml: HWPML table was not closed",
            ));
        }
        Ok(Document {
            blocks: self.blocks,
        })
    }
}

fn attr_ci(element: &BytesStart<'_>, wanted: &str) -> Result<Option<String>> {
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute?;
        if local_name(attribute.key.as_ref()).eq_ignore_ascii_case(wanted) {
            return Ok(Some(
                attribute
                    .normalized_value(quick_xml::XmlVersion::Explicit1_0)?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn attr_usize_ci(element: &BytesStart<'_>, wanted: &str) -> Result<Option<usize>> {
    Ok(attr_ci(element, wanted)?.and_then(|raw| raw.trim().parse().ok()))
}

fn validate_attributes(element: &BytesStart<'_>) -> Result<()> {
    let mut count = 0usize;
    for attribute in element.attributes().with_checks(false) {
        attribute?;
        count += 1;
        if count > MAX_ATTRIBUTES_PER_ELEMENT {
            return Err(limit_error(format!(
                "XML element has more than {MAX_ATTRIBUTES_PER_ELEMENT} attributes"
            )));
        }
    }
    Ok(())
}

fn validate_table_dimensions(rows: usize, cols: usize) -> Result<()> {
    if rows > MAX_TABLE_ROWS || cols > MAX_TABLE_COLS {
        return Err(limit_error(format!(
            "HWPML table dimensions {rows}x{cols} exceed maximum {MAX_TABLE_ROWS}x{MAX_TABLE_COLS}"
        )));
    }
    let slots = rows
        .checked_mul(cols)
        .ok_or_else(|| limit_error("HWPML table grid size overflow"))?;
    if slots > MAX_TABLE_GRID_SLOTS {
        return Err(limit_error(format!(
            "HWPML table grid has {slots} slots (maximum {MAX_TABLE_GRID_SLOTS})"
        )));
    }
    Ok(())
}

fn paragraph_has_content(paragraph: &Paragraph) -> bool {
    paragraph.inlines.iter().any(|inline| match inline {
        Inline::Text(run) => !run.text.trim().is_empty(),
        Inline::Tab
        | Inline::LineBreak
        | Inline::Image(_)
        | Inline::CheckBox(_)
        | Inline::TextField(_) => true,
    })
}

fn is_safe_ignored_control(name: &str) -> bool {
    [
        "SECDEF",
        "COLDEF",
        "TITLEMARK",
        "MARKPENBEGIN",
        "MARKPENEND",
    ]
    .iter()
    .any(|safe| name.eq_ignore_ascii_case(safe))
}

fn is_safe_table_structure(name: &str) -> bool {
    [
        "ROW",
        "SHAPEOBJECT",
        "SIZE",
        "POSITION",
        "OUTSIDEMARGIN",
        "INSIDEMARGIN",
        "PARALIST",
        "CELLMARGIN",
    ]
    .iter()
    .any(|safe| name.eq_ignore_ascii_case(safe))
}

fn parse_number(raw: &str) -> Option<i64> {
    raw.trim().parse::<i64>().ok().or_else(|| {
        raw.trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| value.round() as i64)
    })
}

/// Character-unit values (`12ch`) have no stable twip mapping without the
/// resolved font metrics, so they are deliberately left unset.
fn parse_hwpunit(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.to_ascii_lowercase().ends_with("ch") {
        None
    } else {
        parse_number(raw)
    }
}

fn hwpml_color(raw: String) -> Option<String> {
    normalize_color(raw)
}

fn hwpml_shade_color(raw: String) -> Option<String> {
    let value = raw.trim();
    if value == "4294967295" || value.eq_ignore_ascii_case("FFFFFFFF") || value == "#FFFFFFFF" {
        None
    } else {
        normalize_color(raw)
    }
}

fn limit_error(message: impl Into<String>) -> PluginError {
    PluginError::corrupt(format!("resource limit exceeded: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn ignores_character_unit_margins_without_guessing_font_metrics() {
        assert_eq!(parse_hwpunit("12ch"), None);
        assert_eq!(parse_hwpunit("-500"), Some(-500));
    }

    #[test]
    fn rejects_a_document_type_declaration() {
        let xml = b"<!DOCTYPE HWPML [<!ENTITY x 'value'>]><HWPML><BODY/></HWPML>";
        let error = read_document_from(Cursor::new(xml)).expect_err("DTD must fail closed");
        assert!(error.message.contains("type declarations"), "got: {error}");
    }

    #[test]
    fn rejects_excessive_xml_nesting() {
        let mut xml = String::from("<HWPML><BODY>");
        for _ in 0..MAX_XML_DEPTH {
            xml.push_str("<SECTION>");
        }
        let error = read_document_from(Cursor::new(xml.as_bytes())).expect_err("too deep");
        assert!(error.message.contains("depth"), "got: {error}");
    }
}
