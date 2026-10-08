//! Source annotations for the existing XML event stream, not a conversion model.

use super::binary_catalog::{BinaryCatalog, Resolution};
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ops::Range;

const MAX_SOURCE_DEPTH: usize = 128;
const MAX_SOURCE_ELEMENTS: usize = 100_000;
const MAX_SOURCE_LINKS: usize = 200_000;
const CORE_NAMESPACE: &[u8] = b"http://www.hancom.co.kr/hwpml/2011/core";

#[derive(Clone, Debug)]
struct SourceEntry {
    path: String,
    kind: &'static str,
    range: Range<u64>,
    format: BTreeMap<String, Value>,
    paragraphs: Vec<String>,
    texts: Vec<String>,
    scope: Option<u64>,
}

#[derive(Clone, Debug)]
struct Frame {
    tag: &'static str,
    entry: Option<usize>,
    paragraph: usize,
    next_text: usize,
    start: u64,
}

/// Direct `hc:img` references and `hp:shapeComment` text of one `hp:pic`.
///
/// Picture details are read-only metadata. Malformed or repeated details are
/// omitted instead of failing the whole read session.
#[derive(Clone, Debug, Default)]
struct PictureParts {
    references: Vec<Option<String>>,
    comments: usize,
    comment: String,
    comment_unreadable: bool,
    sizes: BTreeMap<&'static str, Vec<Option<Value>>>,
}

#[derive(Clone, Debug)]
pub(super) struct SourceIndex {
    part: String,
    pub(super) revision: String,
    section: usize,
    entries: Vec<SourceEntry>,
    by_path: BTreeMap<String, usize>,
    stack: Vec<Frame>,
    counts: BTreeMap<&'static str, usize>,
    field_ends: BTreeMap<String, Vec<(u64, Option<u64>)>>,
    pictures: BTreeMap<usize, PictureParts>,
    elements: usize,
    links: usize,
}

impl SourceIndex {
    pub(super) fn new(section: usize, part: &str, xml: &[u8]) -> Self {
        let digest = Sha256::digest(xml);
        Self {
            part: part.into(),
            revision: format!(
                "sha256:{}",
                digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
            section,
            entries: Vec::new(),
            by_path: BTreeMap::new(),
            stack: Vec::new(),
            counts: BTreeMap::new(),
            field_ends: BTreeMap::new(),
            pictures: BTreeMap::new(),
            elements: 0,
            links: 0,
        }
    }

    /// Index of the picture entry when the innermost open element is its
    /// direct `hp:pic` element.
    fn open_picture(&self) -> Option<usize> {
        match self.stack.last() {
            Some(Frame {
                tag: "pic",
                entry: Some(index),
                ..
            }) => Some(*index),
            _ => None,
        }
    }

    /// Picture whose direct `hp:shapeComment` is the innermost open element.
    fn open_shape_comment(&self) -> Option<usize> {
        match self.stack.as_slice() {
            [.., Frame {
                tag: "pic",
                entry: Some(index),
                ..
            }, Frame {
                tag: "shapeComment",
                ..
            }] => Some(*index),
            _ => None,
        }
    }

    pub(super) fn observe(
        &mut self,
        reader: &NsReader<&[u8]>,
        event: &Event<'_>,
        start: u64,
    ) -> Result<()> {
        match event {
            Event::Start(element) | Event::Empty(element) => {
                self.elements += 1;
                if self.elements > MAX_SOURCE_ELEMENTS || self.stack.len() >= MAX_SOURCE_DEPTH {
                    return Err(PluginError::unsupported_feature(
                        "HWPX source index exceeds XML element/depth budget",
                    ));
                }
                let tag = source_tag(reader, element)?;
                let empty = matches!(event, Event::Empty(_));
                let mut frame = Frame {
                    tag,
                    entry: None,
                    paragraph: 0,
                    next_text: 0,
                    start,
                };
                let kind = match tag {
                    "p" => "paragraph",
                    "t" => "text",
                    "tbl" => "table",
                    "tc" => "cell",
                    "footNote" | "endNote" => "note",
                    "fieldBegin" => "field",
                    "pic" => "picture",
                    _ => "",
                };
                let path = if kind == "text" {
                    if self.stack.len() >= 2 && self.stack.last().is_some_and(|f| f.tag == "run") {
                        let n = self.stack.len() - 2;
                        let parent = &mut self.stack[n];
                        if parent.tag == "p" {
                            let path = text_path(self.section, parent.paragraph, parent.next_text);
                            parent.next_text += 1;
                            Some(path)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else if !kind.is_empty() {
                    let ordinal = self.counts.entry(kind).or_default();
                    frame.paragraph = *ordinal;
                    *ordinal += 1;
                    Some(format!("{}/{kind}[{ordinal}]", section_path(self.section)))
                } else {
                    None
                };
                // Fields may cross paragraphs, but never guess across separate subList stories.
                let scope = self
                    .stack
                    .iter()
                    .rev()
                    .find(|f| f.tag == "subList")
                    .map(|f| f.start);

                // Picture details come only from direct children of `hp:pic`.
                if let Some(picture) = self.open_picture() {
                    match tag {
                        "img" => {
                            let reference = exact_attribute(element, b"binaryItemIDRef")?;
                            self.pictures
                                .entry(picture)
                                .or_default()
                                .references
                                .push(reference);
                        }
                        "orgSz" | "curSz" | "sz" => {
                            let key = match tag {
                                "orgSz" => "original_size",
                                "curSz" => "current_size",
                                _ => "size",
                            };
                            let size = picture_size(element)?;
                            self.pictures
                                .entry(picture)
                                .or_default()
                                .sizes
                                .entry(key)
                                .or_default()
                                .push(size);
                        }
                        "shapeComment" => {
                            self.pictures.entry(picture).or_default().comments += 1;
                        }
                        _ => {}
                    }
                }

                if let Some(path) = path {
                    let mut format = BTreeMap::new();
                    let parent = self
                        .stack
                        .iter()
                        .rev()
                        .find_map(|f| f.entry)
                        .map(|i| self.entries[i].path.clone())
                        .unwrap_or_else(|| section_path(self.section));
                    format.insert("parent_path".into(), json!(parent));
                    format.insert("self_closing".into(), json!(empty));
                    match kind {
                        "table" => {
                            number_attr(&mut format, element, b"rowCnt", "rows")?;
                            number_attr(&mut format, element, b"colCnt", "cols")?;
                        }
                        "note" => {
                            format.insert(
                                "note_kind".into(),
                                json!(if tag == "footNote" {
                                    "footnote"
                                } else {
                                    "endnote"
                                }),
                            );
                            number_attr(&mut format, element, b"number", "number")?;
                            string_attr(&mut format, element, b"instId", "instance_id")?;
                        }
                        "field" => {
                            string_attr(&mut format, element, b"id", "field_id")?;
                            string_attr(&mut format, element, b"type", "field_type")?;
                            string_attr(&mut format, element, b"editable", "declared_editable")?;
                        }
                        "picture" => {
                            string_attr(&mut format, element, b"id", "object_id")?;
                        }
                        _ => {}
                    }
                    string_attr(&mut format, element, b"name", "name")?;
                    for i in self.stack.iter().filter_map(|f| f.entry) {
                        let targets = match kind {
                            "paragraph" => &mut self.entries[i].paragraphs,
                            "text" => &mut self.entries[i].texts,
                            _ => continue,
                        };
                        charge_links(&mut self.links, 1)?;
                        targets.push(path.clone());
                    }
                    frame.entry = Some(self.entries.len());
                    if kind == "picture" {
                        self.pictures
                            .insert(self.entries.len(), PictureParts::default());
                    }
                    self.by_path.insert(path.clone(), self.entries.len());
                    self.entries.push(SourceEntry {
                        path,
                        kind,
                        range: start..reader.buffer_position(),
                        format,
                        paragraphs: Vec::new(),
                        texts: Vec::new(),
                        scope,
                    });
                }
                if matches!(tag, "cellAddr" | "cellSpan") {
                    if let Some(Frame {
                        tag: "tc",
                        entry: Some(i),
                        ..
                    }) = self.stack.last()
                    {
                        let format = &mut self.entries[*i].format;
                        if tag == "cellAddr" {
                            number_attr(format, element, b"rowAddr", "row")?;
                            number_attr(format, element, b"colAddr", "col")?;
                        } else {
                            number_attr(format, element, b"rowSpan", "row_span")?;
                            number_attr(format, element, b"colSpan", "col_span")?;
                        }
                    }
                }
                if tag == "fieldEnd" {
                    if let Some(id) = exact_attribute(element, b"beginIDRef")? {
                        self.field_ends.entry(id).or_default().push((start, scope));
                    }
                }
                if !empty {
                    self.stack.push(frame);
                }
            }
            Event::Text(text) => {
                if let Some(picture) = self.open_shape_comment() {
                    let value = text.decode().map(|value| value.into_owned()).ok();
                    self.push_comment(picture, value);
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(picture) = self.open_shape_comment() {
                    let value = resolve_reference(reference).ok();
                    self.push_comment(picture, value);
                }
            }
            Event::CData(text) => {
                if let Some(picture) = self.open_shape_comment() {
                    let value = text.decode().map(|value| value.into_owned()).ok();
                    self.push_comment(picture, value);
                }
            }
            Event::End(_) => {
                let frame = self.stack.pop().ok_or_else(|| {
                    PluginError::corrupt("HWPX source index closing-element underflow")
                })?;
                if let Some(i) = frame.entry {
                    self.entries[i].range.end = reader.buffer_position();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Append decoded comment text; `None` marks the comment unreadable.
    fn push_comment(&mut self, picture: usize, value: Option<String>) {
        let parts = self.pictures.entry(picture).or_default();
        match value {
            Some(value) => parts.comment.push_str(&value),
            None => parts.comment_unreadable = true,
        }
    }

    pub(super) fn finish(&mut self, catalog: &BinaryCatalog) -> Result<()> {
        let mut begins = BTreeMap::<String, usize>::new();
        let texts = self
            .entries
            .iter()
            .filter(|e| e.kind == "text")
            .map(|e| (e.range.clone(), e.path.clone()))
            .collect::<Vec<_>>();
        for entry in self.entries.iter().filter(|e| e.kind == "field") {
            if let Some(id) = entry.format.get("field_id").and_then(Value::as_str) {
                *begins.entry(id.into()).or_default() += 1;
            }
        }
        for entry in self.entries.iter_mut().filter(|e| e.kind == "field") {
            let id = entry.format.get("field_id").and_then(Value::as_str);
            let ends = id.and_then(|id| self.field_ends.get(id));
            let status = match (id, ends) {
                (None | Some(""), _) => "missing_id",
                (Some(id), _) if begins[id] != 1 => "ambiguous",
                (_, Some(ends)) if ends.len() != 1 => "ambiguous",
                (_, None) => "missing_end",
                (_, Some(ends)) if ends[0].0 < entry.range.end => "reversed",
                (_, Some(ends)) if ends[0].1 != entry.scope => "scope_mismatch",
                _ => "matched",
            };
            entry.format.insert("range_status".into(), json!(status));
            if status == "matched" {
                let end = ends.expect("matched end")[0].0;
                entry.format.insert(
                    "content_range".into(),
                    json!({"byte_start":entry.range.end,"byte_end":end}),
                );
                let first = texts.partition_point(|(range, _)| range.start < entry.range.end);
                entry.texts.clear();
                for (range, path) in texts[first..]
                    .iter()
                    .take_while(|(range, _)| range.start < end)
                {
                    if range.end <= end {
                        charge_links(&mut self.links, 1)?;
                        entry.texts.push(path.clone());
                    }
                }
                entry.paragraphs = entry
                    .texts
                    .iter()
                    .filter_map(|path| path.rsplit_once("/text[").map(|(p, _)| p.to_owned()))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                charge_links(&mut self.links, entry.paragraphs.len())?;
            }
        }
        for (index, parts) in std::mem::take(&mut self.pictures) {
            let format = &mut self.entries[index].format;
            if parts.comments == 1 && !parts.comment_unreadable {
                format.insert("shape_comment".into(), json!(parts.comment));
            }
            for (key, values) in parts.sizes {
                // Repeated or unparsable size elements are omitted, not guessed.
                if let [Some(size)] = values.as_slice() {
                    format.insert(key.into(), size.clone());
                }
            }
            let status = match parts.references.as_slice() {
                [] | [None] => "missing_reference",
                [Some(id)] => {
                    format.insert("binary_item_id".into(), json!(id));
                    let resolution = catalog.resolve(id);
                    match &resolution {
                        Resolution::Resolved {
                            part,
                            size,
                            media_type,
                            embedded,
                        } => {
                            format.insert("binary_part".into(), json!(part));
                            format.insert("binary_size".into(), json!(size));
                            if let Some(media_type) = media_type {
                                format.insert("media_type".into(), json!(media_type));
                            }
                            if let Some(embedded) = embedded {
                                format.insert("embedded".into(), json!(embedded));
                            }
                        }
                        Resolution::External => {
                            format.insert("embedded".into(), json!(false));
                        }
                        Resolution::MissingPart {
                            embedded: Some(embedded),
                        } => {
                            format.insert("embedded".into(), json!(embedded));
                        }
                        _ => {}
                    }
                    resolution.status()
                }
                _ => "ambiguous_reference",
            };
            format.insert("binary_status".into(), json!(status));
        }
        Ok(())
    }

    pub(super) fn decorate(
        &self,
        node: &mut DocumentNode,
        editable: bool,
        targets: &BTreeSet<String>,
    ) {
        if let Some(i) = self.by_path.get(&node.path) {
            let entry = &self.entries[*i];
            node.format.extend(entry.format.clone());
            node.format.insert("source".into(), json!({"part":self.part,"revision":self.revision,"byte_start":entry.range.start,"byte_end":entry.range.end}));
            node.format
                .insert("paragraph_paths".into(), json!(entry.paragraphs));
            node.format.insert("text_paths".into(), json!(entry.texts));
            let text_candidate = node.kind == "text" && targets.contains(&node.path);
            let writable = editable && text_candidate;
            let candidate_target_paths = entry
                .texts
                .iter()
                .filter(|path| targets.contains(*path))
                .cloned()
                .collect::<Vec<_>>();
            let target_paths = if editable {
                candidate_target_paths.clone()
            } else {
                Vec::new()
            };
            let (mode, reason) = if !editable {
                ("locked", "read_only_session")
            } else if writable {
                ("text", "plain_text_target")
            } else if !target_paths.is_empty() {
                ("text-targets", "edit_individual_text_targets")
            } else if node.kind == "text" {
                ("locked", "not_paired_plain_text_or_nested_paragraph")
            } else {
                ("locked", "no_supported_text_targets")
            };
            node.format.insert("editable".into(), json!(writable));
            node.format.insert(
                "editability".into(),
                json!({"mode":mode,"reason":reason,"target_paths":target_paths,"text_candidate":text_candidate,"candidate_target_paths":candidate_target_paths}),
            );
        }
        for child in &mut node.children {
            self.decorate(child, editable, targets);
        }
    }

    pub(super) fn structural_nodes(&self) -> impl Iterator<Item = DocumentNode> + '_ {
        self.entries
            .iter()
            .filter(|e| matches!(e.kind, "table" | "cell" | "note" | "field" | "picture"))
            .map(|e| DocumentNode::branch(&e.path, e.kind, Vec::new()))
    }
}

fn charge_links(used: &mut usize, count: usize) -> Result<()> {
    *used = used
        .checked_add(count)
        .filter(|n| *n <= MAX_SOURCE_LINKS)
        .ok_or_else(|| {
            PluginError::unsupported_feature("HWPX source index exceeds relationship budget")
        })?;
    Ok(())
}

fn string_attr(
    format: &mut BTreeMap<String, Value>,
    element: &BytesStart<'_>,
    name: &[u8],
    key: &str,
) -> Result<()> {
    if let Some(value) = exact_attribute(element, name)? {
        format.insert(key.into(), json!(value));
    }
    Ok(())
}

fn number_attr(
    format: &mut BTreeMap<String, Value>,
    element: &BytesStart<'_>,
    name: &[u8],
    key: &str,
) -> Result<()> {
    if let Some(value) = exact_attribute(element, name)? {
        let number = value
            .parse::<u64>()
            .map_err(|_| PluginError::corrupt(format!("HWPX {key} must be an unsigned integer")))?;
        if format.insert(key.into(), json!(number)).is_some() {
            return Err(PluginError::corrupt(format!("HWPX repeats {key} metadata")));
        }
    }
    Ok(())
}

/// Raw HWPUNIT `width`/`height` of a direct picture size child, or `None`
/// when a present dimension is not an integer. Units are not converted.
fn picture_size(element: &BytesStart<'_>) -> Result<Option<Value>> {
    let mut size = Map::new();
    for (name, field) in [
        (b"width".as_slice(), "width"),
        (b"height".as_slice(), "height"),
    ] {
        if let Some(value) = exact_attribute(element, name)? {
            match value.parse::<i64>() {
                Ok(number) => {
                    size.insert(field.into(), json!(number));
                }
                Err(_) => return Ok(None),
            }
        }
    }
    Ok(Some(Value::Object(size)))
}

fn source_tag(reader: &NsReader<&[u8]>, element: &BytesStart<'_>) -> Result<&'static str> {
    let (namespace, name) = reader.resolver().resolve_element(element.name());
    match namespace {
        ResolveResult::Bound(ns) if ns.as_ref() == PARAGRAPH_NAMESPACE => Ok(match name.as_ref() {
            b"p" => "p",
            b"run" => "run",
            b"t" => "t",
            b"tbl" => "tbl",
            b"tc" => "tc",
            b"footNote" => "footNote",
            b"endNote" => "endNote",
            b"fieldBegin" => "fieldBegin",
            b"fieldEnd" => "fieldEnd",
            b"cellAddr" => "cellAddr",
            b"cellSpan" => "cellSpan",
            b"subList" => "subList",
            b"pic" => "pic",
            b"orgSz" => "orgSz",
            b"curSz" => "curSz",
            b"sz" => "sz",
            b"shapeComment" => "shapeComment",
            _ => "",
        }),
        ResolveResult::Bound(ns) if ns.as_ref() == CORE_NAMESPACE => Ok(match name.as_ref() {
            b"img" => "img",
            _ => "",
        }),
        ResolveResult::Unknown(_) => Err(PluginError::corrupt(
            "HWPX source index encountered an undeclared prefix",
        )),
        _ => Ok(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(xml: &[u8]) -> Result<SectionIndex> {
        scan_section(0, "Contents/section0.xml", xml, &BinaryCatalog::default())
    }

    #[test]
    fn nested_structure_and_ambiguous_fields_never_guess_targets() {
        let xml = br#"<s xmlns:q="http://www.hancom.co.kr/hwpml/2011/paragraph"><q:p><q:run>
<q:tbl rowCnt="1"><q:tr><q:tc><q:subList><q:p><q:run><q:tbl colCnt="1"><q:tr><q:tc><q:subList><q:p><q:run><q:t>deep</q:t></q:run></q:p></q:subList></q:tc></q:tr></q:tbl></q:run></q:p></q:subList></q:tc></q:tr></q:tbl>
<q:endNote><q:subList><q:p><q:run><q:t>note</q:t></q:run></q:p></q:subList></q:endNote>
<q:fieldBegin id="1" name="same"/><q:t>a</q:t><q:fieldEnd beginIDRef="1"/>
<q:fieldBegin id="1" name="same"/><q:t>b</q:t><q:fieldEnd beginIDRef="1"/>
<q:fieldBegin id="2"/><q:fieldBegin/><q:fieldEnd beginIDRef="3"/><q:fieldBegin id="3"/>
<q:subList><q:fieldBegin id="4"/></q:subList><q:fieldEnd beginIDRef="4"/>
</q:run></q:p></s>"#;
        let section = scan(xml).unwrap();
        let tree = section.node(0, true);
        let node = |kind: &str, ordinal: usize| {
            find_node(&tree, &format!("/document/section[1]/{kind}[{ordinal}]")).unwrap()
        };
        assert_eq!(
            node("table", 2).format["parent_path"],
            "/document/section[1]/paragraph[2]"
        );
        assert_eq!(
            node("cell", 1).format["paragraph_paths"],
            json!([
                "/document/section[1]/paragraph[2]",
                "/document/section[1]/paragraph[3]"
            ])
        );
        assert!(!node("cell", 2).format.contains_key("row"));
        assert_eq!(node("note", 1).format["note_kind"], "endnote");
        for (i, status) in [
            "ambiguous",
            "ambiguous",
            "missing_end",
            "missing_id",
            "reversed",
            "scope_mismatch",
        ]
        .iter()
        .enumerate()
        {
            let field = node("field", i + 1);
            assert_eq!(field.format["range_status"], *status);
            assert!(field.format["text_paths"].as_array().unwrap().is_empty());
        }
        for path in [
            "/document/section[1]/paragraph[3]/text[1]",
            "/document/section[1]/paragraph[4]/text[1]",
        ] {
            let text = find_node(&tree, path).unwrap();
            let source = &text.format["source"];
            let bytes = &xml[source["byte_start"].as_u64().unwrap() as usize
                ..source["byte_end"].as_u64().unwrap() as usize];
            assert!(bytes.starts_with(b"<q:t>"));
            assert!(bytes.ends_with(b"</q:t>"));
        }
    }

    #[test]
    fn pictures_expose_only_direct_core_references_and_raw_sizes() {
        let hpf = br#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>
<opf:item id="image1" href="BinData/image1.bmp" media-type="image/bmp" isEmbeded="1"/>
<opf:item id="image2" href="C:\photo.jpg" media-type="image/" isEmbeded="0"/>
</opf:manifest></opf:package>"#;
        let catalog = BinaryCatalog::from_parts(
            Some(hpf),
            BTreeMap::from([("BinData/image1.bmp".to_owned(), 77)]),
        );
        let xml = r#"<s xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core"><hp:p><hp:run>
<hp:pic id="9"><hp:orgSz width="15900" height="15180"/><hp:curSz width="0" height="0"/><hc:img binaryItemIDRef="image1"/><hp:sz width="15900" height="15180"/><hp:shapeComment>그림 &amp; 설명</hp:shapeComment></hp:pic>
<hp:pic><hc:img binaryItemIDRef="image2"/></hp:pic>
<hp:pic><hp:img binaryItemIDRef="image1"/><hp:renderingInfo><hc:img binaryItemIDRef="image1"/></hp:renderingInfo></hp:pic>
<hp:pic><hc:img binaryItemIDRef="image1"/><hc:img binaryItemIDRef="image2"/></hp:pic>
<hp:pic><hc:img binaryItemIDRef="stem"/><hp:caption><hp:subList><hp:p><hp:run><hp:t>caption</hp:t></hp:run></hp:p></hp:subList></hp:caption></hp:pic>
<hp:pic><hp:sz width="1" height="2"/><hp:sz width="3" height="4"/><hp:orgSz width="wide" height="5"/><hp:curSz width="7" height="8"/><hp:shapeComment>bad &nbsp; entity</hp:shapeComment><hc:img binaryItemIDRef="image1"/></hp:pic>
</hp:run></hp:p></s>"#
            .as_bytes();
        let section = scan_section(0, "Contents/section0.xml", xml, &catalog).unwrap();
        let tree = section.node(0, false);
        let picture = |ordinal: usize| {
            find_node(&tree, &format!("/document/section[1]/picture[{ordinal}]"))
                .unwrap()
                .format
                .clone()
        };
        let first = picture(1);
        assert_eq!(first["binary_status"], "resolved");
        assert_eq!(first["binary_item_id"], "image1");
        assert_eq!(first["binary_part"], "BinData/image1.bmp");
        assert_eq!(first["binary_size"], 77);
        assert_eq!(first["media_type"], "image/bmp");
        assert_eq!(first["embedded"], true);
        assert_eq!(first["object_id"], "9");
        assert_eq!(
            first["original_size"],
            json!({"width":15900,"height":15180})
        );
        assert_eq!(first["current_size"], json!({"width":0,"height":0}));
        assert_eq!(first["size"], json!({"width":15900,"height":15180}));
        assert_eq!(first["shape_comment"], "그림 & 설명");
        assert_eq!(first["parent_path"], "/document/section[1]/paragraph[1]");
        let second = picture(2);
        assert_eq!(second["binary_status"], "external");
        assert_eq!(second["embedded"], false);
        assert!(!second.contains_key("binary_part"));
        // A paragraph-namespace img and a nested core img are not direct references.
        assert_eq!(picture(3)["binary_status"], "missing_reference");
        assert!(!picture(3).contains_key("binary_item_id"));
        assert_eq!(picture(4)["binary_status"], "ambiguous_reference");
        let fifth = picture(5);
        assert_eq!(fifth["binary_status"], "missing_item");
        assert_eq!(
            fifth["text_paths"],
            json!(["/document/section[1]/paragraph[2]/text[1]"])
        );
        // Repeated or malformed details are omitted; the read still succeeds.
        let sixth = picture(6);
        assert_eq!(sixth["binary_status"], "resolved");
        assert_eq!(sixth["current_size"], json!({"width":7,"height":8}));
        for omitted in ["size", "original_size", "shape_comment"] {
            assert!(!sixth.contains_key(omitted), "{omitted}: {sixth:?}");
        }
        let source = &first["source"];
        let bytes = &xml[source["byte_start"].as_u64().unwrap() as usize
            ..source["byte_end"].as_u64().unwrap() as usize];
        assert!(bytes.starts_with(b"<hp:pic id=\"9\">"));
        assert!(bytes.ends_with(b"</hp:pic>"));
    }

    #[test]
    fn source_budgets_accept_limit_and_reject_first_excess() {
        for (count, valid) in [(MAX_SOURCE_DEPTH, true), (MAX_SOURCE_DEPTH + 1, false)] {
            let xml = format!("{}{}", "<x>".repeat(count), "</x>".repeat(count));
            assert_eq!(scan(xml.as_bytes()).is_ok(), valid);
        }
        for (count, valid) in [
            (MAX_SOURCE_ELEMENTS, true),
            (MAX_SOURCE_ELEMENTS + 1, false),
        ] {
            let xml = format!("<x>{}</x>", "<y/>".repeat(count - 1));
            assert_eq!(scan(xml.as_bytes()).is_ok(), valid);
        }
        let mut links = 0;
        charge_links(&mut links, MAX_SOURCE_LINKS).unwrap();
        assert!(charge_links(&mut links, 1).is_err());
    }
}
