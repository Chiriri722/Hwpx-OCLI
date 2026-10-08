//! Raw ZIP layout for byte-preserving HWPX copy-on-write saves.
//!
//! The package reader and the G0-G2 validators use the `zip` crate. The writer
//! must not re-synthesize the headers of entries it does not intend to change:
//! doing so normalizes producer metadata such as DOS attribute bits, Unix mode
//! bits without a file-type nibble, general-purpose option flags, "version
//! needed" values, and data-descriptor layouts. This module accepts only the
//! classic single-disk layout whose entries are physically contiguous in
//! central-directory order. Unchanged entries and central-directory records are
//! copied verbatim; only placement offsets and the payload fields of explicitly
//! replaced entries change. Anything else fails closed.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::ops::Range;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{PluginError, Result};

const LOCAL_SIGNATURE: [u8; 4] = *b"PK\x03\x04";
const CENTRAL_SIGNATURE: [u8; 4] = *b"PK\x01\x02";
const EOCD_SIGNATURE: [u8; 4] = *b"PK\x05\x06";
const ZIP64_LOCATOR_SIGNATURE: [u8; 4] = *b"PK\x06\x07";
const DESCRIPTOR_SIGNATURE: [u8; 4] = *b"PK\x07\x08";
const LOCAL_FIXED: usize = 30;
const CENTRAL_FIXED: usize = 46;
const EOCD_FIXED: usize = 22;
const ZIP64_LOCATOR_LEN: u64 = 20;
const FLAG_ENCRYPTED: u16 = 0x0001;
const FLAG_DESCRIPTOR: u16 = 0x0008;
const FLAG_STRONG_ENCRYPTION: u16 = 0x0040;
const FLAG_MASKED_HEADER: u16 = 0x2000;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATED: u16 = 8;
/// Local-header CRC-32, compressed size, and uncompressed size.
pub(super) const LOCAL_PAYLOAD_FIELDS: Range<usize> = 14..26;
/// Central-directory CRC-32, compressed size, and uncompressed size.
pub(super) const CENTRAL_PAYLOAD_FIELDS: Range<usize> = 16..28;
/// Central-directory offset of the matching local header.
pub(super) const CENTRAL_OFFSET_FIELD: Range<usize> = 42..46;
/// End-of-central-directory offset of the central directory.
pub(super) const EOCD_OFFSET_FIELD: Range<usize> = 16..20;

/// Raw bytes of one archive entry, in central-directory order.
#[derive(Clone, Debug)]
pub(super) struct RawEntry {
    pub(super) name: Vec<u8>,
    pub(super) local_offset: u64,
    /// Local fixed header, file name, and local extra field.
    pub(super) local_header: Vec<u8>,
    pub(super) data_len: u64,
    /// Data-descriptor bytes after the payload, or empty.
    pub(super) descriptor: Vec<u8>,
    /// Central-directory record including name, extra field, and comment.
    pub(super) central_record: Vec<u8>,
}

impl RawEntry {
    fn flags(&self) -> u16 {
        u16_at(&self.central_record, 8)
    }

    fn method(&self) -> u16 {
        u16_at(&self.central_record, 10)
    }

    fn local_extra_len(&self) -> usize {
        usize::from(u16_at(&self.local_header, 28))
    }

    fn central_extra_len(&self) -> usize {
        usize::from(u16_at(&self.central_record, 30))
    }

    fn end(&self) -> u64 {
        self.local_offset
            + self.local_header.len() as u64
            + self.data_len
            + self.descriptor.len() as u64
    }

    /// Data-descriptor CRC-32 and sizes.
    ///
    /// `read_entry` stores either the 16-byte signed form or the 12-byte
    /// unsigned form, so the length identifies the layout. The first bytes
    /// cannot: an unsigned descriptor's CRC-32 may equal the signature.
    pub(super) fn descriptor_payload_fields(descriptor: &[u8]) -> Range<usize> {
        match descriptor.len() {
            16 => 4..16,
            12 => 0..12,
            _ => 0..0,
        }
    }
}

/// Raw classic ZIP layout of a strict editable package.
#[derive(Clone, Debug)]
pub(super) struct RawLayout {
    pub(super) entries: Vec<RawEntry>,
    /// End-of-central-directory record including the archive comment.
    pub(super) eocd: Vec<u8>,
}

impl RawLayout {
    /// Parse the byte layout that the COW writer can preserve exactly.
    pub(super) fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let length = reader.seek(SeekFrom::End(0))?;
        if length < EOCD_FIXED as u64 {
            return Err(PluginError::corrupt(
                "HWPX package is too short to contain a ZIP end record",
            ));
        }
        let tail_len = length.min(EOCD_FIXED as u64 + u64::from(u16::MAX));
        let tail_start = length - tail_len;
        let tail = read_exact_at(reader, tail_start, tail_len, "end record")?;
        let candidates = (0..=tail.len() - EOCD_FIXED)
            .rev()
            .filter(|&index| {
                tail[index..index + 4] == EOCD_SIGNATURE
                    && index + EOCD_FIXED + usize::from(u16_at(&tail, index + 20)) <= tail.len()
            })
            .collect::<Vec<_>>();
        let eocd_in_tail = candidates
            .iter()
            .copied()
            .find(|&index| {
                index + EOCD_FIXED + usize::from(u16_at(&tail, index + 20)) == tail.len()
            })
            .ok_or_else(|| {
                if candidates.is_empty() {
                    PluginError::corrupt("HWPX package has no valid ZIP end record")
                } else {
                    unsupported("bytes after the ZIP end record")
                }
            })?;
        let eocd_offset = tail_start + eocd_in_tail as u64;
        let eocd = tail[eocd_in_tail..].to_vec();

        let disk = u16_at(&eocd, 4);
        let central_disk = u16_at(&eocd, 6);
        let disk_entries = u16_at(&eocd, 8);
        let total_entries = u16_at(&eocd, 10);
        let central_size = u64::from(u32_at(&eocd, 12));
        let central_offset = u64::from(u32_at(&eocd, 16));
        if disk != 0 || central_disk != 0 || disk_entries != total_entries {
            return Err(unsupported("multi-disk ZIP packages"));
        }
        if total_entries == u16::MAX
            || central_size == u64::from(u32::MAX)
            || central_offset == u64::from(u32::MAX)
        {
            return Err(unsupported("ZIP64 packages"));
        }
        if eocd_offset >= ZIP64_LOCATOR_LEN {
            let locator = read_exact_at(
                reader,
                eocd_offset - ZIP64_LOCATOR_LEN,
                4,
                "ZIP64 locator probe",
            )?;
            if locator == ZIP64_LOCATOR_SIGNATURE {
                return Err(unsupported("ZIP64 packages"));
            }
        }
        if central_offset.checked_add(central_size) != Some(eocd_offset) {
            return Err(unsupported(
                "bytes between the ZIP central directory and its end record",
            ));
        }

        let central = read_exact_at(reader, central_offset, central_size, "central directory")?;
        let mut entries = Vec::with_capacity(usize::from(total_entries));
        let mut cursor = 0usize;
        for index in 0..usize::from(total_entries) {
            let record = central_record(&central, cursor, index)?;
            cursor += record.len();
            entries.push(read_entry(reader, record)?);
        }
        if cursor != central.len() {
            return Err(unsupported(
                "trailing bytes inside the ZIP central directory",
            ));
        }

        let mut expected_offset = 0u64;
        for entry in &entries {
            if entry.local_offset != expected_offset {
                return Err(unsupported(
                    "a non-contiguous ZIP layout or one whose physical order differs from the central directory",
                ));
            }
            expected_offset = entry.end();
        }
        if expected_offset != central_offset {
            return Err(unsupported(
                "bytes between the last ZIP entry and the central directory",
            ));
        }

        Ok(Self { entries, eocd })
    }

    /// Write a candidate that differs from the source only in `replacements`.
    ///
    /// Every other entry is copied byte-for-byte. Replaced entries retain every
    /// header byte except CRC-32 and sizes; their payload uses the source
    /// compression method. `destination` must be empty and positioned at byte 0.
    pub(super) fn rewrite<R: Read + Seek, W: Write>(
        &self,
        source: &mut R,
        destination: &mut W,
        replacements: &BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        let mut position = 0u64;
        let mut placements = Vec::with_capacity(self.entries.len());
        let mut replaced = BTreeSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let offset = checked_u32(position, "a local-header offset")?;
            let replacement = std::str::from_utf8(&entry.name)
                .ok()
                .and_then(|name| replacements.get_key_value(name));
            let payload = match replacement {
                None => {
                    source.seek(SeekFrom::Start(entry.local_offset))?;
                    let length = entry.end() - entry.local_offset;
                    let copied = io::copy(&mut source.by_ref().take(length), destination)?;
                    if copied != length {
                        return Err(PluginError::corrupt(format!(
                            "HWPX source ended while copying ZIP entry {index}"
                        )));
                    }
                    position += length;
                    None
                }
                Some((name, bytes)) => {
                    if !replaced.insert(name.as_str()) {
                        return Err(PluginError::invalid_argument(format!(
                            "replacement for {name:?} matches more than one ZIP entry"
                        )));
                    }
                    if entry.local_extra_len() != 0 || entry.central_extra_len() != 0 {
                        return Err(PluginError::unsupported_feature(format!(
                            "raw-entry COW cannot yet preserve ZIP extra fields on replaced part {name:?}"
                        )));
                    }
                    let (crc32, data) = encode_payload(entry.method(), bytes, name)?;
                    let fields = payload_fields(crc32, data.len(), bytes.len(), name)?;
                    let mut local = entry.local_header.clone();
                    let placeholder = entry.flags() & FLAG_DESCRIPTOR != 0
                        && local[LOCAL_PAYLOAD_FIELDS].iter().all(|byte| *byte == 0);
                    if !placeholder {
                        local[LOCAL_PAYLOAD_FIELDS].copy_from_slice(&fields);
                    }
                    let mut descriptor = entry.descriptor.clone();
                    if !descriptor.is_empty() {
                        let range = RawEntry::descriptor_payload_fields(&descriptor);
                        descriptor
                            .get_mut(range)
                            .filter(|field| field.len() == fields.len())
                            .ok_or_else(|| {
                                PluginError::internal(format!(
                                    "unexpected data descriptor layout on replaced part {name:?}"
                                ))
                            })?
                            .copy_from_slice(&fields);
                    }
                    destination.write_all(&local)?;
                    destination.write_all(&data)?;
                    destination.write_all(&descriptor)?;
                    position += (local.len() + data.len() + descriptor.len()) as u64;
                    Some(fields)
                }
            };
            placements.push((offset, payload));
        }
        if replaced.len() != replacements.len() {
            let missing = replacements
                .keys()
                .filter(|name| !replaced.contains(name.as_str()))
                .collect::<Vec<_>>();
            return Err(PluginError::invalid_argument(format!(
                "replacement names package parts that are not raw ZIP entries: {missing:?}"
            )));
        }

        let central_offset = checked_u32(position, "the central-directory offset")?;
        for (entry, (offset, payload)) in self.entries.iter().zip(placements) {
            let mut record = entry.central_record.clone();
            record[CENTRAL_OFFSET_FIELD].copy_from_slice(&offset.to_le_bytes());
            if let Some(fields) = payload {
                record[CENTRAL_PAYLOAD_FIELDS].copy_from_slice(&fields);
            }
            destination.write_all(&record)?;
            position += record.len() as u64;
        }
        let central_size = checked_u32(
            position - u64::from(central_offset),
            "the central-directory size",
        )?;
        let mut eocd = self.eocd.clone();
        eocd[12..16].copy_from_slice(&central_size.to_le_bytes());
        eocd[EOCD_OFFSET_FIELD].copy_from_slice(&central_offset.to_le_bytes());
        destination.write_all(&eocd)?;
        destination.flush()?;
        Ok(())
    }
}

fn central_record(central: &[u8], cursor: usize, index: usize) -> Result<&[u8]> {
    let truncated =
        || PluginError::corrupt(format!("central-directory record {index} is truncated"));
    let fixed = central
        .get(cursor..cursor + CENTRAL_FIXED)
        .ok_or_else(truncated)?;
    if fixed[..4] != CENTRAL_SIGNATURE {
        return Err(PluginError::corrupt(format!(
            "central-directory record {index} has an invalid signature"
        )));
    }
    let variable = usize::from(u16_at(fixed, 28))
        + usize::from(u16_at(fixed, 30))
        + usize::from(u16_at(fixed, 32));
    central
        .get(cursor..cursor + CENTRAL_FIXED + variable)
        .ok_or_else(truncated)
}

fn read_entry<R: Read + Seek>(reader: &mut R, record: &[u8]) -> Result<RawEntry> {
    let flags = u16_at(record, 8);
    let method = u16_at(record, 10);
    let compressed_size = u32_at(record, 20);
    let uncompressed_size = u32_at(record, 24);
    let name_len = usize::from(u16_at(record, 28));
    let start_disk = u16_at(record, 34);
    let local_offset = u32_at(record, 42);
    let name = record[CENTRAL_FIXED..CENTRAL_FIXED + name_len].to_vec();
    let label = String::from_utf8_lossy(&name).into_owned();
    if flags & (FLAG_ENCRYPTED | FLAG_STRONG_ENCRYPTION | FLAG_MASKED_HEADER) != 0 {
        return Err(unsupported(&format!("encrypted ZIP entry {label:?}")));
    }
    if !matches!(method, METHOD_STORED | METHOD_DEFLATED) {
        return Err(unsupported(&format!(
            "ZIP compression method {method} on entry {label:?}"
        )));
    }
    if start_disk != 0 {
        return Err(unsupported("multi-disk ZIP packages"));
    }
    if [compressed_size, uncompressed_size, local_offset].contains(&u32::MAX) {
        return Err(unsupported("ZIP64 packages"));
    }

    let local_offset = u64::from(local_offset);
    let fixed = read_exact_at(reader, local_offset, LOCAL_FIXED as u64, "local header")?;
    if fixed[..4] != LOCAL_SIGNATURE {
        return Err(PluginError::corrupt(format!(
            "ZIP entry {label:?} has an invalid local-header signature"
        )));
    }
    let local_variable = u64::from(u16_at(&fixed, 26)) + u64::from(u16_at(&fixed, 28));
    let local_header = read_exact_at(
        reader,
        local_offset,
        LOCAL_FIXED as u64 + local_variable,
        "local header",
    )?;
    let local_name_len = usize::from(u16_at(&local_header, 26));
    if local_header[LOCAL_FIXED..LOCAL_FIXED + local_name_len] != name[..] {
        return Err(unsupported(&format!(
            "different local and central file names for ZIP entry {label:?}"
        )));
    }
    let local_flags = u16_at(&local_header, 6);
    if u16_at(&local_header, 8) != method || (local_flags ^ flags) & FLAG_DESCRIPTOR != 0 {
        return Err(unsupported(&format!(
            "inconsistent local and central headers for ZIP entry {label:?}"
        )));
    }
    let central_fields = &record[CENTRAL_PAYLOAD_FIELDS];
    let local_fields = &local_header[LOCAL_PAYLOAD_FIELDS];
    let placeholder = flags & FLAG_DESCRIPTOR != 0 && local_fields.iter().all(|byte| *byte == 0);
    if local_fields != central_fields && !placeholder {
        return Err(unsupported(&format!(
            "inconsistent local and central sizes for ZIP entry {label:?}"
        )));
    }

    let data_len = u64::from(compressed_size);
    let data_end = local_offset + local_header.len() as u64 + data_len;
    let descriptor = if flags & FLAG_DESCRIPTOR == 0 {
        Vec::new()
    } else {
        let probe = read_up_to_at(reader, data_end, 16)?;
        if probe.len() == 16 && probe[..4] == DESCRIPTOR_SIGNATURE && probe[4..] == *central_fields
        {
            probe
        } else if probe.len() >= 12 && probe[..12] == *central_fields {
            probe[..12].to_vec()
        } else {
            return Err(unsupported(&format!(
                "an unrecognized data descriptor on ZIP entry {label:?}"
            )));
        }
    };
    Ok(RawEntry {
        name,
        local_offset,
        local_header,
        data_len,
        descriptor,
        central_record: record.to_vec(),
    })
}

fn encode_payload(method: u16, bytes: &[u8], name: &str) -> Result<(u32, Vec<u8>)> {
    let compression = match method {
        METHOD_STORED => CompressionMethod::Stored,
        METHOD_DEFLATED => CompressionMethod::Deflated,
        other => {
            return Err(unsupported(&format!(
                "ZIP compression method {other} on replaced part {name:?}"
            )));
        }
    };
    // Reuse the crate's configured encoder and CRC so a replaced payload is
    // produced exactly as by the previous ZipWriter-based implementation.
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file(
        "payload",
        SimpleFileOptions::default().compression_method(compression),
    )?;
    writer.write_all(bytes)?;
    let mut archive = ZipArchive::new(writer.finish()?)?;
    let mut raw = archive.by_index_raw(0)?;
    let crc32 = raw.crc32();
    let mut data = Vec::with_capacity(usize::try_from(raw.compressed_size()).unwrap_or(0));
    raw.read_to_end(&mut data)?;
    Ok((crc32, data))
}

fn payload_fields(
    crc32: u32,
    compressed: usize,
    uncompressed: usize,
    name: &str,
) -> Result<[u8; 12]> {
    let too_large = || unsupported(&format!("a ZIP64-sized replacement for {name:?}"));
    let compressed = u32::try_from(compressed)
        .ok()
        .filter(|size| *size != u32::MAX)
        .ok_or_else(too_large)?;
    let uncompressed = u32::try_from(uncompressed)
        .ok()
        .filter(|size| *size != u32::MAX)
        .ok_or_else(too_large)?;
    let mut fields = [0u8; 12];
    fields[..4].copy_from_slice(&crc32.to_le_bytes());
    fields[4..8].copy_from_slice(&compressed.to_le_bytes());
    fields[8..].copy_from_slice(&uncompressed.to_le_bytes());
    Ok(fields)
}

fn checked_u32(value: u64, label: &str) -> Result<u32> {
    u32::try_from(value)
        .ok()
        .filter(|value| *value != u32::MAX)
        .ok_or_else(|| unsupported(&format!("{label} that requires ZIP64")))
}

fn read_exact_at<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    length: u64,
    what: &str,
) -> Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    reader.by_ref().take(length).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length {
        return Err(PluginError::corrupt(format!(
            "HWPX package ended inside the ZIP {what} at byte {offset}"
        )));
    }
    Ok(bytes)
}

fn read_up_to_at<R: Read + Seek>(reader: &mut R, offset: u64, length: u64) -> Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    reader.by_ref().take(length).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn unsupported(what: &str) -> PluginError {
    PluginError::unsupported_feature(format!("byte-preserving HWPX saves do not support {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body, method) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(*method),
                )
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn sample() -> Vec<u8> {
        package(&[
            (
                "mimetype",
                b"application/hwp+zip",
                CompressionMethod::Stored,
            ),
            ("a.xml", b"<a>alpha</a>", CompressionMethod::Deflated),
            ("b.bin", b"opaque", CompressionMethod::Stored),
        ])
    }

    #[test]
    fn empty_replacement_set_reproduces_the_exact_source_bytes() {
        let source = sample();
        let layout = RawLayout::parse(&mut Cursor::new(&source)).unwrap();
        assert_eq!(layout.entries.len(), 3);
        let mut output = Vec::new();
        layout
            .rewrite(&mut Cursor::new(&source), &mut output, &BTreeMap::new())
            .unwrap();
        assert_eq!(output, source);
    }

    #[test]
    fn replacement_changes_only_payload_fields_and_placement() {
        let source = sample();
        let layout = RawLayout::parse(&mut Cursor::new(&source)).unwrap();
        let replacements =
            BTreeMap::from([("a.xml".to_owned(), b"<a>a much longer beta</a>".to_vec())]);
        let mut output = Vec::new();
        layout
            .rewrite(&mut Cursor::new(&source), &mut output, &replacements)
            .unwrap();
        let mut archive = ZipArchive::new(Cursor::new(&output)).unwrap();
        let mut body = String::new();
        archive
            .by_name("a.xml")
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert_eq!(body, "<a>a much longer beta</a>");
        let candidate = RawLayout::parse(&mut Cursor::new(&output)).unwrap();
        for (before, after) in layout.entries.iter().zip(&candidate.entries) {
            let mut left = before.central_record.clone();
            let mut right = after.central_record.clone();
            left[CENTRAL_OFFSET_FIELD].fill(0);
            right[CENTRAL_OFFSET_FIELD].fill(0);
            if before.name == b"a.xml" {
                left[CENTRAL_PAYLOAD_FIELDS].fill(0);
                right[CENTRAL_PAYLOAD_FIELDS].fill(0);
            }
            assert_eq!(left, right);
        }
    }

    /// One stored entry written in streaming form: zero local CRC/sizes and a
    /// 12-byte data descriptor without the optional signature.
    fn unsigned_descriptor_zip(name: &[u8], data: &[u8], crc32: u32) -> Vec<u8> {
        let name_len = u16::try_from(name.len()).unwrap().to_le_bytes();
        let size = u32::try_from(data.len()).unwrap().to_le_bytes();
        let mut out = Vec::new();
        out.extend_from_slice(&LOCAL_SIGNATURE);
        out.extend_from_slice(&[20, 0, 0x08, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&name_len);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32.to_le_bytes());
        out.extend_from_slice(&size);
        out.extend_from_slice(&size);
        let central_offset = u32::try_from(out.len()).unwrap();
        out.extend_from_slice(&CENTRAL_SIGNATURE);
        out.extend_from_slice(&[20, 0, 20, 0, 0x08, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc32.to_le_bytes());
        out.extend_from_slice(&size);
        out.extend_from_slice(&size);
        out.extend_from_slice(&name_len);
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(name);
        let central_size = u32::try_from(out.len()).unwrap() - central_offset;
        out.extend_from_slice(&EOCD_SIGNATURE);
        out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
        out.extend_from_slice(&central_size.to_le_bytes());
        out.extend_from_slice(&central_offset.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn unsigned_descriptor_whose_crc_equals_the_signature_is_rewritten() {
        let crc32 = u32::from_le_bytes(DESCRIPTOR_SIGNATURE);
        let source = unsigned_descriptor_zip(b"a.bin", b"payload", crc32);
        let layout = RawLayout::parse(&mut Cursor::new(&source)).unwrap();
        assert_eq!(layout.entries[0].descriptor.len(), 12);
        assert_eq!(
            RawEntry::descriptor_payload_fields(&layout.entries[0].descriptor),
            0..12
        );
        let replacements = BTreeMap::from([("a.bin".to_owned(), b"other payload".to_vec())]);
        let mut output = Vec::new();
        layout
            .rewrite(&mut Cursor::new(&source), &mut output, &replacements)
            .unwrap();
        let rewritten = RawLayout::parse(&mut Cursor::new(&output)).unwrap();
        let entry = &rewritten.entries[0];
        assert_eq!(
            entry.descriptor,
            entry.central_record[CENTRAL_PAYLOAD_FIELDS]
        );
        assert!(entry.local_header[LOCAL_PAYLOAD_FIELDS]
            .iter()
            .all(|byte| *byte == 0));
        let mut body = Vec::new();
        ZipArchive::new(Cursor::new(&output))
            .unwrap()
            .by_name("a.bin")
            .unwrap()
            .read_to_end(&mut body)
            .unwrap();
        assert_eq!(body, b"other payload");
    }

    #[test]
    fn unknown_replacement_names_fail_before_success() {
        let source = sample();
        let layout = RawLayout::parse(&mut Cursor::new(&source)).unwrap();
        let replacements = BTreeMap::from([("missing.xml".to_owned(), b"x".to_vec())]);
        let error = layout
            .rewrite(&mut Cursor::new(&source), &mut Vec::new(), &replacements)
            .expect_err("unknown part");
        assert_eq!(error.code.as_str(), "invalid_argument");
    }

    #[test]
    fn unsupported_layouts_fail_closed() {
        let source = sample();
        let layout = RawLayout::parse(&mut Cursor::new(&source)).unwrap();
        let central_offset = usize::try_from(u32_at(&layout.eocd, 16)).unwrap();

        // Bytes between the last entry and the central directory.
        let mut gapped = source[..central_offset].to_vec();
        gapped.extend_from_slice(b"gap!");
        gapped.extend_from_slice(&source[central_offset..]);
        let eocd = gapped.len() - layout.eocd.len();
        let moved = (u32::try_from(central_offset).unwrap() + 4).to_le_bytes();
        gapped[eocd + 16..eocd + 20].copy_from_slice(&moved);
        let error = RawLayout::parse(&mut Cursor::new(&gapped)).expect_err("gap");
        assert_eq!(error.code.as_str(), "unsupported_feature");
        assert!(error.message.contains("last ZIP entry"), "{error:?}");

        // A ZIP64 locator immediately before the classic end record.
        let central_end = source.len() - layout.eocd.len();
        let mut zip64 = source[..central_end].to_vec();
        let mut locator = ZIP64_LOCATOR_SIGNATURE.to_vec();
        locator.resize(usize::try_from(ZIP64_LOCATOR_LEN).unwrap(), 0);
        zip64.extend_from_slice(&locator);
        zip64.extend_from_slice(&layout.eocd);
        let error = RawLayout::parse(&mut Cursor::new(&zip64)).expect_err("zip64");
        assert_eq!(error.code.as_str(), "unsupported_feature");
        assert!(error.message.contains("ZIP64"), "{error:?}");

        // Local and central names that disagree.
        let mut renamed = source.clone();
        renamed[LOCAL_FIXED] = b'M';
        let error = RawLayout::parse(&mut Cursor::new(&renamed)).expect_err("renamed");
        assert!(error.message.contains("file names"), "{error:?}");

        // Trailing bytes after the end record (for example an appended payload).
        let mut trailing = source.clone();
        trailing.extend_from_slice(b"tail");
        let error = RawLayout::parse(&mut Cursor::new(&trailing)).expect_err("trailing");
        assert_eq!(error.code.as_str(), "unsupported_feature");
        assert!(
            error.message.contains("after the ZIP end record"),
            "{error:?}"
        );
    }
}
