//! Package-preserving HWPX copy-on-write primitives and G3 verification.
//!
//! This module deliberately works below the lossy conversion model. A baseline
//! records both decompressed part bytes and their compressed representation;
//! candidate saves may alter only the parts named by an explicit mutation plan.
//! The candidate is then reopened through the strict package reader. Mutations
//! may additionally be compared with the complete expected known-semantic model
//! when that model can represent the edited subset.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;

use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::conformance::validate_output_package;
use super::model::Document;
use super::package::{MAX_XML_ENTRY_BYTES, MIMETYPE_ENTRY};
use crate::error::{PluginError, Result};

const HASH_BUFFER_BYTES: usize = 64 * 1024;
const CENTRAL_DIRECTORY_SIGNATURE: [u8; 4] = *b"PK\x01\x02";
const PARAGRAPH_NAMESPACE: &[u8] = b"http://www.hancom.co.kr/hwpml/2011/paragraph";

/// Immutable fingerprint of one source ZIP entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntrySnapshot {
    name: String,
    raw_name: Vec<u8>,
    directory: bool,
    compression: CompressionMethod,
    content_sha256: [u8; 32],
    compressed_sha256: [u8; 32],
    crc32: u32,
    size: u64,
    compressed_size: u64,
    last_modified: Option<DateTime>,
    unix_mode: Option<u32>,
    version_made_by: [u8; 2],
    comment: String,
    extra_data: Option<Vec<u8>>,
}

impl EntrySnapshot {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn content_sha256(&self) -> &[u8; 32] {
        &self.content_sha256
    }

    fn has_same_preserved_metadata(&self, candidate: &Self) -> bool {
        self.directory == candidate.directory
            && self.compression == candidate.compression
            && self.last_modified == candidate.last_modified
            && self.unix_mode == candidate.unix_mode
            && self.version_made_by == candidate.version_made_by
            && self.comment == candidate.comment
            && self.extra_data == candidate.extra_data
    }
}

/// Ordered immutable view of the package at session-open time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageSnapshot {
    archive_comment: Vec<u8>,
    entries: Vec<EntrySnapshot>,
}

impl PackageSnapshot {
    /// Validate and fingerprint a strict editable-package baseline.
    pub fn capture<R: Read + Seek>(mut reader: R) -> Result<Self> {
        validate_output_package(&mut reader)?;
        reader.rewind()?;
        Self::capture_validated(&mut reader)
    }

    fn capture_validated<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let mut archive = ZipArchive::new(reader)?;
        let archive_comment = archive.comment().to_vec();
        let mut entries = Vec::with_capacity(archive.len());
        let mut central_header_starts = Vec::with_capacity(archive.len());

        for index in 0..archive.len() {
            let (
                name,
                raw_name,
                directory,
                compression,
                crc32,
                size,
                compressed_size,
                last_modified,
                unix_mode,
                comment,
                extra_data,
                compressed_sha256,
                central_header_start,
            ) = {
                let file = archive.by_index_raw(index)?;
                let name = file.name().to_owned();
                let raw_name = file.name_raw().to_vec();
                let directory = file.is_dir();
                let compression = file.compression();
                let crc32 = file.crc32();
                let size = file.size();
                let compressed_size = file.compressed_size();
                let last_modified = file.last_modified();
                let unix_mode = file.unix_mode();
                let comment = file.comment().to_owned();
                let extra_data = file.extra_data().map(<[u8]>::to_vec);
                let central_header_start = file.central_header_start();
                let compressed_sha256 = hash_reader(file, &name)?;
                (
                    name,
                    raw_name,
                    directory,
                    compression,
                    crc32,
                    size,
                    compressed_size,
                    last_modified,
                    unix_mode,
                    comment,
                    extra_data,
                    compressed_sha256,
                    central_header_start,
                )
            };

            let content_sha256 = {
                let file = archive.by_index(index)?;
                hash_reader(file, &name)?
            };
            entries.push(EntrySnapshot {
                name,
                raw_name,
                directory,
                compression,
                content_sha256,
                compressed_sha256,
                crc32,
                size,
                compressed_size,
                last_modified,
                unix_mode,
                version_made_by: [0; 2],
                comment,
                extra_data,
            });
            central_header_starts.push(central_header_start);
        }

        let reader = archive.into_inner();
        for (entry, central_header_start) in entries.iter_mut().zip(central_header_starts) {
            entry.version_made_by = read_central_version_made_by(reader, central_header_start)?;
        }

        Ok(Self {
            archive_comment,
            entries,
        })
    }

    pub fn entries(&self) -> &[EntrySnapshot] {
        &self.entries
    }

    fn entry(&self, name: &str) -> Option<&EntrySnapshot> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    fn verify_candidate(&self, candidate: &Self, plan: &MutationPlan) -> Result<()> {
        if self.archive_comment != candidate.archive_comment {
            return Err(verification_error(
                "candidate changed the ZIP archive comment outside the mutation plan",
            ));
        }
        if self.entries.len() != candidate.entries.len() {
            return Err(verification_error(format!(
                "candidate entry count changed from {} to {}",
                self.entries.len(),
                candidate.entries.len()
            )));
        }

        for (index, (source, candidate)) in self.entries.iter().zip(&candidate.entries).enumerate()
        {
            if source.name != candidate.name || source.raw_name != candidate.raw_name {
                return Err(verification_error(format!(
                    "candidate changed entry {index} identity/order from {:?} to {:?}",
                    source.name, candidate.name
                )));
            }

            if plan.changed_parts.contains(&source.name) {
                if source.content_sha256 == candidate.content_sha256 {
                    return Err(verification_error(format!(
                        "planned part {:?} did not change",
                        source.name
                    )));
                }
                if let Some(expected) = plan.expected_content_sha256.get(&source.name) {
                    if candidate.content_sha256 != *expected {
                        return Err(verification_error(format!(
                            "planned part {:?} does not match its exact replacement",
                            source.name
                        )));
                    }
                }
                if !source.has_same_preserved_metadata(candidate) {
                    return Err(verification_error(format!(
                        "planned part {:?} changed preserved ZIP metadata",
                        source.name
                    )));
                }
            } else if source != candidate {
                return Err(verification_error(format!(
                    "unchanged part {:?} did not preserve its payload and ZIP metadata",
                    source.name
                )));
            }
        }
        Ok(())
    }
}

/// Strict package snapshot captured at session-open time.
#[derive(Clone, Debug)]
pub struct PackageBaseline {
    snapshot: PackageSnapshot,
}

impl PackageBaseline {
    pub fn capture<R: Read + Seek>(reader: R) -> Result<Self> {
        Ok(Self {
            snapshot: PackageSnapshot::capture(reader)?,
        })
    }

    pub fn snapshot(&self) -> &PackageSnapshot {
        &self.snapshot
    }

    /// Run the G0-G3 candidate gate and return the independently reopened state.
    pub fn verify_candidate<R: Read + Seek>(
        &self,
        mut candidate: R,
        plan: &MutationPlan,
        semantic_expectation: SemanticExpectation<'_>,
    ) -> Result<VerifiedCandidate> {
        let snapshot = PackageSnapshot::capture(&mut candidate)?;
        self.snapshot.verify_candidate(&snapshot, plan)?;
        let document = match semantic_expectation {
            SemanticExpectation::Unchanged => {
                if !plan.changed_parts.is_empty() {
                    return Err(PluginError::invalid_argument(
                        "changed parts require an explicit semantic expectation",
                    ));
                }
                None
            }
            SemanticExpectation::ExactDocument(expected) => {
                candidate.rewind()?;
                let document = super::read_document_from(&mut candidate)?;
                if &document != expected {
                    return Err(verification_error(
                        "candidate known semantics do not match the requested exact delta",
                    ));
                }
                Some(document)
            }
            SemanticExpectation::ExactText {
                part,
                selector,
                expected,
            } => {
                if plan.changed_parts.len() != 1 || !plan.changed_parts.contains(part) {
                    return Err(PluginError::invalid_argument(
                        "an exact text expectation requires a one-part matching mutation plan",
                    ));
                }
                candidate.rewind()?;
                let actual = read_text_target_from_package(&mut candidate, part, selector)
                    .map_err(|error| {
                        verification_error(format!(
                            "cannot resolve exact text target in candidate: {}",
                            error.message
                        ))
                    })?;
                if actual != expected {
                    return Err(verification_error(format!(
                        "candidate text target is {actual:?}, expected {expected:?}"
                    )));
                }
                None
            }
        };
        Ok(VerifiedCandidate { snapshot, document })
    }
}

/// Semantic proof required after package-level candidate verification.
pub enum SemanticExpectation<'a> {
    /// Only valid for an empty mutation plan. Exact entry equality proves that
    /// no known or opaque semantic content changed without forcing the lossy
    /// DOCX conversion reader to understand unrelated active objects.
    Unchanged,
    /// Reopen with the complete conversion reader and require exact equality
    /// with the caller's expected post-mutation model.
    ExactDocument(&'a Document),
    /// Resolve one surgical text target independently from the candidate and
    /// require the exact requested value. This oracle remains usable when
    /// unrelated active HWPX objects are outside the conversion model.
    ExactText {
        part: &'a str,
        selector: &'a TextNodeSelector,
        expected: &'a str,
    },
}

/// Parts that a single candidate save is required to replace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MutationPlan {
    changed_parts: BTreeSet<String>,
    expected_content_sha256: BTreeMap<String, [u8; 32]>,
}

impl MutationPlan {
    pub fn no_op() -> Self {
        Self::default()
    }

    pub fn replace_existing<I, S>(snapshot: &PackageSnapshot, parts: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut changed_parts = BTreeSet::new();
        for part in parts {
            let part = part.as_ref();
            if part == MIMETYPE_ENTRY {
                return Err(PluginError::unsupported_feature(
                    "the HWPX mimetype entry is immutable",
                ));
            }
            let entry = snapshot.entry(part).ok_or_else(|| {
                PluginError::invalid_argument(format!(
                    "mutation plan names missing package part {part:?}"
                ))
            })?;
            if entry.directory {
                return Err(PluginError::invalid_argument(format!(
                    "mutation plan cannot replace directory entry {part:?}"
                )));
            }
            if !changed_parts.insert(part.to_owned()) {
                return Err(PluginError::invalid_argument(format!(
                    "mutation plan names package part {part:?} more than once"
                )));
            }
        }
        if changed_parts.is_empty() {
            return Err(PluginError::invalid_argument(
                "a replacement mutation plan must name at least one package part",
            ));
        }
        Ok(Self {
            changed_parts,
            expected_content_sha256: BTreeMap::new(),
        })
    }

    /// Plan exact replacement bytes for every changed part.
    pub fn replace_exact<B>(
        snapshot: &PackageSnapshot,
        replacements: &BTreeMap<String, B>,
    ) -> Result<Self>
    where
        B: AsRef<[u8]>,
    {
        let mut plan = Self::replace_existing(snapshot, replacements.keys())?;
        for (part, replacement) in replacements {
            let replacement_sha256 = hash_bytes(replacement.as_ref());
            let source = snapshot
                .entry(part)
                .expect("replace_existing validated every key");
            if source.content_sha256 == replacement_sha256 {
                return Err(PluginError::invalid_argument(format!(
                    "exact replacement for package part {part:?} is a no-op"
                )));
            }
            plan.expected_content_sha256
                .insert(part.clone(), replacement_sha256);
        }
        Ok(plan)
    }

    pub fn changed_parts(&self) -> impl Iterator<Item = &str> {
        self.changed_parts.iter().map(String::as_str)
    }

    fn has_exact_replacements(&self) -> bool {
        self.changed_parts.len() == self.expected_content_sha256.len()
    }
}

/// Stable address of one `hp:t` inside a uniquely identified HWPX paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextNodeSelector {
    paragraph: ParagraphAddress,
    text_ordinal: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ParagraphAddress {
    UniqueId(String),
    Ordinal {
        ordinal: usize,
        expected_id: Option<String>,
    },
}

impl TextNodeSelector {
    /// Address a paragraph by an id that must occur exactly once in the part.
    pub fn new(paragraph_id: impl Into<String>, text_ordinal: usize) -> Result<Self> {
        let paragraph_id = validate_paragraph_id(paragraph_id.into())?;
        Ok(Self {
            paragraph: ParagraphAddress::UniqueId(paragraph_id),
            text_ordinal,
        })
    }

    /// Address a paragraph by zero-based XML order. This is stable for the
    /// closed text-only edit subset even when Hancom repeats sentinel ids.
    pub fn at_paragraph(paragraph_ordinal: usize, text_ordinal: usize) -> Self {
        Self {
            paragraph: ParagraphAddress::Ordinal {
                ordinal: paragraph_ordinal,
                expected_id: None,
            },
            text_ordinal,
        }
    }

    /// Add the source paragraph id as a precondition to an ordinal address.
    pub fn at_paragraph_with_id(
        paragraph_ordinal: usize,
        expected_id: impl Into<String>,
        text_ordinal: usize,
    ) -> Result<Self> {
        Ok(Self {
            paragraph: ParagraphAddress::Ordinal {
                ordinal: paragraph_ordinal,
                expected_id: Some(validate_paragraph_id(expected_id.into())?),
            },
            text_ordinal,
        })
    }

    pub fn paragraph_id(&self) -> Option<&str> {
        match &self.paragraph {
            ParagraphAddress::UniqueId(id) => Some(id),
            ParagraphAddress::Ordinal { expected_id, .. } => expected_id.as_deref(),
        }
    }

    pub fn paragraph_ordinal(&self) -> Option<usize> {
        match &self.paragraph {
            ParagraphAddress::UniqueId(_) => None,
            ParagraphAddress::Ordinal { ordinal, .. } => Some(*ordinal),
        }
    }

    pub fn text_ordinal(&self) -> usize {
        self.text_ordinal
    }

    fn matches_paragraph(&self, ordinal: usize, id: Option<&str>) -> Result<bool> {
        match &self.paragraph {
            ParagraphAddress::UniqueId(expected) => Ok(id == Some(expected.as_str())),
            ParagraphAddress::Ordinal {
                ordinal: expected_ordinal,
                expected_id,
            } => {
                if ordinal != *expected_ordinal {
                    return Ok(false);
                }
                if let Some(expected_id) = expected_id {
                    if id != Some(expected_id.as_str()) {
                        return Err(PluginError::invalid_argument(format!(
                            "HWPX paragraph ordinal {ordinal} has id {id:?}, expected {expected_id:?}"
                        )));
                    }
                }
                Ok(true)
            }
        }
    }

    fn description(&self) -> String {
        match &self.paragraph {
            ParagraphAddress::UniqueId(id) => format!("paragraph id {id:?}"),
            ParagraphAddress::Ordinal {
                ordinal,
                expected_id,
            } => match expected_id {
                Some(id) => format!("paragraph ordinal {ordinal} with id {id:?}"),
                None => format!("paragraph ordinal {ordinal}"),
            },
        }
    }

    fn uses_unique_id(&self) -> bool {
        matches!(&self.paragraph, ParagraphAddress::UniqueId(_))
    }
}

fn validate_paragraph_id(paragraph_id: String) -> Result<String> {
    if paragraph_id.is_empty()
        || paragraph_id.len() > 256
        || paragraph_id.chars().any(char::is_control)
    {
        return Err(PluginError::invalid_argument(
            "HWPX text selector paragraph id must be 1..=256 non-control UTF-8 bytes",
        ));
    }
    Ok(paragraph_id)
}

#[derive(Debug)]
struct LocatedText {
    inner_range: Range<usize>,
    value: String,
}

#[derive(Debug)]
struct ActiveText {
    open_depth: usize,
    content_start: usize,
    value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParagraphElement {
    Paragraph,
    Run,
    Text,
    Other,
}

/// Replace exactly one plain-text `hp:t` payload without reserializing its XML
/// ancestors, siblings, whitespace, namespace declarations, or attributes.
pub fn replace_text_node(
    xml: &[u8],
    selector: &TextNodeSelector,
    expected: &str,
    replacement: &str,
) -> Result<Vec<u8>> {
    if expected == replacement {
        return Err(PluginError::invalid_argument(
            "HWPX text replacement is a no-op",
        ));
    }
    if replacement
        .chars()
        .any(|character| !is_xml_10_character(character))
    {
        return Err(PluginError::invalid_argument(
            "HWPX text replacement contains an XML 1.0-forbidden character",
        ));
    }

    let located = locate_text_node(xml, selector)?;
    if located.value != expected {
        return Err(PluginError::invalid_argument(format!(
            "HWPX text target contains {:?}, not the expected text {expected:?}",
            located.value
        )));
    }

    let escaped = BytesText::new(replacement);
    let output_len = xml
        .len()
        .checked_sub(located.inner_range.len())
        .and_then(|length| length.checked_add(escaped.len()))
        .ok_or_else(|| PluginError::unsupported_feature("edited HWPX XML size overflowed"))?;
    if u64::try_from(output_len).unwrap_or(u64::MAX) > MAX_XML_ENTRY_BYTES {
        return Err(PluginError::unsupported_feature(format!(
            "edited HWPX XML exceeds {MAX_XML_ENTRY_BYTES} bytes"
        )));
    }

    let mut output = Vec::with_capacity(output_len);
    output.extend_from_slice(&xml[..located.inner_range.start]);
    output.extend_from_slice(escaped.as_ref());
    output.extend_from_slice(&xml[located.inner_range.end..]);
    Ok(output)
}

/// Read one surgical text target through the same namespace-aware locator used
/// by mutation and G3 verification.
pub fn read_text_node(xml: &[u8], selector: &TextNodeSelector) -> Result<String> {
    Ok(locate_text_node(xml, selector)?.value)
}

fn locate_text_node(xml: &[u8], selector: &TextNodeSelector) -> Result<LocatedText> {
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut target_paragraph_depth = None;
    let mut target_paragraph_count = 0usize;
    let mut next_paragraph_ordinal = 0usize;
    let mut target_run_depth = None;
    let mut text_ordinal = 0usize;
    let mut active_text: Option<ActiveText> = None;
    let mut located = None;

    loop {
        let event_start = usize::try_from(reader.buffer_position()).map_err(|_| {
            PluginError::corrupt("HWPX XML event position exceeds the current address space")
        })?;
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => {
                if active_text.is_some() {
                    return Err(non_plain_text_error(selector));
                }
                match paragraph_element(&reader, &event)? {
                    ParagraphElement::Paragraph => {
                        if target_paragraph_depth.is_some() {
                            return Err(PluginError::unsupported_feature(
                                "a nested HWPX paragraph cannot be a surgical text target",
                            ));
                        }
                        let paragraph_ordinal = next_paragraph_ordinal;
                        next_paragraph_ordinal =
                            next_paragraph_ordinal.checked_add(1).ok_or_else(|| {
                                PluginError::unsupported_feature(
                                    "HWPX paragraph ordinal overflowed",
                                )
                            })?;
                        let paragraph_id = exact_attribute(&event, b"id")?;
                        if selector.matches_paragraph(paragraph_ordinal, paragraph_id.as_deref())? {
                            target_paragraph_count += 1;
                            if selector.uses_unique_id() && target_paragraph_count > 1 {
                                return Err(PluginError::invalid_argument(format!(
                                    "section contains more than one {}",
                                    selector.description()
                                )));
                            }
                            target_paragraph_depth = Some(depth);
                            text_ordinal = 0;
                        }
                    }
                    ParagraphElement::Run if target_paragraph_depth.is_some() => {
                        let paragraph_depth = target_paragraph_depth.expect("checked above");
                        let expected_run_depth =
                            paragraph_depth.checked_add(1).ok_or_else(|| {
                                PluginError::unsupported_feature("HWPX XML depth overflowed")
                            })?;
                        if depth != expected_run_depth || target_run_depth.is_some() {
                            return Err(PluginError::unsupported_feature(
                                "surgical HWPX text requires hp:run directly under the target hp:p",
                            ));
                        }
                        target_run_depth = Some(depth);
                    }
                    ParagraphElement::Text if target_paragraph_depth.is_some() => {
                        if target_run_depth != depth.checked_sub(1) {
                            return Err(PluginError::unsupported_feature(
                                "surgical HWPX text requires hp:t directly under hp:run",
                            ));
                        }
                        if text_ordinal == selector.text_ordinal() {
                            active_text = Some(ActiveText {
                                open_depth: depth,
                                content_start: usize::try_from(reader.buffer_position()).map_err(
                                    |_| {
                                        PluginError::corrupt(
                                            "HWPX XML text position exceeds the current address space",
                                        )
                                    },
                                )?,
                                value: String::new(),
                            });
                        }
                        text_ordinal = text_ordinal.checked_add(1).ok_or_else(|| {
                            PluginError::unsupported_feature("HWPX text ordinal overflowed")
                        })?;
                    }
                    _ => {}
                }
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| PluginError::unsupported_feature("HWPX XML depth overflowed"))?;
            }
            Event::Empty(event) => {
                if active_text.is_some() {
                    return Err(non_plain_text_error(selector));
                }
                match paragraph_element(&reader, &event)? {
                    ParagraphElement::Paragraph => {
                        if target_paragraph_depth.is_some() {
                            return Err(PluginError::unsupported_feature(
                                "a nested HWPX paragraph cannot be a surgical text target",
                            ));
                        }
                        let paragraph_ordinal = next_paragraph_ordinal;
                        next_paragraph_ordinal =
                            next_paragraph_ordinal.checked_add(1).ok_or_else(|| {
                                PluginError::unsupported_feature(
                                    "HWPX paragraph ordinal overflowed",
                                )
                            })?;
                        let paragraph_id = exact_attribute(&event, b"id")?;
                        if selector.matches_paragraph(paragraph_ordinal, paragraph_id.as_deref())? {
                            target_paragraph_count += 1;
                            if selector.uses_unique_id() && target_paragraph_count > 1 {
                                return Err(PluginError::invalid_argument(format!(
                                    "section contains more than one {}",
                                    selector.description()
                                )));
                            }
                        }
                    }
                    ParagraphElement::Text if target_paragraph_depth.is_some() => {
                        if target_run_depth != depth.checked_sub(1) {
                            return Err(PluginError::unsupported_feature(
                                "surgical HWPX text requires hp:t directly under hp:run",
                            ));
                        }
                        if text_ordinal == selector.text_ordinal() {
                            return Err(non_plain_text_error(selector));
                        }
                        text_ordinal = text_ordinal.checked_add(1).ok_or_else(|| {
                            PluginError::unsupported_feature("HWPX text ordinal overflowed")
                        })?;
                    }
                    _ => {}
                }
            }
            Event::Text(text) => {
                if let Some(active) = active_text.as_mut() {
                    active.value.push_str(&text.decode()?);
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(active) = active_text.as_mut() {
                    active.value.push_str(&resolve_xml_reference(&reference)?);
                }
            }
            Event::CData(_) | Event::Comment(_) | Event::PI(_) if active_text.is_some() => {
                return Err(non_plain_text_error(selector));
            }
            Event::End(_) => {
                let closing_depth = depth.checked_sub(1).ok_or_else(|| {
                    PluginError::corrupt("HWPX XML contains an unmatched closing element")
                })?;
                if active_text
                    .as_ref()
                    .is_some_and(|active| active.open_depth == closing_depth)
                {
                    let active = active_text.take().expect("active text checked above");
                    if located.is_some() {
                        return Err(PluginError::invalid_argument(
                            "HWPX text selector resolved more than once",
                        ));
                    }
                    located = Some(LocatedText {
                        inner_range: active.content_start..event_start,
                        value: active.value,
                    });
                }
                if target_paragraph_depth == Some(closing_depth) {
                    target_run_depth = None;
                    target_paragraph_depth = None;
                } else if target_run_depth == Some(closing_depth) {
                    target_run_depth = None;
                }
                depth = closing_depth;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    if target_paragraph_count == 0 {
        return Err(PluginError::invalid_argument(format!(
            "section does not contain {}",
            selector.description()
        )));
    }
    located.ok_or_else(|| {
        PluginError::invalid_argument(format!(
            "{} does not contain hp:t ordinal {}",
            selector.description(),
            selector.text_ordinal()
        ))
    })
}

fn paragraph_element(reader: &NsReader<&[u8]>, event: &BytesStart<'_>) -> Result<ParagraphElement> {
    let (namespace, local_name) = reader.resolver().resolve_element(event.name());
    match namespace {
        ResolveResult::Bound(namespace) if namespace.as_ref() == PARAGRAPH_NAMESPACE => {
            Ok(match local_name.as_ref() {
                b"p" => ParagraphElement::Paragraph,
                b"run" => ParagraphElement::Run,
                b"t" => ParagraphElement::Text,
                _ => ParagraphElement::Other,
            })
        }
        ResolveResult::Unknown(prefix) => Err(PluginError::corrupt(format!(
            "HWPX XML uses undeclared namespace prefix {:?}",
            String::from_utf8_lossy(&prefix)
        ))),
        _ => Ok(ParagraphElement::Other),
    }
}

fn exact_attribute(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    let mut value = None;
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| {
            PluginError::corrupt(format!("HWPX XML has an invalid attribute: {error}"))
        })?;
        if attribute.key.as_ref() == name {
            if value.is_some() {
                return Err(PluginError::corrupt(format!(
                    "HWPX XML repeats attribute {:?}",
                    String::from_utf8_lossy(name)
                )));
            }
            let normalized = attribute
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|error| {
                    PluginError::corrupt(format!(
                        "HWPX XML attribute cannot be normalized: {error}"
                    ))
                })?;
            value = Some(normalized.into_owned());
        }
    }
    Ok(value)
}

fn resolve_xml_reference(reference: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    if let Some(character) = reference.resolve_char_ref().map_err(|error| {
        PluginError::corrupt(format!(
            "HWPX text contains an invalid character reference: {error}"
        ))
    })? {
        return Ok(character.to_string());
    }
    let name = reference.decode()?;
    quick_xml::escape::resolve_predefined_entity(&name)
        .map(str::to_owned)
        .ok_or_else(|| {
            PluginError::corrupt(format!(
                "HWPX text contains unsupported entity reference &{name};"
            ))
        })
}

fn non_plain_text_error(selector: &TextNodeSelector) -> PluginError {
    PluginError::unsupported_feature(format!(
        "HWPX {} text ordinal {} is not a plain text-only hp:t",
        selector.description(),
        selector.text_ordinal()
    ))
}

fn is_xml_10_character(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{A}' | '\u{D}')
        || ('\u{20}'..='\u{D7FF}').contains(&character)
        || ('\u{E000}'..='\u{FFFD}').contains(&character)
        || ('\u{10000}'..='\u{10FFFF}').contains(&character)
}

/// Candidate state proven by the strict output, preservation, and semantic gates.
#[derive(Clone, Debug)]
pub struct VerifiedCandidate {
    snapshot: PackageSnapshot,
    document: Option<Document>,
}

impl VerifiedCandidate {
    pub fn snapshot(&self) -> &PackageSnapshot {
        &self.snapshot
    }

    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }
}

/// Copy a strict HWPX package with raw compressed payloads and entry order intact.
pub fn copy_package<R, W>(mut source: R, destination: W) -> Result<W>
where
    R: Read + Seek,
    W: Write + Seek,
{
    validate_output_package(&mut source)?;
    source.rewind()?;
    let archive = ZipArchive::new(source)?;
    let comment = archive.comment().to_vec();
    let mut writer = ZipWriter::new(destination);
    writer.set_raw_comment(comment.into_boxed_slice())?;
    writer.merge_archive(archive)?;
    Ok(writer.finish()?)
}

/// Build a raw-entry COW candidate and return it only after source TOCTOU,
/// exact replacement, G0-G3, and scoped semantic verification all succeed.
pub fn rewrite_and_verify<R, W>(
    baseline: &PackageBaseline,
    mut source: R,
    mut destination: W,
    plan: &MutationPlan,
    replacements: &BTreeMap<String, Vec<u8>>,
    semantic_expectation: SemanticExpectation<'_>,
) -> Result<(W, VerifiedCandidate)>
where
    R: Read + Seek,
    W: Read + Write + Seek,
{
    let current_source = PackageSnapshot::capture(&mut source)?;
    if current_source != baseline.snapshot {
        return Err(verification_error("source changed since session open"));
    }
    source.rewind()?;

    let replacement_keys = replacements.keys().cloned().collect::<BTreeSet<_>>();
    if replacement_keys != plan.changed_parts {
        return Err(PluginError::invalid_argument(
            "candidate replacement keys must exactly match the mutation plan",
        ));
    }
    if !plan.has_exact_replacements() {
        return Err(PluginError::invalid_argument(
            "raw-entry COW requires exact replacement content hashes",
        ));
    }
    if destination.seek(SeekFrom::End(0))? != 0 {
        return Err(PluginError::invalid_argument(
            "HWPX candidate destination must be empty",
        ));
    }
    destination.rewind()?;

    let mut output = if plan.changed_parts.is_empty() {
        copy_package(source, destination)?
    } else {
        let mut archive = ZipArchive::new(source)?;
        let archive_comment = archive.comment().to_vec();
        let mut writer = ZipWriter::new(destination);
        writer.set_raw_comment(archive_comment.into_boxed_slice())?;

        for index in 0..archive.len() {
            let file = archive.by_index(index)?;
            if file.extra_data().is_some_and(|extra| !extra.is_empty()) {
                return Err(PluginError::unsupported_feature(format!(
                    "raw-entry COW cannot yet preserve ZIP extra fields on part {:?}",
                    file.name()
                )));
            }
            if let Some(replacement) = replacements.get(file.name()) {
                let name = file.name().to_owned();
                let comment = file.comment().to_owned();
                let mut options = file.options().into_full_options();
                if !comment.is_empty() {
                    options = options.with_file_comment(comment);
                }
                writer.start_file(name, options)?;
                writer.write_all(replacement)?;
            } else {
                writer.raw_copy_file(file)?;
            }
        }

        let mut output = writer.finish()?;
        restore_central_version_made_by(&mut output, baseline.snapshot())?;
        output
    };

    output.rewind()?;
    let verified = baseline.verify_candidate(&mut output, plan, semantic_expectation)?;
    output.rewind()?;
    Ok((output, verified))
}

fn restore_central_version_made_by<W: Read + Write + Seek>(
    output: &mut W,
    source: &PackageSnapshot,
) -> Result<()> {
    output.rewind()?;
    let offsets = {
        let mut archive = ZipArchive::new(&mut *output)?;
        if archive.len() != source.entries.len() {
            return Err(verification_error(
                "candidate entry count changed before metadata restoration",
            ));
        }
        let mut offsets = Vec::with_capacity(archive.len());
        for (index, source_entry) in source.entries.iter().enumerate() {
            let file = archive.by_index(index)?;
            if file.name() != source_entry.name || file.name_raw() != source_entry.raw_name {
                return Err(verification_error(
                    "candidate entry identity/order changed before metadata restoration",
                ));
            }
            offsets.push(file.central_header_start());
        }
        offsets
    };

    for (offset, source_entry) in offsets.into_iter().zip(&source.entries) {
        output.seek(SeekFrom::Start(offset))?;
        let mut signature = [0u8; 4];
        output.read_exact(&mut signature)?;
        if signature != CENTRAL_DIRECTORY_SIGNATURE {
            return Err(PluginError::corrupt(format!(
                "candidate central-directory entry at offset {offset} has an invalid signature"
            )));
        }
        output.write_all(&source_entry.version_made_by)?;
    }
    output.rewind()?;
    Ok(())
}

fn read_text_target_from_package<R: Read + Seek>(
    reader: R,
    part: &str,
    selector: &TextNodeSelector,
) -> Result<String> {
    let mut archive = ZipArchive::new(reader)?;
    let mut file = archive.by_name(part).map_err(|error| {
        PluginError::corrupt(format!(
            "cannot read text expectation part {part:?}: {error}"
        ))
    })?;
    if file.size() > MAX_XML_ENTRY_BYTES {
        return Err(PluginError::unsupported_feature(format!(
            "text expectation part {part:?} exceeds {MAX_XML_ENTRY_BYTES} bytes"
        )));
    }
    let mut xml = Vec::new();
    file.by_ref()
        .take(MAX_XML_ENTRY_BYTES.saturating_add(1))
        .read_to_end(&mut xml)?;
    if u64::try_from(xml.len()).unwrap_or(u64::MAX) > MAX_XML_ENTRY_BYTES {
        return Err(PluginError::unsupported_feature(format!(
            "text expectation part {part:?} exceeded {MAX_XML_ENTRY_BYTES} bytes while reading"
        )));
    }
    Ok(locate_text_node(&xml, selector)?.value)
}

fn hash_reader<R: Read>(mut reader: R, part: &str) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            PluginError::corrupt(format!("cannot fingerprint package part {part:?}: {error}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn read_central_version_made_by<R: Read + Seek>(reader: &mut R, offset: u64) -> Result<[u8; 2]> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut header = [0u8; 6];
    reader.read_exact(&mut header).map_err(|error| {
        PluginError::corrupt(format!(
            "cannot read central-directory metadata at offset {offset}: {error}"
        ))
    })?;
    if header[..4] != CENTRAL_DIRECTORY_SIGNATURE {
        return Err(PluginError::corrupt(format!(
            "central-directory entry at offset {offset} has an invalid signature"
        )));
    }
    Ok([header[4], header[5]])
}

fn verification_error(message: impl Into<String>) -> PluginError {
    PluginError::internal(format!("HWPX save verification failed: {}", message.into()))
}
