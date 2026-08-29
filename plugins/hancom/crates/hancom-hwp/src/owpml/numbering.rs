//! OWPML paragraph heading and numbering tables.
//!
//! `hh:numbering/@id` and `hh:bullet/@id` use separate source namespaces.
//! This module assigns one collision-free DOCX id space, but materializes only
//! definitions that an authored paragraph actually references. Unsupported
//! dormant templates therefore do not block conversion.

use std::collections::{BTreeMap, HashMap};

use quick_xml::events::Event;
use quick_xml::Reader;

use super::model::{
    Block, CharStyle, Inline, NumberingDefinition, NumberingFormat, NumberingJustification,
    NumberingLevel, Section,
};
use super::xml::{attr, local_name, resolve_entity};
use crate::error::{PluginError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SourceKind {
    Number,
    Bullet,
}

impl SourceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Number => "numbering",
            Self::Bullet => "bullet",
        }
    }

    fn element(self) -> &'static str {
        match self {
            Self::Number => "numbering",
            Self::Bullet => "bullet",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RawHeading {
    pub kind: Option<String>,
    pub id_ref: Option<String>,
    pub level: Option<String>,
}

impl RawHeading {
    pub fn from_start(start: &quick_xml::events::BytesStart<'_>) -> Self {
        Self {
            kind: attr(start, "type"),
            id_ref: attr(start, "idRef"),
            level: attr(start, "level"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SourceKey {
    kind: SourceKind,
    id: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NumberingCatalog {
    definitions: Vec<RawDefinition>,
    index: HashMap<SourceKey, IndexedDefinition>,
    order: Vec<SourceKey>,
}

#[derive(Debug, Clone)]
struct IndexedDefinition {
    target_id: u32,
    definition_index: usize,
    duplicate: bool,
}

#[derive(Debug, Clone)]
struct RawDefinition {
    kind: SourceKind,
    id: Option<String>,
    complete: bool,
    bullet_char: Option<String>,
    use_image: Option<String>,
    has_image: bool,
    levels: Vec<RawLevel>,
}

impl RawDefinition {
    fn from_start(kind: SourceKind, start: &quick_xml::events::BytesStart<'_>) -> Self {
        Self {
            kind,
            id: attr(start, "id"),
            complete: false,
            bullet_char: attr(start, "char"),
            use_image: attr(start, "useImage"),
            has_image: false,
            levels: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct RawLevel {
    level: Option<String>,
    start: Option<String>,
    align: Option<String>,
    use_instance_width: Option<String>,
    auto_indent: Option<String>,
    width_adjust: Option<String>,
    text_offset_type: Option<String>,
    text_offset: Option<String>,
    checkable: Option<String>,
    num_format: Option<String>,
    char_style_ref: Option<String>,
    text: String,
}

impl RawLevel {
    fn from_start(start: &quick_xml::events::BytesStart<'_>) -> Self {
        Self {
            level: attr(start, "level"),
            start: attr(start, "start"),
            align: attr(start, "align"),
            use_instance_width: attr(start, "useInstWidth"),
            auto_indent: attr(start, "autoIndent"),
            width_adjust: attr(start, "widthAdjust"),
            text_offset_type: attr(start, "textOffsetType"),
            text_offset: attr(start, "textOffset"),
            checkable: attr(start, "checkable"),
            num_format: attr(start, "numFormat"),
            char_style_ref: attr(start, "charPrIDRef"),
            text: String::new(),
        }
    }
}

impl NumberingCatalog {
    /// Parse as much of a possibly damaged header as possible. Structural and
    /// semantic errors become fatal only when a paragraph activates the entry.
    pub fn parse(xml: &str) -> Self {
        let mut reader = Reader::from_str(xml);
        let config = reader.config_mut();
        config.trim_text(false);
        config.expand_empty_elements = true;

        let mut definitions = Vec::new();
        let mut current: Option<RawDefinition> = None;
        let mut current_level: Option<RawLevel> = None;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Err(_) | Ok(Event::Eof) => break,
                Ok(Event::Start(start)) => {
                    let name = local_name(start.name().as_ref());
                    if current.is_none() {
                        let kind = match name.as_str() {
                            "numbering" => Some(SourceKind::Number),
                            "bullet" => Some(SourceKind::Bullet),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            current = Some(RawDefinition::from_start(kind, &start));
                        }
                    } else if name == "paraHead" && current_level.is_none() {
                        current_level = Some(RawLevel::from_start(&start));
                    } else if name == "img" {
                        if let Some(definition) = current.as_mut() {
                            definition.has_image = true;
                        }
                    }
                }
                Ok(Event::Text(text)) => {
                    if let Some(level) = current_level.as_mut() {
                        if let Ok(decoded) = text.decode() {
                            level.text.push_str(&decoded);
                        }
                    }
                }
                Ok(Event::CData(text)) => {
                    if let Some(level) = current_level.as_mut() {
                        level.text.push_str(&String::from_utf8_lossy(text.as_ref()));
                    }
                }
                Ok(Event::GeneralRef(reference)) => {
                    if let Some(level) = current_level.as_mut() {
                        match resolve_entity(&reference) {
                            Ok(value) => level.text.push_str(&value),
                            Err(_) => level.text.push_str("&?;"),
                        }
                    }
                }
                Ok(Event::End(end)) => {
                    let name = local_name(end.name().as_ref());
                    if name == "paraHead" {
                        if let (Some(definition), Some(level)) =
                            (current.as_mut(), current_level.take())
                        {
                            definition.levels.push(level);
                        }
                    } else if current
                        .as_ref()
                        .is_some_and(|definition| definition.kind.element() == name)
                    {
                        let mut definition = current.take().expect("checked above");
                        definition.complete = true;
                        definitions.push(definition);
                    }
                }
                _ => {}
            }
            buf.clear();
        }

        if let Some(mut definition) = current {
            if let Some(level) = current_level {
                definition.levels.push(level);
            }
            definitions.push(definition);
        }

        let mut catalog = Self {
            definitions,
            ..Self::default()
        };
        catalog.build_index();
        catalog
    }

    fn build_index(&mut self) {
        for (definition_index, definition) in self.definitions.iter().enumerate() {
            let Some(id) = definition.id.as_ref().filter(|id| !id.trim().is_empty()) else {
                continue;
            };
            let key = SourceKey {
                kind: definition.kind,
                id: id.clone(),
            };
            if let Some(existing) = self.index.get_mut(&key) {
                existing.duplicate = true;
                continue;
            }
            let Ok(target_id) = u32::try_from(self.order.len() + 1) else {
                break;
            };
            self.order.push(key.clone());
            self.index.insert(
                key,
                IndexedDefinition {
                    target_id,
                    definition_index,
                    duplicate: false,
                },
            );
        }
    }

    pub fn resolve_target(&self, kind: SourceKind, source_id: &str) -> Result<u32> {
        let key = SourceKey {
            kind,
            id: source_id.to_string(),
        };
        let Some(indexed) = self.index.get(&key) else {
            return Err(PluginError::corrupt(format!(
                "paragraph references missing {} id {source_id}",
                kind.label()
            )));
        };
        if indexed.duplicate {
            return Err(PluginError::corrupt(format!(
                "paragraph references ambiguous duplicate {} id {source_id}",
                kind.label()
            )));
        }
        Ok(indexed.target_id)
    }

    pub fn materialize(
        &self,
        sections: &[Section],
        char_styles: &HashMap<String, CharStyle>,
    ) -> Result<Vec<NumberingDefinition>> {
        let active = collect_active_numberings(sections);
        let mut output = Vec::with_capacity(active.len());

        for key in &self.order {
            let indexed = self.index.get(key).expect("order and index stay in sync");
            let Some(&max_level) = active.get(&indexed.target_id) else {
                continue;
            };
            if indexed.duplicate {
                return Err(PluginError::corrupt(format!(
                    "active {} id {} has duplicate definitions",
                    key.kind.label(),
                    key.id
                )));
            }
            let raw = &self.definitions[indexed.definition_index];
            if !raw.complete {
                return Err(PluginError::corrupt(format!(
                    "active {} id {} is incomplete in header.xml",
                    key.kind.label(),
                    key.id
                )));
            }
            let levels = match key.kind {
                SourceKind::Number => {
                    materialize_number_levels(raw, max_level, char_styles, &key.id)?
                }
                SourceKind::Bullet => {
                    materialize_bullet_levels(raw, max_level, char_styles, &key.id)?
                }
            };
            output.push(NumberingDefinition {
                id: indexed.target_id,
                bullet: key.kind == SourceKind::Bullet,
                levels,
            });
        }
        Ok(output)
    }
}

fn collect_active_numberings(sections: &[Section]) -> BTreeMap<u32, u8> {
    fn walk(blocks: &[Block], active: &mut BTreeMap<u32, u8>) {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    if let Some(numbering) = paragraph.style.numbering {
                        active
                            .entry(numbering.num_id)
                            .and_modify(|level| *level = (*level).max(numbering.level))
                            .or_insert(numbering.level);
                    }
                    for inline in &paragraph.inlines {
                        if let Inline::Note(note) = inline {
                            walk(&note.blocks, active);
                        }
                    }
                }
                Block::Table(table) => {
                    for cell in &table.cells {
                        walk(&cell.blocks, active);
                    }
                }
            }
        }
    }

    let mut active = BTreeMap::new();
    for section in sections {
        walk(&section.blocks, &mut active);
        for story in section.headers.iter().chain(&section.footers) {
            walk(&story.blocks, &mut active);
        }
    }
    active
}

fn materialize_number_levels(
    raw: &RawDefinition,
    max_level: u8,
    char_styles: &HashMap<String, CharStyle>,
    source_id: &str,
) -> Result<Vec<NumberingLevel>> {
    let mut selected: BTreeMap<u8, &RawLevel> = BTreeMap::new();
    for level in &raw.levels {
        let source_level = parse_u8(level.level.as_deref(), "numbering paraHead level")?;
        if source_level == 0 {
            return Err(PluginError::corrupt(format!(
                "numbering id {source_id} has invalid one-based paraHead level 0"
            )));
        }
        let target_level = source_level - 1;
        if target_level > max_level {
            continue;
        }
        if selected.insert(target_level, level).is_some() {
            return Err(PluginError::corrupt(format!(
                "numbering id {source_id} repeats paraHead level {source_level}"
            )));
        }
    }

    let mut output = Vec::with_capacity(usize::from(max_level) + 1);
    for target_level in 0..=max_level {
        let Some(raw_level) = selected.get(&target_level).copied() else {
            return Err(PluginError::corrupt(format!(
                "numbering id {source_id} is missing required paraHead level {}",
                target_level + 1
            )));
        };
        let format = parse_number_format(raw_level.num_format.as_deref())?;
        let text = expand_numbering_text(raw_level.text.trim(), target_level)?;
        if text.is_empty() {
            return Err(PluginError::corrupt(format!(
                "numbering id {source_id} level {} has an empty marker template",
                target_level + 1
            )));
        }
        output.push(NumberingLevel {
            level: target_level,
            start: parse_u32(raw_level.start.as_deref(), "numbering start")?,
            format,
            text,
            justification: validate_level_layout(raw_level, "numbering", source_id)?,
            marker_style: resolve_marker_style(raw_level, char_styles, "numbering", source_id)?,
        });
    }
    Ok(output)
}

fn materialize_bullet_levels(
    raw: &RawDefinition,
    max_level: u8,
    char_styles: &HashMap<String, CharStyle>,
    source_id: &str,
) -> Result<Vec<NumberingLevel>> {
    if parse_bool(raw.use_image.as_deref(), false, "bullet useImage")? || raw.has_image {
        return Err(PluginError::unsupported_feature(format!(
            "active bullet id {source_id} uses an image marker"
        )));
    }
    if raw.levels.len() != 1 {
        return Err(PluginError::corrupt(format!(
            "active bullet id {source_id} must contain exactly one paraHead"
        )));
    }
    let raw_level = &raw.levels[0];
    if parse_bool(raw_level.checkable.as_deref(), false, "bullet checkable")? {
        return Err(PluginError::unsupported_feature(format!(
            "active bullet id {source_id} is checkable"
        )));
    }
    let source_level = parse_u8(raw_level.level.as_deref(), "bullet paraHead level")?;
    if source_level != 0 {
        return Err(PluginError::corrupt(format!(
            "bullet id {source_id} paraHead level must be 0, got {source_level}"
        )));
    }
    let marker = raw
        .bullet_char
        .as_ref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            PluginError::unsupported_feature(format!(
                "active bullet id {source_id} has no textual marker"
            ))
        })?
        .clone();
    let justification = validate_level_layout(raw_level, "bullet", source_id)?;
    let marker_style = resolve_marker_style(raw_level, char_styles, "bullet", source_id)?;

    Ok((0..=max_level)
        .map(|level| NumberingLevel {
            level,
            start: 1,
            format: NumberingFormat::Bullet,
            text: marker.clone(),
            justification,
            marker_style: marker_style.clone(),
        })
        .collect())
}

fn validate_level_layout(
    level: &RawLevel,
    kind: &str,
    source_id: &str,
) -> Result<NumberingJustification> {
    if parse_bool(level.checkable.as_deref(), false, "paraHead checkable")? {
        return Err(PluginError::unsupported_feature(format!(
            "active {kind} id {source_id} uses a checkable marker"
        )));
    }
    if let Some(raw) = level.width_adjust.as_deref() {
        let value = parse_i64(raw).ok_or_else(|| {
            PluginError::corrupt(format!(
                "active {kind} id {source_id} has invalid widthAdjust {raw:?}"
            ))
        })?;
        if value != 0 {
            return Err(PluginError::unsupported_feature(format!(
                "active {kind} id {source_id} uses widthAdjust={raw}, which DOCX numbering cannot represent"
            )));
        }
    }

    // HWP's instance-width and automatic-hanging controls do not have direct
    // OOXML fields. Bound the compatibility mapping to profiles seen in the
    // public corpus and Hancom's own DOCX export oracle:
    //   * automatic NUMBER: autoIndent=1, PERCENT/50
    //   * automatic BULLET: autoIndent=1, PERCENT/{10,15,50}
    //   * neutral:   autoIndent=0, offset=0
    // Hancom emits all three automatic BULLET offsets with the same OOXML
    // level geometry (zero explicit indent/hanging and a space suffix). Do not
    // extend that evidence to NUMBER or to unobserved offset values.
    // Both useInstWidth values occur in native-authored NUMBER/BULLET data;
    // validate the boolean but preserve neither as a guessed fixed indent.
    let _use_instance_width = parse_bool(
        level.use_instance_width.as_deref(),
        true,
        "paraHead useInstWidth",
    )?;
    let auto_indent = parse_bool(level.auto_indent.as_deref(), true, "paraHead autoIndent")?;
    let offset_type = level
        .text_offset_type
        .as_deref()
        .unwrap_or("PERCENT")
        .to_ascii_uppercase();
    let offset = level
        .text_offset
        .as_deref()
        .map(|raw| {
            parse_i64(raw).ok_or_else(|| {
                PluginError::corrupt(format!(
                    "active {kind} id {source_id} has invalid textOffset {raw:?}"
                ))
            })
        })
        .transpose()?
        .unwrap_or(if auto_indent { 50 } else { 0 });
    let automatic_profile = auto_indent
        && offset_type == "PERCENT"
        && (offset == 50 || (kind == "bullet" && matches!(offset, 10 | 15)));
    let supported_profile = automatic_profile
        || (!auto_indent && offset == 0 && matches!(offset_type.as_str(), "PERCENT" | "HWPUNIT"));
    if !supported_profile {
        return Err(PluginError::unsupported_feature(format!(
            "active {kind} id {source_id} uses unverified list layout autoIndent={} textOffsetType={} textOffset={offset}",
            u8::from(auto_indent),
            offset_type
        )));
    }
    match level
        .align
        .as_deref()
        .unwrap_or("LEFT")
        .to_ascii_uppercase()
        .as_str()
    {
        "LEFT" => Ok(NumberingJustification::Left),
        "CENTER" => Ok(NumberingJustification::Center),
        "RIGHT" => Ok(NumberingJustification::Right),
        other => Err(PluginError::unsupported_feature(format!(
            "active {kind} id {source_id} uses unsupported marker alignment {other}"
        ))),
    }
}

fn resolve_marker_style(
    level: &RawLevel,
    char_styles: &HashMap<String, CharStyle>,
    kind: &str,
    source_id: &str,
) -> Result<CharStyle> {
    let Some(reference) = level
        .char_style_ref
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "4294967295")
    else {
        return Ok(CharStyle::default());
    };
    let style = char_styles.get(reference).ok_or_else(|| {
        PluginError::corrupt(format!(
            "active {kind} id {source_id} references missing charPr {reference}"
        ))
    })?;
    if style.underline || style.strike || style.highlight.is_some() || style.vert_align.is_some() {
        return Err(PluginError::unsupported_feature(format!(
            "active {kind} id {source_id} uses marker character formatting that OfficeCLI abstractNum cannot represent"
        )));
    }
    Ok(style.clone())
}

fn parse_number_format(raw: Option<&str>) -> Result<NumberingFormat> {
    let raw = raw
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| PluginError::corrupt("active numbering paraHead has no numFormat"))?;
    match raw.to_ascii_uppercase().as_str() {
        "DIGIT" => Ok(NumberingFormat::Decimal),
        "CIRCLED_DIGIT" => Ok(NumberingFormat::DecimalEnclosedCircle),
        "ROMAN_CAPITAL" => Ok(NumberingFormat::UpperRoman),
        "ROMAN_SMALL" => Ok(NumberingFormat::LowerRoman),
        "LATIN_CAPITAL" => Ok(NumberingFormat::UpperLetter),
        "LATIN_SMALL" => Ok(NumberingFormat::LowerLetter),
        "HANGUL_SYLLABLE" => Ok(NumberingFormat::Ganada),
        "HANGUL_JAMO" => Ok(NumberingFormat::Chosung),
        other => Err(PluginError::unsupported_feature(format!(
            "active numbering uses numFormat {other}, which has no verified DOCX mapping"
        ))),
    }
}

fn expand_numbering_text(raw: &str, level: u8) -> Result<String> {
    let path = || {
        (1..=u16::from(level) + 1)
            .map(|part| format!("%{part}"))
            .collect::<Vec<_>>()
            .join(".")
    };
    let mut output = String::with_capacity(raw.len() + usize::from(level) * 2);
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' && chars.peek().is_some_and(char::is_ascii_digit) {
            return Err(PluginError::unsupported_feature(
                "numbering marker contains a literal % followed by a digit; DOCX would reinterpret it as a level placeholder",
            ));
        }
        if character != '^' {
            output.push(character);
            continue;
        }
        let Some(token) = chars.next() else {
            return Err(PluginError::corrupt(
                "numbering marker ends with an incomplete ^ token",
            ));
        };
        match token {
            'n' => output.push_str(&path()),
            'N' => {
                output.push_str(&path());
                output.push('.');
            }
            '1'..='9' => {
                let referenced = token.to_digit(10).expect("matched digit") as u8;
                if referenced > level + 1 {
                    return Err(PluginError::corrupt(format!(
                        "numbering level {} marker references unavailable level {referenced}",
                        level + 1
                    )));
                }
                output.push('%');
                output.push(token);
            }
            other => {
                return Err(PluginError::unsupported_feature(format!(
                    "numbering marker uses unsupported ^{other} token"
                )));
            }
        }
    }
    Ok(output)
}

fn parse_bool(raw: Option<&str>, default: bool, label: &str) -> Result<bool> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" | "" => Ok(false),
        _ => Err(PluginError::corrupt(format!(
            "invalid {label} boolean {raw:?}"
        ))),
    }
}

fn parse_u8(raw: Option<&str>, label: &str) -> Result<u8> {
    raw.and_then(|value| value.trim().parse::<u8>().ok())
        .ok_or_else(|| PluginError::corrupt(format!("missing or invalid {label}")))
        .and_then(|value| {
            if value <= 9 {
                Ok(value)
            } else {
                Err(PluginError::unsupported_feature(format!(
                    "{label} {value} exceeds the DOCX numbering limit"
                )))
            }
        })
}

fn parse_u32(raw: Option<&str>, label: &str) -> Result<u32> {
    let value = raw
        .and_then(|value| value.trim().parse::<u32>().ok())
        .ok_or_else(|| PluginError::corrupt(format!("missing or invalid {label}")))?;
    if value > i32::MAX as u32 {
        return Err(PluginError::unsupported_feature(format!(
            "{label} {value} exceeds the DOCX integer range"
        )));
    }
    Ok(value)
}

fn parse_i64(raw: &str) -> Option<i64> {
    raw.trim().parse::<i64>().ok()
}
