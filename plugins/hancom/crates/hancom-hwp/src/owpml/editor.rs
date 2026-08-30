//! Package-preserving HWPX copy-on-write primitives and G3 verification.
//!
//! This module deliberately works below the lossy conversion model. A baseline
//! records both decompressed part bytes and their compressed representation;
//! candidate saves may alter only the parts named by an explicit mutation plan.
//! The candidate is then reopened through the strict package reader. Mutations
//! may additionally be compared with the complete expected known-semantic model
//! when that model can represent the edited subset.

use std::collections::BTreeSet;
use std::io::{Read, Seek, Write};

use sha2::{Digest, Sha256};
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::conformance::validate_output_package;
use super::model::Document;
use super::package::MIMETYPE_ENTRY;
use crate::error::{PluginError, Result};

const HASH_BUFFER_BYTES: usize = 64 * 1024;

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
    version_made_by: (u8, u8),
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
                version_made_by,
                comment,
                extra_data,
                compressed_sha256,
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
                let version_made_by = file.version_made_by();
                let comment = file.comment().to_owned();
                let extra_data = file.extra_data().map(<[u8]>::to_vec);
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
                    version_made_by,
                    comment,
                    extra_data,
                    compressed_sha256,
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
                version_made_by,
                comment,
                extra_data,
            });
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
}

/// Parts that a single candidate save is required to replace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MutationPlan {
    changed_parts: BTreeSet<String>,
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
        Ok(Self { changed_parts })
    }

    pub fn changed_parts(&self) -> impl Iterator<Item = &str> {
        self.changed_parts.iter().map(String::as_str)
    }
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

fn verification_error(message: impl Into<String>) -> PluginError {
    PluginError::internal(format!("HWPX save verification failed: {}", message.into()))
}
