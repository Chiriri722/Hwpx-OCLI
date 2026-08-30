//! HWPX ZIP 컨테이너 판독.
//!
//! 구조 (근거: `unhwp-0.7.0/src/hwpx/container.rs`):
//! ```text
//! mimetype                  → "application/hwp+zip"
//! Contents/content.hpf       → OPF 패키지. manifest(id→href) + spine(순서)
//! Contents/header.xml        → 글자/문단 모양 정의
//! Contents/section0.xml ...  → 본문
//! BinData/...                → 이미지 등 바이너리
//! ```

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};
use std::sync::Arc;

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

use crate::error::{PluginError, Result};

pub use officecli_hancom_core::container::{
    HWPX_MIMETYPE_ENTRY as MIMETYPE_ENTRY, HWPX_MIMETYPE_VALUE as MIMETYPE_VALUE,
    HWPX_SECTION_PREFIX as SECTION_PREFIX, MAX_MIMETYPE_BYTES,
};
pub const HPF_ENTRY: &str = "Contents/content.hpf";
pub const HEADER_ENTRY: &str = "Contents/header.xml";
pub const BINDATA_PREFIX: &str = "BinData/";
pub const CHART_PREFIX: &str = "Chart/";

pub(super) const MAX_HPF_BYTES: u64 = 4 * 1024 * 1024;
pub(super) const MAX_XML_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CHART_XML_DEPTH: usize = 256;
const MAX_BINARY_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;
pub(super) const MAX_MANIFEST_ITEMS: usize = 4096;
pub(super) const MAX_SPINE_ITEMS: usize = 2048;
pub(super) const MAX_SECTION_COUNT: usize = 1024;
const MAX_COMPRESSION_RATIO: u64 = 1000;

pub struct Package<R: Read + Seek> {
    archive: zip::ZipArchive<R>,
    /// content.hpf manifest의 `id` → `href`.
    manifest: HashMap<String, String>,
    /// spine 순서로 정렬된 섹션 XML 경로.
    section_paths: Vec<String>,
    /// 확장자를 뺀 소문자 stem → BinData ZIP 경로 목록.
    bin_items_by_stem: HashMap<String, Vec<String>>,
    /// 이미 압축 해제한 BinData. 같은 그림을 반복 참조할 때 다시 읽지 않는다.
    bin_cache: HashMap<String, (Arc<[u8]>, String)>,
    /// 검증을 마친 관계 없는 OOXML 차트 파트.
    chart_cache: HashMap<String, Arc<str>>,
    /// 실제로 읽은 압축 해제 바이트의 문서 전체 잔여 예산.
    remaining_expanded_bytes: u64,
}

impl<R: Read + Seek> Package<R> {
    pub fn open(reader: R) -> Result<Self> {
        let mut archive = zip::ZipArchive::new(reader)?;
        validate_archive(&mut archive)?;
        let mut remaining_expanded_bytes = MAX_TOTAL_EXPANDED_BYTES;

        // mimetype은 있으면 검증하고, 없으면 통과시킨다. 일부 생성기가 빠뜨린다.
        if let Some(s) = read_entry_to_string_limited(
            &mut archive,
            MIMETYPE_ENTRY,
            MAX_MIMETYPE_BYTES,
            &mut remaining_expanded_bytes,
        )? {
            let s = s.trim();
            if !s.is_empty() && s != MIMETYPE_VALUE {
                return Err(PluginError::corrupt(format!(
                    "unexpected mimetype: {s:?} (expected {MIMETYPE_VALUE:?})"
                )));
            }
        }

        let hpf = read_entry_to_string_limited(
            &mut archive,
            HPF_ENTRY,
            MAX_HPF_BYTES,
            &mut remaining_expanded_bytes,
        )?
        .unwrap_or_default();
        let manifest = parse_hpf_manifest(&hpf);
        let spine_order = parse_hpf_spine(&hpf);

        let section_paths = resolve_section_paths(&mut archive, &manifest, &spine_order)?;
        let mut bin_items_by_stem: HashMap<String, Vec<String>> = HashMap::new();
        for name in archive
            .file_names()
            .filter(|name| name.starts_with(BINDATA_PREFIX))
        {
            let stem = name
                .rsplit('/')
                .next()
                .unwrap_or(name)
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .unwrap_or(name)
                .to_ascii_lowercase();
            bin_items_by_stem
                .entry(stem)
                .or_default()
                .push(name.to_string());
        }

        Ok(Self {
            archive,
            manifest,
            section_paths,
            bin_items_by_stem,
            bin_cache: HashMap::new(),
            chart_cache: HashMap::new(),
            remaining_expanded_bytes,
        })
    }

    pub fn section_paths(&self) -> &[String] {
        &self.section_paths
    }

    pub fn read_header_xml(&mut self) -> Result<Option<String>> {
        read_entry_to_string_limited(
            &mut self.archive,
            HEADER_ENTRY,
            MAX_XML_ENTRY_BYTES,
            &mut self.remaining_expanded_bytes,
        )
    }

    pub fn read_section_xml(&mut self, path: &str) -> Result<String> {
        read_entry_to_string_limited(
            &mut self.archive,
            path,
            MAX_XML_ENTRY_BYTES,
            &mut self.remaining_expanded_bytes,
        )?
        .ok_or_else(|| PluginError::corrupt(format!("cannot read section: {path}")))
    }

    /// `binaryItemIDRef` → BinData 바이트.
    ///
    /// 우선 content.hpf manifest에서 id로 href를 찾고, 실패하면 BinData 안에서
    /// 확장자를 뺀 파일명이 일치하는 항목을 찾는다.
    pub fn read_bin_item(&mut self, id: &str) -> Result<Option<(Arc<[u8]>, String)>> {
        let mut candidates: Vec<String> = Vec::new();

        if let Some(href) = self.manifest.get(id) {
            candidates.push(href.clone());
            if !href.starts_with(BINDATA_PREFIX) {
                candidates.push(format!("{BINDATA_PREFIX}{href}"));
            }
        }

        if let Some(names) = self.bin_items_by_stem.get(&id.to_ascii_lowercase()) {
            candidates.extend(names.iter().cloned());
        }

        let mut seen = HashSet::new();
        for name in candidates
            .into_iter()
            .filter(|name| seen.insert(name.clone()))
        {
            if let Some((bytes, content_type)) = self.bin_cache.get(&name) {
                return Ok(Some((Arc::clone(bytes), content_type.clone())));
            }
            if let Some(buf) = read_entry_bytes_limited(
                &mut self.archive,
                &name,
                MAX_BINARY_ENTRY_BYTES,
                &mut self.remaining_expanded_bytes,
            )? {
                if !buf.is_empty() {
                    let bytes: Arc<[u8]> = buf.into();
                    let content_type = guess_content_type(&name).to_string();
                    self.bin_cache
                        .insert(name, (Arc::clone(&bytes), content_type.clone()));
                    return Ok(Some((bytes, content_type)));
                }
            }
        }
        Ok(None)
    }

    pub fn read_chart_part(&mut self, path: &str) -> Result<Arc<str>> {
        if !is_safe_chart_part_path(path) {
            return Err(PluginError::corrupt(format!(
                "chartIDRef must be a relative Chart/*.xml path, got {path:?}"
            )));
        }
        if let Some(xml) = self.chart_cache.get(path) {
            return Ok(Arc::clone(xml));
        }
        let bytes = read_entry_bytes_limited(
            &mut self.archive,
            path,
            MAX_XML_ENTRY_BYTES,
            &mut self.remaining_expanded_bytes,
        )?
        .ok_or_else(|| PluginError::corrupt(format!("cannot read chart part: {path}")))?;
        let xml = String::from_utf8(bytes).map_err(|error| {
            PluginError::corrupt(format!("chart part {path:?} is not valid UTF-8: {error}"))
        })?;
        let xml = xml.trim_start_matches('\u{feff}').to_string();
        if xml.trim().is_empty() {
            return Err(PluginError::corrupt(format!(
                "chart part {path:?} is empty"
            )));
        }
        validate_chart_part_xml(&xml, path)?;
        let xml: Arc<str> = xml.into();
        self.chart_cache.insert(path.to_string(), Arc::clone(&xml));
        Ok(xml)
    }
}

pub(super) fn is_safe_chart_part_path(path: &str) -> bool {
    let Some(file_name) = path.strip_prefix(CHART_PREFIX) else {
        return false;
    };
    !file_name.is_empty()
        && file_name != ".xml"
        && !file_name.contains('/')
        && !file_name.contains("..")
        && file_name.ends_with(".xml")
        && file_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

const CHART_NS: &[u8] = b"http://schemas.openxmlformats.org/drawingml/2006/chart";
const DRAWING_NS: &[u8] = b"http://schemas.openxmlformats.org/drawingml/2006/main";
const MC_NS: &[u8] = b"http://schemas.openxmlformats.org/markup-compatibility/2006";
const C14_NS: &[u8] = b"http://schemas.microsoft.com/office/drawing/2007/8/2/chart";
const HANCOM_OFFICE_NS: &[u8] = b"http://schemas.haansoft.com/office/8.0";
const RELATIONSHIPS_NS: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XML_NS: &[u8] = b"http://www.w3.org/XML/1998/namespace";

fn validate_chart_part_xml(xml: &str, path: &str) -> Result<()> {
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut root_seen = false;
    let mut root_closed = false;
    let mut saw_chart = false;
    let mut depth = 0usize;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(event) => {
                if depth == 0 {
                    if root_seen || root_closed {
                        return Err(PluginError::corrupt(format!(
                            "chart part {path:?} contains more than one root element"
                        )));
                    }
                    root_seen = true;
                    validate_chart_part_element(&reader, &event, true, &mut saw_chart, path)?;
                } else {
                    validate_chart_part_element(&reader, &event, false, &mut saw_chart, path)?;
                }
                depth = depth.checked_add(1).ok_or_else(|| {
                    PluginError::corrupt(format!("chart part {path:?} XML depth overflowed"))
                })?;
                if depth > MAX_CHART_XML_DEPTH {
                    return Err(PluginError::corrupt(format!(
                        "chart part {path:?} nesting exceeds {MAX_CHART_XML_DEPTH} elements"
                    )));
                }
            }
            Event::Empty(event) => {
                if depth == 0 {
                    if root_seen || root_closed {
                        return Err(PluginError::corrupt(format!(
                            "chart part {path:?} contains more than one root element"
                        )));
                    }
                    root_seen = true;
                    root_closed = true;
                    validate_chart_part_element(&reader, &event, true, &mut saw_chart, path)?;
                } else {
                    validate_chart_part_element(&reader, &event, false, &mut saw_chart, path)?;
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    PluginError::corrupt(format!(
                        "chart part {path:?} has an unmatched closing element"
                    ))
                })?;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Event::Text(text) if depth == 0 && !text.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "chart part {path:?} contains text outside its root element"
                )));
            }
            Event::CData(text)
                if depth == 0 && !String::from_utf8_lossy(text.as_ref()).trim().is_empty() =>
            {
                return Err(PluginError::corrupt(format!(
                    "chart part {path:?} contains CDATA outside its root element"
                )));
            }
            Event::GeneralRef(reference) => {
                let reference = reference.decode()?;
                let predefined =
                    matches!(reference.as_ref(), "amp" | "lt" | "gt" | "apos" | "quot")
                        || reference.starts_with('#');
                if depth == 0 || !predefined {
                    return Err(PluginError::corrupt(format!(
                        "chart part {path:?} contains unsupported entity reference &{reference};"
                    )));
                }
            }
            Event::DocType(_) => {
                return Err(PluginError::corrupt(format!(
                    "chart part {path:?} must not contain a DTD"
                )));
            }
            Event::PI(_) => {
                return Err(PluginError::corrupt(format!(
                    "chart part {path:?} must not contain processing instructions"
                )));
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    if !root_seen || !root_closed || depth != 0 {
        return Err(PluginError::corrupt(format!(
            "chart part {path:?} does not contain one complete chartSpace root"
        )));
    }
    if !saw_chart {
        return Err(PluginError::corrupt(format!(
            "chart part {path:?} is missing c:chart"
        )));
    }
    Ok(())
}

fn validate_chart_part_element(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    root: bool,
    saw_chart: &mut bool,
    path: &str,
) -> Result<()> {
    let (resolved, local) = reader.resolver().resolve_element(event.name());
    let namespace = match resolved {
        ResolveResult::Bound(namespace) => namespace,
        ResolveResult::Unbound => {
            return Err(PluginError::corrupt(format!(
                "chart part {path:?} contains an unqualified element {:?}",
                String::from_utf8_lossy(local.as_ref())
            )));
        }
        ResolveResult::Unknown(prefix) => {
            return Err(PluginError::corrupt(format!(
                "chart part {path:?} uses undeclared namespace prefix {:?}",
                String::from_utf8_lossy(&prefix)
            )));
        }
    };
    let namespace = namespace.as_ref();
    let local = local.as_ref();
    if root && (namespace != CHART_NS || local != b"chartSpace") {
        return Err(PluginError::corrupt(format!(
            "chart part {path:?} root must be c:chartSpace"
        )));
    }
    if !is_allowed_chart_namespace(namespace) {
        return Err(PluginError::unsupported_feature(format!(
            "chart part {path:?} contains active namespace {:?}",
            String::from_utf8_lossy(namespace)
        )));
    }
    if namespace == CHART_NS && local == b"chart" {
        *saw_chart = true;
    }
    if [
        b"externalData".as_slice(),
        b"userShapes".as_slice(),
        b"pivotSource".as_slice(),
        b"oleObject".as_slice(),
        b"blip".as_slice(),
        b"hlinkClick".as_slice(),
        b"hlinkHover".as_slice(),
    ]
    .contains(&local)
    {
        return Err(PluginError::unsupported_feature(format!(
            "chart part {path:?} contains external-resource element {}",
            String::from_utf8_lossy(local)
        )));
    }

    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| {
            PluginError::corrupt(format!(
                "chart part {path:?} has an invalid attribute: {error}"
            ))
        })?;
        let raw_name = attribute.key.as_ref();
        if raw_name == b"xmlns" || raw_name.starts_with(b"xmlns:") {
            continue;
        }
        match reader.resolver().resolve_attribute(attribute.key).0 {
            ResolveResult::Unbound => {}
            ResolveResult::Bound(namespace) if namespace.as_ref() == RELATIONSHIPS_NS => {
                return Err(PluginError::unsupported_feature(format!(
                    "chart part {path:?} contains an external relationship attribute"
                )));
            }
            ResolveResult::Bound(namespace)
                if namespace.as_ref() == XML_NS
                    || is_allowed_chart_namespace(namespace.as_ref()) => {}
            ResolveResult::Bound(namespace) => {
                return Err(PluginError::unsupported_feature(format!(
                    "chart part {path:?} contains an active attribute namespace {:?}",
                    String::from_utf8_lossy(namespace.as_ref())
                )));
            }
            ResolveResult::Unknown(prefix) => {
                return Err(PluginError::corrupt(format!(
                    "chart part {path:?} uses undeclared attribute prefix {:?}",
                    String::from_utf8_lossy(&prefix)
                )));
            }
        }
    }
    Ok(())
}

fn is_allowed_chart_namespace(namespace: &[u8]) -> bool {
    [CHART_NS, DRAWING_NS, MC_NS, C14_NS, HANCOM_OFFICE_NS].contains(&namespace)
}

fn read_entry_to_string_limited<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    max_bytes: u64,
    remaining_bytes: &mut u64,
) -> Result<Option<String>> {
    let Some(buf) = read_entry_bytes_limited(archive, name, max_bytes, remaining_bytes)? else {
        return Ok(None);
    };
    // BOM이 붙어 있으면 제거한다.
    let s = String::from_utf8_lossy(&buf).into_owned();
    Ok(Some(s.trim_start_matches('\u{feff}').to_string()))
}

fn read_entry_bytes_limited<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    max_bytes: u64,
    remaining_bytes: &mut u64,
) -> Result<Option<Vec<u8>>> {
    let mut file = match archive.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    if file.size() > max_bytes {
        return Err(resource_limit(format!(
            "entry {name:?} declares {} expanded bytes (maximum {max_bytes})",
            file.size()
        )));
    }
    let read_limit = max_bytes.min(*remaining_bytes);
    let mut buf = Vec::new();
    file.by_ref()
        .take(read_limit.saturating_add(1))
        .read_to_end(&mut buf)?;
    let actual = u64::try_from(buf.len()).unwrap_or(u64::MAX);
    if actual > max_bytes {
        return Err(resource_limit(format!(
            "entry {name:?} exceeded {max_bytes} expanded bytes while reading"
        )));
    }
    if actual > *remaining_bytes {
        return Err(resource_limit(format!(
            "document exceeded {MAX_TOTAL_EXPANDED_BYTES} cumulative expanded bytes"
        )));
    }
    *remaining_bytes -= actual;
    Ok(Some(buf))
}

pub(super) fn validate_archive<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> Result<()> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(resource_limit(format!(
            "archive entry count {} exceeds maximum {MAX_ARCHIVE_ENTRIES}",
            archive.len()
        )));
    }

    let mut total = 0u64;
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let name = file.name().to_string();
        if !names.insert(name.clone()) {
            return Err(PluginError::corrupt(format!(
                "duplicate archive entry name: {name:?}"
            )));
        }
        if file.size() > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(resource_limit(format!(
                "entry {name:?} declares {} expanded bytes (maximum {MAX_ARCHIVE_ENTRY_BYTES})",
                file.size()
            )));
        }
        total = total
            .checked_add(file.size())
            .ok_or_else(|| resource_limit("archive expanded-size total overflowed".to_string()))?;
        if total > MAX_TOTAL_EXPANDED_BYTES {
            return Err(resource_limit(format!(
                "archive declares {total} cumulative expanded bytes (maximum {MAX_TOTAL_EXPANDED_BYTES})"
            )));
        }
        if file.size() > 1024 * 1024
            && file.compressed_size() > 0
            && file.size() / file.compressed_size() > MAX_COMPRESSION_RATIO
        {
            return Err(resource_limit(format!(
                "entry {name:?} expansion ratio exceeds {MAX_COMPRESSION_RATIO}:1"
            )));
        }
    }
    Ok(())
}

fn resource_limit(message: String) -> PluginError {
    PluginError::corrupt(format!("resource limit exceeded: {message}"))
}

/// spine 순서를 우선 쓰고, 없으면 `Contents/section*.xml`을 정렬해서 쓴다.
fn resolve_section_paths<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    manifest: &HashMap<String, String>,
    spine: &[String],
) -> Result<Vec<String>> {
    if manifest.len() > MAX_MANIFEST_ITEMS {
        return Err(resource_limit(format!(
            "manifest item count {} exceeds maximum {MAX_MANIFEST_ITEMS}",
            manifest.len()
        )));
    }
    if spine.len() > MAX_SPINE_ITEMS {
        return Err(resource_limit(format!(
            "spine item count {} exceeds maximum {MAX_SPINE_ITEMS}",
            spine.len()
        )));
    }

    let existing: HashSet<String> = archive.file_names().map(|s| s.to_string()).collect();
    let mut seen = HashSet::new();

    let mut ordered: Vec<String> = spine
        .iter()
        .filter_map(|idref| manifest.get(idref).cloned())
        .filter(|href| href.ends_with(".xml") && href.to_ascii_lowercase().contains("section"))
        .filter(|href| existing.contains(href))
        .filter(|href| seen.insert(href.clone()))
        .collect();

    if ordered.is_empty() {
        // 폴백: 파일명 정렬. section2.xml < section10.xml이 되도록 숫자로 비교한다.
        let mut found: Vec<String> = existing
            .iter()
            .filter(|n| n.starts_with(SECTION_PREFIX) && n.ends_with(".xml"))
            .cloned()
            .collect();
        found.sort_by_key(|n| section_index(n));
        ordered = found;
    }

    if ordered.len() > MAX_SECTION_COUNT {
        return Err(resource_limit(format!(
            "section count {} exceeds maximum {MAX_SECTION_COUNT}",
            ordered.len()
        )));
    }

    if ordered.is_empty() {
        return Err(PluginError::corrupt(
            "no section xml found (Contents/section*.xml missing)",
        ));
    }
    Ok(ordered)
}

/// `Contents/section12.xml` → 12. 숫자를 못 찾으면 최대값으로 밀어 뒤에 둔다.
fn section_index(name: &str) -> u32 {
    let digits: String = name
        .trim_start_matches(SECTION_PREFIX)
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(u32::MAX)
}

/// `<opf:item id="X" href="Y"/>` 를 전부 모은다.
///
/// 실제 HWPX는 XML 전체가 한 줄인 경우가 많으므로 행 단위로 훑지 않고
/// `<` 단위로 태그를 스캔한다.
pub fn parse_hpf_manifest(hpf: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for tag in iter_tags(hpf) {
        if !tag_local_name_is(tag, "item") {
            continue;
        }
        if let (Some(id), Some(href)) = (attr_value(tag, "id"), attr_value(tag, "href")) {
            map.insert(id, href);
        }
    }
    map
}

/// `<opf:itemref idref="X"/>` 의 idref를 등장 순서대로 모은다.
pub fn parse_hpf_spine(hpf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tag in iter_tags(hpf) {
        if !tag_local_name_is(tag, "itemref") {
            continue;
        }
        if let Some(idref) = attr_value(tag, "idref") {
            out.push(idref);
        }
    }
    out
}

/// `<` 와 `>` 사이 조각들을 순서대로 돌려준다.
fn iter_tags(xml: &str) -> impl Iterator<Item = &str> {
    xml.split('<').skip(1).filter_map(|chunk| {
        let end = chunk.find('>')?;
        Some(&chunk[..end])
    })
}

/// 네임스페이스 접두사를 무시하고 태그 이름을 비교한다.
fn tag_local_name_is(tag: &str, want: &str) -> bool {
    let name = tag
        .trim_start_matches('/')
        .split([' ', '\t', '\n', '\r', '/'])
        .next()
        .unwrap_or("");
    let local = name.rsplit(':').next().unwrap_or(name);
    local == want
}

/// `name="value"` 또는 `name='value'`를 뽑는다.
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = tag[from..].find(name) {
        let start = from + rel;
        let after = start + name.len();
        // 앞은 공백이어야 하고(부분일치 방지: href의 ref 등), 뒤는 =(공백허용)여야 한다.
        let boundary_ok = start == 0 || bytes[start - 1].is_ascii_whitespace();
        let rest = tag[after..].trim_start();
        if boundary_ok && rest.starts_with('=') {
            let rest = rest[1..].trim_start();
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &rest[1..];
                let end = body.find(quote)?;
                return Some(body[..end].to_string());
            }
        }
        from = after;
    }
    None
}

fn guess_content_type(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    match lower.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "wmf" => "image/wmf",
        "emf" => "image/emf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 실제 HWPX처럼 XML 전체가 한 줄인 경우.
    const SINGLE_LINE_HPF: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/">"#,
        r#"<opf:manifest>"#,
        r#"<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>"#,
        r#"<opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>"#,
        r#"<opf:item id="section1" href="Contents/section1.xml" media-type="application/xml"/>"#,
        r#"<opf:item id="image1" href="BinData/image1.png" media-type="image/png"/>"#,
        r#"</opf:manifest>"#,
        r#"<opf:spine>"#,
        r#"<opf:itemref idref="header" linear="yes"/>"#,
        r#"<opf:itemref idref="section0" linear="yes"/>"#,
        r#"<opf:itemref idref="section1" linear="yes"/>"#,
        r#"</opf:spine>"#,
        r#"</opf:package>"#,
    );

    #[test]
    fn parses_manifest_from_single_line_xml() {
        let m = parse_hpf_manifest(SINGLE_LINE_HPF);
        assert_eq!(
            m.get("section0").map(String::as_str),
            Some("Contents/section0.xml")
        );
        assert_eq!(
            m.get("image1").map(String::as_str),
            Some("BinData/image1.png")
        );
        assert_eq!(m.len(), 4);
    }

    #[test]
    fn parses_spine_order() {
        let s = parse_hpf_spine(SINGLE_LINE_HPF);
        assert_eq!(s, vec!["header", "section0", "section1"]);
    }

    #[test]
    fn attr_value_requires_word_boundary() {
        // href를 찾을 때 media-type의 'e'나 다른 부분문자열에 걸리지 않아야 한다.
        let tag = r#"opf:item id="x" href="Contents/a.xml" media-type="application/xml""#;
        assert_eq!(attr_value(tag, "id").as_deref(), Some("x"));
        assert_eq!(attr_value(tag, "href").as_deref(), Some("Contents/a.xml"));
        assert_eq!(
            attr_value(tag, "media-type").as_deref(),
            Some("application/xml")
        );
        assert_eq!(attr_value(tag, "missing"), None);
    }

    #[test]
    fn attr_value_handles_single_quotes() {
        assert_eq!(
            attr_value("hp:img binaryItemIDRef='image3'", "binaryItemIDRef").as_deref(),
            Some("image3")
        );
    }

    #[test]
    fn tag_local_name_ignores_namespace_prefix() {
        assert!(tag_local_name_is("opf:item id=\"a\"", "item"));
        assert!(tag_local_name_is("item id=\"a\"", "item"));
        assert!(tag_local_name_is("/opf:item", "item"));
        assert!(!tag_local_name_is("opf:itemref idref=\"a\"", "item"));
    }

    #[test]
    fn section_index_sorts_numerically_not_lexically() {
        assert_eq!(section_index("Contents/section2.xml"), 2);
        assert_eq!(section_index("Contents/section10.xml"), 10);
        let mut v = vec![
            "Contents/section10.xml".to_string(),
            "Contents/section2.xml".to_string(),
            "Contents/section1.xml".to_string(),
        ];
        v.sort_by_key(|n| section_index(n));
        assert_eq!(
            v,
            vec![
                "Contents/section1.xml",
                "Contents/section2.xml",
                "Contents/section10.xml"
            ]
        );
    }

    #[test]
    fn guesses_image_content_types() {
        assert_eq!(guess_content_type("BinData/image1.PNG"), "image/png");
        assert_eq!(guess_content_type("BinData/a.jpeg"), "image/jpeg");
        assert_eq!(
            guess_content_type("BinData/a.unknown"),
            "application/octet-stream"
        );
    }
}
