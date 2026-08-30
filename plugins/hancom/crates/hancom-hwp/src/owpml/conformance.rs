//! Strict validation for HWPX packages emitted by the writer.
//!
//! The ordinary reader intentionally accepts a few incomplete packages found in
//! the wild. Writer output has a narrower contract: it must be portable, safe to
//! replace the source with, internally connected, and reopenable without relying
//! on those compatibility fallbacks.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use quick_xml::XmlVersion;
use zip::{CompressionMethod, ZipArchive};

use super::package::{
    validate_archive, HEADER_ENTRY, HPF_ENTRY, MAX_HPF_BYTES, MAX_MANIFEST_ITEMS,
    MAX_SECTION_COUNT, MAX_SPINE_ITEMS, MAX_XML_ENTRY_BYTES, MIMETYPE_ENTRY, MIMETYPE_VALUE,
};
use crate::error::{PluginError, Result};

const VERSION_ENTRY: &str = "version.xml";
const META_MANIFEST_ENTRY: &str = "META-INF/manifest.xml";
const CONTAINER_ENTRY: &str = "META-INF/container.xml";
const SECTION_PREFIX: &str = "Contents/section";
const MAX_OUTPUT_XML_DEPTH: usize = 256;

const VERSION_NS: &[u8] = b"http://www.hancom.co.kr/hwpml/2011/version";
const ODF_MANIFEST_NS: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
const OCF_NS: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:container";
const OPF_NS: &[u8] = b"http://www.idpf.org/2007/opf/";
const HEAD_NS: &[u8] = b"http://www.hancom.co.kr/hwpml/2011/head";
const SECTION_NS: &[u8] = b"http://www.hancom.co.kr/hwpml/2011/section";
const HPF_MEDIA_TYPE: &str = "application/hwpml-package+xml";

/// Validate the package contract required of every OfficeCLI HWPX writer output.
///
/// This is deliberately stricter than [`super::read_document`]. It is a
/// structural and security gate, not a claim of KS X 6101/XSD conformance.
pub fn validate_output_package<R: Read + Seek>(reader: R) -> Result<()> {
    let mut archive = ZipArchive::new(reader)?;
    validate_archive(&mut archive)?;
    validate_entry_metadata(&mut archive)?;

    let names: HashSet<String> = archive.file_names().map(str::to_owned).collect();
    for required in [
        MIMETYPE_ENTRY,
        VERSION_ENTRY,
        META_MANIFEST_ENTRY,
        CONTAINER_ENTRY,
        HPF_ENTRY,
        HEADER_ENTRY,
    ] {
        if !names.contains(required) {
            return Err(corrupt(format!(
                "missing required archive entry {required:?}"
            )));
        }
    }

    let xml_parts = read_and_validate_payloads(&mut archive)?;
    validate_expected_root(
        required_xml(&xml_parts, VERSION_ENTRY)?,
        VERSION_ENTRY,
        VERSION_NS,
        b"HCFVersion",
    )?;
    validate_expected_root(
        required_xml(&xml_parts, META_MANIFEST_ENTRY)?,
        META_MANIFEST_ENTRY,
        ODF_MANIFEST_NS,
        b"manifest",
    )?;
    validate_container(required_xml(&xml_parts, CONTAINER_ENTRY)?, &names)?;
    let header_xml = required_xml(&xml_parts, HEADER_ENTRY)?;
    validate_expected_root(header_xml, HEADER_ENTRY, HEAD_NS, b"head")?;
    let declared_section_count =
        required_root_attribute(header_xml, HEADER_ENTRY, HEAD_NS, b"head", b"secCnt")?
            .parse::<usize>()
            .map_err(|error| xml_error(HEADER_ENTRY, &format!("has invalid secCnt: {error}")))?;
    validate_hpf(
        required_xml(&xml_parts, HPF_ENTRY)?,
        &names,
        &xml_parts,
        declared_section_count,
    )?;

    Ok(())
}

fn validate_entry_metadata<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<()> {
    if archive.is_empty() {
        return Err(corrupt("writer output is an empty ZIP archive"));
    }

    let mut folded_names = HashMap::<String, String>::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let name = file.name();
        validate_portable_path(name, file.is_dir())?;
        if file.encrypted() {
            return Err(corrupt(format!(
                "encrypted archive entry is not supported in writer output: {name:?}"
            )));
        }
        if matches!(file.unix_mode(), Some(mode) if mode & 0o170000 == 0o120000) {
            return Err(corrupt(format!(
                "symbolic link archive entry is not allowed: {name:?}"
            )));
        }

        let folded = name.to_lowercase();
        if let Some(previous) = folded_names.insert(folded, name.to_owned()) {
            if previous != name {
                return Err(corrupt(format!(
                    "case-insensitive archive path collision: {previous:?} and {name:?}"
                )));
            }
        }
    }

    let first = archive.by_index(0)?;
    if first.name() != MIMETYPE_ENTRY {
        return Err(corrupt(format!(
            "first entry must be {MIMETYPE_ENTRY:?}, got {:?}",
            first.name()
        )));
    }
    if first.compression() != CompressionMethod::Stored {
        return Err(corrupt("mimetype entry must be stored without compression"));
    }
    if first.header_start() != 0 {
        return Err(corrupt(format!(
            "first physical ZIP entry must start at byte 0, got byte {}",
            first.header_start()
        )));
    }
    Ok(())
}

fn validate_portable_path(path: &str, directory: bool) -> Result<()> {
    let path_without_trailing_slash = if directory {
        path.strip_suffix('/').unwrap_or(path)
    } else {
        path
    };
    let invalid = path_without_trailing_slash.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path_without_trailing_slash.split('/').any(|component| {
            component.is_empty()
                || component == "."
                || component == ".."
                || component.contains(':')
                || component.ends_with(['.', ' '])
                || is_windows_reserved_component(component)
        });
    if invalid {
        return Err(corrupt(format!(
            "unsafe or non-portable archive path: {path:?}"
        )));
    }
    Ok(())
}

fn is_windows_reserved_component(component: &str) -> bool {
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn read_and_validate_payloads<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<HashMap<String, String>> {
    let mut xml_parts = HashMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_owned();
        if file.is_dir() {
            if file.size() != 0 {
                return Err(corrupt(format!(
                    "directory archive entry has a payload: {name:?}"
                )));
            }
            continue;
        }

        if name == MIMETYPE_ENTRY {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|error| payload_error(&name, error))?;
            if bytes.as_slice() != MIMETYPE_VALUE.as_bytes() {
                return Err(corrupt(format!(
                    "mimetype entry must contain exactly {MIMETYPE_VALUE:?}"
                )));
            }
            continue;
        }

        if is_xml_part(&name) {
            let max_bytes = if name == HPF_ENTRY {
                MAX_HPF_BYTES
            } else {
                MAX_XML_ENTRY_BYTES
            };
            if file.size() > max_bytes {
                return Err(corrupt(format!(
                    "XML entry {name:?} exceeds the {max_bytes}-byte writer limit"
                )));
            }
            let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
            file.read_to_end(&mut bytes)
                .map_err(|error| payload_error(&name, error))?;
            let xml = std::str::from_utf8(&bytes)
                .map_err(|error| corrupt(format!("XML entry {name:?} is not UTF-8: {error}")))?;
            validate_xml_document(xml, &name)?;
            xml_parts.insert(name, xml.to_owned());
        } else {
            let expected = file.size();
            let actual = std::io::copy(&mut file, &mut std::io::sink())
                .map_err(|error| payload_error(&name, error))?;
            if actual != expected {
                return Err(corrupt(format!(
                    "archive entry {name:?} yielded {actual} bytes, expected {expected}"
                )));
            }
        }
    }
    Ok(xml_parts)
}

fn payload_error(path: &str, error: std::io::Error) -> PluginError {
    corrupt(format!("cannot verify archive entry {path:?}: {error}"))
}

fn is_xml_part(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with(".xml") || path.ends_with(".hpf") || path.ends_with(".rdf")
}

fn required_xml<'a>(parts: &'a HashMap<String, String>, path: &str) -> Result<&'a str> {
    parts
        .get(path)
        .map(String::as_str)
        .ok_or_else(|| corrupt(format!("required XML entry was not validated: {path:?}")))
}

fn validate_xml_document(xml: &str, path: &str) -> Result<()> {
    if xml.starts_with('\u{feff}') {
        return Err(xml_error(path, "must not start with a UTF-8 BOM"));
    }
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut root_closed = false;
    let mut declaration_seen = false;

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| corrupt(format!("invalid XML in {path:?}: {error}")))?;
        match event {
            Event::Start(element) => {
                if depth == 0 {
                    if root_seen || root_closed {
                        return Err(xml_error(path, "contains more than one root element"));
                    }
                    root_seen = true;
                }
                validate_namespaces(&reader, &element, path)?;
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| xml_error(path, "element depth overflowed"))?;
                if depth > MAX_OUTPUT_XML_DEPTH {
                    return Err(xml_error(
                        path,
                        &format!("nesting exceeds {MAX_OUTPUT_XML_DEPTH} elements"),
                    ));
                }
            }
            Event::Empty(element) => {
                if depth == 0 {
                    if root_seen || root_closed {
                        return Err(xml_error(path, "contains more than one root element"));
                    }
                    root_seen = true;
                    root_closed = true;
                }
                validate_namespaces(&reader, &element, path)?;
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| xml_error(path, "contains an unmatched closing element"))?;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Event::Decl(declaration) => {
                if declaration_seen || root_seen {
                    return Err(xml_error(path, "contains a misplaced XML declaration"));
                }
                let version = declaration
                    .version()
                    .map_err(|error| xml_error(path, &format!("invalid version: {error}")))?;
                if version.as_ref() != b"1.0" {
                    return Err(xml_error(path, "must declare XML version 1.0"));
                }
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding
                        .map_err(|error| xml_error(path, &format!("invalid encoding: {error}")))?;
                    if !encoding.eq_ignore_ascii_case(b"UTF-8")
                        && !encoding.eq_ignore_ascii_case(b"UTF8")
                    {
                        return Err(xml_error(path, "must declare UTF-8 encoding"));
                    }
                }
                declaration_seen = true;
            }
            Event::DocType(_) => {
                return Err(xml_error(path, "must not contain a DTD"));
            }
            Event::PI(_) => {
                return Err(xml_error(path, "must not contain processing instructions"));
            }
            Event::GeneralRef(reference) => {
                let character_reference = reference.resolve_char_ref().map_err(|error| {
                    xml_error(path, &format!("invalid character reference: {error}"))
                })?;
                let reference = reference
                    .decode()
                    .map_err(|error| xml_error(path, &format!("invalid entity: {error}")))?;
                let predefined =
                    matches!(reference.as_ref(), "amp" | "lt" | "gt" | "apos" | "quot")
                        || character_reference.is_some();
                if depth == 0 || !predefined {
                    return Err(xml_error(
                        path,
                        &format!("contains unsupported entity reference &{reference};"),
                    ));
                }
            }
            Event::Text(text) if depth == 0 => {
                let text = text
                    .decode()
                    .map_err(|error| xml_error(path, &format!("invalid text: {error}")))?;
                if !text.trim().is_empty() {
                    return Err(xml_error(path, "contains text outside its root element"));
                }
            }
            Event::CData(text)
                if depth == 0 && !String::from_utf8_lossy(text.as_ref()).trim().is_empty() =>
            {
                return Err(xml_error(path, "contains CDATA outside its root element"));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    if !root_seen || !root_closed || depth != 0 {
        return Err(xml_error(
            path,
            "does not contain one complete root element",
        ));
    }
    Ok(())
}

fn validate_namespaces(
    reader: &NsReader<&[u8]>,
    element: &BytesStart<'_>,
    path: &str,
) -> Result<()> {
    if let ResolveResult::Unknown(prefix) = reader.resolver().resolve_element(element.name()).0 {
        return Err(xml_error(
            path,
            &format!(
                "uses undeclared element prefix {:?}",
                String::from_utf8_lossy(&prefix)
            ),
        ));
    }
    for attribute in element.attributes().with_checks(true) {
        let attribute =
            attribute.map_err(|error| xml_error(path, &format!("invalid attribute: {error}")))?;
        attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| xml_error(path, &format!("invalid attribute value: {error}")))?;
        let raw_name = attribute.key.as_ref();
        if raw_name == b"xmlns" || raw_name.starts_with(b"xmlns:") {
            continue;
        }
        if let ResolveResult::Unknown(prefix) = reader.resolver().resolve_attribute(attribute.key).0
        {
            return Err(xml_error(
                path,
                &format!(
                    "uses undeclared attribute prefix {:?}",
                    String::from_utf8_lossy(&prefix)
                ),
            ));
        }
    }
    Ok(())
}

fn validate_expected_root(xml: &str, path: &str, namespace: &[u8], local: &[u8]) -> Result<()> {
    let mut reader = NsReader::from_str(xml);
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| xml_error(path, &format!("invalid XML: {error}")))?
        {
            Event::Start(element) | Event::Empty(element) => {
                if !is_element(&reader, &element, namespace, local) {
                    return Err(xml_error(
                        path,
                        &format!(
                            "root must be {{{}}}{}",
                            String::from_utf8_lossy(namespace),
                            String::from_utf8_lossy(local)
                        ),
                    ));
                }
                return Ok(());
            }
            Event::Eof => return Err(xml_error(path, "is missing its root element")),
            _ => {}
        }
        buffer.clear();
    }
}

fn required_root_attribute(
    xml: &str,
    path: &str,
    namespace: &[u8],
    local: &[u8],
    attribute: &[u8],
) -> Result<String> {
    let mut reader = NsReader::from_str(xml);
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| xml_error(path, &format!("invalid XML: {error}")))?
        {
            Event::Start(element) | Event::Empty(element) => {
                if !is_element(&reader, &element, namespace, local) {
                    return Err(xml_error(path, "has an unexpected root element"));
                }
                return required_attribute(&element, attribute, path);
            }
            Event::Eof => return Err(xml_error(path, "is missing its root element")),
            _ => {}
        }
        buffer.clear();
    }
}

fn validate_container(xml: &str, archive_names: &HashSet<String>) -> Result<()> {
    validate_expected_root(xml, CONTAINER_ENTRY, OCF_NS, b"container")?;
    let mut reader = NsReader::from_str(xml);
    let mut buffer = Vec::new();
    let mut canonical_rootfiles = 0usize;
    let mut rootfiles_elements = 0usize;
    let mut rootfile_paths = HashSet::<String>::new();
    let mut parent_is_rootfiles = Vec::<bool>::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| xml_error(CONTAINER_ENTRY, &format!("invalid XML: {error}")))?
        {
            Event::Start(element) => {
                let is_rootfiles = inspect_container_element(
                    &reader,
                    &element,
                    &parent_is_rootfiles,
                    archive_names,
                    &mut rootfiles_elements,
                    &mut rootfile_paths,
                    &mut canonical_rootfiles,
                )?;
                parent_is_rootfiles.push(is_rootfiles);
            }
            Event::Empty(element) => {
                inspect_container_element(
                    &reader,
                    &element,
                    &parent_is_rootfiles,
                    archive_names,
                    &mut rootfiles_elements,
                    &mut rootfile_paths,
                    &mut canonical_rootfiles,
                )?;
            }
            Event::End(_) => {
                parent_is_rootfiles.pop().ok_or_else(|| {
                    xml_error(CONTAINER_ENTRY, "contains an unmatched closing element")
                })?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if rootfiles_elements != 1 {
        return Err(xml_error(
            CONTAINER_ENTRY,
            "must contain exactly one direct ocf:rootfiles element",
        ));
    }
    if canonical_rootfiles != 1 {
        return Err(xml_error(
            CONTAINER_ENTRY,
            "must contain exactly one canonical content.hpf rootfile",
        ));
    }
    Ok(())
}

fn inspect_container_element(
    reader: &NsReader<&[u8]>,
    element: &BytesStart<'_>,
    parent_is_rootfiles: &[bool],
    archive_names: &HashSet<String>,
    rootfiles_elements: &mut usize,
    rootfile_paths: &mut HashSet<String>,
    canonical_rootfiles: &mut usize,
) -> Result<bool> {
    let is_rootfiles = is_element(reader, element, OCF_NS, b"rootfiles");
    if is_rootfiles {
        if parent_is_rootfiles.len() != 1 {
            return Err(xml_error(
                CONTAINER_ENTRY,
                "ocf:rootfiles must be a direct child of ocf:container",
            ));
        }
        *rootfiles_elements += 1;
    }

    if is_element(reader, element, OCF_NS, b"rootfile") {
        if parent_is_rootfiles.last() != Some(&true) {
            return Err(xml_error(
                CONTAINER_ENTRY,
                "ocf:rootfile must be a direct child of ocf:rootfiles",
            ));
        }
        let full_path = required_attribute(element, b"full-path", CONTAINER_ENTRY)?;
        let media_type = required_attribute(element, b"media-type", CONTAINER_ENTRY)?;
        validate_portable_path(&full_path, false)?;
        if !rootfile_paths.insert(full_path.clone()) {
            return Err(xml_error(
                CONTAINER_ENTRY,
                &format!("contains duplicate rootfile path {full_path:?}"),
            ));
        }
        if !archive_names.contains(&full_path) {
            return Err(xml_error(
                CONTAINER_ENTRY,
                &format!("rootfile names missing archive entry {full_path:?}"),
            ));
        }
        if full_path == HPF_ENTRY && media_type == HPF_MEDIA_TYPE {
            *canonical_rootfiles += 1;
        }
    }
    Ok(is_rootfiles)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HpfParent {
    Package,
    Manifest,
    Spine,
    Other,
}

#[derive(Default)]
struct HpfScan {
    parents: Vec<HpfParent>,
    manifest_elements: usize,
    spine_elements: usize,
    manifest: HashMap<String, String>,
    spine: Vec<String>,
}

impl HpfScan {
    fn inspect_element(
        &mut self,
        reader: &NsReader<&[u8]>,
        element: &BytesStart<'_>,
    ) -> Result<HpfParent> {
        if is_element(reader, element, OPF_NS, b"package") {
            if !self.parents.is_empty() {
                return Err(xml_error(HPF_ENTRY, "opf:package must be the root element"));
            }
            return Ok(HpfParent::Package);
        }
        if is_element(reader, element, OPF_NS, b"manifest") {
            if self.parents != [HpfParent::Package] {
                return Err(xml_error(
                    HPF_ENTRY,
                    "opf:manifest must be a direct child of opf:package",
                ));
            }
            self.manifest_elements += 1;
            return Ok(HpfParent::Manifest);
        }
        if is_element(reader, element, OPF_NS, b"spine") {
            if self.parents != [HpfParent::Package] {
                return Err(xml_error(
                    HPF_ENTRY,
                    "opf:spine must be a direct child of opf:package",
                ));
            }
            self.spine_elements += 1;
            return Ok(HpfParent::Spine);
        }
        if is_element(reader, element, OPF_NS, b"item") {
            if self.parents.last() != Some(&HpfParent::Manifest) {
                return Err(xml_error(
                    HPF_ENTRY,
                    "opf:item must be a direct child of opf:manifest",
                ));
            }
            let id = required_attribute(element, b"id", HPF_ENTRY)?;
            let href = required_attribute(element, b"href", HPF_ENTRY)?;
            validate_portable_path(&href, false)?;
            if self.manifest.insert(id.clone(), href).is_some() {
                return Err(xml_error(
                    HPF_ENTRY,
                    &format!("contains duplicate manifest id {id:?}"),
                ));
            }
            if self.manifest.len() > MAX_MANIFEST_ITEMS {
                return Err(xml_error(
                    HPF_ENTRY,
                    &format!("manifest item count exceeds maximum {MAX_MANIFEST_ITEMS}"),
                ));
            }
        }
        if is_element(reader, element, OPF_NS, b"itemref") {
            if self.parents.last() != Some(&HpfParent::Spine) {
                return Err(xml_error(
                    HPF_ENTRY,
                    "opf:itemref must be a direct child of opf:spine",
                ));
            }
            self.spine
                .push(required_attribute(element, b"idref", HPF_ENTRY)?);
            if self.spine.len() > MAX_SPINE_ITEMS {
                return Err(xml_error(
                    HPF_ENTRY,
                    &format!("spine item count exceeds maximum {MAX_SPINE_ITEMS}"),
                ));
            }
        }
        Ok(HpfParent::Other)
    }
}

fn validate_hpf(
    xml: &str,
    archive_names: &HashSet<String>,
    xml_parts: &HashMap<String, String>,
    declared_section_count: usize,
) -> Result<()> {
    validate_expected_root(xml, HPF_ENTRY, OPF_NS, b"package")?;
    if let Some(path) = archive_names.iter().find(|path| {
        path.starts_with(SECTION_PREFIX) && path.ends_with(".xml") && section_index(path).is_none()
    }) {
        return Err(xml_error(
            HPF_ENTRY,
            &format!("archive contains non-canonical section path {path:?}"),
        ));
    }
    if declared_section_count > MAX_SECTION_COUNT {
        return Err(xml_error(
            HEADER_ENTRY,
            &format!("section count exceeds maximum {MAX_SECTION_COUNT}"),
        ));
    }
    let mut reader = NsReader::from_str(xml);
    let mut buffer = Vec::new();
    let mut scan = HpfScan::default();

    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| xml_error(HPF_ENTRY, &format!("invalid XML: {error}")))?
        {
            Event::Start(element) => {
                let parent = scan.inspect_element(&reader, &element)?;
                scan.parents.push(parent);
            }
            Event::Empty(element) => {
                scan.inspect_element(&reader, &element)?;
            }
            Event::End(_) => {
                scan.parents
                    .pop()
                    .ok_or_else(|| xml_error(HPF_ENTRY, "contains an unmatched closing element"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if scan.manifest_elements != 1 {
        return Err(xml_error(
            HPF_ENTRY,
            "must contain exactly one direct opf:manifest element",
        ));
    }
    if scan.spine_elements != 1 {
        return Err(xml_error(
            HPF_ENTRY,
            "must contain exactly one direct opf:spine element",
        ));
    }

    let mut header_in_spine = false;
    let mut spine_sections = HashSet::<String>::new();
    for idref in &scan.spine {
        let href = scan.manifest.get(idref).ok_or_else(|| {
            xml_error(
                HPF_ENTRY,
                &format!("spine references unknown manifest id {idref:?}"),
            )
        })?;
        header_in_spine |= href == HEADER_ENTRY;
        if is_section_path(href) && !spine_sections.insert(href.clone()) {
            return Err(xml_error(
                HPF_ENTRY,
                &format!("spine references section {href:?} more than once"),
            ));
        }
    }
    if !header_in_spine {
        return Err(xml_error(
            HPF_ENTRY,
            "spine does not reference Contents/header.xml",
        ));
    }
    if spine_sections.is_empty() {
        return Err(xml_error(
            HPF_ENTRY,
            "spine does not reference a section XML part",
        ));
    }

    let mut manifest_sections = HashSet::<String>::new();
    for href in scan.manifest.values() {
        if !archive_names.contains(href) {
            return Err(xml_error(
                HPF_ENTRY,
                &format!("manifest href names missing archive entry {href:?}"),
            ));
        }
        if is_section_path(href) {
            if !manifest_sections.insert(href.clone()) {
                return Err(xml_error(
                    HPF_ENTRY,
                    &format!("manifest references section {href:?} more than once"),
                ));
            }
            validate_expected_root(required_xml(xml_parts, href)?, href, SECTION_NS, b"sec")?;
        }
    }
    if manifest_sections.is_empty() {
        return Err(xml_error(
            HPF_ENTRY,
            "manifest does not contain a Contents/section*.xml part",
        ));
    }
    if manifest_sections.len() > MAX_SECTION_COUNT {
        return Err(xml_error(
            HPF_ENTRY,
            &format!("section count exceeds maximum {MAX_SECTION_COUNT}"),
        ));
    }
    if manifest_sections != spine_sections {
        return Err(xml_error(
            HPF_ENTRY,
            "manifest section set and spine section set do not match",
        ));
    }

    let archive_sections: HashSet<String> = archive_names
        .iter()
        .filter(|path| is_section_path(path))
        .cloned()
        .collect();
    if archive_sections != manifest_sections {
        return Err(xml_error(
            HPF_ENTRY,
            "archive section set and manifest section set do not match",
        ));
    }

    let mut indices: Vec<usize> = manifest_sections
        .iter()
        .map(|path| {
            section_index(path).expect("is_section_path and section_index share the same grammar")
        })
        .collect();
    indices.sort_unstable();
    if indices.iter().copied().ne(0..indices.len()) {
        return Err(xml_error(
            HPF_ENTRY,
            "section part indexes must be contiguous from section0.xml",
        ));
    }
    if declared_section_count != manifest_sections.len() {
        return Err(xml_error(
            HEADER_ENTRY,
            &format!(
                "secCnt declares {declared_section_count} sections but the package contains {}",
                manifest_sections.len()
            ),
        ));
    }
    Ok(())
}

fn is_section_path(path: &str) -> bool {
    section_index(path).is_some()
}

fn section_index(path: &str) -> Option<usize> {
    let index = path
        .strip_prefix(SECTION_PREFIX)
        .and_then(|value| value.strip_suffix(".xml"))?;
    if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if index.len() > 1 && index.starts_with('0') {
        return None;
    }
    index.parse().ok()
}

fn is_element(
    reader: &NsReader<&[u8]>,
    element: &BytesStart<'_>,
    namespace: &[u8],
    local: &[u8],
) -> bool {
    let (resolved, resolved_local) = reader.resolver().resolve_element(element.name());
    matches!(
        resolved,
        ResolveResult::Bound(found) if found.as_ref() == namespace && resolved_local.as_ref() == local
    )
}

fn required_attribute(element: &BytesStart<'_>, name: &[u8], path: &str) -> Result<String> {
    for attribute in element.attributes().with_checks(true) {
        let attribute =
            attribute.map_err(|error| xml_error(path, &format!("invalid attribute: {error}")))?;
        if attribute.key.as_ref() == name {
            return attribute
                .normalized_value(XmlVersion::Explicit1_0)
                .map(|value| value.into_owned())
                .map_err(|error| xml_error(path, &format!("invalid attribute value: {error}")));
        }
    }
    Err(xml_error(
        path,
        &format!(
            "element is missing required attribute {:?}",
            String::from_utf8_lossy(name)
        ),
    ))
}

fn xml_error(path: &str, message: &str) -> PluginError {
    corrupt(format!("XML entry {path:?} {message}"))
}

fn corrupt(message: impl Into<String>) -> PluginError {
    PluginError::corrupt(message.into())
}
