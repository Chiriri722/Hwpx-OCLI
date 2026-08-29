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
    Block, Cell, Equation, EquationMode, HeaderFooter, HeaderFooterPage, Image, Inline, Note,
    NoteKind, NoteLine, NoteLineType, NoteLineWidth, NoteNumberFormat, NoteNumberRestart,
    NotePosition, NoteProperties, NoteSpacing, PageNumberField, PageNumberKind, Paragraph,
    Rectangle, RectangleHorizontalPosition, RectangleLine, RectangleMargins, RectanglePlacement,
    RectangleText, RectangleWrap, RectangleWrapSide, Section, ShapeGeometry, Table, TextBoxAnchor,
    TextBoxDirection, TextField, TextRun,
};
use super::styles::{normalize_color, SectionStyles, StyleTable};
use super::xml::{attr, attr_i64, attr_usize, local_name, resolve_entity};
use crate::error::{PluginError, Result};

/// 셀 안에 또 표가 나오는 등 재귀가 깊어질 때의 상한. 악의적 입력 방어.
const MAX_DEPTH: usize = 32;
const MAX_TABLE_ROWS: usize = 32_768;
const MAX_TABLE_COLS: usize = 512;
const MAX_TABLE_CELLS: usize = 100_000;
const MAX_TABLE_GRID_SLOTS: usize = 1_000_000;
const UNSUPPORTED_SHAPE_ELEMENTS: &[&str] = &[
    "line",
    "arc",
    "polygon",
    "curve",
    "connectLine",
    "container",
    "textart",
    "ole",
    "video",
    // T2-7 owns semantic chart conversion. Until then, fail closed.
    "chart",
];

pub fn parse_section(xml: &str, styles: &StyleTable) -> Result<Section> {
    parse_section_with_outline(xml, styles).map(|(section, _)| section)
}

pub(crate) fn parse_section_with_outline(
    xml: &str,
    styles: &StyleTable,
) -> Result<(Section, Option<String>)> {
    let outline_id = find_section_outline_id(xml)?;
    let styles = styles.scoped(outline_id.as_deref());
    let mut section = Section::default();
    parse_section_metadata(xml, &styles, &mut section)?;
    section.blocks = parse_section_body(xml, &styles)?;
    validate_active_note_layouts(&section)?;
    Ok((section, outline_id))
}

/// `secPr` is nested in the first paragraph, while header/footer stories can
/// appear elsewhere in that paragraph. Read the section outline reference in a
/// small pre-pass so every story and nested table resolves OUTLINE headings
/// against the same section-scoped numbering definition.
fn find_section_outline_id(xml: &str) -> Result<Option<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => return Ok(None),
            Event::Start(start) if local_name(start.name().as_ref()) == "secPr" => {
                return Ok(
                    attr(&start, "outlineShapeIDRef").filter(|value| !value.trim().is_empty())
                );
            }
            _ => {}
        }
        buf.clear();
    }
}

fn validate_active_note_layouts(section: &Section) -> Result<()> {
    for (kind, properties) in [
        (NoteKind::Footnote, section.footnote_properties.as_ref()),
        (NoteKind::Endnote, section.endnote_properties.as_ref()),
    ] {
        let Some(properties) = properties else {
            continue;
        };
        if properties.note_line.is_none() && properties.note_spacing.is_none() {
            continue;
        }
        if blocks_contain_note_kind(&section.blocks, kind) {
            let label = match kind {
                NoteKind::Footnote => "footnote",
                NoteKind::Endnote => "endnote",
            };
            let present = match (
                properties.note_line.is_some(),
                properties.note_spacing.is_some(),
            ) {
                (true, true) => "noteLine and noteSpacing",
                (true, false) => "noteLine",
                (false, true) => "noteSpacing",
                (false, false) => unreachable!(),
            };
            return Err(PluginError::unsupported_feature(format!(
                "section contains an active {label} and authored {present}; DOCX has no equivalent section-scoped note layout, so conversion would lose formatting"
            )));
        }
    }
    Ok(())
}

/// 구역 정의와 머리말/꼬리말 story를 먼저 읽는다.
///
/// 이들은 본문 첫 문단의 `run/ctrl` 안에 들어가므로 본문 파서와 같은 스트림에서
/// 수집하면 중첩 문단 종료를 바깥 문단 종료로 오인하기 쉽다. 입력은 이미 패키지
/// 한계 안의 문자열이므로 두 번 순회해 경계를 단순하고 검증 가능하게 유지한다.
fn parse_section_metadata(
    xml: &str,
    styles: &SectionStyles<'_>,
    section: &mut Section,
) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = true;

    let mut saw_section_properties = false;
    let mut body_started = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "secPr" => {
                        if saw_section_properties {
                            return Err(PluginError::corrupt(
                                "section contains more than one secPr",
                            ));
                        }
                        saw_section_properties = true;
                        parse_section_properties(&mut reader, section)?;
                    }
                    "header" | "footer" => {
                        if body_started {
                            return Err(PluginError::unsupported_feature(format!(
                                "{name} starts after body content; mid-section header/footer activation cannot be represented faithfully in DOCX"
                            )));
                        }
                        let owned = e.into_owned();
                        let story = parse_header_footer(&mut reader, &owned, styles)?;
                        let stories = if name == "header" {
                            &mut section.headers
                        } else {
                            &mut section.footers
                        };
                        stories.push(story);
                    }
                    _ if starts_visible_body_content(&name) => body_started = true,
                    _ => {}
                }
            }
            Event::Text(text) if !text.decode()?.trim().is_empty() => body_started = true,
            Event::CData(text) if !String::from_utf8_lossy(text.as_ref()).trim().is_empty() => {
                body_started = true;
            }
            Event::GeneralRef(_) => body_started = true,
            // A completed empty paragraph is still an authored body block. A
            // header/footer control in any later paragraph is a mid-section
            // activation even though no visible text preceded it.
            Event::End(event) if local_name(event.name().as_ref()) == "p" => {
                body_started = true;
            }
            _ => {}
        }
        buf.clear();
    }

    validate_story_set("header", &section.headers)?;
    validate_story_set("footer", &section.footers)?;
    validate_first_page_story(section)?;
    Ok(())
}

fn starts_visible_body_content(name: &str) -> bool {
    matches!(
        name,
        "tbl"
            | "pic"
            | "equation"
            | "rect"
            | "ellipse"
            | "line"
            | "arc"
            | "polygon"
            | "curve"
            | "connectLine"
            | "container"
            | "textart"
            | "ole"
            | "video"
            | "checkBtn"
            | "fieldBegin"
            | "autoNum"
            | "footNote"
            | "endNote"
            | "tab"
            | "lineBreak"
            | "linebreak"
    )
}

/// Multiple same-slot controls and BOTH+parity mixtures are valid HWP timelines,
/// but their page-activation order cannot be lowered to one DOCX section yet.
/// A single ODD or EVEN definition is also exact: the missing DOCX slot is
/// materialized as an empty part so it cannot inherit content from a prior section.
fn validate_story_set(kind: &str, stories: &[HeaderFooter]) -> Result<()> {
    let supported = match stories {
        [] => true,
        [_] => true,
        [first, second] => {
            matches!(
                (first.page, second.page),
                (HeaderFooterPage::Odd, HeaderFooterPage::Even)
                    | (HeaderFooterPage::Even, HeaderFooterPage::Odd)
            )
        }
        _ => false,
    };
    if supported {
        return Ok(());
    }

    Err(PluginError::unsupported_feature(format!(
        "section {kind} controls form an unverified overlap timeline; supported shapes are one BOTH/ODD/EVEN definition or one ODD+EVEN pair"
    )))
}

fn validate_first_page_story(section: &Section) -> Result<()> {
    if section.hide_first_header == section.hide_first_footer {
        return Ok(());
    }
    let (kind, stories) = if section.hide_first_header {
        ("footer", &section.footers)
    } else {
        ("header", &section.headers)
    };
    if stories
        .iter()
        .any(|story| story.page != HeaderFooterPage::Both)
    {
        return Err(PluginError::unsupported_feature(format!(
            "one-sided first-page hiding requires choosing an unverified ODD/EVEN {kind} for the first page"
        )));
    }
    Ok(())
}

/// `hs:sec`의 직접 자식 문단만 본문으로 읽는다.
fn parse_section_body(xml: &str, styles: &SectionStyles<'_>) -> Result<Vec<Block>> {
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = true;

    let mut blocks = Vec::new();
    let mut root_seen = false;
    let mut root_closed = false;
    let mut depth = 0usize;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                if !root_seen {
                    if name != "sec" {
                        return Err(PluginError::corrupt(format!(
                            "section XML root must be sec, got {name}"
                        )));
                    }
                    root_seen = true;
                } else if depth == 0 && name == "p" {
                    let owned = event.into_owned();
                    blocks.extend(parse_paragraph(&mut reader, &owned, styles, 0, None)?);
                } else {
                    depth = depth.saturating_add(1);
                }
            }
            Event::End(event) if root_seen => {
                let name = local_name(event.name().as_ref());
                if depth == 0 && name == "sec" {
                    root_closed = true;
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        buf.clear();
    }

    if !root_seen || !root_closed {
        return Err(PluginError::corrupt(
            "section XML is missing a complete sec root element",
        ));
    }
    Ok(blocks)
}

fn parse_section_properties(reader: &mut Reader<&[u8]>, section: &mut Section) -> Result<()> {
    let mut depth = 0usize;
    let mut saw_visibility = false;
    let mut saw_footnote_properties = false;
    let mut saw_endnote_properties = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt("unexpected end of XML inside secPr"));
            }
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                if depth == 0 {
                    match name.as_str() {
                        "visibility" => {
                            if saw_visibility {
                                return Err(PluginError::corrupt(
                                    "secPr contains more than one visibility element",
                                ));
                            }
                            saw_visibility = true;
                            section.hide_first_header = parse_bool_attr(&event, "hideFirstHeader")?;
                            section.hide_first_footer = parse_bool_attr(&event, "hideFirstFooter")?;
                        }
                        "footNotePr" => {
                            if saw_footnote_properties {
                                return Err(PluginError::corrupt(
                                    "secPr contains more than one footNotePr element",
                                ));
                            }
                            saw_footnote_properties = true;
                            section.footnote_properties = Some(parse_note_properties(
                                reader,
                                "footNotePr",
                                NoteKind::Footnote,
                            )?);
                            buf.clear();
                            continue;
                        }
                        "endNotePr" => {
                            if saw_endnote_properties {
                                return Err(PluginError::corrupt(
                                    "secPr contains more than one endNotePr element",
                                ));
                            }
                            saw_endnote_properties = true;
                            section.endnote_properties = Some(parse_note_properties(
                                reader,
                                "endNotePr",
                                NoteKind::Endnote,
                            )?);
                            buf.clear();
                            continue;
                        }
                        _ => {}
                    }
                }
                depth = depth.saturating_add(1);
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if depth == 0 && name == "secPr" {
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

fn parse_note_properties(
    reader: &mut Reader<&[u8]>,
    parent_tag: &str,
    kind: NoteKind,
) -> Result<NoteProperties> {
    let mut auto_format = None;
    let mut numbering = None;
    let mut position = None;
    let mut note_line = None;
    let mut note_spacing = None;
    let mut depth = 0usize;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(format!(
                    "unexpected end of XML inside {parent_tag}"
                )));
            }
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                if depth == 0 {
                    match name.as_str() {
                        "autoNumFormat" => {
                            if auto_format.is_some() {
                                return Err(PluginError::corrupt(format!(
                                    "{parent_tag} contains more than one autoNumFormat element"
                                )));
                            }
                            auto_format = Some(parse_note_auto_format(&event, parent_tag)?);
                        }
                        "numbering" => {
                            if numbering.is_some() {
                                return Err(PluginError::corrupt(format!(
                                    "{parent_tag} contains more than one numbering element"
                                )));
                            }
                            numbering = Some(parse_note_numbering(&event, parent_tag, kind)?);
                        }
                        "placement" => {
                            if position.is_some() {
                                return Err(PluginError::corrupt(format!(
                                    "{parent_tag} contains more than one placement element"
                                )));
                            }
                            position = Some(parse_note_position(&event, parent_tag, kind)?);
                        }
                        "noteLine" => {
                            if note_line.is_some() {
                                return Err(PluginError::corrupt(format!(
                                    "{parent_tag} contains more than one noteLine element"
                                )));
                            }
                            note_line = Some(parse_note_line(&event, parent_tag)?);
                        }
                        "noteSpacing" => {
                            if note_spacing.is_some() {
                                return Err(PluginError::corrupt(format!(
                                    "{parent_tag} contains more than one noteSpacing element"
                                )));
                            }
                            note_spacing = Some(parse_note_spacing(&event, parent_tag)?);
                        }
                        _ => {
                            return Err(PluginError::corrupt(format!(
                                "{parent_tag} contains unknown direct child {name}"
                            )));
                        }
                    }
                } else {
                    return Err(PluginError::corrupt(format!(
                        "{parent_tag} child elements must be empty; found nested {name}"
                    )));
                }
                depth = depth.saturating_add(1);
            }
            Event::Text(text) if !text.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "{parent_tag} contains unexpected text"
                )));
            }
            Event::CData(text) if !String::from_utf8_lossy(text.as_ref()).trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "{parent_tag} contains unexpected CDATA"
                )));
            }
            Event::GeneralRef(_) => {
                return Err(PluginError::corrupt(format!(
                    "{parent_tag} contains an unexpected entity reference"
                )));
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if depth == 0 && name == parent_tag {
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        buf.clear();
    }

    let (number_format, prefix, suffix, superscript) = auto_format
        .ok_or_else(|| PluginError::corrupt(format!("{parent_tag} is missing autoNumFormat")))?;
    let (restart, start) = numbering
        .ok_or_else(|| PluginError::corrupt(format!("{parent_tag} is missing numbering")))?;
    let position = position
        .ok_or_else(|| PluginError::corrupt(format!("{parent_tag} is missing placement")))?;

    Ok(NoteProperties {
        number_format,
        restart,
        start,
        position,
        prefix,
        suffix,
        superscript,
        note_line,
        note_spacing,
    })
}

fn required_attr(
    event: &quick_xml::events::BytesStart<'_>,
    parent_tag: &str,
    child_tag: &str,
    name: &str,
) -> Result<String> {
    attr(event, name).ok_or_else(|| {
        PluginError::corrupt(format!(
            "{parent_tag}/{child_tag} is missing required {name}"
        ))
    })
}

fn parse_note_line(
    event: &quick_xml::events::BytesStart<'_>,
    parent_tag: &str,
) -> Result<NoteLine> {
    let raw_length = required_attr(event, parent_tag, "noteLine", "length")?;
    let length = raw_length.parse::<i32>().map_err(|_| {
        PluginError::corrupt(format!(
            "{parent_tag}/noteLine has invalid length {raw_length:?}"
        ))
    })?;

    let raw_type = required_attr(event, parent_tag, "noteLine", "type")?;
    let line_type = match raw_type.trim().to_ascii_uppercase().as_str() {
        "NONE" => NoteLineType::None,
        "SOLID" => NoteLineType::Solid,
        "DOT" => NoteLineType::Dot,
        "DASH" => NoteLineType::Dash,
        "DASH_DOT" => NoteLineType::DashDot,
        "DASH_DOT_DOT" => NoteLineType::DashDotDot,
        "LONG_DASH" => NoteLineType::LongDash,
        "CIRCLE" => NoteLineType::Circle,
        "DOUBLE_SLIM" => NoteLineType::DoubleSlim,
        "SLIM_THICK" => NoteLineType::SlimThick,
        "THICK_SLIM" => NoteLineType::ThickSlim,
        "SLIM_THICK_SLIM" => NoteLineType::SlimThickSlim,
        "WAVE" => NoteLineType::Wave,
        "DOUBLEWAVE" => NoteLineType::DoubleWave,
        "THICK3D" => NoteLineType::Thick3d,
        "THICKREV3D" => NoteLineType::ThickRev3d,
        "3D" => NoteLineType::ThreeD,
        "REV3D" => NoteLineType::Rev3d,
        _ => {
            return Err(PluginError::corrupt(format!(
                "{parent_tag}/noteLine has invalid type {raw_type:?}"
            )));
        }
    };

    let raw_width = required_attr(event, parent_tag, "noteLine", "width")?;
    let width = match raw_width.trim() {
        "0.1 mm" => NoteLineWidth::Mm0_1,
        "0.12 mm" => NoteLineWidth::Mm0_12,
        "0.15 mm" => NoteLineWidth::Mm0_15,
        "0.2 mm" => NoteLineWidth::Mm0_2,
        "0.25 mm" => NoteLineWidth::Mm0_25,
        "0.3 mm" => NoteLineWidth::Mm0_3,
        "0.4 mm" => NoteLineWidth::Mm0_4,
        "0.5 mm" => NoteLineWidth::Mm0_5,
        "0.6 mm" => NoteLineWidth::Mm0_6,
        "0.7 mm" => NoteLineWidth::Mm0_7,
        "1.0 mm" => NoteLineWidth::Mm1_0,
        "1.5 mm" => NoteLineWidth::Mm1_5,
        "2.0 mm" => NoteLineWidth::Mm2_0,
        "3.0 mm" => NoteLineWidth::Mm3_0,
        "4.0 mm" => NoteLineWidth::Mm4_0,
        "5.0 mm" => NoteLineWidth::Mm5_0,
        _ => {
            return Err(PluginError::corrupt(format!(
                "{parent_tag}/noteLine has invalid width {raw_width:?}"
            )));
        }
    };

    let raw_color = required_attr(event, parent_tag, "noteLine", "color")?;
    let rgb = raw_color
        .trim()
        .strip_prefix('#')
        .unwrap_or(raw_color.trim());
    if rgb.len() != 6 || !rgb.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(PluginError::corrupt(format!(
            "{parent_tag}/noteLine has invalid RGB color {raw_color:?}"
        )));
    }
    let color = normalize_color(raw_color.clone()).ok_or_else(|| {
        PluginError::corrupt(format!(
            "{parent_tag}/noteLine has invalid color {raw_color:?}"
        ))
    })?;

    Ok(NoteLine {
        length,
        line_type,
        width,
        color,
    })
}

fn parse_note_spacing(
    event: &quick_xml::events::BytesStart<'_>,
    parent_tag: &str,
) -> Result<NoteSpacing> {
    fn parse_value(
        event: &quick_xml::events::BytesStart<'_>,
        parent_tag: &str,
        name: &str,
    ) -> Result<u32> {
        let raw = required_attr(event, parent_tag, "noteSpacing", name)?;
        raw.parse::<u32>().map_err(|_| {
            PluginError::corrupt(format!(
                "{parent_tag}/noteSpacing has invalid {name} {raw:?}"
            ))
        })
    }

    Ok(NoteSpacing {
        between_notes: parse_value(event, parent_tag, "betweenNotes")?,
        below_line: parse_value(event, parent_tag, "belowLine")?,
        above_line: parse_value(event, parent_tag, "aboveLine")?,
    })
}

type ParsedAutoFormat = (NoteNumberFormat, String, String, bool);

fn parse_note_auto_format(
    event: &quick_xml::events::BytesStart<'_>,
    parent_tag: &str,
) -> Result<ParsedAutoFormat> {
    let raw = attr(event, "type").ok_or_else(|| {
        PluginError::corrupt(format!("{parent_tag}/autoNumFormat is missing type"))
    })?;
    let number_format = match raw.trim().to_ascii_uppercase().as_str() {
        "DIGIT" => NoteNumberFormat::Decimal,
        "ROMAN_SMALL" => NoteNumberFormat::LowerRoman,
        "ROMAN_CAPITAL" => NoteNumberFormat::UpperRoman,
        "LATIN_SMALL" => NoteNumberFormat::LowerLetter,
        "LATIN_CAPITAL" => NoteNumberFormat::UpperLetter,
        // Both formats use the conventional *, dagger, double-dagger sequence.
        "SYMBOL" => NoteNumberFormat::Chicago,
        unsupported => {
            return Err(PluginError::unsupported_feature(format!(
                "{parent_tag} uses unsupported automatic note number format {unsupported}"
            )));
        }
    };
    if let Some(user_char) = attr(event, "userChar").filter(|value| !value.is_empty()) {
        return Err(PluginError::unsupported_feature(format!(
            "{parent_tag}/autoNumFormat userChar {user_char:?} cannot be preserved without changing automatic note numbering"
        )));
    }
    Ok((
        number_format,
        attr(event, "prefixChar").unwrap_or_default(),
        attr(event, "suffixChar").unwrap_or_default(),
        parse_bool_attr(event, "supscript")?,
    ))
}

fn parse_note_numbering(
    event: &quick_xml::events::BytesStart<'_>,
    parent_tag: &str,
    kind: NoteKind,
) -> Result<(NoteNumberRestart, usize)> {
    let raw = attr(event, "type")
        .ok_or_else(|| PluginError::corrupt(format!("{parent_tag}/numbering is missing type")))?;
    let restart = match raw.trim().to_ascii_uppercase().as_str() {
        "CONTINUOUS" => NoteNumberRestart::Continuous,
        "ON_SECTION" => NoteNumberRestart::EachSection,
        "ON_PAGE" if kind == NoteKind::Footnote => NoteNumberRestart::EachPage,
        "ON_PAGE" => {
            return Err(PluginError::unsupported_feature(
                "endNotePr cannot preserve ON_PAGE numbering in DOCX",
            ));
        }
        invalid => {
            return Err(PluginError::corrupt(format!(
                "{parent_tag}/numbering has invalid type {invalid}"
            )));
        }
    };
    let start = match attr(event, "newNum") {
        Some(raw_start) => raw_start.parse::<usize>().map_err(|_| {
            PluginError::corrupt(format!(
                "{parent_tag}/numbering has invalid newNum {raw_start:?}"
            ))
        })?,
        None => 1,
    };
    Ok((restart, start))
}

fn parse_note_position(
    event: &quick_xml::events::BytesStart<'_>,
    parent_tag: &str,
    kind: NoteKind,
) -> Result<NotePosition> {
    let raw = attr(event, "place")
        .ok_or_else(|| PluginError::corrupt(format!("{parent_tag}/placement is missing place")))?;
    let beneath_text = parse_bool_attr(event, "beneathText")?;
    match kind {
        NoteKind::Footnote => match raw.trim().to_ascii_uppercase().as_str() {
            "EACH_COLUMN" => Ok(if beneath_text {
                NotePosition::BeneathText
            } else {
                NotePosition::PageBottom
            }),
            "MERGED_COLUMN" | "RIGHT_MOST_COLUMN" => Err(PluginError::unsupported_feature(
                format!("DOCX cannot preserve footNotePr placement {raw}"),
            )),
            invalid => Err(PluginError::corrupt(format!(
                "footNotePr/placement has invalid place {invalid}"
            ))),
        },
        NoteKind::Endnote => {
            if beneath_text {
                return Err(PluginError::unsupported_feature(
                    "DOCX cannot preserve endNotePr beneathText=true",
                ));
            }
            match raw.trim().to_ascii_uppercase().as_str() {
                "END_OF_DOCUMENT" => Ok(NotePosition::DocumentEnd),
                "END_OF_SECTION" => Ok(NotePosition::SectionEnd),
                invalid => Err(PluginError::corrupt(format!(
                    "endNotePr/placement has invalid place {invalid}"
                ))),
            }
        }
    }
}

fn parse_bool_attr(event: &quick_xml::events::BytesStart<'_>, name: &str) -> Result<bool> {
    let Some(raw) = attr(event, name) else {
        return Ok(false);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(PluginError::corrupt(format!(
            "{name} has invalid boolean value {raw:?}"
        ))),
    }
}

fn parse_header_footer(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &SectionStyles<'_>,
) -> Result<HeaderFooter> {
    let tag = local_name(start.name().as_ref());
    let raw_page = attr(start, "applyPageType").unwrap_or_else(|| "BOTH".to_owned());
    let page = match raw_page.trim().to_ascii_uppercase().as_str() {
        "BOTH" => HeaderFooterPage::Both,
        "ODD" => HeaderFooterPage::Odd,
        "EVEN" => HeaderFooterPage::Even,
        _ => {
            return Err(PluginError::corrupt(format!(
                "{tag} has invalid applyPageType {raw_page:?}"
            )));
        }
    };

    let mut blocks = Vec::new();
    let mut saw_sub_list = false;
    let mut inside_sub_list = false;
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
                if name == "subList" {
                    if saw_sub_list {
                        return Err(PluginError::corrupt(format!(
                            "{tag} contains more than one subList"
                        )));
                    }
                    saw_sub_list = true;
                    inside_sub_list = true;
                } else if !inside_sub_list {
                    return Err(PluginError::corrupt(format!(
                        "{tag} contains {name} outside subList"
                    )));
                } else if name == "p" {
                    let owned = event.into_owned();
                    blocks.extend(parse_paragraph(reader, &owned, styles, 0, None)?);
                } else {
                    return Err(PluginError::corrupt(format!(
                        "{tag} subList contains unexpected direct child {name}"
                    )));
                }
            }
            Event::Text(text) if !text.decode()?.trim().is_empty() => {
                let location = if inside_sub_list {
                    "direct text inside subList"
                } else {
                    "text outside subList"
                };
                return Err(PluginError::corrupt(format!("{tag} contains {location}")));
            }
            Event::CData(text) if !String::from_utf8_lossy(text.as_ref()).trim().is_empty() => {
                let location = if inside_sub_list {
                    "direct CDATA inside subList"
                } else {
                    "CDATA outside subList"
                };
                return Err(PluginError::corrupt(format!("{tag} contains {location}")));
            }
            Event::GeneralRef(_) => {
                return Err(PluginError::corrupt(format!(
                    "{tag} contains an unexpected entity reference"
                )));
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if name == "subList" && inside_sub_list {
                    inside_sub_list = false;
                } else if name == tag {
                    if inside_sub_list {
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
    if blocks_contain_note(&blocks) {
        return Err(PluginError::unsupported_feature(format!(
            "{tag} contains a footnote or endnote, which DOCX headers and footers cannot contain"
        )));
    }

    Ok(HeaderFooter {
        id: attr(start, "id"),
        page,
        blocks,
    })
}

/// `hp:p` 하나를 읽어 블록들로 변환한다.
///
/// 표를 품고 있으면 여러 블록으로 쪼개진다. 그래서 반환형이 `Vec<Block>`이다.
fn parse_paragraph(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &SectionStyles<'_>,
    depth: usize,
    note_context: Option<NoteKind>,
) -> Result<Vec<Block>> {
    let mut para_style = styles.para_style(attr(start, "paraPrIDRef").as_deref())?;
    para_style.named_style_id = styles.named_style_id(attr(start, "styleIDRef").as_deref())?;

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
                    // 구역 메타데이터와 반복 story는 별도 1차 순회에서 읽는다.
                    // 여기서 소비하지 않으면 그 안의 `p` 종료를 바깥 본문 문단
                    // 종료로 오인하고 텍스트도 본문으로 유출한다.
                    "secPr" => skip_element(reader, "secPr")?,
                    "header" => skip_element(reader, "header")?,
                    "footer" => skip_element(reader, "footer")?,
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
                    "autoNum" => {
                        let owned = e.into_owned();
                        if let Some(field) = parse_auto_number(
                            reader,
                            &owned,
                            run_style.clone().unwrap_or_default(),
                            note_context,
                        )? {
                            current.inlines.push(Inline::PageNumber(field));
                        }
                    }
                    "equation" => {
                        let owned = e.into_owned();
                        current
                            .inlines
                            .push(Inline::Equation(parse_equation(reader, &owned)?));
                    }
                    "rect" | "ellipse" if note_context.is_some() => {
                        return Err(PluginError::unsupported_feature(
                            "shape inside a footnote or endnote cannot be hosted by the current DOCX drawing surface",
                        ));
                    }
                    "rect" if depth < MAX_DEPTH => {
                        let owned = e.into_owned();
                        current.inlines.push(Inline::Rectangle(parse_rectangle(
                            reader,
                            &owned,
                            styles,
                            depth + 1,
                        )?));
                    }
                    "rect" => {
                        return Err(PluginError::corrupt(format!(
                            "rectangle nesting exceeds the maximum depth of {MAX_DEPTH}"
                        )));
                    }
                    "ellipse" if depth < MAX_DEPTH => {
                        let owned = e.into_owned();
                        current.inlines.push(Inline::Rectangle(parse_ellipse(
                            reader,
                            &owned,
                            styles,
                            depth + 1,
                        )?));
                    }
                    "ellipse" => {
                        return Err(PluginError::corrupt(format!(
                            "ellipse nesting exceeds the maximum depth of {MAX_DEPTH}"
                        )));
                    }
                    name if UNSUPPORTED_SHAPE_ELEMENTS
                        .iter()
                        .any(|candidate| name.eq_ignore_ascii_case(candidate)) =>
                    {
                        return Err(PluginError::unsupported_feature(format!(
                            "active {name} shape is not yet representable as an editable DOCX object"
                        )));
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
                    name if name.eq_ignore_ascii_case("footnote")
                        || name.eq_ignore_ascii_case("endnote") =>
                    {
                        return Err(PluginError::corrupt(format!(
                            "unrecognized case-confused note element {name}"
                        )));
                    }
                    "tbl" if depth < MAX_DEPTH => {
                        // 표 앞까지의 문단을 먼저 확정한다.
                        validate_display_equation_placement(&current)?;
                        flush_paragraph(&mut out, &mut current, &para_style);
                        let owned = e.into_owned();
                        let table = parse_table(reader, &owned, styles, depth + 1, note_context)?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimpleShapeKind {
    Rectangle,
    Ellipse,
}

const EMU_PER_HWPUNIT: u64 = 127;
const DOCX_MAX_COORDINATE_EMU: u64 = i32::MAX as u64;
const DOCX_MAX_WRAP_DISTANCE_EMU: u64 = u32::MAX as u64;
const DOCX_MAX_LINE_WIDTH_EMU: u64 = 20_116_800;

impl SimpleShapeKind {
    fn element_name(self) -> &'static str {
        match self {
            Self::Rectangle => "rect",
            Self::Ellipse => "ellipse",
        }
    }

    fn is_geometry_cache(self, name: &str) -> bool {
        match self {
            Self::Rectangle => matches!(
                name,
                "offset" | "orgSz" | "curSz" | "renderingInfo" | "pt0" | "pt1" | "pt2" | "pt3"
            ),
            Self::Ellipse => matches!(
                name,
                "offset"
                    | "orgSz"
                    | "curSz"
                    | "renderingInfo"
                    | "center"
                    | "ax1"
                    | "ax2"
                    | "start1"
                    | "end1"
                    | "start2"
                    | "end2"
            ),
        }
    }
}

fn parse_rectangle(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &SectionStyles<'_>,
    depth: usize,
) -> Result<Rectangle> {
    parse_simple_shape(reader, start, styles, depth, SimpleShapeKind::Rectangle)
}

fn parse_ellipse(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &SectionStyles<'_>,
    depth: usize,
) -> Result<Rectangle> {
    parse_simple_shape(reader, start, styles, depth, SimpleShapeKind::Ellipse)
}

/// Parse the closed, evidence-backed `hp:rect`/`hp:ellipse` subset. The final
/// `sz` and `pos` elements are authoritative for DOCX layout; rendering
/// matrices and point caches are producer caches, while every active visual
/// feature is validated explicitly.
fn parse_simple_shape(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    styles: &SectionStyles<'_>,
    depth: usize,
    kind: SimpleShapeKind,
) -> Result<Rectangle> {
    let shape_name = kind.element_name();
    let common_attributes = &[
        "id",
        "zOrder",
        "numberingType",
        "textWrap",
        "textFlow",
        "lock",
        "dropcapstyle",
        "href",
        "groupLevel",
        "instid",
        "instId",
    ];
    match kind {
        SimpleShapeKind::Rectangle => {
            let mut allowed = common_attributes.to_vec();
            allowed.push("ratio");
            validate_shape_attributes(start, shape_name, &allowed)?;
        }
        SimpleShapeKind::Ellipse => {
            let mut allowed = common_attributes.to_vec();
            allowed.extend(["intervalDirty", "hasArcPr", "arcType"]);
            validate_shape_attributes(start, shape_name, &allowed)?;
        }
    }

    let geometry = match kind {
        SimpleShapeKind::Rectangle => {
            let corner_radius_percent = parse_optional_u32(start, "rect", "ratio", 0)?;
            if corner_radius_percent > 50 {
                return Err(PluginError::unsupported_feature(format!(
                    "rect corner ratio {corner_radius_percent}% exceeds the verified 0..=50 range"
                )));
            }
            ShapeGeometry::Rectangle {
                corner_radius_percent,
            }
        }
        SimpleShapeKind::Ellipse => {
            if parse_optional_u32(start, "ellipse", "intervalDirty", 0)? != 0 {
                return Err(PluginError::unsupported_feature(
                    "ellipse intervalDirty metadata is active and outside the verified profile",
                ));
            }
            if parse_required_shape_bool(start, "ellipse", "hasArcPr")? {
                return Err(PluginError::unsupported_feature(
                    "ellipse arc properties are outside the verified whole-ellipse profile",
                ));
            }
            let arc_type = required_shape_attr(start, "ellipse", "arcType")?;
            if !arc_type.eq_ignore_ascii_case("normal") {
                return Err(PluginError::unsupported_feature(format!(
                    "ellipse arcType {arc_type} is outside the verified NORMAL profile"
                )));
            }
            ShapeGeometry::Ellipse
        }
    };

    let z_order = parse_required_u32(start, shape_name, "zOrder")?;
    let numbering_type = required_shape_attr(start, shape_name, "numberingType")?;
    match numbering_type.trim().to_ascii_uppercase().as_str() {
        "NONE" | "PICTURE" => {}
        other => {
            return Err(PluginError::unsupported_feature(format!(
                "{shape_name} numberingType {other} requires an unsupported caption/numbering mapping"
            )));
        }
    }
    let wrap = match required_shape_attr(start, shape_name, "textWrap")?
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "SQUARE" => RectangleWrap::Square,
        "TOP_AND_BOTTOM" => RectangleWrap::TopAndBottom,
        "BEHIND_TEXT" => RectangleWrap::BehindText,
        "IN_FRONT_OF_TEXT" => RectangleWrap::InFrontOfText,
        other => {
            return Err(PluginError::corrupt(format!(
                "{shape_name} has invalid textWrap {other}"
            )));
        }
    };
    let wrap_side = match required_shape_attr(start, shape_name, "textFlow")?
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "BOTH_SIDES" => RectangleWrapSide::BothSides,
        "LEFT_ONLY" => RectangleWrapSide::Left,
        "RIGHT_ONLY" => RectangleWrapSide::Right,
        "LARGEST_ONLY" => RectangleWrapSide::Largest,
        other => {
            return Err(PluginError::corrupt(format!(
                "{shape_name} has invalid textFlow {other}"
            )));
        }
    };
    if parse_required_shape_bool(start, shape_name, "lock")? {
        return Err(PluginError::unsupported_feature(format!(
            "locked {shape_name} cannot be preserved by the editable DOCX shape surface"
        )));
    }
    if attr(start, "dropcapstyle").is_some_and(|value| !value.eq_ignore_ascii_case("none")) {
        return Err(PluginError::unsupported_feature(format!(
            "{shape_name} drop-cap layout is not representable as a DOCX drawing"
        )));
    }
    if attr(start, "href").is_some_and(|value| !value.trim().is_empty()) {
        return Err(PluginError::unsupported_feature(format!(
            "hyperlinked {shape_name} is not supported by the typed DOCX shape surface"
        )));
    }
    if parse_optional_u32(start, shape_name, "groupLevel", 0)? != 0 {
        return Err(PluginError::unsupported_feature(format!(
            "grouped {shape_name} requires group geometry preservation"
        )));
    }
    let mut line = None;
    let mut fill = None;
    let mut saw_shadow = false;
    let mut size = None;
    let mut placement = None;
    let mut outer_margins = None;
    let mut saw_shape_comment = false;
    let mut description = None;
    let mut text = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(format!(
                    "unexpected end of XML inside {shape_name}"
                )));
            }
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                let owned = event.into_owned();
                match name.as_str() {
                    "lineShape" => {
                        if line.is_some() {
                            return Err(PluginError::corrupt(format!(
                                "{shape_name} contains more than one lineShape"
                            )));
                        }
                        line = Some(parse_rectangle_line(&owned, shape_name)?);
                        skip_element(reader, "lineShape")?;
                    }
                    "fillBrush" => {
                        if fill.is_some() {
                            return Err(PluginError::corrupt(format!(
                                "{shape_name} contains more than one fillBrush"
                            )));
                        }
                        fill = Some(parse_rectangle_fill(reader, &owned, shape_name)?);
                    }
                    "shadow" => {
                        if saw_shadow {
                            return Err(PluginError::corrupt(format!(
                                "{shape_name} contains more than one shadow"
                            )));
                        }
                        validate_rectangle_shadow(&owned, shape_name)?;
                        saw_shadow = true;
                        skip_element(reader, "shadow")?;
                    }
                    "drawText" => {
                        if kind == SimpleShapeKind::Ellipse {
                            return Err(PluginError::unsupported_feature(
                                "ellipse drawText has no native round-trip evidence",
                            ));
                        }
                        if text.is_some() {
                            return Err(PluginError::corrupt(
                                "rect contains more than one drawText",
                            ));
                        }
                        text = Some(parse_rectangle_text(reader, &owned, styles, depth)?);
                    }
                    "sz" => {
                        if size.is_some() {
                            return Err(PluginError::corrupt(format!(
                                "{shape_name} contains more than one sz"
                            )));
                        }
                        size = Some(parse_rectangle_size(&owned, shape_name)?);
                        skip_element(reader, "sz")?;
                    }
                    "pos" => {
                        if placement.is_some() {
                            return Err(PluginError::corrupt(format!(
                                "{shape_name} contains more than one pos"
                            )));
                        }
                        placement = Some(parse_rectangle_position(&owned, shape_name)?);
                        skip_element(reader, "pos")?;
                    }
                    "outMargin" => {
                        if outer_margins.is_some() {
                            return Err(PluginError::corrupt(format!(
                                "{shape_name} contains more than one outMargin"
                            )));
                        }
                        outer_margins = Some(parse_rectangle_margins(
                            &owned,
                            &format!("{shape_name}/outMargin"),
                            DOCX_MAX_WRAP_DISTANCE_EMU,
                            "wp:anchor wrap distance",
                        )?);
                        skip_element(reader, "outMargin")?;
                    }
                    "flip" => {
                        let label = format!("{shape_name}/flip");
                        validate_shape_attributes(&owned, &label, &["horizontal", "vertical"])?;
                        if parse_optional_shape_bool(&owned, &label, "horizontal", false)?
                            || parse_optional_shape_bool(&owned, &label, "vertical", false)?
                        {
                            return Err(PluginError::unsupported_feature(format!(
                                "flipped {shape_name} is outside the verified DOCX shape subset"
                            )));
                        }
                        skip_element(reader, "flip")?;
                    }
                    "rotationInfo" => {
                        let label = format!("{shape_name}/rotationInfo");
                        validate_shape_attributes(
                            &owned,
                            &label,
                            &["angle", "centerX", "centerY", "rotateimage"],
                        )?;
                        if parse_optional_i64(&owned, &label, "angle", 0)? != 0 {
                            return Err(PluginError::unsupported_feature(format!(
                                "rotated {shape_name} is outside the verified DOCX shape subset"
                            )));
                        }
                        skip_element(reader, "rotationInfo")?;
                    }
                    "shapeComment" => {
                        if saw_shape_comment {
                            return Err(PluginError::corrupt(format!(
                                "{shape_name} contains more than one shapeComment"
                            )));
                        }
                        saw_shape_comment = true;
                        let comment = read_text_until_end(reader, "shapeComment")?;
                        if !comment.trim().is_empty() {
                            description = Some(comment);
                        }
                    }
                    // Geometry provenance and rendering caches do not change the
                    // final axis-aligned rect once sz/pos/rotation/flip pass.
                    _ if kind.is_geometry_cache(&name) => skip_element(reader, &name)?,
                    "caption" => {
                        return Err(PluginError::unsupported_feature(format!(
                            "{shape_name} caption cannot be preserved by the typed DOCX shape surface"
                        )));
                    }
                    other => {
                        return Err(PluginError::unsupported_feature(format!(
                            "{shape_name} contains unsupported active child {other}"
                        )));
                    }
                }
            }
            Event::Text(value) if !value.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "{shape_name} contains direct text"
                )));
            }
            Event::CData(value) if !String::from_utf8_lossy(value.as_ref()).trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "{shape_name} contains direct CDATA"
                )));
            }
            Event::GeneralRef(_) => {
                return Err(PluginError::corrupt(format!(
                    "{shape_name} contains an unexpected entity reference"
                )));
            }
            Event::End(event) if local_name(event.name().as_ref()) == shape_name => break,
            _ => {}
        }
        buf.clear();
    }

    let (width_hwpunit, height_hwpunit) =
        size.ok_or_else(|| PluginError::corrupt(format!("{shape_name} is missing required sz")))?;
    Ok(Rectangle {
        width_hwpunit,
        height_hwpunit,
        geometry,
        placement: placement
            .ok_or_else(|| PluginError::corrupt(format!("{shape_name} is missing required pos")))?,
        wrap,
        wrap_side,
        outer_margins: outer_margins.ok_or_else(|| {
            PluginError::corrupt(format!("{shape_name} is missing required outMargin"))
        })?,
        z_order,
        fill: match kind {
            SimpleShapeKind::Rectangle => Some(
                fill.ok_or_else(|| PluginError::corrupt("rect is missing required fillBrush"))?,
            ),
            SimpleShapeKind::Ellipse => fill,
        },
        line: line.ok_or_else(|| {
            PluginError::corrupt(format!("{shape_name} is missing required lineShape"))
        })?,
        description,
        text,
    })
}

fn parse_rectangle_line(
    event: &quick_xml::events::BytesStart<'_>,
    shape_name: &str,
) -> Result<RectangleLine> {
    let label = format!("{shape_name}/lineShape");
    validate_shape_attributes(
        event,
        &label,
        &[
            "color",
            "width",
            "style",
            "endCap",
            "headStyle",
            "tailStyle",
            "headfill",
            "tailfill",
            "headSz",
            "tailSz",
            "outlineStyle",
            "alpha",
        ],
    )?;
    let style = required_shape_attr(event, &label, "style")?
        .trim()
        .to_ascii_uppercase();
    let visible = match style.as_str() {
        "SOLID" => true,
        "NONE" => false,
        _ => {
            return Err(PluginError::unsupported_feature(format!(
                "{shape_name} line style {style} is outside the verified SOLID/NONE subset"
            )));
        }
    };
    for (name, expected) in [
        ("endCap", "FLAT"),
        ("headStyle", "NORMAL"),
        ("tailStyle", "NORMAL"),
        ("outlineStyle", "NORMAL"),
    ] {
        if attr(event, name).is_some_and(|value| !value.eq_ignore_ascii_case(expected)) {
            return Err(PluginError::unsupported_feature(format!(
                "{shape_name} line {name} is outside the verified {expected} profile"
            )));
        }
    }
    if parse_optional_u32(event, &label, "alpha", 0)? != 0 {
        return Err(PluginError::unsupported_feature(format!(
            "transparent {shape_name} outlines are not supported by the verified typed mapping"
        )));
    }
    let raw_color = required_shape_attr(event, &label, "color")?;
    let color = if visible {
        normalize_color(raw_color).ok_or_else(|| {
            PluginError::corrupt(format!("{shape_name}/lineShape has invalid color"))
        })?
    } else {
        // The color is not rendered for style=NONE. Hancom commonly serializes
        // the literal sentinel `none`; retain a harmless canonical value.
        normalize_color(raw_color).unwrap_or_else(|| "#000000".to_string())
    };
    let width_hwpunit = parse_required_u32(event, &label, "width")?;
    if visible {
        ensure_hwpunit_fits_docx_unsigned(
            width_hwpunit,
            DOCX_MAX_LINE_WIDTH_EMU,
            &format!("{label} width"),
            "a:ln width",
        )?;
    }
    Ok(RectangleLine {
        visible,
        color,
        width_hwpunit,
    })
}

fn parse_rectangle_fill(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    shape_name: &str,
) -> Result<String> {
    let label = format!("{shape_name}/fillBrush");
    let brush_label = format!("{label}/winBrush");
    validate_shape_attributes(start, &label, &[])?;
    let mut color = None;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(format!(
                    "unexpected end of XML inside {label}"
                )));
            }
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                let owned = event.into_owned();
                if name != "winBrush" {
                    return Err(PluginError::unsupported_feature(format!(
                        "{shape_name} fill uses unsupported {name}"
                    )));
                }
                if color.is_some() {
                    return Err(PluginError::corrupt(format!(
                        "{label} contains more than one winBrush"
                    )));
                }
                validate_shape_attributes(
                    &owned,
                    &brush_label,
                    &["faceColor", "hatchColor", "alpha"],
                )?;
                if parse_optional_u32(&owned, &brush_label, "alpha", 0)? != 0 {
                    return Err(PluginError::unsupported_feature(format!(
                        "transparent {shape_name} fill is outside the verified opaque mapping"
                    )));
                }
                color = Some(
                    normalize_color(required_shape_attr(&owned, &brush_label, "faceColor")?)
                        .ok_or_else(|| {
                            PluginError::corrupt(format!("{brush_label} has invalid faceColor"))
                        })?,
                );
                skip_element(reader, "winBrush")?;
            }
            Event::Text(value) if !value.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "{label} contains direct text"
                )));
            }
            Event::CData(value) if !String::from_utf8_lossy(value.as_ref()).trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "{label} contains direct CDATA"
                )));
            }
            Event::End(event) if local_name(event.name().as_ref()) == "fillBrush" => break,
            _ => {}
        }
        buf.clear();
    }
    color.ok_or_else(|| PluginError::corrupt(format!("{label} is missing winBrush")))
}

fn validate_rectangle_shadow(
    event: &quick_xml::events::BytesStart<'_>,
    shape_name: &str,
) -> Result<()> {
    let label = format!("{shape_name}/shadow");
    validate_shape_attributes(
        event,
        &label,
        &["type", "color", "offsetX", "offsetY", "alpha"],
    )?;
    let kind = required_shape_attr(event, &label, "type")?;
    if !kind.eq_ignore_ascii_case("none") {
        return Err(PluginError::unsupported_feature(format!(
            "{shape_name} shadow type {kind} is outside the verified no-shadow subset"
        )));
    }
    Ok(())
}

fn parse_rectangle_size(
    event: &quick_xml::events::BytesStart<'_>,
    shape_name: &str,
) -> Result<(u32, u32)> {
    let label = format!("{shape_name}/sz");
    validate_shape_attributes(
        event,
        &label,
        &["width", "widthRelTo", "height", "heightRelTo", "protect"],
    )?;
    for name in ["widthRelTo", "heightRelTo"] {
        let value = required_shape_attr(event, &label, name)?;
        if !value.eq_ignore_ascii_case("absolute") {
            return Err(PluginError::unsupported_feature(format!(
                "{label} {name}={value} is relative and cannot be lowered without its layout frame"
            )));
        }
    }
    if parse_optional_shape_bool(event, &label, "protect", false)? {
        return Err(PluginError::unsupported_feature(format!(
            "protected {shape_name} size cannot be preserved by the editable DOCX shape surface"
        )));
    }
    let width = parse_required_u32(event, &label, "width")?;
    let height = parse_required_u32(event, &label, "height")?;
    if width == 0 || height == 0 {
        return Err(PluginError::corrupt(format!(
            "{label} width and height must both be positive"
        )));
    }
    ensure_hwpunit_fits_docx_unsigned(
        width,
        DOCX_MAX_COORDINATE_EMU,
        &format!("{label} width"),
        "wp:extent/a:ext coordinate",
    )?;
    ensure_hwpunit_fits_docx_unsigned(
        height,
        DOCX_MAX_COORDINATE_EMU,
        &format!("{label} height"),
        "wp:extent/a:ext coordinate",
    )?;
    Ok((width, height))
}

fn parse_rectangle_position(
    event: &quick_xml::events::BytesStart<'_>,
    shape_name: &str,
) -> Result<RectanglePlacement> {
    let label = format!("{shape_name}/pos");
    validate_shape_attributes(
        event,
        &label,
        &[
            "treatAsChar",
            "affectLSpacing",
            "flowWithText",
            "allowOverlap",
            "holdAnchorAndSO",
            "vertRelTo",
            "horzRelTo",
            "vertAlign",
            "horzAlign",
            "vertOffset",
            "horzOffset",
        ],
    )?;
    let inline = parse_required_shape_bool(event, &label, "treatAsChar")?;
    if parse_required_shape_bool(event, &label, "affectLSpacing")? {
        return Err(PluginError::unsupported_feature(format!(
            "{label} affectLSpacing=1 is outside the verified inline layout profile"
        )));
    }
    let flow_with_text = parse_required_shape_bool(event, &label, "flowWithText")?;
    let allow_overlap = parse_required_shape_bool(event, &label, "allowOverlap")?;
    if parse_required_shape_bool(event, &label, "holdAnchorAndSO")? {
        return Err(PluginError::unsupported_feature(format!(
            "{label} holdAnchorAndSO=1 is outside the verified layout profile"
        )));
    }
    let vert_rel = required_shape_attr(event, &label, "vertRelTo")?;
    let horz_rel = required_shape_attr(event, &label, "horzRelTo")?;
    let vert_align = required_shape_attr(event, &label, "vertAlign")?;
    let horz_align = required_shape_attr(event, &label, "horzAlign")?;
    // Both attributes remain schema-validated integers, but alignment makes
    // the offset on that axis semantically inactive. Hancom writes unsigned
    // sentinel values for CENTER, so do not range-convert or emit that X.
    let y = parse_required_i64(event, &label, "vertOffset")?;
    let x = parse_required_i64(event, &label, "horzOffset")?;

    if inline {
        // Hancom ignores the relative-frame coordinates for treatAsChar=1;
        // wp:inline carries only the final extent and outer distances. The
        // official grammar likewise limits flowWithText/allowOverlap to
        // floating objects, so producer defaults in those fields are inert.
        return Ok(RectanglePlacement::Inline);
    }
    if flow_with_text && vert_rel.eq_ignore_ascii_case("para") {
        return Err(PluginError::unsupported_feature(format!(
            "floating {shape_name} constrained to the paragraph text area has no verified DOCX mapping"
        )));
    }
    if !vert_rel.eq_ignore_ascii_case("paper")
        || !horz_rel.eq_ignore_ascii_case("paper")
        || !vert_align.eq_ignore_ascii_case("top")
    {
        return Err(PluginError::unsupported_feature(format!(
            "floating {shape_name} requires verified PAPER/PAPER TOP placement, got {vert_rel}/{horz_rel} {vert_align}/{horz_align}"
        )));
    }
    ensure_hwpunit_fits_docx_position(y, shape_name)?;
    let horizontal = if horz_align.eq_ignore_ascii_case("left") {
        ensure_hwpunit_fits_docx_position(x, shape_name)?;
        RectangleHorizontalPosition::Offset(x)
    } else if horz_align.eq_ignore_ascii_case("center") {
        RectangleHorizontalPosition::Center
    } else {
        return Err(PluginError::unsupported_feature(format!(
            "floating {shape_name} horizontal alignment {horz_align} is outside the verified LEFT/CENTER profile"
        )));
    };
    Ok(RectanglePlacement::Page {
        horizontal,
        y_hwpunit: y,
        allow_overlap,
    })
}

fn parse_rectangle_margins(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    max_emu: u64,
    docx_domain: &str,
) -> Result<RectangleMargins> {
    validate_shape_attributes(event, label, &["left", "right", "top", "bottom"])?;
    let margins = RectangleMargins {
        left_hwpunit: parse_required_u32(event, label, "left")?,
        right_hwpunit: parse_required_u32(event, label, "right")?,
        top_hwpunit: parse_required_u32(event, label, "top")?,
        bottom_hwpunit: parse_required_u32(event, label, "bottom")?,
    };
    for (name, value) in [
        ("left", margins.left_hwpunit),
        ("right", margins.right_hwpunit),
        ("top", margins.top_hwpunit),
        ("bottom", margins.bottom_hwpunit),
    ] {
        ensure_hwpunit_fits_docx_unsigned(value, max_emu, &format!("{label} {name}"), docx_domain)?;
    }
    Ok(margins)
}

fn ensure_hwpunit_fits_docx_unsigned(
    value: u32,
    max_emu: u64,
    label: &str,
    docx_domain: &str,
) -> Result<()> {
    let emu = u64::from(value) * EMU_PER_HWPUNIT;
    if emu > max_emu {
        return Err(PluginError::unsupported_feature(format!(
            "{label}={value} HWPUNIT exceeds the DOCX {docx_domain} range"
        )));
    }
    Ok(())
}

fn ensure_hwpunit_fits_docx_position(value: i64, shape_name: &str) -> Result<()> {
    let emu = value.checked_mul(EMU_PER_HWPUNIT as i64).ok_or_else(|| {
        PluginError::unsupported_feature(format!(
            "floating {shape_name} coordinates exceed the exact DOCX EMU range"
        ))
    })?;
    if emu < i64::from(i32::MIN) || emu > i64::from(i32::MAX) {
        return Err(PluginError::unsupported_feature(format!(
            "floating {shape_name} coordinates exceed the DOCX wp:posOffset range"
        )));
    }
    Ok(())
}

fn parse_rectangle_text(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    styles: &SectionStyles<'_>,
    depth: usize,
) -> Result<RectangleText> {
    validate_shape_attributes(start, "rect/drawText", &["lastWidth", "name", "editable"])?;
    if parse_optional_shape_bool(start, "rect/drawText", "editable", false)? {
        return Err(PluginError::unsupported_feature(
            "editable HWPX drawText metadata has no verified DOCX equivalent",
        ));
    }
    if let Some(last_width) = attr(start, "lastWidth") {
        let parsed = last_width.parse::<i64>().map_err(|_| {
            PluginError::corrupt(format!(
                "rect/drawText has invalid lastWidth {last_width:?}"
            ))
        })?;
        if parsed < 0 {
            return Err(PluginError::corrupt(
                "rect/drawText lastWidth cannot be negative",
            ));
        }
    }

    let name = attr(start, "name").filter(|value| !value.trim().is_empty());
    let mut sub_list = None;
    let mut margins = None;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(
                    "unexpected end of XML inside rect/drawText",
                ));
            }
            Event::Start(event) => {
                let child = local_name(event.name().as_ref());
                let owned = event.into_owned();
                match child.as_str() {
                    "subList" => {
                        if sub_list.is_some() {
                            return Err(PluginError::corrupt(
                                "rect/drawText contains more than one subList",
                            ));
                        }
                        sub_list =
                            Some(parse_rectangle_text_sublist(reader, &owned, styles, depth)?);
                    }
                    "textMargin" => {
                        if margins.is_some() {
                            return Err(PluginError::corrupt(
                                "rect/drawText contains more than one textMargin",
                            ));
                        }
                        margins = Some(parse_rectangle_margins(
                            &owned,
                            "rect/drawText/textMargin",
                            DOCX_MAX_COORDINATE_EMU,
                            "a:bodyPr text inset",
                        )?);
                        skip_element(reader, "textMargin")?;
                    }
                    other => {
                        return Err(PluginError::unsupported_feature(format!(
                            "rect/drawText contains unsupported child {other}"
                        )));
                    }
                }
            }
            Event::Text(value) if !value.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt("rect/drawText contains direct text"));
            }
            Event::CData(value) if !String::from_utf8_lossy(value.as_ref()).trim().is_empty() => {
                return Err(PluginError::corrupt("rect/drawText contains direct CDATA"));
            }
            Event::End(event) if local_name(event.name().as_ref()) == "drawText" => break,
            _ => {}
        }
        buf.clear();
    }

    let (direction, anchor, blocks) = sub_list
        .ok_or_else(|| PluginError::corrupt("rect/drawText is missing required subList"))?;
    if blocks_contain_rectangle(&blocks) {
        return Err(PluginError::unsupported_feature(
            "nested rect inside drawText would create an invalid nested DOCX textbox",
        ));
    }
    Ok(RectangleText {
        name,
        direction,
        anchor,
        margins: margins
            .ok_or_else(|| PluginError::corrupt("rect/drawText is missing textMargin"))?,
        blocks,
    })
}

fn parse_rectangle_text_sublist(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    styles: &SectionStyles<'_>,
    depth: usize,
) -> Result<(TextBoxDirection, TextBoxAnchor, Vec<Block>)> {
    validate_shape_attributes(
        start,
        "rect/drawText/subList",
        &[
            "id",
            "textDirection",
            "lineWrap",
            "vertAlign",
            "linkListIDRef",
            "linkListNextIDRef",
            "textWidth",
            "textHeight",
            "hasTextRef",
            "hasNumRef",
        ],
    )?;
    let direction = match required_shape_attr(start, "rect/drawText/subList", "textDirection")?
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "HORIZONTAL" => TextBoxDirection::Horizontal,
        "VERTICALALL" => TextBoxDirection::VerticalAll,
        "VERTICAL" => {
            return Err(PluginError::unsupported_feature(
                "drawText textDirection=VERTICAL has no native-oracle mapping yet",
            ));
        }
        other => {
            return Err(PluginError::corrupt(format!(
                "rect/drawText/subList has invalid textDirection {other}"
            )));
        }
    };
    let line_wrap = required_shape_attr(start, "rect/drawText/subList", "lineWrap")?;
    if !line_wrap.eq_ignore_ascii_case("break") {
        return Err(PluginError::unsupported_feature(format!(
            "drawText lineWrap={line_wrap} is outside the verified BREAK profile"
        )));
    }
    let anchor = match required_shape_attr(start, "rect/drawText/subList", "vertAlign")?
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "TOP" => TextBoxAnchor::Top,
        "CENTER" => TextBoxAnchor::Center,
        "BOTTOM" => TextBoxAnchor::Bottom,
        other => {
            return Err(PluginError::corrupt(format!(
                "rect/drawText/subList has invalid vertAlign {other}"
            )));
        }
    };
    for name in [
        "linkListIDRef",
        "linkListNextIDRef",
        "hasTextRef",
        "hasNumRef",
    ] {
        if parse_optional_u32(start, "rect/drawText/subList", name, 0)? != 0 {
            return Err(PluginError::unsupported_feature(format!(
                "rect/drawText/subList {name} is active and has no verified DOCX mapping"
            )));
        }
    }
    for name in ["textWidth", "textHeight"] {
        let _ = parse_optional_u32(start, "rect/drawText/subList", name, 0)?;
    }

    let mut blocks = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(
                    "unexpected end of XML inside rect/drawText/subList",
                ));
            }
            Event::Start(event) => {
                let child = local_name(event.name().as_ref());
                if child != "p" {
                    return Err(PluginError::corrupt(format!(
                        "rect/drawText/subList contains unexpected direct child {child}"
                    )));
                }
                let owned = event.into_owned();
                blocks.extend(parse_paragraph(reader, &owned, styles, depth, None)?);
            }
            Event::Text(value) if !value.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(
                    "rect/drawText/subList contains direct text",
                ));
            }
            Event::CData(value) if !String::from_utf8_lossy(value.as_ref()).trim().is_empty() => {
                return Err(PluginError::corrupt(
                    "rect/drawText/subList contains direct CDATA",
                ));
            }
            Event::End(event) if local_name(event.name().as_ref()) == "subList" => break,
            _ => {}
        }
        buf.clear();
    }
    Ok((direction, anchor, blocks))
}

fn blocks_contain_rectangle(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(paragraph) => paragraph.inlines.iter().any(|inline| match inline {
            Inline::Rectangle(_) => true,
            Inline::Note(note) => blocks_contain_rectangle(&note.blocks),
            _ => false,
        }),
        Block::Table(table) => table
            .cells
            .iter()
            .any(|cell| blocks_contain_rectangle(&cell.blocks)),
    })
}

fn validate_shape_attributes(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    allowed: &[&str],
) -> Result<()> {
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| {
            PluginError::corrupt(format!("{label} has an invalid attribute: {error}"))
        })?;
        let raw_name = String::from_utf8_lossy(attribute.key.as_ref());
        if raw_name == "xmlns" || raw_name.starts_with("xmlns:") {
            continue;
        }
        let name = local_name(attribute.key.as_ref());
        if !allowed.iter().any(|candidate| *candidate == name) {
            return Err(PluginError::unsupported_feature(format!(
                "{label} contains unsupported active attribute {name}"
            )));
        }
    }
    Ok(())
}

fn required_shape_attr(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    name: &str,
) -> Result<String> {
    attr(event, name)
        .ok_or_else(|| PluginError::corrupt(format!("{label} is missing required {name}")))
}

fn parse_required_u32(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    name: &str,
) -> Result<u32> {
    let raw = required_shape_attr(event, label, name)?;
    raw.trim().parse::<u32>().map_err(|_| {
        PluginError::corrupt(format!("{label} has invalid unsigned {name} value {raw:?}"))
    })
}

fn parse_optional_u32(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    name: &str,
    default: u32,
) -> Result<u32> {
    let Some(raw) = attr(event, name) else {
        return Ok(default);
    };
    raw.trim().parse::<u32>().map_err(|_| {
        PluginError::corrupt(format!("{label} has invalid unsigned {name} value {raw:?}"))
    })
}

fn parse_required_i64(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    name: &str,
) -> Result<i64> {
    let raw = required_shape_attr(event, label, name)?;
    raw.trim().parse::<i64>().map_err(|_| {
        PluginError::corrupt(format!("{label} has invalid integer {name} value {raw:?}"))
    })
}

fn parse_optional_i64(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    name: &str,
    default: i64,
) -> Result<i64> {
    let Some(raw) = attr(event, name) else {
        return Ok(default);
    };
    raw.trim().parse::<i64>().map_err(|_| {
        PluginError::corrupt(format!("{label} has invalid integer {name} value {raw:?}"))
    })
}

fn parse_required_shape_bool(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    name: &str,
) -> Result<bool> {
    let raw = required_shape_attr(event, label, name)?;
    parse_shape_bool_value(&raw, label, name)
}

fn parse_optional_shape_bool(
    event: &quick_xml::events::BytesStart<'_>,
    label: &str,
    name: &str,
    default: bool,
) -> Result<bool> {
    let Some(raw) = attr(event, name) else {
        return Ok(default);
    };
    parse_shape_bool_value(&raw, label, name)
}

fn parse_shape_bool_value(raw: &str, label: &str, name: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(PluginError::corrupt(format!(
            "{label} has invalid boolean {name} value {raw:?}"
        ))),
    }
}

/// `hp:autoNum` 가운데 동적 페이지 계수기만 모델로 올린다.
///
/// 각주/미주 본문 첫 런의 FOOTNOTE/ENDNOTE autoNum은 참조 표식의 구조적 사본이다.
/// 실제 DOCX 표식은 `add footnote|endnote`가 다시 만들므로 그 두 종류는 소비만 한다.
/// 그 밖의 표·그림·수식 번호는 T2-4의 목록 구조 없이는 정확히 낮출 수 없으므로
/// 조용히 지우지 않고 거부한다.
fn parse_auto_number(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'static>,
    style: super::model::CharStyle,
    note_context: Option<NoteKind>,
) -> Result<Option<PageNumberField>> {
    let raw_kind = attr(start, "numType")
        .ok_or_else(|| PluginError::corrupt("autoNum is missing required numType"))?;
    let kind = match raw_kind.trim().to_ascii_uppercase().as_str() {
        "PAGE" => Some(PageNumberKind::Page),
        "TOTAL_PAGE" => Some(PageNumberKind::TotalPages),
        "FOOTNOTE" if note_context == Some(NoteKind::Footnote) => None,
        "ENDNOTE" if note_context == Some(NoteKind::Endnote) => None,
        "FOOTNOTE" | "ENDNOTE" if note_context.is_some() => {
            let enclosing = match note_context.expect("guarded by is_some") {
                NoteKind::Footnote => "footnote",
                NoteKind::Endnote => "endnote",
            };
            return Err(PluginError::corrupt(format!(
                "autoNum numType {} does not match its enclosing {enclosing}",
                raw_kind.trim().to_ascii_uppercase()
            )));
        }
        "FOOTNOTE" | "ENDNOTE" => {
            return Err(PluginError::unsupported_feature(format!(
                "autoNum numType {} appears outside its matching note and cannot be dropped",
                raw_kind.trim().to_ascii_uppercase()
            )));
        }
        other => {
            return Err(PluginError::unsupported_feature(format!(
                "autoNum numType {other} cannot yet be represented without its numbering structure"
            )));
        }
    };

    let mut saw_format = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt("unexpected end of XML inside autoNum"));
            }
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                if name != "autoNumFormat" {
                    return Err(PluginError::unsupported_feature(format!(
                        "autoNum contains unsupported child {name}"
                    )));
                }
                if saw_format {
                    return Err(PluginError::corrupt(
                        "autoNum contains more than one autoNumFormat",
                    ));
                }
                saw_format = true;
                if kind.is_some() {
                    validate_page_auto_number_format(&event)?;
                }
                consume_empty_auto_number_format(reader)?;
            }
            Event::Text(text) if !text.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(
                    "autoNum contains unexpected text content",
                ));
            }
            Event::CData(text) if !String::from_utf8_lossy(text.as_ref()).trim().is_empty() => {
                return Err(PluginError::corrupt(
                    "autoNum contains unexpected CDATA content",
                ));
            }
            Event::GeneralRef(_) => {
                return Err(PluginError::corrupt(
                    "autoNum contains an unexpected entity reference",
                ));
            }
            Event::End(event) if local_name(event.name().as_ref()) == "autoNum" => break,
            _ => {}
        }
        buf.clear();
    }

    if !saw_format {
        return Err(PluginError::corrupt(
            "autoNum is missing required autoNumFormat",
        ));
    }

    Ok(kind.map(|kind| PageNumberField { kind, style }))
}

fn validate_page_auto_number_format(event: &quick_xml::events::BytesStart<'_>) -> Result<()> {
    let number_format = attr(event, "type").unwrap_or_else(|| "DIGIT".to_owned());
    let user_char = attr(event, "userChar").unwrap_or_default();
    let prefix = attr(event, "prefixChar").unwrap_or_default();
    let suffix = attr(event, "suffixChar").unwrap_or_default();
    let superscript = parse_bool_attr(event, "supscript")?;

    if number_format.eq_ignore_ascii_case("DIGIT")
        && user_char.is_empty()
        && prefix.is_empty()
        && suffix.is_empty()
        && !superscript
    {
        return Ok(());
    }

    Err(PluginError::unsupported_feature(format!(
        "autoNum PAGE/TOTAL_PAGE format is not an exact DOCX mapping: type={number_format:?}, userChar={user_char:?}, prefixChar={prefix:?}, suffixChar={suffix:?}, supscript={superscript}"
    )))
}

fn consume_empty_auto_number_format(reader: &mut Reader<&[u8]>) -> Result<()> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(PluginError::corrupt(
                    "unexpected end of XML inside autoNumFormat",
                ));
            }
            Event::Start(event) => {
                return Err(PluginError::corrupt(format!(
                    "autoNumFormat must be empty, found child {}",
                    local_name(event.name().as_ref())
                )));
            }
            Event::Text(text) if !text.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt("autoNumFormat must not contain text"));
            }
            Event::CData(text) if !String::from_utf8_lossy(text.as_ref()).trim().is_empty() => {
                return Err(PluginError::corrupt("autoNumFormat must not contain CDATA"));
            }
            Event::GeneralRef(_) => {
                return Err(PluginError::corrupt(
                    "autoNumFormat must not contain entity references",
                ));
            }
            Event::End(event) if local_name(event.name().as_ref()) == "autoNumFormat" => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
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
    styles: &SectionStyles<'_>,
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
    if let Some(user_char) = parse_note_wchar_attr(start, "userChar", &tag)? {
        return Err(PluginError::unsupported_feature(format!(
            "{tag} userChar {user_char:?} cannot be preserved without changing automatic note numbering"
        )));
    }
    let reference_prefix = parse_note_wchar_attr(start, "prefixChar", &tag)?;
    let reference_suffix = parse_note_wchar_attr(start, "suffixChar", &tag)?;

    let mut blocks = Vec::new();
    let mut saw_sub_list = false;
    let mut inside_sub_list = false;
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
                if name == "subList" {
                    if saw_sub_list {
                        return Err(PluginError::corrupt(format!(
                            "{tag} contains more than one subList"
                        )));
                    }
                    saw_sub_list = true;
                    inside_sub_list = true;
                } else if !inside_sub_list {
                    return Err(PluginError::corrupt(format!(
                        "{tag} contains {name} outside subList"
                    )));
                } else if name == "p" {
                    let owned = event.into_owned();
                    blocks.extend(parse_paragraph(reader, &owned, styles, depth, Some(kind))?);
                } else {
                    return Err(PluginError::corrupt(format!(
                        "{tag} subList contains unexpected direct child {name}"
                    )));
                }
            }
            Event::Text(text) if !text.decode()?.trim().is_empty() => {
                let location = if inside_sub_list {
                    "direct text inside subList"
                } else {
                    "text outside subList"
                };
                return Err(PluginError::corrupt(format!("{tag} contains {location}")));
            }
            Event::CData(text) if !String::from_utf8_lossy(text.as_ref()).trim().is_empty() => {
                let location = if inside_sub_list {
                    "direct CDATA inside subList"
                } else {
                    "CDATA outside subList"
                };
                return Err(PluginError::corrupt(format!("{tag} contains {location}")));
            }
            Event::GeneralRef(_) => {
                return Err(PluginError::corrupt(format!(
                    "{tag} contains an unexpected entity reference"
                )));
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if name == "subList" && inside_sub_list {
                    inside_sub_list = false;
                } else if name == tag {
                    if inside_sub_list {
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
        reference_prefix,
        reference_suffix,
        blocks,
    })
}

fn parse_note_wchar_attr(
    start: &quick_xml::events::BytesStart<'_>,
    name: &str,
    tag: &str,
) -> Result<Option<String>> {
    let Some(raw) = attr(start, name) else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() || raw == "0" {
        return Ok(None);
    }
    let value = raw.parse::<u32>().map_err(|_| {
        PluginError::corrupt(format!("{tag} has invalid UTF-16 {name} value {raw:?}"))
    })?;
    if value > u16::MAX as u32 {
        return Err(PluginError::corrupt(format!(
            "{tag} {name} value {value} exceeds a UTF-16 code unit"
        )));
    }
    let character = char::from_u32(value).ok_or_else(|| {
        PluginError::corrupt(format!(
            "{tag} {name} value {value} is an unpaired UTF-16 surrogate"
        ))
    })?;
    Ok(Some(character.to_string()))
}

fn blocks_contain_note(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(paragraph) => paragraph.inlines.iter().any(|inline| match inline {
            Inline::Note(_) => true,
            Inline::Rectangle(rectangle) => rectangle
                .text
                .as_ref()
                .is_some_and(|text| blocks_contain_note(&text.blocks)),
            _ => false,
        }),
        Block::Table(table) => table
            .cells
            .iter()
            .any(|cell| blocks_contain_note(&cell.blocks)),
    })
}

fn blocks_contain_note_kind(blocks: &[Block], kind: NoteKind) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(paragraph) => paragraph.inlines.iter().any(|inline| match inline {
            Inline::Note(note) => note.kind == kind,
            Inline::Rectangle(rectangle) => rectangle
                .text
                .as_ref()
                .is_some_and(|text| blocks_contain_note_kind(&text.blocks, kind)),
            _ => false,
        }),
        Block::Table(table) => table
            .cells
            .iter()
            .any(|cell| blocks_contain_note_kind(&cell.blocks, kind)),
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
    styles: &SectionStyles<'_>,
    depth: usize,
    note_context: Option<NoteKind>,
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
                        let mut cell = parse_cell(reader, &owned, styles, depth, note_context)?;
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
    styles: &SectionStyles<'_>,
    depth: usize,
    note_context: Option<NoteKind>,
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
                        cell.blocks.extend(parse_paragraph(
                            reader,
                            &owned,
                            styles,
                            depth + 1,
                            note_context,
                        )?);
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
