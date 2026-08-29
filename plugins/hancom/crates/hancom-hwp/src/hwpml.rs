//! Legacy HWPML (`.hml`) single-XML reader.
//!
//! Hancom's public HWPML 2.8 specification (revision 1.2) defines the body as
//! `HWPML > BODY > SECTION > P > TEXT`, with character data normally wrapped
//! by `CHAR`. Text outside that documented wrapper is rejected rather than
//! guessed. Explicitly recognized metadata-only subtrees are skipped as a
//! unit, while unsupported content-bearing controls fail instead of leaking or
//! disappearing from document text.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use officecli_hancom_core::budget::ResourceBudget;
use officecli_hancom_core::model::{
    derive_col_widths, hwpunit_to_point, hwpunit_to_twip, Align, Block, Cell, CharStyle, Document,
    Inline, ParaStyle, Paragraph, Table, TextRun, VertAlign,
};
use officecli_hancom_core::xml_encoding::{bom_encoding, decode_xml, XmlEncoding};
use quick_xml::events::{BytesDecl, BytesStart, Event};
use quick_xml::Reader;

use crate::error::{PluginError, Result};
use crate::owpml::styles::normalize_color;

const MAX_HWPML_SOURCE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_HWPML_DECODED_BYTES: u64 = 100 * 1024 * 1024;
// Interoperability allowlist for the conservative common subset implemented
// below. This does not claim full grammar coverage for every listed revision.
const SUPPORTED_HWPML_VERSIONS: [&str; 4] = ["2.1", "2.8", "2.9", "2.91"];
const MAX_XML_EVENTS: u64 = 4_000_000;
const MAX_XML_DEPTH: usize = 256;
const MAX_PARAGRAPHS: u64 = 200_000;
const MAX_INLINES: u64 = 2_000_000;
const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TEXT_NODE_BYTES: usize = 8 * 1024 * 1024;
const MAX_STYLES: u64 = 65_536;
const MAX_FONTS: u64 = 65_536;
const MAX_MAPPING_ID: u32 = 1_000_000;
const MAX_ATTRIBUTES_PER_ELEMENT: usize = 256;
const MAX_ATTRIBUTE_BYTES_PER_ELEMENT: usize = 256 * 1024;
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
    reader
        .take(MAX_HWPML_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let source_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if source_len > MAX_HWPML_SOURCE_BYTES {
        return Err(limit_error(format!(
            "HWPML source has {source_len} bytes (maximum {MAX_HWPML_SOURCE_BYTES})"
        )));
    }
    let encoding_has_bom = bom_encoding(&bytes).is_some();
    let decoded = decode_xml(&bytes, MAX_HWPML_DECODED_BYTES)?;
    let mut xml = Reader::from_str(&decoded.text);
    let config = xml.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = true;

    let mut parser = HwpmlParser::default();
    let mut buf = Vec::new();
    let mut declaration_seen = false;
    let mut non_declaration_event_seen = false;
    let mut doctype_seen = false;
    loop {
        parser.events.consume(1)?;
        let event = xml.read_event_into(&mut buf)?;
        if xml.buffer_position() > MAX_HWPML_DECODED_BYTES {
            return Err(limit_error(format!(
                "decoded HWPML XML exceeds {MAX_HWPML_DECODED_BYTES} bytes"
            )));
        }
        if !matches!(&event, Event::Decl(_) | Event::Eof) {
            non_declaration_event_seen = true;
        }

        match event {
            Event::Start(element) => {
                let is_root = !parser.root_seen;
                parser.start(&element)?;
                if is_root && doctype_seen {
                    return Err(PluginError::unsupported_feature(
                        "HWPML document type declarations are not supported",
                    ));
                }
            }
            Event::Empty(element) => {
                // `expand_empty_elements` normally turns this into Start+End,
                // but handle Empty as well so parser correctness is not tied to
                // that reader setting.
                let name = element.name().as_ref().to_vec();
                let is_root = !parser.root_seen;
                parser.start(&element)?;
                if is_root && doctype_seen {
                    return Err(PluginError::unsupported_feature(
                        "HWPML document type declarations are not supported",
                    ));
                }
                parser.end(&name)?;
            }
            Event::End(element) => parser.end(element.name().as_ref())?,
            Event::Decl(declaration) => {
                if declaration_seen || non_declaration_event_seen {
                    return Err(PluginError::corrupt(
                        "XML declaration must be the first and only declaration in the HWPML document",
                    ));
                }
                declaration_seen = true;
                validate_xml_declaration(&declaration, decoded.encoding, encoding_has_bom)?;
            }
            Event::Text(text) => parser.text(&text.decode()?)?,
            Event::CData(text) => parser.text(&String::from_utf8_lossy(text.as_ref()))?,
            Event::GeneralRef(reference) => parser.text(&resolve_hwpml_entity(&reference)?)?,
            Event::DocType(_) => {
                if parser.root_seen || doctype_seen {
                    return Err(PluginError::corrupt(
                        "HWPML DOCTYPE is duplicated or appears outside the XML prolog",
                    ));
                }
                doctype_seen = true;
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
    element_stack: Vec<String>,
    root_seen: bool,
    root_closed: bool,
    head_seen: bool,
    body_seen: bool,
    tail_seen: bool,
    head_depth: Option<usize>,
    body_depth: Option<usize>,
    section_depth: Option<usize>,
    font_face: Option<(usize, FontLanguage)>,
    fonts_by_language: HashMap<(FontLanguage, u32), String>,
    char_styles: HashMap<u32, CharStyle>,
    para_styles: HashMap<u32, ParaStyle>,
    current_char_style: Option<(usize, u32, CharStyle)>,
    current_para_style: Option<(usize, u32, ParaStyle)>,
    paragraph_depth: Option<usize>,
    text_depth: Option<usize>,
    char_depth: Option<usize>,
    empty_control: Option<(usize, String)>,
    skip_depth: Option<usize>,
    table_depth: Option<usize>,
    cell_depth: Option<usize>,
    para_list_depth: Option<usize>,
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
    fonts: ResourceBudget,
    tables: ResourceBudget,
    table_cells: ResourceBudget,
}

struct SuspendedParagraph {
    paragraph_depth: usize,
    text_depth: usize,
    active_text_style: CharStyle,
    trailing: Paragraph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FontLanguage {
    Hangul,
    Latin,
    Hanja,
    Japanese,
    Other,
    Symbol,
    User,
}

impl FontLanguage {
    fn parse(raw: &str) -> Result<Self> {
        if raw.eq_ignore_ascii_case("Hangul") {
            Ok(Self::Hangul)
        } else if raw.eq_ignore_ascii_case("Latin") {
            Ok(Self::Latin)
        } else if raw.eq_ignore_ascii_case("Hanja") {
            Ok(Self::Hanja)
        } else if raw.eq_ignore_ascii_case("Japanese") {
            Ok(Self::Japanese)
        } else if raw.eq_ignore_ascii_case("Other") {
            Ok(Self::Other)
        } else if raw.eq_ignore_ascii_case("Symbol") {
            Ok(Self::Symbol)
        } else if raw.eq_ignore_ascii_case("User") {
            Ok(Self::User)
        } else {
            Err(PluginError::corrupt(format!(
                "HWPML FONTFACE has unknown Lang {raw:?}"
            )))
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Hangul => "Hangul",
            Self::Latin => "Latin",
            Self::Hanja => "Hanja",
            Self::Japanese => "Japanese",
            Self::Other => "Other",
            Self::Symbol => "Symbol",
            Self::User => "User",
        }
    }
}

impl Default for HwpmlParser {
    fn default() -> Self {
        Self {
            depth: 0,
            element_stack: Vec::new(),
            root_seen: false,
            root_closed: false,
            head_seen: false,
            body_seen: false,
            tail_seen: false,
            head_depth: None,
            body_depth: None,
            section_depth: None,
            font_face: None,
            fonts_by_language: HashMap::new(),
            char_styles: HashMap::new(),
            para_styles: HashMap::new(),
            current_char_style: None,
            current_para_style: None,
            paragraph_depth: None,
            text_depth: None,
            char_depth: None,
            empty_control: None,
            skip_depth: None,
            table_depth: None,
            cell_depth: None,
            para_list_depth: None,
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
            fonts: ResourceBudget::new("HWPML font count", MAX_FONTS),
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
        let name = exact_name(raw_name.as_ref())?;
        if name.contains(':')
            || attr_exact(element, "xmlns")?.is_some_and(|namespace| !namespace.is_empty())
        {
            return Err(PluginError::corrupt(format!(
                "namespace-qualified HWPML element <{name}> is not supported by the legacy grammar"
            )));
        }
        if let Some((_, control)) = self.empty_control.as_ref() {
            return Err(PluginError::corrupt(format!(
                "HWPML empty control <{control}> contains a child element"
            )));
        }
        let parent = self.element_stack.last().cloned();
        self.element_stack.push(name.to_string());

        if self.depth == 1 {
            if self.root_seen {
                return Err(PluginError::corrupt(
                    "HWPML XML contains more than one root element",
                ));
            }
            if name != "HWPML" {
                return Err(PluginError::corrupt(format!(
                    "XML root <{name}> is not an HWPML document"
                )));
            }
            let version = attr_exact(element, "Version")?.ok_or_else(|| {
                PluginError::corrupt("HWPML root is missing the required Version attribute")
            })?;
            if !SUPPORTED_HWPML_VERSIONS.contains(&version.as_str()) {
                return Err(PluginError::unsupported_feature(format!(
                    "HWPML version {version:?} is outside the supported common-subset allowlist ({})",
                    SUPPORTED_HWPML_VERSIONS.join(", ")
                )));
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

        if self.depth == 2 && name == "HEAD" {
            if self.head_seen {
                return Err(PluginError::corrupt(
                    "HWPML document contains more than one HEAD element",
                ));
            }
            if self.body_seen || self.tail_seen {
                return Err(PluginError::corrupt(
                    "HWPML HEAD element appears out of document order",
                ));
            }
            self.head_seen = true;
            self.head_depth = Some(self.depth);
            return Ok(());
        }
        if self.depth == 2 && name == "BODY" {
            if self.body_seen {
                return Err(PluginError::corrupt(
                    "HWPML document contains more than one BODY element",
                ));
            }
            if self.tail_seen {
                return Err(PluginError::corrupt(
                    "HWPML BODY element appears out of document order",
                ));
            }
            self.body_seen = true;
            self.body_depth = Some(self.depth);
            return Ok(());
        }
        if self.depth == 2 && name == "TAIL" {
            if self.tail_seen {
                return Err(PluginError::corrupt(
                    "HWPML document contains more than one TAIL element",
                ));
            }
            if !self.body_seen {
                return Err(PluginError::corrupt(
                    "HWPML TAIL element appears out of document order",
                ));
            }
            self.tail_seen = true;
            self.skip_depth = Some(self.depth);
            return Ok(());
        }
        if self.depth == 2 {
            return Err(PluginError::unsupported_feature(format!(
                "HWPML root element <{name}> is not supported; refusing to drop its content"
            )));
        }

        if self.head_depth.is_some() {
            self.start_head(name, parent.as_deref(), element)?;
        } else if self.body_depth.is_some() {
            self.start_body(name, parent.as_deref(), element)?;
        }
        Ok(())
    }

    fn start_head(
        &mut self,
        name: &str,
        parent: Option<&str>,
        element: &BytesStart<'_>,
    ) -> Result<()> {
        let expected_parent = match name {
            "MAPPINGTABLE" => Some("HEAD"),
            "FACENAMELIST" | "CHARSHAPELIST" | "PARASHAPELIST" => Some("MAPPINGTABLE"),
            "FONTFACE" => Some("FACENAMELIST"),
            "FONT" => Some("FONTFACE"),
            "CHARSHAPE" => Some("CHARSHAPELIST"),
            "PARASHAPE" => Some("PARASHAPELIST"),
            "FONTID" | "BOLD" | "ITALIC" | "UNDERLINE" | "STRIKEOUT" | "SUPERSCRIPT"
            | "SUBSCRIPT" => Some("CHARSHAPE"),
            "PARAMARGIN" => Some("PARASHAPE"),
            _ => None,
        };
        if expected_parent.is_some_and(|expected| parent != Some(expected)) {
            return Err(PluginError::corrupt(format!(
                "HWPML mapping element <{name}> appears under the wrong parent"
            )));
        }
        if name == "FONTFACE" {
            let language = attr_exact(element, "Lang")?
                .ok_or_else(|| PluginError::corrupt("HWPML FONTFACE is missing Lang"))?;
            self.font_face = Some((self.depth, FontLanguage::parse(&language)?));
        } else if name == "FONT" {
            let (_, language) = self
                .font_face
                .ok_or_else(|| PluginError::corrupt("HWPML FONT appears outside FONTFACE"))?;
            let id = required_mapping_id(element, "Id", "FONT")?;
            let font_name = attr_exact(element, "Name")?
                .ok_or_else(|| PluginError::corrupt("HWPML FONT is missing Name"))?;
            self.fonts.consume(1)?;
            if self
                .fonts_by_language
                .insert((language, id), font_name)
                .is_some()
            {
                return Err(PluginError::corrupt(format!(
                    "duplicate HWPML FONT Id {id} in {} FONTFACE",
                    language.label()
                )));
            }
        } else if name == "CHARSHAPE" {
            let id = required_mapping_id(element, "Id", "CHARSHAPE")?;
            let mut style = CharStyle::default();
            if let Some(height) = attr_exact(element, "Height")?.and_then(|raw| parse_number(&raw))
            {
                style.size_pt = Some(hwpunit_to_point(height));
            }
            if let Some(color) = attr_exact(element, "TextColor")?.and_then(hwpml_color) {
                style.color = Some(color);
            }
            if let Some(color) = attr_exact(element, "ShadeColor")?.and_then(hwpml_shade_color) {
                style.highlight = Some(color);
            }
            self.current_char_style = Some((self.depth, id, style));
        } else if name == "PARASHAPE" {
            let id = required_mapping_id(element, "Id", "PARASHAPE")?;
            let style = ParaStyle {
                align: attr_exact(element, "Align")?
                    .as_deref()
                    .and_then(Align::from_owpml),
                ..ParaStyle::default()
            };
            self.current_para_style = Some((self.depth, id, style));
        } else if name == "FONTID" && self.current_char_style.is_some() {
            let font = self.resolve_font_id(element)?;
            if let Some((_, _, style)) = self.current_char_style.as_mut() {
                style.font = font;
            }
        } else if let Some((_, _, style)) = self.current_char_style.as_mut() {
            if name == "BOLD" {
                style.bold = true;
            } else if name == "ITALIC" {
                style.italic = true;
            } else if name == "UNDERLINE" {
                style.underline = !attr_exact(element, "Type")?
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("None"));
            } else if name == "STRIKEOUT" {
                style.strike = !attr_exact(element, "Type")?
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("None"));
            } else if name == "SUPERSCRIPT" {
                if matches!(style.vert_align, Some(VertAlign::Subscript)) {
                    return Err(PluginError::corrupt(
                        "HWPML CHARSHAPE cannot contain both SUPERSCRIPT and SUBSCRIPT",
                    ));
                }
                style.vert_align = Some(VertAlign::Superscript);
            } else if name == "SUBSCRIPT" {
                if matches!(style.vert_align, Some(VertAlign::Superscript)) {
                    return Err(PluginError::corrupt(
                        "HWPML CHARSHAPE cannot contain both SUPERSCRIPT and SUBSCRIPT",
                    ));
                }
                style.vert_align = Some(VertAlign::Subscript);
            }
        } else if let Some((_, _, style)) = self.current_para_style.as_mut() {
            if name == "PARAMARGIN" {
                if let Some(value) = attr_exact(element, "Indent")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                {
                    style.set_first_line_indent(hwpunit_to_twip(value));
                }
                style.indent_left_twip = attr_exact(element, "Left")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                    .map(hwpunit_to_twip);
                style.space_before_twip = attr_exact(element, "Prev")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                    .map(hwpunit_to_twip);
                style.space_after_twip = attr_exact(element, "Next")?
                    .as_deref()
                    .and_then(parse_hwpunit)
                    .map(hwpunit_to_twip);

                let spacing_type = attr_exact(element, "LineSpacingType")?
                    .unwrap_or_else(|| "Percent".to_string());
                if spacing_type.eq_ignore_ascii_case("Percent") {
                    style.line_spacing_ratio = attr_exact(element, "LineSpacing")?
                        .as_deref()
                        .and_then(parse_number)
                        .filter(|value| *value > 0)
                        .map(|value| value as f64 / 100.0);
                }
            }
        }
        Ok(())
    }

    fn start_body(
        &mut self,
        name: &str,
        parent: Option<&str>,
        element: &BytesStart<'_>,
    ) -> Result<()> {
        if self.current_table.is_some() {
            return self.start_table_body(name, parent, element);
        }

        if self.current_paragraph.is_none() {
            if name == "SECTION" {
                if self.section_depth.is_some() {
                    return Err(PluginError::corrupt("nested HWPML SECTION elements"));
                }
                let expected_depth = self.body_depth.ok_or_else(|| {
                    parser_state_error("BODY depth is missing during SECTION start")
                })? + 1;
                if self.depth != expected_depth {
                    return Err(PluginError::unsupported_feature(
                        "HWPML SECTION must be a direct child of BODY",
                    ));
                }
                self.section_depth = Some(self.depth);
                return Ok(());
            }
            if name == "P" {
                let expected_depth = self.section_depth.ok_or_else(|| {
                    PluginError::unsupported_feature(
                        "HWPML paragraph outside the documented BODY/SECTION path",
                    )
                })? + 1;
                if self.depth != expected_depth {
                    return Err(PluginError::unsupported_feature(
                        "HWPML paragraph must be a direct child of SECTION",
                    ));
                }
                return self.start_paragraph(element);
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML BODY element <{name}> is not supported yet; refusing to drop its content"
            )));
        }

        if name == "TEXT" && self.text_depth.is_none() {
            return self.start_text(element);
        }

        if self.text_depth.is_none() {
            if is_text_control(name) || is_char_control(name) {
                return Err(PluginError::corrupt(format!(
                    "HWPML control <{name}> appears under the wrong parent"
                )));
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML control <{name}> beside TEXT is not supported yet; refusing to drop its content"
            )));
        }

        if name == "TABLE" {
            if parent != Some("TEXT") {
                return Err(PluginError::corrupt(
                    "HWPML TABLE appears under the wrong parent; expected TEXT",
                ));
            }
            return self.start_table(element);
        }
        self.start_inline_content(name)
    }

    fn start_paragraph(&mut self, element: &BytesStart<'_>) -> Result<()> {
        self.paragraphs.consume(1)?;
        let style = match optional_mapping_id(element, "ParaShape", "P")? {
            Some(id) => self.para_styles.get(&id).cloned().ok_or_else(|| {
                PluginError::corrupt(format!("HWPML P has dangling ParaShape reference {id}"))
            })?,
            None => ParaStyle::default(),
        };
        self.current_paragraph = Some(Paragraph {
            style,
            inlines: Vec::new(),
        });
        self.current_paragraph_is_table_trailing = false;
        self.paragraph_depth = Some(self.depth);
        Ok(())
    }

    fn start_text(&mut self, element: &BytesStart<'_>) -> Result<()> {
        self.active_text_style = match optional_mapping_id(element, "CharShape", "TEXT")? {
            Some(id) => self.char_styles.get(&id).cloned().ok_or_else(|| {
                PluginError::corrupt(format!("HWPML TEXT has dangling CharShape reference {id}"))
            })?,
            None => CharStyle::default(),
        };
        self.text_depth = Some(self.depth);
        Ok(())
    }

    fn start_inline_content(&mut self, name: &str) -> Result<()> {
        if name == "CHAR" {
            if self.char_depth.is_some() {
                return Err(PluginError::corrupt("nested HWPML CHAR elements"));
            }
            self.char_depth = Some(self.depth);
            return Ok(());
        }
        if self.char_depth.is_none() {
            if is_text_control(name) {
                // Section/column definitions are documented TEXT children.
                self.skip_depth = Some(self.depth);
                return Ok(());
            }
            if is_char_control(name) {
                return Err(PluginError::corrupt(format!(
                    "HWPML control <{name}> appears under the wrong parent; expected CHAR"
                )));
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML control <{name}> appears outside the documented CHAR wrapper"
            )));
        }
        if is_text_control(name) {
            return Err(PluginError::corrupt(format!(
                "HWPML control <{name}> appears under the wrong parent; expected TEXT"
            )));
        }
        if is_char_marker(name) {
            self.empty_control = Some((self.depth, name.to_string()));
            return Ok(());
        }
        if matches!(name, "HYPEN" | "FWSPACE") {
            return Err(PluginError::unsupported_feature(format!(
                "HWPML control <{name}> has no unambiguous Unicode projection"
            )));
        }
        if name == "TAB" {
            self.push_inline(Inline::Tab)?;
            self.empty_control = Some((self.depth, name.to_string()));
        } else if name == "LINEBREAK" {
            self.push_inline(Inline::LineBreak)?;
            self.empty_control = Some((self.depth, name.to_string()));
        } else if name == "NBSPACE" {
            // The public HWPML grammar defines this as a word-joining blank.
            // U+00A0 preserves that semantic in the plain-text document model;
            // controls without an equally direct projection fail above.
            self.push_text("\u{00A0}")?;
            self.empty_control = Some((self.depth, name.to_string()));
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
        let rows = attr_usize_exact(element, "RowCount")?.unwrap_or(0);
        let cols = attr_usize_exact(element, "ColCount")?.unwrap_or(0);
        validate_table_dimensions(rows, cols)?;

        let before = self
            .current_paragraph
            .take()
            .ok_or_else(|| parser_state_error("TABLE has no containing paragraph"))?;
        let trailing_style = before.style.clone();
        if paragraph_has_content(&before) {
            self.blocks.push(Block::Paragraph(before));
        }
        let paragraph_depth = self
            .paragraph_depth
            .take()
            .ok_or_else(|| parser_state_error("TABLE has no containing paragraph depth"))?;
        let text_depth = self
            .text_depth
            .take()
            .ok_or_else(|| parser_state_error("TABLE has no containing TEXT depth"))?;
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

    fn start_table_body(
        &mut self,
        name: &str,
        parent: Option<&str>,
        element: &BytesStart<'_>,
    ) -> Result<()> {
        if name == "TABLE" {
            return Err(PluginError::unsupported_feature(
                "nested HWPML tables are not supported yet",
            ));
        }
        if let Some(expected) = table_structure_parent(name) {
            if parent != Some(expected) {
                return Err(PluginError::corrupt(format!(
                    "HWPML table element <{name}> appears under the wrong parent; expected {expected}"
                )));
            }
        }
        if name == "CELL" {
            if self.current_cell.is_some() {
                return Err(PluginError::corrupt("nested HWPML CELL elements"));
            }
            self.table_cells.consume(1)?;
            let row = attr_usize_exact(element, "RowAddr")?.unwrap_or(0);
            let col = attr_usize_exact(element, "ColAddr")?.unwrap_or(0);
            let row_span = attr_usize_exact(element, "RowSpan")?.unwrap_or(1);
            let col_span = attr_usize_exact(element, "ColSpan")?.unwrap_or(1);
            if row_span == 0 || col_span == 0 {
                return Err(PluginError::corrupt(
                    "HWPML CELL RowSpan and ColSpan must be positive integers",
                ));
            }
            let row_end = row
                .checked_add(row_span)
                .ok_or_else(|| limit_error("HWPML CELL row coordinate overflow"))?;
            let col_end = col
                .checked_add(col_span)
                .ok_or_else(|| limit_error("HWPML CELL column coordinate overflow"))?;
            let table = self
                .current_table
                .as_ref()
                .ok_or_else(|| parser_state_error("CELL started without an active TABLE"))?;
            if (table.rows != 0 && row_end > table.rows)
                || (table.cols != 0 && col_end > table.cols)
            {
                return Err(PluginError::corrupt(format!(
                    "HWPML CELL ({row},{col}) span lies outside the declared {}x{} table grid",
                    table.rows, table.cols
                )));
            }
            let width_twip = attr_exact(element, "Width")?
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
        if name == "PARALIST" && self.current_paragraph.is_none() {
            let cell_depth = self
                .cell_depth
                .ok_or_else(|| PluginError::corrupt("HWPML PARALIST appears outside CELL"))?;
            if self.para_list_depth.is_some() || self.depth != cell_depth + 1 {
                return Err(PluginError::corrupt(
                    "HWPML PARALIST must be a direct, non-nested child of CELL",
                ));
            }
            self.para_list_depth = Some(self.depth);
            return Ok(());
        }
        if name == "P" && self.current_paragraph.is_none() {
            let para_list_depth = self.para_list_depth.ok_or_else(|| {
                PluginError::corrupt("HWPML table-cell paragraph must be inside PARALIST")
            })?;
            if self.current_cell.is_none() || self.depth != para_list_depth + 1 {
                return Err(PluginError::corrupt(
                    "HWPML table-cell paragraph must be a direct child of PARALIST",
                ));
            }
            return self.start_paragraph(element);
        }
        if self.current_paragraph.is_some() {
            if name == "TEXT" && self.text_depth.is_none() {
                return self.start_text(element);
            }
            if self.text_depth.is_some() {
                return self.start_inline_content(name);
            }
            if is_text_control(name) || is_char_control(name) {
                return Err(PluginError::corrupt(format!(
                    "HWPML control <{name}> appears under the wrong parent"
                )));
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML table-cell control <{name}> is not supported yet; refusing to drop its content"
            )));
        }
        if table_structure_parent(name).is_some() {
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
        let name = exact_name(raw_name)?;
        let opened = self
            .element_stack
            .pop()
            .ok_or_else(|| PluginError::corrupt("HWPML element stack underflow"))?;
        if opened != name {
            return Err(PluginError::corrupt(format!(
                "HWPML element <{opened}> closed as <{name}>"
            )));
        }

        if let Some((control_depth, control)) = self.empty_control.as_ref() {
            if *control_depth == self.depth {
                if control != name {
                    return Err(PluginError::corrupt(format!(
                        "HWPML empty control <{control}> closed as <{name}>"
                    )));
                }
                self.empty_control = None;
            }
        }

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
                && name == "CHARSHAPE"
            {
                let (_, id, style) = self
                    .current_char_style
                    .take()
                    .ok_or_else(|| parser_state_error("CHARSHAPE state disappeared at close"))?;
                self.styles.consume(1)?;
                if self.char_styles.contains_key(&id) {
                    return Err(PluginError::corrupt(format!(
                        "duplicate HWPML CHARSHAPE Id {id:?}"
                    )));
                }
                self.char_styles.insert(id, style);
            } else if self
                .current_para_style
                .as_ref()
                .is_some_and(|(depth, _, _)| *depth == self.depth)
                && name == "PARASHAPE"
            {
                let (_, id, style) = self
                    .current_para_style
                    .take()
                    .ok_or_else(|| parser_state_error("PARASHAPE state disappeared at close"))?;
                self.styles.consume(1)?;
                if self.para_styles.contains_key(&id) {
                    return Err(PluginError::corrupt(format!(
                        "duplicate HWPML PARASHAPE Id {id:?}"
                    )));
                }
                self.para_styles.insert(id, style);
            }

            if self.font_face.is_some_and(|(depth, _)| depth == self.depth) && name == "FONTFACE" {
                self.font_face = None;
            }
        }

        if self.body_depth.is_some() {
            if self.char_depth == Some(self.depth) && name == "CHAR" {
                self.char_depth = None;
            } else if self.text_depth == Some(self.depth) && name == "TEXT" {
                self.text_depth = None;
                self.active_text_style = CharStyle::default();
            } else if self.paragraph_depth == Some(self.depth) && name == "P" {
                let paragraph = self
                    .current_paragraph
                    .take()
                    .ok_or_else(|| parser_state_error("paragraph state disappeared at close"))?;
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
            } else if self.cell_depth == Some(self.depth) && name == "CELL" {
                if self.current_paragraph.is_some() {
                    return Err(PluginError::corrupt(
                        "HWPML CELL closed before its paragraph",
                    ));
                }
                let cell = self
                    .current_cell
                    .take()
                    .ok_or_else(|| parser_state_error("CELL state disappeared at close"))?;
                self.current_table
                    .as_mut()
                    .ok_or_else(|| parser_state_error("CELL closed without an active TABLE"))?
                    .cells
                    .push(cell);
                self.cell_depth = None;
            } else if self.table_depth == Some(self.depth) && name == "TABLE" {
                self.finish_table()?;
            }
        }

        if self.para_list_depth == Some(self.depth) && name == "PARALIST" {
            if self.current_paragraph.is_some() {
                return Err(PluginError::corrupt(
                    "HWPML PARALIST closed before its paragraph",
                ));
            }
            self.para_list_depth = None;
        }

        if self.head_depth == Some(self.depth) && name == "HEAD" {
            self.head_depth = None;
        }
        if self.body_depth == Some(self.depth) && name == "BODY" {
            self.body_depth = None;
        }
        if self.section_depth == Some(self.depth) && name == "SECTION" {
            self.section_depth = None;
        }
        if self.depth == 1 && name == "HWPML" {
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
        let mut table = self
            .current_table
            .take()
            .ok_or_else(|| parser_state_error("TABLE state disappeared at close"))?;
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
            .ok_or_else(|| parser_state_error("TABLE has no suspended outer paragraph"))?;
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
        if !self.root_seen && !text.trim().is_empty() {
            return Err(PluginError::corrupt(
                "non-whitespace content appears before the HWPML root element",
            ));
        }
        if self.root_closed && !text.trim().is_empty() {
            return Err(PluginError::corrupt(
                "non-whitespace content appears after the HWPML root element",
            ));
        }
        if let Some((_, control)) = self.empty_control.as_ref() {
            if !text.is_empty() {
                return Err(PluginError::corrupt(format!(
                    "HWPML empty control <{control}> contains character data"
                )));
            }
            return Ok(());
        }
        if self.skip_depth.is_none() && self.body_depth.is_some() {
            if self.current_paragraph.is_some()
                && self.text_depth.is_some()
                && self.char_depth.is_some()
            {
                return self.push_text(text);
            }
            if text.trim().is_empty() {
                return Ok(());
            }
            if self.current_paragraph.is_none() {
                return Err(PluginError::unsupported_feature(
                    "HWPML character data outside the documented P element is not supported",
                ));
            }
            if self.text_depth.is_none() {
                return Err(PluginError::unsupported_feature(
                    "HWPML character data outside the documented TEXT element is not supported",
                ));
            }
            return Err(PluginError::unsupported_feature(
                "HWPML character data outside the documented CHAR wrapper is not supported",
            ));
        }
        Ok(())
    }

    fn push_text(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.text_bytes
            .consume(u64::try_from(text.len()).unwrap_or(u64::MAX))?;
        let paragraph = self
            .current_paragraph
            .as_mut()
            .ok_or_else(|| parser_state_error("text emitted without an active paragraph"))?;
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
            .ok_or_else(|| parser_state_error("inline emitted without an active paragraph"))?
            .inlines
            .push(inline);
        Ok(())
    }

    fn resolve_font_id(&self, element: &BytesStart<'_>) -> Result<Option<String>> {
        let attributes = [
            ("Hangul", FontLanguage::Hangul),
            ("Latin", FontLanguage::Latin),
            ("Hanja", FontLanguage::Hanja),
            ("Japanese", FontLanguage::Japanese),
            ("Other", FontLanguage::Other),
            ("Symbol", FontLanguage::Symbol),
            ("User", FontLanguage::User),
        ];
        let mut has_reference = false;
        let mut resolved = None;
        for (attribute, language) in attributes {
            if let Some(id) = optional_mapping_id(element, attribute, "FONTID")? {
                has_reference = true;
                if resolved.is_none() {
                    resolved = self.fonts_by_language.get(&(language, id)).cloned();
                }
            }
        }
        if has_reference && resolved.is_none() {
            return Err(PluginError::corrupt(
                "HWPML FONTID contains only dangling font references",
            ));
        }
        Ok(resolved)
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
        if !self.element_stack.is_empty() {
            return Err(PluginError::corrupt(
                "malformed xml: HWPML element stack is not empty at EOF",
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
            || self.empty_control.is_some()
            || self.para_list_depth.is_some()
        {
            return Err(PluginError::corrupt(
                "malformed xml: HWPML table was not closed",
            ));
        }
        Ok(Document::from_blocks(self.blocks))
    }
}

fn exact_name(raw: &[u8]) -> Result<&str> {
    std::str::from_utf8(raw).map_err(|error| {
        PluginError::corrupt(format!(
            "HWPML XML element name is not valid UTF-8: {error}"
        ))
    })
}

fn resolve_hwpml_entity(reference: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    match reference.resolve_char_ref() {
        Ok(Some(character)) => return Ok(character.to_string()),
        Ok(None) => {}
        Err(error) => {
            return Err(PluginError::corrupt(format!(
                "invalid XML character reference: {error}"
            )));
        }
    }
    let name = reference.decode()?;
    quick_xml::escape::resolve_predefined_entity(&name)
        .map(str::to_string)
        .ok_or_else(|| PluginError::corrupt(format!("undeclared XML entity reference &{name};")))
}

fn attr_exact(element: &BytesStart<'_>, wanted: &str) -> Result<Option<String>> {
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute?;
        if attribute.key.as_ref() == wanted.as_bytes() {
            return Ok(Some(
                attribute
                    .normalized_value(quick_xml::XmlVersion::Explicit1_0)?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn attr_usize_exact(element: &BytesStart<'_>, wanted: &str) -> Result<Option<usize>> {
    attr_exact(element, wanted)?
        .map(|raw| {
            raw.trim().parse::<usize>().map_err(|_| {
                PluginError::corrupt(format!(
                    "HWPML attribute {wanted}={raw:?} is not a non-negative integer"
                ))
            })
        })
        .transpose()
}

fn required_mapping_id(element: &BytesStart<'_>, attribute: &str, owner: &str) -> Result<u32> {
    let raw = attr_exact(element, attribute)?.ok_or_else(|| {
        PluginError::corrupt(format!("HWPML {owner} is missing required {attribute} Id"))
    })?;
    parse_mapping_id(&raw, attribute, owner)
}

fn optional_mapping_id(
    element: &BytesStart<'_>,
    attribute: &str,
    owner: &str,
) -> Result<Option<u32>> {
    attr_exact(element, attribute)?
        .map(|raw| parse_mapping_id(&raw, attribute, owner))
        .transpose()
}

fn parse_mapping_id(raw: &str, attribute: &str, owner: &str) -> Result<u32> {
    let value = raw.trim().parse::<u32>().map_err(|_| {
        PluginError::corrupt(format!(
            "HWPML {owner} {attribute} Id/reference {raw:?} is not a non-negative integer"
        ))
    })?;
    if value > MAX_MAPPING_ID {
        return Err(PluginError::corrupt(format!(
            "HWPML {owner} {attribute} Id/reference {value} exceeds maximum {MAX_MAPPING_ID}"
        )));
    }
    Ok(value)
}

fn validate_attributes(element: &BytesStart<'_>) -> Result<()> {
    let mut count = 0usize;
    let mut bytes = 0usize;
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute?;
        count = count
            .checked_add(1)
            .ok_or_else(|| limit_error("HWPML XML attribute count overflow"))?;
        if count > MAX_ATTRIBUTES_PER_ELEMENT {
            return Err(limit_error(format!(
                "XML element has more than {MAX_ATTRIBUTES_PER_ELEMENT} attributes"
            )));
        }
        bytes = bytes
            .checked_add(attribute.key.as_ref().len())
            .and_then(|total| total.checked_add(attribute.value.as_ref().len()))
            .ok_or_else(|| limit_error("HWPML XML attribute byte count overflow"))?;
        if bytes > MAX_ATTRIBUTE_BYTES_PER_ELEMENT {
            return Err(limit_error(format!(
                "HWPML XML element has {bytes} attribute bytes (maximum {MAX_ATTRIBUTE_BYTES_PER_ELEMENT})"
            )));
        }
        attribute.normalized_value(quick_xml::XmlVersion::Explicit1_0)?;
    }
    Ok(())
}

fn validate_xml_declaration(
    declaration: &BytesDecl<'_>,
    actual: XmlEncoding,
    encoding_has_bom: bool,
) -> Result<()> {
    let raw = std::str::from_utf8(declaration.as_ref()).map_err(|error| {
        PluginError::corrupt(format!("XML declaration is not valid ASCII: {error}"))
    })?;
    let pseudo = BytesStart::from_content(raw, 3);
    let mut stage = 0_u8;
    let mut count = 0_usize;
    let mut bytes = 0_usize;
    let mut declared_encoding = None;

    for attribute in pseudo.attributes().with_checks(true) {
        let attribute = attribute
            .map_err(|error| PluginError::corrupt(format!("malformed XML declaration: {error}")))?;
        count = count
            .checked_add(1)
            .ok_or_else(|| limit_error("XML declaration attribute count overflow"))?;
        if count > MAX_ATTRIBUTES_PER_ELEMENT {
            return Err(limit_error(format!(
                "XML declaration has more than {MAX_ATTRIBUTES_PER_ELEMENT} pseudo-attributes"
            )));
        }
        bytes = bytes
            .checked_add(attribute.key.as_ref().len())
            .and_then(|total| total.checked_add(attribute.value.as_ref().len()))
            .ok_or_else(|| limit_error("XML declaration attribute byte count overflow"))?;
        if bytes > MAX_ATTRIBUTE_BYTES_PER_ELEMENT {
            return Err(limit_error(format!(
                "XML declaration has {bytes} pseudo-attribute bytes (maximum {MAX_ATTRIBUTE_BYTES_PER_ELEMENT})"
            )));
        }

        let key = attribute.key.as_ref();
        let next_stage = match (stage, key) {
            (0, b"version") => 1,
            (1, b"encoding") => 2,
            (1 | 2, b"standalone") => 3,
            _ => {
                return Err(PluginError::corrupt(format!(
                    "malformed XML declaration pseudo-attribute ordering near {:?}",
                    String::from_utf8_lossy(key)
                )));
            }
        };
        let value = std::str::from_utf8(attribute.value.as_ref()).map_err(|error| {
            PluginError::corrupt(format!(
                "XML declaration pseudo-attribute value is not valid ASCII: {error}"
            ))
        })?;
        match key {
            b"version" if value != "1.0" => {
                return Err(PluginError::unsupported_feature(format!(
                    "HWPML XML version {value:?} is not supported (expected 1.0)"
                )));
            }
            b"encoding" => declared_encoding = Some(value.to_string()),
            b"standalone" if !matches!(value, "yes" | "no") => {
                return Err(PluginError::corrupt(format!(
                    "malformed XML declaration standalone value {value:?}"
                )));
            }
            _ => {}
        }
        stage = next_stage;
    }
    if stage == 0 {
        return Err(PluginError::corrupt(
            "malformed XML declaration: required version pseudo-attribute is missing",
        ));
    }

    if let Some(declared) = declared_encoding {
        let normalized = declared.to_ascii_uppercase();
        let supported = matches!(
            normalized.as_str(),
            "UTF-8" | "UTF8" | "UTF-16" | "UTF-16LE" | "UTF-16BE"
        );
        if !supported {
            if encoding_has_bom {
                return Err(PluginError::corrupt(format!(
                    "XML encoding declaration {declared:?} conflicts with the BOM-selected document byte order {actual:?}"
                )));
            }
            return Err(PluginError::unsupported_feature(format!(
                "HWPML XML encoding {declared:?} is not supported"
            )));
        }
        let agrees = match actual {
            XmlEncoding::Utf8 => matches!(normalized.as_str(), "UTF-8" | "UTF8"),
            XmlEncoding::Utf16Le => matches!(normalized.as_str(), "UTF-16" | "UTF-16LE"),
            XmlEncoding::Utf16Be => matches!(normalized.as_str(), "UTF-16" | "UTF-16BE"),
        };
        if !agrees {
            return Err(PluginError::corrupt(format!(
                "XML encoding declaration {declared:?} conflicts with the document byte order {actual:?}"
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
        Inline::Text(run) => !run.text.is_empty(),
        Inline::Tab
        | Inline::LineBreak
        | Inline::Image(_)
        | Inline::CheckBox(_)
        | Inline::TextField(_)
        | Inline::PageNumber(_)
        | Inline::Note(_)
        | Inline::Equation(_)
        | Inline::Rectangle(_) => true,
    })
}

fn is_text_control(name: &str) -> bool {
    matches!(name, "SECDEF" | "COLDEF")
}

fn is_char_marker(name: &str) -> bool {
    matches!(name, "TITLEMARK" | "MARKPENBEGIN" | "MARKPENEND")
}

fn is_char_control(name: &str) -> bool {
    is_char_marker(name) || matches!(name, "TAB" | "LINEBREAK" | "HYPEN" | "NBSPACE" | "FWSPACE")
}

fn table_structure_parent(name: &str) -> Option<&'static str> {
    match name {
        "ROW" | "SHAPEOBJECT" | "INSIDEMARGIN" => Some("TABLE"),
        "CELL" => Some("ROW"),
        "SIZE" | "POSITION" | "OUTSIDEMARGIN" => Some("SHAPEOBJECT"),
        "CELLMARGIN" | "PARALIST" => Some("CELL"),
        "P" => Some("PARALIST"),
        _ => None,
    }
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

fn parser_state_error(message: impl Into<String>) -> PluginError {
    PluginError::internal(format!(
        "inconsistent HWPML parser state: {}",
        message.into()
    ))
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
        let xml = b"<!DOCTYPE HWPML [<!ENTITY x 'value'>]><HWPML Version=\"2.91\"><BODY/></HWPML>";
        let error = read_document_from(Cursor::new(xml)).expect_err("DTD must fail closed");
        assert!(error.message.contains("type declarations"), "got: {error}");
    }

    #[test]
    fn rejects_excessive_xml_nesting() {
        let mut xml = String::from("<HWPML Version=\"2.91\"><HEAD>");
        for _ in 0..MAX_XML_DEPTH {
            xml.push_str("<X>");
        }
        let error = read_document_from(Cursor::new(xml.as_bytes())).expect_err("too deep");
        assert!(error.message.contains("depth"), "got: {error}");
    }
}
