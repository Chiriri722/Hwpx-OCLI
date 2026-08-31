//! Conservative bridge for modern Hancom Cell and Show files that are already OOXML.
//!
//! Public samples establish only two narrow profiles: Cell 12.0300 is an XLSX
//! package and Show 12.0000 is a PPTX package. This module validates profile
//! markers, archive safety, OPC closure, and every entry CRC before making a
//! byte-for-byte native sibling. These markers classify the evidence-backed
//! subset; they do not authenticate Hancom as the producer. Unknown and legacy
//! generations remain unsupported instead of being guessed.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, FileTimes, Metadata};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use quick_xml::events::{BytesDecl, BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use quick_xml::XmlVersion;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use zip::{CompressionMethod, ZipArchive};

use crate::error::{ExitCode, PluginError, Result};

const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const ZIP_SIGNATURES: [[u8; 4]; 3] = [*b"PK\x03\x04", *b"PK\x05\x06", *b"PK\x07\x08"];
const CONTENT_TYPES: &str = "[Content_Types].xml";
const ROOT_RELS: &str = "_rels/.rels";
const APP_PROPERTIES: &str = "docProps/app.xml";
const CONTENT_TYPES_NS: &[u8] = b"http://schemas.openxmlformats.org/package/2006/content-types";
const PACKAGE_RELS_NS: &[u8] = b"http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const APP_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const CORE_PROPERTIES_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const THUMBNAIL_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
const HYPERLINK_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const APP_NS: &[u8] = b"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const DOCUMENT_REL_NS: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DRAWING_NS: &[u8] = b"http://schemas.openxmlformats.org/drawingml/2006/main";
const SPREADSHEET_NS: &[u8] = b"http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const PRESENTATION_NS: &[u8] = b"http://schemas.openxmlformats.org/presentationml/2006/main";
const HANCOM_SPREADSHEET_NS: &[u8] = b"http://schemas.haansoft.com/office/spreadsheet/8.0";
const CELL_MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const CELL_MACRO_MAIN_TYPE: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
const SHOW_MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_XML_BYTES: u64 = 16 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 1000;
const MAX_XML_DEPTH: usize = 256;
const MAX_XML_EVENTS: usize = 1_000_000;
const MAX_XML_ATTRIBUTES_PER_ELEMENT: usize = 1024;
const MAX_XML_NAMESPACES_PER_ELEMENT: usize = 256;
const MAX_XML_NAME_BYTES: usize = 1024;
const MAX_XML_ATTRIBUTE_VALUE_BYTES: usize = 1024 * 1024;
const HANCOM_PUBLIC_SPEC_NOTICE: &str =
    "본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierFamily {
    Cell,
    Show,
}

#[derive(Debug)]
struct InternalRelationship {
    id: String,
    relationship_type: String,
    target: String,
}

impl CarrierFamily {
    fn plugin_name(self) -> &'static str {
        match self {
            Self::Cell => "officecli-hancom-cell",
            Self::Show => "officecli-hancom-show",
        }
    }

    fn source_extension(self) -> &'static str {
        match self {
            Self::Cell => ".cell",
            Self::Show => ".show",
        }
    }

    fn target(self) -> &'static str {
        match self {
            Self::Cell => "xlsx",
            Self::Show => "pptx",
        }
    }

    fn main_part(self) -> &'static str {
        match self {
            Self::Cell => "xl/workbook.xml",
            Self::Show => "ppt/presentation.xml",
        }
    }

    fn application(self) -> &'static str {
        match self {
            Self::Cell => "Cell",
            Self::Show => "Show",
        }
    }

    fn profile_version(self) -> &'static str {
        match self {
            Self::Cell => "12.0300",
            Self::Show => "12.0000",
        }
    }

    fn profile_capability(self) -> &'static str {
        match self {
            Self::Cell => "hancom-cell-12.0300-ooxml-carrier-subset",
            Self::Show => "hancom-show-12.0000-ooxml-carrier-subset",
        }
    }
}

pub fn carrier_manifest(family: CarrierFamily) -> Value {
    let summary = match family {
        CarrierFamily::Cell => {
            "Validated byte-preserving bridge for the evidence-backed Hancom Cell 12.0300 OOXML carrier subset."
        }
        CarrierFamily::Show => {
            "Validated byte-preserving bridge for the evidence-backed Hancom Show 12.0000 OOXML carrier subset."
        }
    };
    let description = format!("{summary} {HANCOM_PUBLIC_SPEC_NOTICE}");
    json!({
        "name": family.plugin_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": 1,
        "kinds": ["dump-reader"],
        "extensions": [family.source_extension()],
        "target": family.target(),
        "runtime": "rust",
        "idle_timeout_seconds": {
            "default": 60,
            "verbs": { "dump": 30 }
        },
        "description": description,
        "license": "MIT",
        "supports": [family.profile_capability(), "byte-preserving", "direct-native"]
    })
}

pub fn main_entry(family: CarrierFamily) -> std::process::ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr();
    let result = run_args(family, args, &mut stdout, &mut stderr);
    let code = match result {
        Ok(()) => ExitCode::Success,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "{}",
                crate::escape_diagnostic_text(&error.to_string())
            );
            error.exit_code()
        }
    };
    std::process::ExitCode::from(u8::try_from(code.as_i32()).unwrap_or(70))
}

pub fn run_args<O, E>(
    family: CarrierFamily,
    args: Vec<OsString>,
    stdout: &mut O,
    stderr: &mut E,
) -> Result<()>
where
    O: Write,
    E: Write,
{
    let Some(first) = args.first() else {
        write_help(family, stderr)?;
        return Ok(());
    };
    let command = first
        .to_str()
        .ok_or_else(|| PluginError::unsupported_command("subcommand must be valid UTF-8"))?;
    match command {
        "--info" | "--version" | "-V" if args.len() == 1 => {
            writeln!(stdout, "{}", carrier_manifest(family))?;
            stdout.flush()?;
            Ok(())
        }
        "--help" | "-h" if args.len() == 1 => write_help(family, stderr),
        "dump" => {
            let source = parse_dump_source(&args)?;
            bridge_ooxml(&source, family)?;
            Ok(())
        }
        other => Err(PluginError::unsupported_command(format!(
            "unknown subcommand: {}",
            other
        ))),
    }
}

fn parse_dump_source(args: &[OsString]) -> Result<PathBuf> {
    let mut source = None;
    let mut index = 1_usize;
    while index < args.len() {
        let argument = &args[index];
        if argument == OsStr::new("--media-dir") || argument == OsStr::new("--log-file") {
            let flag = argument.to_string_lossy();
            index += 1;
            if index == args.len() {
                return Err(PluginError::invalid_argument(format!(
                    "{flag} requires a value"
                )));
            }
        } else if argument == OsStr::new("--quiet") {
            // The carrier emits no progress diagnostics, so quiet is already
            // the effective behavior. Accept it as required by protocol §5.4.
        } else if argument
            .to_str()
            .is_some_and(|value| value.starts_with("--"))
        {
            return Err(PluginError::invalid_argument(format!(
                "unknown option for dump: {}",
                argument.to_string_lossy()
            )));
        } else {
            let positional = PathBuf::from(argument);
            if source.replace(positional).is_some() {
                return Err(PluginError::invalid_argument(format!(
                    "unexpected extra argument: {}",
                    argument.to_string_lossy()
                )));
            }
        }
        index += 1;
    }
    source.ok_or_else(|| PluginError::invalid_argument("dump requires a <source-file> argument"))
}

fn write_help<E: Write>(family: CarrierFamily, stderr: &mut E) -> Result<()> {
    writeln!(
        stderr,
        "{} — validated Hancom {} {} OOXML carrier subset\n\nUsage:\n  {} --info\n  {} dump <source{}> [--media-dir <dir>] [--log-file <path>] [--quiet]\n\nNOTICE:\n{}",
        family.plugin_name(),
        family.application(),
        family.profile_version(),
        family.plugin_name(),
        family.plugin_name(),
        family.source_extension(),
        HANCOM_PUBLIC_SPEC_NOTICE
    )?;
    stderr.flush()?;
    Ok(())
}

pub fn bridge_ooxml(source: &Path, family: CarrierFamily) -> Result<PathBuf> {
    let expected_extension = family.source_extension().trim_start_matches('.');
    if !source
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected_extension))
    {
        return Err(PluginError::invalid_argument(format!(
            "{} accepts only {} source paths",
            family.plugin_name(),
            family.source_extension()
        )));
    }
    let source_link_metadata = fs::symlink_metadata(source)
        .map_err(|error| PluginError::corrupt(format!("cannot inspect source file: {error}")))?;
    if is_reparseable(&source_link_metadata) || !source_link_metadata.file_type().is_file() {
        return Err(PluginError::invalid_argument(
            "source must be a non-reparse regular file",
        ));
    }
    let mut source_file = open_source_without_reparse(source).map_err(|error| {
        PluginError::invalid_argument(format!("cannot open source file safely: {error}"))
    })?;
    let initial_metadata = source_file.metadata()?;
    if is_reparseable(&initial_metadata) || !initial_metadata.file_type().is_file() {
        return Err(PluginError::invalid_argument(
            "opened source must be a non-reparse regular file",
        ));
    }
    if initial_metadata.len() > MAX_SOURCE_BYTES {
        return Err(resource_limit(format!(
            "source size {} exceeds {MAX_SOURCE_BYTES} bytes",
            initial_metadata.len()
        )));
    }
    validate_supported_source_security_metadata(source, &initial_metadata)?;

    let mut target = source.to_path_buf();
    target.set_extension(family.target());
    if source == target {
        return Err(PluginError::invalid_argument(
            "native sibling path aliases the source path",
        ));
    }
    drop(ensure_target_safe(&source_file, &target)?);

    let parent = target
        .parent()
        .ok_or_else(|| PluginError::invalid_argument("source path has no parent directory"))?;
    let mut candidate = new_private_candidate(parent)?;
    // Install the admitted access-control metadata while the candidate is
    // still empty. On Windows the retained handle also denies new data opens;
    // on Unix tempfile starts at mode 0600. No document bytes are exposed
    // under inherited directory permissions while the candidate is populated.
    prepare_private_candidate_access_control(source, candidate.path())?;
    source_file.seek(SeekFrom::Start(0))?;
    let candidate_hash = copy_and_hash(&mut source_file, candidate.as_file_mut())?;
    let source_hash = hash_file(&mut source_file)?;
    let copied_metadata = source_file.metadata()?;
    if source_hash != candidate_hash
        || copied_metadata.len() != initial_metadata.len()
        || copied_metadata.modified().ok() != initial_metadata.modified().ok()
    {
        return Err(PluginError::corrupt(
            "source changed while the native sibling was being created",
        ));
    }

    // Validate a private snapshot of the exact bytes that will be published.
    // The Show timestamp compatibility rewrite is applied only to a second,
    // anonymous validation copy; neither this candidate nor the source is
    // modified by validation.
    classify_container(candidate.as_file_mut())?;
    let mut validation_copy = sanitized_validation_copy(candidate.as_file_mut(), family)?;
    validate_package(&mut validation_copy, family)?;

    candidate.as_file_mut().flush()?;
    preserve_output_security_metadata(source, &source_file, candidate.path(), candidate.as_file())?;
    candidate
        .as_file()
        .set_permissions(initial_metadata.permissions())?;
    if let Ok(modified) = initial_metadata.modified() {
        candidate
            .as_file()
            .set_times(FileTimes::new().set_modified(modified))?;
    }
    preserve_output_access_control(source, candidate.path())?;
    candidate.as_file().sync_all()?;
    if !security_metadata_matches(source, &source_file, candidate.path(), candidate.as_file())? {
        return Err(PluginError::invalid_argument(
            "native sibling candidate did not preserve source filesystem trust metadata",
        ));
    }

    // Metadata reconciliation and fsync can take long enough for another
    // writer to race the source. Recheck the open source handle only after
    // those operations, immediately before inspecting/publishing the target.
    let final_hash = hash_file(&mut source_file)?;
    let final_metadata = source_file.metadata()?;
    if final_hash != candidate_hash
        || final_metadata.len() != initial_metadata.len()
        || final_metadata.modified().ok() != initial_metadata.modified().ok()
    {
        return Err(PluginError::corrupt(
            "source changed before the native sibling could be committed",
        ));
    }
    if !source_path_still_names_file(source, &source_file)?
        || !security_metadata_matches(source, &source_file, candidate.path(), candidate.as_file())?
    {
        return Err(PluginError::corrupt(
            "source identity or trust metadata changed before the native sibling could be committed",
        ));
    }

    if let Some(mut existing_target) = ensure_target_safe(&source_file, &target)? {
        let existing_metadata = existing_target.metadata()?;
        let existing_hash = hash_file(&mut existing_target)?;
        if existing_metadata.len() == initial_metadata.len()
            && existing_hash == candidate_hash
            && security_metadata_matches(source, &source_file, &target, &existing_target)?
        {
            return Ok(target);
        }
        return Err(PluginError::invalid_argument(
            "native sibling already exists with different content; move or remove it explicitly",
        ));
    }
    install_candidate(candidate, &target)?;
    verify_published_candidate(source, &source_file, &target, candidate_hash)?;
    Ok(target)
}

fn new_private_candidate(parent: &Path) -> Result<NamedTempFile> {
    NamedTempFile::new_in(parent).map_err(|error| {
        PluginError::internal(format!("cannot create native sibling candidate: {error}"))
    })
}

fn source_path_still_names_file(source: &Path, source_file: &File) -> Result<bool> {
    let current = match open_source_without_reparse(source) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(PluginError::corrupt(format!(
                "cannot recheck source path identity: {error}"
            )));
        }
    };
    Ok(same_file(source_file, &current))
}

fn verify_published_candidate(
    source: &Path,
    source_file: &File,
    target: &Path,
    expected_hash: [u8; 32],
) -> Result<()> {
    if !source_path_still_names_file(source, source_file)? {
        return Err(PluginError::corrupt(
            "source path changed during native sibling publication",
        ));
    }
    let mut published = ensure_target_safe(source_file, target)?.ok_or_else(|| {
        PluginError::internal("published native sibling disappeared during verification")
    })?;
    if hash_file(&mut published)? != expected_hash
        || !security_metadata_matches(source, source_file, target, &published)?
    {
        return Err(PluginError::internal(
            "published native sibling failed non-mutating identity or metadata verification",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_source_without_reparse(source: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(source)
}

#[cfg(windows)]
fn open_source_without_reparse(source: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};

    fs::OpenOptions::new()
        .read(true)
        // Keep pathname-based ADS/DACL inspection bound to this file object:
        // other readers remain compatible, while write/delete sharing (and
        // therefore rename or replacement) is denied for the handle lifetime.
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(source)
}

#[cfg(not(any(unix, windows)))]
fn open_source_without_reparse(source: &Path) -> io::Result<File> {
    File::open(source)
}

fn classify_container(file: &mut File) -> Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut signature = [0_u8; 8];
    let count = file.read(&mut signature)?;
    file.seek(SeekFrom::Start(0))?;
    if count >= CFB_SIGNATURE.len() && signature == CFB_SIGNATURE {
        return Err(PluginError::unsupported_feature(
            "legacy CFB Cell/Show generations are not identified by this build",
        ));
    }
    if count < 4 || !ZIP_SIGNATURES.iter().any(|magic| signature[..4] == *magic) {
        return Err(PluginError::unsupported_feature(
            "unrecognized Cell/Show container; only the evidence-backed Cell 12.0300 and Show 12.0000 OOXML carrier subsets are supported",
        ));
    }
    Ok(())
}

fn validate_package(file: &mut File, expected: CarrierFamily) -> Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(file)?;
    let names = validate_archive(&mut archive)?;
    let has_types = names.contains(CONTENT_TYPES);
    let has_rels = names.contains(ROOT_RELS);
    if !has_types && !has_rels {
        return Err(PluginError::unsupported_feature(
            "ZIP container is not an OOXML package",
        ));
    }
    if !has_types || !has_rels {
        return Err(PluginError::corrupt(
            "OOXML package is missing a required root metadata part",
        ));
    }

    validate_opc_relationships(&mut archive, &names, expected)?;

    let relationships = read_xml_part(&mut archive, ROOT_RELS)?;
    let (main_target, app_target) = parse_root_relationships(&relationships)?;
    let observed = match main_target.as_str() {
        "xl/workbook.xml" => CarrierFamily::Cell,
        "ppt/presentation.xml" => CarrierFamily::Show,
        _ => {
            return Err(PluginError::unsupported_feature(format!(
                "OOXML officeDocument target {main_target:?} is not a verified Cell/Show carrier"
            )));
        }
    };
    if observed != expected {
        return Err(PluginError::unsupported_feature(format!(
            "{} input contains the opposite OOXML document family",
            expected.source_extension()
        )));
    }
    if app_target != APP_PROPERTIES {
        return Err(PluginError::unsupported_feature(format!(
            "unverified producer metadata location {app_target:?}"
        )));
    }

    let content_types = read_xml_part(&mut archive, CONTENT_TYPES)?;
    validate_supported_content_types(&content_types, &names, expected)?;
    require_main_content_type(&content_types, expected)?;
    let application = read_xml_part(&mut archive, APP_PROPERTIES)?;
    require_hancom_v12_application(&application, expected)?;
    let main = read_xml_part(&mut archive, expected.main_part())?;
    require_main_fingerprint(&main, expected)?;
    validate_main_collection_relationships(&mut archive, &names, &main, expected)?;
    Ok(())
}

fn sanitized_validation_copy(source: &mut File, family: CarrierFamily) -> Result<File> {
    source.seek(SeekFrom::Start(0))?;
    let mut copy = tempfile::tempfile().map_err(|error| {
        PluginError::internal(format!(
            "cannot create ZIP validation scratch file: {error}"
        ))
    })?;
    io::copy(source, &mut copy)?;
    copy.flush()?;
    sanitize_invalid_extended_timestamps(&mut copy, family)?;
    copy.seek(SeekFrom::Start(0))?;
    Ok(copy)
}

fn sanitize_invalid_extended_timestamps(file: &mut File, family: CarrierFamily) -> Result<()> {
    const EOCD_MIN: usize = 22;
    const EOCD_SEARCH: u64 = EOCD_MIN as u64 + u16::MAX as u64;
    const CENTRAL_HEADER: usize = 46;
    const LOCAL_HEADER: usize = 30;

    let file_len = file.seek(SeekFrom::End(0))?;
    let tail_len = file_len.min(EOCD_SEARCH);
    file.seek(SeekFrom::End(
        -(i64::try_from(tail_len).unwrap_or(i64::MAX)),
    ))?;
    let mut tail = vec![0_u8; usize::try_from(tail_len).unwrap_or(0)];
    file.read_exact(&mut tail)?;
    let eocd_in_tail = tail
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .ok_or_else(|| PluginError::corrupt("ZIP end-of-central-directory record is missing"))?;
    if eocd_in_tail + EOCD_MIN > tail.len() {
        return Err(PluginError::corrupt(
            "ZIP end-of-central-directory record is truncated",
        ));
    }
    let eocd = &tail[eocd_in_tail..];
    let comment_len = usize::from(le_u16(eocd, 20)?);
    if EOCD_MIN + comment_len != eocd.len() {
        return Err(PluginError::corrupt(
            "ZIP has trailing bytes or a malformed end comment",
        ));
    }
    let disk = le_u16(eocd, 4)?;
    let central_disk = le_u16(eocd, 6)?;
    let disk_entries = le_u16(eocd, 8)?;
    let total_entries = le_u16(eocd, 10)?;
    let central_size = u64::from(le_u32(eocd, 12)?);
    let central_offset = u64::from(le_u32(eocd, 16)?);
    if disk != 0 || central_disk != 0 || disk_entries != total_entries {
        return Err(PluginError::unsupported_feature(
            "multi-disk Cell/Show ZIP packages are not supported",
        ));
    }
    if total_entries == u16::MAX
        || central_size == u64::from(u32::MAX)
        || central_offset == u64::from(u32::MAX)
    {
        return Err(PluginError::unsupported_feature(
            "ZIP64 Cell/Show packages are outside the verified subset",
        ));
    }
    let eocd_offset = file_len - tail_len + u64::try_from(eocd_in_tail).unwrap_or(u64::MAX);
    let central_end = central_offset
        .checked_add(central_size)
        .ok_or_else(|| PluginError::corrupt("ZIP central-directory size overflowed"))?;
    if central_end != eocd_offset {
        return Err(PluginError::corrupt(
            "ZIP central-directory bounds do not match the end record",
        ));
    }

    let mut cursor = central_offset;
    let mut data_regions = Vec::with_capacity(usize::from(total_entries));
    for _ in 0..total_entries {
        let header = read_exact_at(file, cursor, CENTRAL_HEADER)?;
        if &header[..4] != b"PK\x01\x02" {
            return Err(PluginError::corrupt(
                "ZIP central-directory entry signature is invalid",
            ));
        }
        if le_u16(&header, 34)? != 0 {
            return Err(PluginError::unsupported_feature(
                "multi-disk ZIP entry is not supported",
            ));
        }
        let name_len = u64::from(le_u16(&header, 28)?);
        let extra_len = u64::from(le_u16(&header, 30)?);
        let comment_len = u64::from(le_u16(&header, 32)?);
        let central_flags = le_u16(&header, 8)?;
        let central_method = le_u16(&header, 10)?;
        let central_crc = le_u32(&header, 16)?;
        let central_compressed = u64::from(le_u32(&header, 20)?);
        let central_expanded = le_u32(&header, 24)?;
        let local_offset = u64::from(le_u32(&header, 42)?);
        if local_offset == u64::from(u32::MAX) {
            return Err(PluginError::unsupported_feature(
                "ZIP64 entry offsets are outside the verified subset",
            ));
        }
        let central_extra = cursor
            .checked_add(CENTRAL_HEADER as u64)
            .and_then(|value| value.checked_add(name_len))
            .ok_or_else(|| PluginError::corrupt("ZIP central entry offset overflowed"))?;
        let central_name = read_exact_at(
            file,
            cursor + CENTRAL_HEADER as u64,
            usize::try_from(name_len)
                .map_err(|_| PluginError::corrupt("ZIP entry name is too large"))?,
        )?;
        neutralize_invalid_timestamp_fields(file, central_extra, extra_len, family)?;

        let local = read_exact_at(file, local_offset, LOCAL_HEADER)?;
        if &local[..4] != b"PK\x03\x04" {
            return Err(PluginError::corrupt("ZIP local entry signature is invalid"));
        }
        let local_name_len = u64::from(le_u16(&local, 26)?);
        let local_extra_len = u64::from(le_u16(&local, 28)?);
        let local_flags = le_u16(&local, 6)?;
        let local_method = le_u16(&local, 8)?;
        let local_crc = le_u32(&local, 14)?;
        let local_compressed = u64::from(le_u32(&local, 18)?);
        let local_expanded = le_u32(&local, 22)?;
        let local_name = read_exact_at(
            file,
            local_offset + LOCAL_HEADER as u64,
            usize::try_from(local_name_len)
                .map_err(|_| PluginError::corrupt("ZIP local entry name is too large"))?,
        )?;
        if central_name != local_name
            || name_len != local_name_len
            || central_flags != local_flags
            || central_method != local_method
            || central_crc != local_crc
            || central_compressed != local_compressed
            || central_expanded != local_expanded
        {
            return Err(PluginError::corrupt(
                "ZIP local and central entry metadata do not match",
            ));
        }
        if central_flags & !0x0806 != 0
            || (central_method == 0 && central_flags & 0x0006 != 0)
            || !matches!(central_method, 0 | 8)
        {
            return Err(PluginError::unsupported_feature(
                "ZIP flags or data-descriptor mode are outside the verified carrier subset",
            ));
        }
        let local_extra = local_offset
            .checked_add(LOCAL_HEADER as u64)
            .and_then(|value| value.checked_add(local_name_len))
            .ok_or_else(|| PluginError::corrupt("ZIP local entry offset overflowed"))?;
        neutralize_invalid_timestamp_fields(file, local_extra, local_extra_len, family)?;
        let data_start = local_extra
            .checked_add(local_extra_len)
            .ok_or_else(|| PluginError::corrupt("ZIP entry data offset overflowed"))?;
        let data_end = data_start
            .checked_add(central_compressed)
            .ok_or_else(|| PluginError::corrupt("ZIP entry data length overflowed"))?;
        if data_end > central_offset {
            return Err(PluginError::corrupt(
                "ZIP entry data overlaps the central directory",
            ));
        }
        data_regions.push((local_offset, data_end));

        cursor = central_extra
            .checked_add(extra_len)
            .and_then(|value| value.checked_add(comment_len))
            .ok_or_else(|| PluginError::corrupt("ZIP central entry length overflowed"))?;
        if cursor > central_end {
            return Err(PluginError::corrupt(
                "ZIP central-directory entry exceeds declared bounds",
            ));
        }
    }
    if cursor != central_end {
        return Err(PluginError::corrupt(
            "ZIP central-directory entry count does not consume its declared bounds",
        ));
    }
    data_regions.sort_unstable_by_key(|region| region.0);
    let mut expected_offset = 0_u64;
    for (start, end) in data_regions {
        if start != expected_offset || end < start {
            return Err(PluginError::corrupt(
                "ZIP local entry data regions overlap or contain unexplained bytes",
            ));
        }
        expected_offset = end;
    }
    if expected_offset != central_offset {
        return Err(PluginError::corrupt(
            "ZIP local entries do not exactly consume the pre-central region",
        ));
    }
    Ok(())
}

fn neutralize_invalid_timestamp_fields(
    file: &mut File,
    start: u64,
    len: u64,
    family: CarrierFamily,
) -> Result<()> {
    let bytes = read_exact_at(
        file,
        start,
        usize::try_from(len).map_err(|_| PluginError::corrupt("ZIP extra field is too large"))?,
    )?;
    let mut cursor = 0_usize;
    let mut kinds = HashSet::new();
    while cursor < bytes.len() {
        if bytes.len() - cursor < 4 {
            return Err(PluginError::corrupt("ZIP extra field header is truncated"));
        }
        let kind = le_u16(&bytes, cursor)?;
        if !kinds.insert(kind) {
            return Err(PluginError::corrupt(format!(
                "ZIP extra field {kind:#06x} is duplicated"
            )));
        }
        let value_len = usize::from(le_u16(&bytes, cursor + 2)?);
        let value_start = cursor + 4;
        let value_end = value_start
            .checked_add(value_len)
            .ok_or_else(|| PluginError::corrupt("ZIP extra field length overflowed"))?;
        if value_end > bytes.len() {
            return Err(PluginError::corrupt("ZIP extra field value is truncated"));
        }
        let value = &bytes[value_start..value_end];
        match kind {
            0x5455 if value.len() == 5 && value[0] == 0b0000_0010 => {}
            0x5455 if is_verified_show_timestamp_quirk(value) => {
                if family == CarrierFamily::Show {
                    file.seek(SeekFrom::Start(
                        start + u64::try_from(cursor).unwrap_or(u64::MAX),
                    ))?;
                    file.write_all(&0xCAFE_u16.to_le_bytes())?;
                }
            }
            0x5455 => {
                return Err(PluginError::corrupt(
                    "ZIP extended timestamp field has an unverified shape",
                ));
            }
            _ => {
                return Err(PluginError::unsupported_feature(format!(
                    "ZIP extra field {kind:#06x} is outside the verified carrier subset"
                )));
            }
        }
        cursor = value_end;
    }
    Ok(())
}

fn is_verified_show_timestamp_quirk(value: &[u8]) -> bool {
    value.len() == 13
        && value[0] == 0b0000_0010
        && value[1..5] == value[5..9]
        && value[1..5] == value[9..13]
}

fn read_exact_at(file: &mut File, offset: u64, len: usize) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0_u8; len];
    file.read_exact(&mut bytes)
        .map_err(|error| PluginError::corrupt(format!("ZIP metadata is truncated: {error}")))?;
    Ok(bytes)
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| PluginError::corrupt("ZIP metadata is truncated"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| PluginError::corrupt("ZIP metadata is truncated"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn validate_archive<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<HashSet<String>> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(resource_limit(format!(
            "archive entry count {} exceeds {MAX_ARCHIVE_ENTRIES}",
            archive.len()
        )));
    }
    let mut names = HashSet::new();
    let mut folded_names = HashSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.encrypted() {
            return Err(PluginError::corrupt(
                "encrypted ZIP entries are not supported",
            ));
        }
        let name = safe_entry_name(entry.name_raw(), entry.is_dir())?;
        if entry.is_dir() && (entry.size() != 0 || entry.compressed_size() != 0) {
            return Err(PluginError::corrupt(format!(
                "ZIP directory entry {name:?} contains payload bytes"
            )));
        }
        let folded = name.to_lowercase();
        let is_xml = folded.ends_with(".xml") || folded.ends_with(".rels");
        if !folded_names.insert(folded) {
            return Err(PluginError::corrupt(format!(
                "duplicate or case-colliding ZIP entry: {name:?}"
            )));
        }
        reject_unverified_active_part(&name)?;
        names.insert(name.clone());
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            let expected_kind = if entry.is_dir() { 0o040000 } else { 0o100000 };
            if kind != 0 && kind != expected_kind {
                return Err(PluginError::corrupt(format!(
                    "ZIP entry {name:?} is not a regular file or directory"
                )));
            }
        }
        if entry.compression() != CompressionMethod::Stored
            && entry.compression() != CompressionMethod::Deflated
        {
            return Err(PluginError::corrupt(format!(
                "ZIP entry {name:?} uses unsupported compression {:?}",
                entry.compression()
            )));
        }
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(resource_limit(format!(
                "entry {name:?} declares {} expanded bytes",
                entry.size()
            )));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| resource_limit("expanded byte count overflowed".to_string()))?;
        if total > MAX_TOTAL_EXPANDED_BYTES {
            return Err(resource_limit(format!(
                "archive declares {total} cumulative expanded bytes"
            )));
        }
        if entry.size() > 0
            && (entry.compressed_size() == 0
                || entry.size()
                    > entry
                        .compressed_size()
                        .saturating_mul(MAX_COMPRESSION_RATIO))
        {
            return Err(resource_limit(format!(
                "entry {name:?} expansion ratio exceeds {MAX_COMPRESSION_RATIO}:1"
            )));
        }

        if !entry.is_dir() && is_xml {
            if entry.size() > MAX_XML_BYTES {
                return Err(resource_limit(format!(
                    "XML entry {name:?} exceeds {MAX_XML_BYTES} bytes"
                )));
            }
            let mut bytes = Vec::with_capacity(
                usize::try_from(entry.size())
                    .unwrap_or(0)
                    .min(MAX_XML_BYTES as usize),
            );
            let declared_size = entry.size();
            let mut bounded = (&mut entry).take(declared_size.saturating_add(1));
            bounded.read_to_end(&mut bytes)?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != declared_size {
                return Err(PluginError::corrupt(format!(
                    "ZIP entry {name:?} expanded length disagrees with its declaration"
                )));
            }
            validate_xml(&bytes, &name)?;
        } else {
            let declared_size = entry.size();
            let actual_size = io::copy(
                &mut (&mut entry).take(declared_size.saturating_add(1)),
                &mut io::sink(),
            )?;
            if actual_size != declared_size {
                return Err(PluginError::corrupt(format!(
                    "ZIP entry {name:?} expanded length disagrees with its declaration"
                )));
            }
        }
    }
    Ok(names)
}

fn reject_unverified_active_part(name: &str) -> Result<()> {
    let folded = name.to_ascii_lowercase();
    let segments = folded.trim_end_matches('/').split('/').collect::<Vec<_>>();
    let active_segment = segments.iter().any(|segment| {
        matches!(
            *segment,
            "activex"
                | "ctrlprops"
                | "embeddings"
                | "externallinks"
                | "macrosheets"
                | "querytables"
                | "customui"
                | "_xmlsignatures"
        )
    });
    let active_file = segments.last().is_some_and(|file| {
        matches!(
            *file,
            "vbaproject.bin" | "vbaprojectsignature.bin" | "connections.xml" | "origin.sigs"
        )
    });
    if active_segment || active_file {
        return Err(PluginError::unsupported_feature(format!(
            "OOXML active or embedded part {name:?} is outside the verified carrier subset"
        )));
    }
    Ok(())
}

fn safe_entry_name(raw: &[u8], directory: bool) -> Result<String> {
    let name = std::str::from_utf8(raw)
        .map_err(|_| PluginError::corrupt("ZIP entry name is not valid UTF-8"))?;
    if name.is_empty()
        || !name.is_ascii()
        || name.contains('\\')
        || name.contains('\0')
        || name.contains(['%', '?', '#'])
        || name.starts_with('/')
        || name.contains(':')
        || name.chars().any(char::is_control)
    {
        return Err(PluginError::corrupt(format!(
            "unsafe ZIP entry path: {name:?}"
        )));
    }
    if directory != name.ends_with('/') {
        return Err(PluginError::corrupt(format!(
            "ZIP directory marker disagrees with entry path: {name:?}"
        )));
    }
    let trimmed = name.trim_end_matches('/');
    let path = Path::new(trimmed);
    if path.components().any(|component| {
        !matches!(component, Component::Normal(_))
            || component.as_os_str() == OsStr::new(".")
            || component.as_os_str() == OsStr::new("..")
    }) {
        return Err(PluginError::corrupt(format!(
            "unsafe ZIP entry path: {name:?}"
        )));
    }
    Ok(name.to_string())
}

fn validate_xml(bytes: &[u8], part: &str) -> Result<()> {
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut depth = 0_usize;
    let mut roots = 0_usize;
    let mut declaration_seen = false;
    let mut content_seen = false;
    let mut events = 0_usize;
    let mut buffer = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buffer)?;
        if !matches!(&event, Event::Eof) {
            events = events
                .checked_add(1)
                .ok_or_else(|| resource_limit("XML event count overflowed".to_string()))?;
            if events > MAX_XML_EVENTS {
                return Err(resource_limit(format!(
                    "XML part {part:?} exceeds {MAX_XML_EVENTS} events"
                )));
            }
        }
        match event {
            Event::Decl(declaration) => {
                if declaration_seen || content_seen {
                    return Err(PluginError::corrupt(format!(
                        "XML part {part:?} has an invalid or misplaced declaration"
                    )));
                }
                validate_xml_declaration(&declaration, part)?;
                declaration_seen = true;
            }
            Event::Start(event) => {
                content_seen = true;
                require_resolved_namespaces(&reader, &event, part)?;
                if depth == 0 {
                    roots += 1;
                }
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| PluginError::corrupt("XML depth overflowed"))?;
                if depth > MAX_XML_DEPTH {
                    return Err(PluginError::corrupt(format!(
                        "XML part {part:?} exceeds nesting depth {MAX_XML_DEPTH}"
                    )));
                }
            }
            Event::Empty(event) => {
                content_seen = true;
                require_resolved_namespaces(&reader, &event, part)?;
                if depth == 0 {
                    roots += 1;
                }
            }
            Event::End(_) => {
                content_seen = true;
                depth = depth.checked_sub(1).ok_or_else(|| {
                    PluginError::corrupt(format!(
                        "XML part {part:?} has an unmatched closing element"
                    ))
                })?;
            }
            Event::Text(text) => {
                let decoded = text.decode()?;
                validate_xml_characters(&decoded, part)?;
                if depth == 0 && !decoded.trim().is_empty() {
                    return Err(PluginError::corrupt(format!(
                        "XML part {part:?} contains text outside its root"
                    )));
                }
                content_seen = true;
            }
            Event::CData(text) => {
                let decoded = text.decode()?;
                validate_xml_characters(&decoded, part)?;
                if depth == 0 && !decoded.trim().is_empty() {
                    return Err(PluginError::corrupt(format!(
                        "XML part {part:?} contains CDATA outside its root"
                    )));
                }
                content_seen = true;
            }
            Event::Comment(comment) => {
                let decoded = comment.decode()?;
                validate_xml_characters(&decoded, part)?;
                if decoded.contains("--") || decoded.ends_with('-') {
                    return Err(PluginError::corrupt(format!(
                        "XML part {part:?} contains an invalid comment"
                    )));
                }
                content_seen = true;
            }
            Event::GeneralRef(reference) => {
                let name = reference.decode()?;
                if depth == 0 || !is_supported_xml_reference(&name) {
                    return Err(PluginError::corrupt(format!(
                        "XML part {part:?} contains unsupported entity reference"
                    )));
                }
            }
            Event::DocType(_) => {
                return Err(PluginError::corrupt(format!(
                    "XML part {part:?} must not contain a DTD"
                )));
            }
            Event::PI(_) => {
                return Err(PluginError::corrupt(format!(
                    "XML part {part:?} must not contain processing instructions"
                )));
            }
            Event::Eof => break,
        }
        buffer.clear();
    }
    if depth != 0 || roots != 1 {
        return Err(PluginError::corrupt(format!(
            "XML part {part:?} must contain exactly one complete root"
        )));
    }
    Ok(())
}

fn validate_xml_declaration(declaration: &BytesDecl<'_>, part: &str) -> Result<()> {
    let raw = std::str::from_utf8(declaration.as_ref()).map_err(|_| {
        PluginError::corrupt(format!("XML part {part:?} has a non-ASCII declaration"))
    })?;
    if !raw.starts_with("xml") {
        return Err(PluginError::corrupt(format!(
            "XML part {part:?} has an invalid declaration"
        )));
    }
    let start = BytesStart::from_content(raw, 3);
    let mut position = 0_usize;
    let mut encoding_seen = false;
    let mut standalone_seen = false;
    for item in start.attributes().with_checks(true) {
        let item = item?;
        let valid = match item.key.as_ref() {
            b"version" if position == 0 => item.value.as_ref() == b"1.0",
            b"encoding" if position == 1 && !encoding_seen && !standalone_seen => {
                encoding_seen = true;
                item.value.as_ref().eq_ignore_ascii_case(b"UTF-8")
            }
            b"standalone"
                if (position == 1 || (position == 2 && encoding_seen)) && !standalone_seen =>
            {
                standalone_seen = true;
                matches!(item.value.as_ref(), b"yes" | b"no")
            }
            _ => false,
        };
        if !valid {
            return Err(PluginError::corrupt(format!(
                "XML part {part:?} has an unsupported declaration"
            )));
        }
        position += 1;
    }
    if position == 0 {
        return Err(PluginError::corrupt(format!(
            "XML part {part:?} declaration omits version"
        )));
    }
    Ok(())
}

fn require_resolved_namespaces(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    part: &str,
) -> Result<()> {
    std::str::from_utf8(event.name().as_ref()).map_err(|_| {
        PluginError::corrupt(format!("XML part {part:?} has a non-UTF-8 element name"))
    })?;
    if event.name().as_ref().len() > MAX_XML_NAME_BYTES {
        return Err(resource_limit(format!(
            "XML part {part:?} contains an oversized element name"
        )));
    }
    if matches!(
        reader.resolver().resolve_element(event.name()).0,
        ResolveResult::Unknown(_)
    ) {
        return Err(PluginError::corrupt(format!(
            "XML part {part:?} uses an undeclared element namespace prefix"
        )));
    }
    let mut attributes = 0_usize;
    let mut namespace_declarations = 0_usize;
    let mut expanded_names = HashSet::new();
    for item in event.attributes().with_checks(true) {
        let item = item?;
        attributes += 1;
        if attributes > MAX_XML_ATTRIBUTES_PER_ELEMENT {
            return Err(resource_limit(format!(
                "XML part {part:?} exceeds {MAX_XML_ATTRIBUTES_PER_ELEMENT} attributes on one element"
            )));
        }
        let raw_name = item.key.as_ref();
        std::str::from_utf8(raw_name).map_err(|_| {
            PluginError::corrupt(format!("XML part {part:?} has a non-UTF-8 attribute name"))
        })?;
        if raw_name.len() > MAX_XML_NAME_BYTES || item.value.len() > MAX_XML_ATTRIBUTE_VALUE_BYTES {
            return Err(resource_limit(format!(
                "XML part {part:?} contains an oversized attribute name or value"
            )));
        }
        if raw_name == b"xmlns" || raw_name.starts_with(b"xmlns:") {
            namespace_declarations += 1;
            if namespace_declarations > MAX_XML_NAMESPACES_PER_ELEMENT {
                return Err(resource_limit(format!(
                    "XML part {part:?} exceeds {MAX_XML_NAMESPACES_PER_ELEMENT} namespace declarations on one element"
                )));
            }
        }
        let namespace = match reader.resolver().resolve_attribute(item.key).0 {
            ResolveResult::Bound(namespace) => Some(namespace.as_ref().to_vec()),
            ResolveResult::Unbound => None,
            ResolveResult::Unknown(_) => {
                return Err(PluginError::corrupt(format!(
                    "XML part {part:?} uses an undeclared attribute namespace prefix"
                )));
            }
        };
        let expanded_name = (namespace, item.key.local_name().as_ref().to_vec());
        if !expanded_names.insert(expanded_name) {
            return Err(PluginError::corrupt(format!(
                "XML part {part:?} contains duplicate expanded attribute names"
            )));
        }
        let value = item.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())?;
        validate_xml_characters(&value, part)?;
    }
    Ok(())
}

fn validate_xml_characters(value: &str, part: &str) -> Result<()> {
    if value.chars().all(|character| {
        let value = character as u32;
        matches!(value, 0x9 | 0xA | 0xD)
            || (0x20..=0xD7FF).contains(&value)
            || (0xE000..=0xFFFD).contains(&value)
            || (0x10000..=0x10FFFF).contains(&value)
    }) {
        return Ok(());
    }
    Err(PluginError::corrupt(format!(
        "XML part {part:?} contains a character forbidden by XML 1.0"
    )))
}

fn is_supported_xml_reference(name: &str) -> bool {
    if matches!(name, "amp" | "lt" | "gt" | "apos" | "quot") {
        return true;
    }
    let Some(numeric) = name.strip_prefix('#') else {
        return false;
    };
    let scalar = if let Some(hex) = numeric.strip_prefix('x') {
        u32::from_str_radix(hex, 16).ok()
    } else {
        numeric.parse::<u32>().ok()
    };
    scalar.is_some_and(|value| {
        matches!(value, 0x9 | 0xA | 0xD)
            || (0x20..=0xD7FF).contains(&value)
            || (0xE000..=0xFFFD).contains(&value)
            || (0x10000..=0x10FFFF).contains(&value)
    })
}

fn read_xml_part<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let mut entry = archive.by_name(name).map_err(|error| {
        PluginError::corrupt(format!("cannot read required OOXML part {name:?}: {error}"))
    })?;
    if entry.size() > MAX_XML_BYTES {
        return Err(resource_limit(format!(
            "required XML part {name:?} exceeds {MAX_XML_BYTES} bytes"
        )));
    }
    let declared_size = entry.size();
    let mut bytes = Vec::with_capacity(usize::try_from(declared_size).unwrap_or(0));
    let mut bounded = (&mut entry).take(declared_size.saturating_add(1));
    bounded.read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != declared_size {
        return Err(PluginError::corrupt(format!(
            "required XML part {name:?} length disagrees with its declaration"
        )));
    }
    Ok(bytes)
}

fn validate_opc_relationships<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    names: &HashSet<String>,
    family: CarrierFamily,
) -> Result<()> {
    let mut relationship_parts = names
        .iter()
        .filter(|name| name.ends_with(".rels"))
        .cloned()
        .collect::<Vec<_>>();
    relationship_parts.sort_unstable();
    let mut relationship_parts_by_source = HashMap::new();
    let mut targets_by_source = HashMap::new();
    for relationship_part in relationship_parts {
        let source_part = relationship_source_part(&relationship_part)?;
        if source_part
            .as_ref()
            .is_some_and(|source| !names.contains(source))
        {
            return Err(PluginError::corrupt(format!(
                "relationship part {relationship_part:?} has no source part"
            )));
        }
        let bytes = read_xml_part(archive, &relationship_part)?;
        let relationships = validate_relationship_part(
            &bytes,
            &relationship_part,
            source_part.as_deref(),
            names,
            family,
        )?;
        relationship_parts_by_source.insert(source_part.clone(), relationship_part);
        targets_by_source.insert(source_part, relationships);
    }

    let mut reachable = HashSet::from([CONTENT_TYPES.to_string()]);
    let mut pending = VecDeque::from([None]);
    while let Some(source) = pending.pop_front() {
        if let Some(relationship_part) = relationship_parts_by_source.get(&source) {
            reachable.insert(relationship_part.clone());
        }
        if let Some(relationships) = targets_by_source.get(&source) {
            for relationship in relationships {
                if reachable.insert(relationship.target.clone()) {
                    pending.push_back(Some(relationship.target.clone()));
                }
            }
        }
    }
    let mut unreachable = names
        .iter()
        .filter(|name| !name.ends_with('/') && !reachable.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if !unreachable.is_empty() {
        unreachable.sort_unstable();
        return Err(PluginError::unsupported_feature(format!(
            "OOXML package contains parts outside the root relationship closure: {unreachable:?}"
        )));
    }
    if family == CarrierFamily::Show {
        validate_show_gif_parts(archive, names, &targets_by_source)?;
    }
    Ok(())
}

fn validate_show_gif_parts<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    names: &HashSet<String>,
    relationships_by_source: &HashMap<Option<String>, Vec<InternalRelationship>>,
) -> Result<()> {
    let mut gif_parts = names
        .iter()
        .filter(|name| part_has_extension(name, "gif"))
        .cloned()
        .collect::<Vec<_>>();
    gif_parts.sort_unstable();
    if gif_parts.is_empty() {
        return Ok(());
    }
    if let Some(name) = gif_parts.iter().find(|name| !is_show_gif_media_part(name)) {
        return Err(PluginError::unsupported_feature(format!(
            "Show GIF part {name:?} is outside the verified ppt/media role"
        )));
    }

    let gif_part_set = gif_parts.iter().cloned().collect::<HashSet<_>>();
    let mut references_by_slide: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (source, relationships) in relationships_by_source {
        for relationship in relationships {
            if !gif_part_set.contains(&relationship.target) {
                continue;
            }
            let Some(source) = source.as_deref() else {
                return Err(PluginError::unsupported_feature(
                    "Show GIF parts must not be package-level relationship targets",
                ));
            };
            if relationship.relationship_type != IMAGE_REL || !is_show_slide_part(source) {
                return Err(PluginError::unsupported_feature(format!(
                    "Show GIF part {:?} must be an internal image target of a slide",
                    relationship.target
                )));
            }
            references_by_slide
                .entry(source.to_string())
                .or_default()
                .push((relationship.id.clone(), relationship.target.clone()));
        }
    }

    let mut referenced_parts = HashSet::new();
    let mut slides = references_by_slide.keys().cloned().collect::<Vec<_>>();
    slides.sort_unstable();
    for slide in slides {
        let bytes = read_xml_part(archive, &slide)?;
        let embedded_ids = collect_embedded_relationship_ids(&bytes)?;
        let references = references_by_slide.remove(&slide).unwrap_or_default();
        for (id, target) in references {
            if !embedded_ids.contains(&id) {
                return Err(PluginError::unsupported_feature(format!(
                    "Show slide {slide:?} does not embed GIF relationship {id:?}"
                )));
            }
            referenced_parts.insert(target);
        }
    }

    for gif_part in gif_parts {
        if !referenced_parts.contains(&gif_part) {
            return Err(PluginError::unsupported_feature(format!(
                "Show GIF part {gif_part:?} is not embedded by a slide image relationship"
            )));
        }
        let relationship_part = relationship_part_for_source(&gif_part)?;
        if names.contains(&relationship_part) {
            return Err(PluginError::unsupported_feature(format!(
                "Show GIF part {gif_part:?} has an unsupported outgoing relationship part"
            )));
        }
        let mut entry = archive.by_name(&gif_part).map_err(|error| {
            PluginError::corrupt(format!("cannot read Show GIF part {gif_part:?}: {error}"))
        })?;
        let mut header = [0_u8; 6];
        if entry.size() < u64::try_from(header.len()).unwrap_or(u64::MAX) {
            return Err(PluginError::unsupported_feature(format!(
                "Show GIF part {gif_part:?} is shorter than a GIF signature"
            )));
        }
        entry.read_exact(&mut header).map_err(|error| {
            PluginError::corrupt(format!(
                "cannot read the signature of Show GIF part {gif_part:?}: {error}"
            ))
        })?;
        if header != *b"GIF87a" && header != *b"GIF89a" {
            return Err(PluginError::unsupported_feature(format!(
                "Show GIF part {gif_part:?} lacks a verified GIF87a/GIF89a signature"
            )));
        }
    }
    Ok(())
}

fn part_has_extension(name: &str, extension: &str) -> bool {
    name.rsplit('/')
        .next()
        .and_then(|leaf| leaf.rsplit_once('.').map(|(_, value)| value))
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn is_show_gif_media_part(name: &str) -> bool {
    name.strip_prefix("ppt/media/")
        .and_then(|leaf| leaf.strip_suffix(".gif"))
        .is_some_and(|stem| !stem.is_empty() && !stem.contains('/'))
}

fn is_show_slide_part(name: &str) -> bool {
    name.strip_prefix("ppt/slides/slide")
        .and_then(|rest| rest.strip_suffix(".xml"))
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|value| value.is_ascii_digit())
        })
}

fn relationship_part_for_source(source: &str) -> Result<String> {
    let Some((parent, leaf)) = source.rsplit_once('/') else {
        return Err(PluginError::corrupt(format!(
            "OOXML part {source:?} has no relationship-part parent"
        )));
    };
    Ok(format!("{parent}/_rels/{leaf}.rels"))
}

fn collect_embedded_relationship_ids(bytes: &[u8]) -> Result<HashSet<String>> {
    let mut reader = NsReader::from_reader(bytes);
    let mut ids = HashSet::new();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == b"blip"
                    && element_namespace_matches(&reader, &event, DRAWING_NS)? =>
            {
                if let Some(id) =
                    namespaced_attribute_value(&reader, &event, b"embed", DOCUMENT_REL_NS)?
                {
                    ids.insert(id);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(ids)
}

fn relationship_source_part(relationship_part: &str) -> Result<Option<String>> {
    if relationship_part == ROOT_RELS {
        return Ok(None);
    }
    let Some((parent, file)) = relationship_part.rsplit_once("/_rels/") else {
        return Err(PluginError::corrupt(format!(
            "invalid OPC relationship part path: {relationship_part:?}"
        )));
    };
    let Some(file) = file.strip_suffix(".rels") else {
        return Err(PluginError::corrupt(format!(
            "invalid OPC relationship part suffix: {relationship_part:?}"
        )));
    };
    if parent.is_empty() || file.is_empty() || file.contains('/') {
        return Err(PluginError::corrupt(format!(
            "invalid OPC relationship source mapping: {relationship_part:?}"
        )));
    }
    Ok(Some(format!("{parent}/{file}")))
}

fn validate_relationship_part(
    bytes: &[u8],
    relationship_part: &str,
    source_part: Option<&str>,
    names: &HashSet<String>,
    family: CarrierFamily,
) -> Result<Vec<InternalRelationship>> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut ids = HashSet::new();
    let mut package_relationship_types = HashSet::new();
    let mut internal_relationships = Vec::new();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) if depth == 0 => {
                require_element(
                    &reader,
                    &event,
                    b"Relationships",
                    PACKAGE_RELS_NS,
                    relationship_part,
                )?;
                require_only_attributes(&event, &[], relationship_part)?;
                root_seen = true;
                depth = 1;
            }
            Event::Start(_) => {
                return Err(PluginError::corrupt(format!(
                    "relationship entries in {relationship_part:?} must be empty elements"
                )));
            }
            Event::Empty(event) if depth == 0 => {
                require_element(
                    &reader,
                    &event,
                    b"Relationships",
                    PACKAGE_RELS_NS,
                    relationship_part,
                )?;
                require_only_attributes(&event, &[], relationship_part)?;
                root_seen = true;
            }
            Event::Empty(event) if depth == 1 => {
                require_element(
                    &reader,
                    &event,
                    b"Relationship",
                    PACKAGE_RELS_NS,
                    relationship_part,
                )?;
                require_only_attributes(
                    &event,
                    &[b"Id", b"Type", b"Target", b"TargetMode"],
                    relationship_part,
                )?;
                validate_relationship_entry(
                    &reader,
                    &event,
                    relationship_part,
                    source_part,
                    names,
                    family,
                    &mut ids,
                    &mut package_relationship_types,
                    &mut internal_relationships,
                )?;
            }
            Event::Empty(_) => {
                return Err(PluginError::corrupt(format!(
                    "relationship part {relationship_part:?} contains nested elements"
                )));
            }
            Event::Text(text) if depth == 1 && !text.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "relationship part {relationship_part:?} contains unexpected text"
                )));
            }
            Event::CData(text) if depth == 1 && !text.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(format!(
                    "relationship part {relationship_part:?} contains unexpected CDATA"
                )));
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    PluginError::corrupt(format!(
                        "relationship part {relationship_part:?} has an unmatched close"
                    ))
                })?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(PluginError::corrupt(format!(
            "relationship part {relationship_part:?} has no complete root"
        )));
    }
    Ok(internal_relationships)
}

#[allow(clippy::too_many_arguments)]
fn validate_relationship_entry(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    relationship_part: &str,
    source_part: Option<&str>,
    names: &HashSet<String>,
    family: CarrierFamily,
    ids: &mut HashSet<String>,
    package_relationship_types: &mut HashSet<String>,
    internal_relationships: &mut Vec<InternalRelationship>,
) -> Result<()> {
    let id = attribute(reader, event, b"Id")?
        .ok_or_else(|| PluginError::corrupt("relationship omits Id"))?;
    if id.is_empty() || id.len() > 256 || !ids.insert(id.clone()) {
        return Err(PluginError::corrupt(format!(
            "relationship part {relationship_part:?} has an invalid or duplicate Id {id:?}"
        )));
    }
    let relationship_type = attribute(reader, event, b"Type")?
        .ok_or_else(|| PluginError::corrupt("relationship omits Type"))?;
    if !is_supported_relationship_type(family, &relationship_type) {
        return Err(PluginError::unsupported_feature(format!(
            "relationship type {relationship_type:?} is outside the verified carrier subset"
        )));
    }
    let package_level_type = matches!(
        relationship_type.as_str(),
        OFFICE_REL | APP_REL | CORE_PROPERTIES_REL | THUMBNAIL_REL
    );
    if (source_part.is_none() && !package_level_type)
        || (source_part.is_some() && package_level_type)
    {
        return Err(PluginError::corrupt(format!(
            "relationship type {relationship_type:?} is invalid for source part {source_part:?}"
        )));
    }
    if source_part.is_none()
        && package_level_type
        && !package_relationship_types.insert(relationship_type.clone())
    {
        return Err(PluginError::corrupt(format!(
            "package relationship type {relationship_type:?} is declared more than once"
        )));
    }
    let target = attribute(reader, event, b"Target")?
        .ok_or_else(|| PluginError::corrupt("relationship omits Target"))?;
    match attribute(reader, event, b"TargetMode")?.as_deref() {
        None | Some("Internal") => {
            let resolved = resolve_internal_relationship_target(source_part, &target)?;
            if !names.contains(&resolved) {
                return Err(PluginError::corrupt(format!(
                    "relationship target {target:?} from {relationship_part:?} resolves to missing part {resolved:?}"
                )));
            }
            internal_relationships.push(InternalRelationship {
                id,
                relationship_type,
                target: resolved,
            });
        }
        Some("External")
            if family == CarrierFamily::Show
                && relationship_type == HYPERLINK_REL
                && is_safe_https_hyperlink(&target) => {}
        Some("External") => {
            return Err(PluginError::unsupported_feature(format!(
                "external relationship {relationship_type:?} to {target:?} is outside the verified carrier subset"
            )));
        }
        Some(mode) => {
            return Err(PluginError::corrupt(format!(
                "relationship has invalid TargetMode {mode:?}"
            )));
        }
    }
    Ok(())
}

fn is_supported_relationship_type(family: CarrierFamily, relationship_type: &str) -> bool {
    if matches!(
        relationship_type,
        OFFICE_REL | APP_REL | CORE_PROPERTIES_REL | THUMBNAIL_REL
    ) {
        return true;
    }
    match family {
        CarrierFamily::Cell => matches!(
            relationship_type,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing"
                | IMAGE_REL
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"
        ),
        CarrierFamily::Show => matches!(
            relationship_type,
            HYPERLINK_REL
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/audio"
                | IMAGE_REL
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme"
                | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps"
        ),
    }
}

fn resolve_internal_relationship_target(source_part: Option<&str>, target: &str) -> Result<String> {
    if target.is_empty()
        || target.len() > 4096
        || target.starts_with('/')
        || target.contains(['\\', '#', '?', '%'])
        || target.chars().any(char::is_control)
    {
        return Err(PluginError::corrupt(format!(
            "unsafe internal OOXML relationship target: {target:?}"
        )));
    }
    let mut segments = source_part
        .and_then(|source| source.rsplit_once('/').map(|(parent, _)| parent))
        .map(|parent| parent.split('/').map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    for segment in target.split('/') {
        match segment {
            "" | "." => {
                return Err(PluginError::corrupt(format!(
                    "unsafe internal OOXML relationship target: {target:?}"
                )));
            }
            ".." => {
                if segments.pop().is_none() {
                    return Err(PluginError::corrupt(format!(
                        "OOXML relationship target escapes the package root: {target:?}"
                    )));
                }
            }
            value if value.contains(':') => {
                return Err(PluginError::corrupt(format!(
                    "unsafe internal OOXML relationship target: {target:?}"
                )));
            }
            value => segments.push(value.to_string()),
        }
    }
    if segments.is_empty() {
        return Err(PluginError::corrupt(
            "relationship target resolves to package root",
        ));
    }
    Ok(segments.join("/"))
}

fn is_safe_https_hyperlink(target: &str) -> bool {
    let Some(rest) = target.strip_prefix("https://") else {
        return false;
    };
    !rest.is_empty()
        && target.len() <= 4096
        && !target.contains('\\')
        && !target
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
}

fn validate_supported_content_types(
    bytes: &[u8],
    names: &HashSet<String>,
    family: CarrierFamily,
) -> Result<()> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut defaults = HashMap::new();
    let mut overrides = HashMap::new();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) if depth == 0 => {
                require_element(&reader, &event, b"Types", CONTENT_TYPES_NS, CONTENT_TYPES)?;
                require_only_attributes(&event, &[], CONTENT_TYPES)?;
                root_seen = true;
                depth = 1;
            }
            Event::Start(_) => {
                return Err(PluginError::corrupt(
                    "OOXML content type declarations must be empty direct children",
                ));
            }
            Event::Empty(event) if depth == 0 => {
                require_element(&reader, &event, b"Types", CONTENT_TYPES_NS, CONTENT_TYPES)?;
                require_only_attributes(&event, &[], CONTENT_TYPES)?;
                root_seen = true;
            }
            Event::Empty(event) if depth == 1 => match event.local_name().as_ref() {
                b"Default" => {
                    require_element(&reader, &event, b"Default", CONTENT_TYPES_NS, CONTENT_TYPES)?;
                    require_only_attributes(
                        &event,
                        &[b"Extension", b"ContentType"],
                        CONTENT_TYPES,
                    )?;
                    let extension = attribute(&reader, &event, b"Extension")?
                        .ok_or_else(|| PluginError::corrupt("OOXML Default omits Extension"))?;
                    let content_type = attribute(&reader, &event, b"ContentType")?
                        .ok_or_else(|| PluginError::corrupt("OOXML Default omits ContentType"))?;
                    if extension.is_empty()
                        || extension.len() > 255
                        || extension.starts_with('.')
                        || !extension.chars().all(|value| value.is_ascii_alphanumeric())
                    {
                        return Err(PluginError::corrupt(format!(
                            "OOXML Default has an unsafe extension {extension:?}"
                        )));
                    }
                    let extension = extension.to_ascii_lowercase();
                    if !is_supported_default_content_type(family, &extension, &content_type) {
                        return Err(PluginError::unsupported_feature(format!(
                            "OOXML default mapping {extension:?} -> {content_type:?} is outside the verified carrier subset"
                        )));
                    }
                    if defaults.insert(extension.clone(), content_type).is_some() {
                        return Err(PluginError::corrupt(format!(
                            "OOXML content types repeat default extension {extension:?}"
                        )));
                    }
                }
                b"Override" => {
                    require_element(
                        &reader,
                        &event,
                        b"Override",
                        CONTENT_TYPES_NS,
                        CONTENT_TYPES,
                    )?;
                    require_only_attributes(&event, &[b"PartName", b"ContentType"], CONTENT_TYPES)?;
                    let part_name = attribute(&reader, &event, b"PartName")?
                        .ok_or_else(|| PluginError::corrupt("OOXML Override omits PartName"))?;
                    let content_type = attribute(&reader, &event, b"ContentType")?
                        .ok_or_else(|| PluginError::corrupt("OOXML Override omits ContentType"))?;
                    let part = part_name.strip_prefix('/').ok_or_else(|| {
                        PluginError::corrupt(format!(
                            "OOXML Override has an unsafe part name {part_name:?}"
                        ))
                    })?;
                    let normalized = resolve_internal_relationship_target(None, part)?;
                    if normalized != part || part == CONTENT_TYPES || !names.contains(part) {
                        return Err(PluginError::corrupt(format!(
                            "OOXML Override names a missing or invalid part {part_name:?}"
                        )));
                    }
                    if part_has_extension(part, "gif") {
                        return Err(PluginError::unsupported_feature(format!(
                            "OOXML GIF part {part_name:?} must use the verified Show default mapping"
                        )));
                    }
                    if !is_supported_content_type(family, &content_type) {
                        return Err(PluginError::unsupported_feature(format!(
                            "OOXML content type {content_type:?} is outside the verified carrier subset"
                        )));
                    }
                    let folded = part.to_lowercase();
                    if overrides.insert(folded, content_type).is_some() {
                        return Err(PluginError::corrupt(format!(
                            "OOXML content types repeat override part {part_name:?}"
                        )));
                    }
                }
                _ => {
                    return Err(PluginError::corrupt(
                        "OOXML content types contain an unexpected direct child",
                    ));
                }
            },
            Event::Empty(_) => {
                return Err(PluginError::corrupt(
                    "OOXML content types contain a nested declaration",
                ));
            }
            Event::Text(text) if depth == 1 && !text.decode()?.trim().is_empty() => {
                return Err(PluginError::corrupt(
                    "OOXML content types contain unexpected text",
                ));
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    PluginError::corrupt("OOXML content types have an unmatched close")
                })?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(PluginError::corrupt(
            "OOXML content types have no complete root",
        ));
    }
    for name in names {
        if name == CONTENT_TYPES
            || name.ends_with('/')
            || overrides.contains_key(&name.to_lowercase())
        {
            continue;
        }
        let extension = name
            .rsplit('/')
            .next()
            .and_then(|leaf| leaf.rsplit_once('.').map(|(_, extension)| extension))
            .map(str::to_ascii_lowercase)
            .filter(|extension| !extension.is_empty())
            .ok_or_else(|| {
                PluginError::corrupt(format!(
                    "OOXML part {name:?} has neither an override nor an extension"
                ))
            })?;
        if !defaults.contains_key(&extension) {
            return Err(PluginError::corrupt(format!(
                "OOXML part {name:?} has no content type declaration"
            )));
        }
    }
    Ok(())
}

fn is_supported_default_content_type(
    family: CarrierFamily,
    extension: &str,
    content_type: &str,
) -> bool {
    matches!(
        (extension, content_type),
        (
            "rels",
            "application/vnd.openxmlformats-package.relationships+xml"
        ) | ("xml", "application/xml")
            | ("jpeg", "image/jpeg")
            | ("png", "image/png")
    ) || (family == CarrierFamily::Show
        && matches!(
            (extension, content_type),
            ("gif", "image/gif") | ("wav", "audio/wav")
        ))
}

fn is_supported_content_type(family: CarrierFamily, content_type: &str) -> bool {
    if matches!(
        content_type,
        "application/xml"
            | "application/vnd.openxmlformats-package.relationships+xml"
            | "application/vnd.openxmlformats-package.core-properties+xml"
            | "application/vnd.openxmlformats-officedocument.extended-properties+xml"
            | "application/vnd.openxmlformats-officedocument.theme+xml"
            | "image/jpeg"
            | "image/png"
    ) {
        return true;
    }
    match family {
        CarrierFamily::Cell => matches!(
            content_type,
            CELL_MAIN_TYPE
                | CELL_MACRO_MAIN_TYPE
                | "application/vnd.openxmlformats-officedocument.drawing+xml"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"
        ),
        CarrierFamily::Show => matches!(
            content_type,
            SHOW_MAIN_TYPE
                | "audio/wav"
                | "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"
                | "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"
                | "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml"
                | "application/vnd.openxmlformats-officedocument.presentationml.slide+xml"
                | "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"
                | "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"
                | "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml"
                | "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml"
        ),
    }
}

fn validate_main_collection_relationships<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    names: &HashSet<String>,
    main: &[u8],
    family: CarrierFamily,
) -> Result<()> {
    let referenced = collect_main_collection_ids(main, family)?;
    let relationship_part = match family {
        CarrierFamily::Cell => "xl/_rels/workbook.xml.rels",
        CarrierFamily::Show => "ppt/_rels/presentation.xml.rels",
    };
    if !names.contains(relationship_part) {
        if referenced.is_empty() {
            return Ok(());
        }
        return Err(PluginError::corrupt(format!(
            "{} references sheet or slide relationships but {relationship_part:?} is missing",
            family.main_part()
        )));
    }
    let relationships = read_xml_part(archive, relationship_part)?;
    let declared = collect_collection_relationship_ids(&relationships, relationship_part, family)?;
    if referenced != declared {
        return Err(PluginError::corrupt(format!(
            "{} sheet or slide relationship IDs do not match {relationship_part:?}",
            family.main_part()
        )));
    }
    Ok(())
}

fn collect_main_collection_ids(bytes: &[u8], family: CarrierFamily) -> Result<HashSet<String>> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut ids = HashSet::new();
    let mut collection_parent_stack = Vec::new();
    let mut collection_parent_seen = false;
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => {
                inspect_main_collection_element(
                    &reader,
                    &event,
                    depth,
                    collection_parent_stack.last().copied().unwrap_or(false),
                    family,
                    &mut ids,
                )?;
                collection_parent_stack.push(is_main_collection_parent(
                    &reader,
                    &event,
                    depth,
                    family,
                    &mut collection_parent_seen,
                )?);
                depth += 1;
            }
            Event::Empty(event) => {
                inspect_main_collection_element(
                    &reader,
                    &event,
                    depth,
                    collection_parent_stack.last().copied().unwrap_or(false),
                    family,
                    &mut ids,
                )?;
                is_main_collection_parent(
                    &reader,
                    &event,
                    depth,
                    family,
                    &mut collection_parent_seen,
                )?;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                collection_parent_stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(ids)
}

fn inspect_main_collection_element(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    depth: usize,
    canonical_parent: bool,
    family: CarrierFamily,
    ids: &mut HashSet<String>,
) -> Result<()> {
    let (local, namespace) = match family {
        CarrierFamily::Cell => (b"sheet".as_slice(), SPREADSHEET_NS),
        CarrierFamily::Show => (b"sldId".as_slice(), PRESENTATION_NS),
    };
    if event.local_name().as_ref() != local {
        return Ok(());
    }
    require_element(reader, event, local, namespace, family.main_part())?;
    if depth != 2 || !canonical_parent {
        return Err(PluginError::corrupt(format!(
            "{} contains a sheet or slide outside its canonical collection",
            family.main_part()
        )));
    }
    let id =
        namespaced_attribute_value(reader, event, b"id", DOCUMENT_REL_NS)?.ok_or_else(|| {
            PluginError::corrupt(format!(
                "{} collection entry omits its relationship ID",
                family.main_part()
            ))
        })?;
    if id.is_empty() || !ids.insert(id.clone()) {
        return Err(PluginError::corrupt(format!(
            "{} contains an invalid or duplicate collection relationship ID {id:?}",
            family.main_part()
        )));
    }
    Ok(())
}

fn is_main_collection_parent(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    depth: usize,
    family: CarrierFamily,
    seen: &mut bool,
) -> Result<bool> {
    let (local, namespace) = match family {
        CarrierFamily::Cell => (b"sheets".as_slice(), SPREADSHEET_NS),
        CarrierFamily::Show => (b"sldIdLst".as_slice(), PRESENTATION_NS),
    };
    if depth != 1 || event.local_name().as_ref() != local {
        return Ok(false);
    }
    require_element(reader, event, local, namespace, family.main_part())?;
    if std::mem::replace(seen, true) {
        return Err(PluginError::corrupt(format!(
            "{} repeats its sheet or slide collection",
            family.main_part()
        )));
    }
    Ok(true)
}

fn collect_collection_relationship_ids(
    bytes: &[u8],
    relationship_part: &str,
    family: CarrierFamily,
) -> Result<HashSet<String>> {
    let expected_type = match family {
        CarrierFamily::Cell => {
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"
        }
        CarrierFamily::Show => {
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"
        }
    };
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut ids = HashSet::new();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(_) => depth += 1,
            Event::Empty(event)
                if depth == 1
                    && event.local_name().as_ref() == b"Relationship"
                    && attribute(&reader, &event, b"Type")?.as_deref() == Some(expected_type) =>
            {
                let id = attribute(&reader, &event, b"Id")?
                    .ok_or_else(|| PluginError::corrupt("relationship omits Id"))?;
                if !ids.insert(id) {
                    return Err(PluginError::corrupt(format!(
                        "{relationship_part:?} repeats a collection relationship ID"
                    )));
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(ids)
}

fn namespaced_attribute_value(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    local: &[u8],
    namespace: &[u8],
) -> Result<Option<String>> {
    let mut value = None;
    for item in event.attributes().with_checks(true) {
        let item = item?;
        if item.key.local_name().as_ref() != local {
            continue;
        }
        match reader.resolver().resolve_attribute(item.key).0 {
            ResolveResult::Bound(actual) if actual.as_ref() == namespace => {
                let decoded = item
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())?
                    .into_owned();
                if value.replace(decoded).is_some() {
                    return Err(PluginError::corrupt(
                        "OOXML element repeats a namespaced relationship attribute",
                    ));
                }
            }
            ResolveResult::Unknown(_) => {
                return Err(PluginError::corrupt(
                    "OOXML part uses an undeclared attribute namespace prefix",
                ));
            }
            _ => {}
        }
    }
    Ok(value)
}

fn parse_root_relationships(bytes: &[u8]) -> Result<(String, String)> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut office_target = None;
    let mut app_target = None;
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => {
                if depth == 0 {
                    require_element(
                        &reader,
                        &event,
                        b"Relationships",
                        PACKAGE_RELS_NS,
                        ROOT_RELS,
                    )?;
                    root_seen = true;
                } else if depth == 1 && event.local_name().as_ref() == b"Relationship" {
                    require_element(&reader, &event, b"Relationship", PACKAGE_RELS_NS, ROOT_RELS)?;
                    record_relationship(&reader, &event, &mut office_target, &mut app_target)?;
                }
                depth += 1;
            }
            Event::Empty(event) => {
                if depth == 0 {
                    require_element(
                        &reader,
                        &event,
                        b"Relationships",
                        PACKAGE_RELS_NS,
                        ROOT_RELS,
                    )?;
                    root_seen = true;
                } else if depth == 1 && event.local_name().as_ref() == b"Relationship" {
                    require_element(&reader, &event, b"Relationship", PACKAGE_RELS_NS, ROOT_RELS)?;
                    record_relationship(&reader, &event, &mut office_target, &mut app_target)?;
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen {
        return Err(PluginError::corrupt("root relationships part has no root"));
    }
    Ok((
        office_target.ok_or_else(|| {
            PluginError::corrupt("root relationships omit the officeDocument target")
        })?,
        app_target.ok_or_else(|| {
            PluginError::unsupported_feature(
                "root relationships omit the verified Hancom producer metadata target",
            )
        })?,
    ))
}

fn record_relationship(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    office_target: &mut Option<String>,
    app_target: &mut Option<String>,
) -> Result<()> {
    let relationship_type = attribute(reader, event, b"Type")?
        .ok_or_else(|| PluginError::corrupt("relationship omits Type"))?;
    if relationship_type != OFFICE_REL && relationship_type != APP_REL {
        return Ok(());
    }
    let target = attribute(reader, event, b"Target")?
        .ok_or_else(|| PluginError::corrupt("relationship omits Target"))?;
    if attribute(reader, event, b"TargetMode")?
        .is_some_and(|mode| !mode.eq_ignore_ascii_case("internal"))
    {
        return Err(PluginError::corrupt(
            "required OOXML relationship must not be external",
        ));
    }
    safe_relationship_target(&target)?;
    let slot = if relationship_type == OFFICE_REL {
        office_target
    } else {
        app_target
    };
    if slot.replace(target).is_some() {
        return Err(PluginError::corrupt(
            "required OOXML relationship is declared more than once",
        ));
    }
    Ok(())
}

fn safe_relationship_target(target: &str) -> Result<()> {
    if target.is_empty()
        || target.contains('\\')
        || target.contains('#')
        || target.contains('?')
        || target.starts_with('/')
        || target.as_bytes().get(1) == Some(&b':')
        || Path::new(target)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PluginError::corrupt(format!(
            "unsafe OOXML relationship target: {target:?}"
        )));
    }
    Ok(())
}

fn require_main_content_type(bytes: &[u8], family: CarrierFamily) -> Result<()> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut matched = false;
    let mut declarations = 0_usize;
    let mut buffer = Vec::new();
    let expected_part = format!("/{}", family.main_part());
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => {
                if depth == 0 {
                    require_element(&reader, &event, b"Types", CONTENT_TYPES_NS, CONTENT_TYPES)?;
                    root_seen = true;
                } else if depth == 1
                    && event.local_name().as_ref() == b"Override"
                    && attribute(&reader, &event, b"PartName")?.as_deref()
                        == Some(expected_part.as_str())
                {
                    require_element(
                        &reader,
                        &event,
                        b"Override",
                        CONTENT_TYPES_NS,
                        CONTENT_TYPES,
                    )?;
                    declarations += 1;
                    matched = is_expected_main_content_type(
                        family,
                        attribute(&reader, &event, b"ContentType")?.as_deref(),
                    );
                }
                depth += 1;
            }
            Event::Empty(event) => {
                if depth == 0 {
                    require_element(&reader, &event, b"Types", CONTENT_TYPES_NS, CONTENT_TYPES)?;
                    root_seen = true;
                } else if depth == 1
                    && event.local_name().as_ref() == b"Override"
                    && attribute(&reader, &event, b"PartName")?.as_deref()
                        == Some(expected_part.as_str())
                {
                    require_element(
                        &reader,
                        &event,
                        b"Override",
                        CONTENT_TYPES_NS,
                        CONTENT_TYPES,
                    )?;
                    declarations += 1;
                    matched = is_expected_main_content_type(
                        family,
                        attribute(&reader, &event, b"ContentType")?.as_deref(),
                    );
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || declarations != 1 || !matched {
        return Err(PluginError::corrupt(format!(
            "OOXML content types do not declare {:?} as a verified native family",
            family.main_part()
        )));
    }
    Ok(())
}

fn is_expected_main_content_type(family: CarrierFamily, value: Option<&str>) -> bool {
    match family {
        CarrierFamily::Cell => matches!(value, Some(CELL_MAIN_TYPE | CELL_MACRO_MAIN_TYPE)),
        CarrierFamily::Show => value == Some(SHOW_MAIN_TYPE),
    }
}

fn require_hancom_v12_application(bytes: &[u8], family: CarrierFamily) -> Result<()> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut capture = None;
    let mut application = String::new();
    let mut version = String::new();
    let mut application_count = 0_usize;
    let mut version_count = 0_usize;
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => {
                if depth == 0 {
                    require_element(&reader, &event, b"Properties", APP_NS, APP_PROPERTIES)?;
                    root_seen = true;
                } else if depth == 1 {
                    capture = match event.local_name().as_ref() {
                        b"Application" => {
                            require_element(
                                &reader,
                                &event,
                                b"Application",
                                APP_NS,
                                APP_PROPERTIES,
                            )?;
                            application_count += 1;
                            Some(0_u8)
                        }
                        b"AppVersion" => {
                            require_element(
                                &reader,
                                &event,
                                b"AppVersion",
                                APP_NS,
                                APP_PROPERTIES,
                            )?;
                            version_count += 1;
                            Some(1_u8)
                        }
                        _ => None,
                    };
                }
                depth += 1;
            }
            Event::Text(text) if depth == 2 => match capture {
                Some(0) => application.push_str(&text.decode()?),
                Some(1) => version.push_str(&text.decode()?),
                _ => {}
            },
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 1 {
                    capture = None;
                }
            }
            Event::Empty(event) => {
                if depth == 0 {
                    require_element(&reader, &event, b"Properties", APP_NS, APP_PROPERTIES)?;
                    root_seen = true;
                } else if depth == 1 {
                    match event.local_name().as_ref() {
                        b"Application" => {
                            require_element(
                                &reader,
                                &event,
                                b"Application",
                                APP_NS,
                                APP_PROPERTIES,
                            )?;
                            application_count += 1;
                        }
                        b"AppVersion" => {
                            require_element(
                                &reader,
                                &event,
                                b"AppVersion",
                                APP_NS,
                                APP_PROPERTIES,
                            )?;
                            version_count += 1;
                        }
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen {
        return Err(PluginError::corrupt("app properties part has no root"));
    }
    if application_count != 1 || version_count != 1 {
        return Err(PluginError::corrupt(
            "app properties must contain exactly one Application and AppVersion",
        ));
    }
    if application.trim() != family.application()
        || !is_verified_v12_version(family, version.trim())
    {
        return Err(PluginError::unsupported_feature(format!(
            "profile markers {:?} {:?} are outside the evidence-backed Hancom {} subset",
            application.trim(),
            version.trim(),
            family.application()
        )));
    }
    Ok(())
}

fn is_verified_v12_version(family: CarrierFamily, version: &str) -> bool {
    match family {
        CarrierFamily::Cell => version == "12.0300",
        CarrierFamily::Show => version == "12.0000",
    }
}

fn require_main_fingerprint(bytes: &[u8], family: CarrierFamily) -> Result<()> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut markers = CellMarkers::default();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => {
                inspect_main_element(&reader, &event, depth, family, &mut root_seen, &mut markers)?;
                depth += 1;
            }
            Event::Empty(event) => {
                inspect_main_element(&reader, &event, depth, family, &mut root_seen, &mut markers)?;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen {
        return Err(PluginError::corrupt("OOXML main part has no root"));
    }
    if family == CarrierFamily::Cell && (!markers.hcell_version || !markers.hancom_calc) {
        return Err(PluginError::unsupported_feature(
            "spreadsheet lacks the verified HCell 12.0300 profile markers",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct CellMarkers {
    file_version_seen: bool,
    calc_properties_seen: bool,
    hcell_version: bool,
    hancom_calc: bool,
}

fn inspect_main_element(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    depth: usize,
    family: CarrierFamily,
    root_seen: &mut bool,
    markers: &mut CellMarkers,
) -> Result<()> {
    if depth == 0 {
        let (local, namespace) = match family {
            CarrierFamily::Cell => (b"workbook".as_slice(), SPREADSHEET_NS),
            CarrierFamily::Show => (b"presentation".as_slice(), PRESENTATION_NS),
        };
        require_element(reader, event, local, namespace, family.main_part())?;
        *root_seen = true;
    }
    if family == CarrierFamily::Cell && depth == 1 {
        if event.local_name().as_ref() == b"fileVersion" {
            if std::mem::replace(&mut markers.file_version_seen, true) {
                return Err(PluginError::corrupt(
                    "spreadsheet declares fileVersion more than once",
                ));
            }
            require_element(
                reader,
                event,
                b"fileVersion",
                SPREADSHEET_NS,
                family.main_part(),
            )?;
            if attribute(reader, event, b"appName")?.as_deref() == Some("HCell") {
                markers.hcell_version = true;
            }
        }
        if event.local_name().as_ref() == b"calcPr" {
            if std::mem::replace(&mut markers.calc_properties_seen, true) {
                return Err(PluginError::corrupt(
                    "spreadsheet declares calcPr more than once",
                ));
            }
            require_element(reader, event, b"calcPr", SPREADSHEET_NS, family.main_part())?;
            if has_namespaced_attribute(reader, event, b"hclCalcId", HANCOM_SPREADSHEET_NS)? {
                markers.hancom_calc = true;
            }
        }
    }
    Ok(())
}

fn has_namespaced_attribute(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    local: &[u8],
    namespace: &[u8],
) -> Result<bool> {
    for item in event.attributes().with_checks(true) {
        let item = item?;
        if item.key.local_name().as_ref() != local {
            continue;
        }
        return match reader.resolver().resolve_attribute(item.key).0 {
            ResolveResult::Bound(actual) => Ok(actual.as_ref() == namespace),
            ResolveResult::Unbound => Ok(namespace.is_empty()),
            ResolveResult::Unknown(_) => Err(PluginError::corrupt(
                "OOXML part uses an undeclared attribute namespace prefix",
            )),
        };
    }
    Ok(false)
}

fn require_element(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    local: &[u8],
    namespace: &[u8],
    part: &str,
) -> Result<()> {
    if event.local_name().as_ref() != local || !element_namespace_matches(reader, event, namespace)?
    {
        return Err(PluginError::corrupt(format!(
            "OOXML part {part:?} has an unexpected root element"
        )));
    }
    Ok(())
}

fn element_namespace_matches(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    expected: &[u8],
) -> Result<bool> {
    let name = event.name();
    match reader.resolver().resolve_element(name).0 {
        ResolveResult::Bound(namespace) => Ok(namespace.as_ref() == expected),
        ResolveResult::Unbound => Ok(expected.is_empty()),
        ResolveResult::Unknown(_) => Err(PluginError::corrupt(
            "OOXML part uses an undeclared namespace prefix",
        )),
    }
}

fn attribute(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>> {
    for item in event.attributes().with_checks(true) {
        let item = item?;
        if item.key.as_ref() == name {
            return Ok(Some(
                item.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn require_only_attributes(event: &BytesStart<'_>, allowed: &[&[u8]], part: &str) -> Result<()> {
    for item in event.attributes().with_checks(true) {
        let item = item?;
        let name = item.key.as_ref();
        if name == b"xmlns" || name.starts_with(b"xmlns:") {
            continue;
        }
        if !allowed.contains(&name) {
            return Err(PluginError::corrupt(format!(
                "OOXML part {part:?} contains unexpected attribute {:?}",
                String::from_utf8_lossy(name)
            )));
        }
    }
    Ok(())
}

fn hash_file(file: &mut File) -> Result<[u8; 32]> {
    file.seek(SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

fn copy_and_hash(source: &mut File, target: &mut File) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        target.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

fn ensure_target_safe(source: &File, target: &Path) -> Result<Option<File>> {
    let metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PluginError::invalid_argument(format!(
                "cannot inspect native sibling path: {error}"
            )));
        }
    };
    if is_reparseable(&metadata) || !metadata.file_type().is_file() {
        return Err(PluginError::invalid_argument(
            "native sibling must be a non-reparse regular file",
        ));
    }
    let target_file = open_source_without_reparse(target).map_err(|error| {
        PluginError::invalid_argument(format!("cannot inspect native sibling identity: {error}"))
    })?;
    if same_file(source, &target_file) {
        return Err(PluginError::invalid_argument(
            "native sibling must not alias the source file",
        ));
    }
    if has_multiple_links(&target_file) {
        return Err(PluginError::invalid_argument(
            "native sibling must not have additional hard links",
        ));
    }
    Ok(Some(target_file))
}

#[cfg(unix)]
fn is_reparseable(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_reparseable(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_reparseable(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn same_file(left: &File, right: &File) -> bool {
    use std::os::unix::fs::MetadataExt;

    matches!(
        (left.metadata(), right.metadata()),
        (Ok(left), Ok(right)) if left.dev() == right.dev() && left.ino() == right.ino()
    )
}

#[cfg(windows)]
fn same_file(left: &File, right: &File) -> bool {
    file_identity(left).is_some_and(|identity| Some(identity) == file_identity(right))
}

#[cfg(windows)]
fn file_identity(file: &File) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) };
    if ok == 0 {
        return None;
    }
    Some((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(not(any(unix, windows)))]
fn same_file(_left: &File, _right: &File) -> bool {
    false
}

#[cfg(unix)]
fn has_multiple_links(file: &File) -> bool {
    use std::os::unix::fs::MetadataExt;

    file.metadata().is_ok_and(|metadata| metadata.nlink() > 1)
}

#[cfg(windows)]
fn has_multiple_links(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) };
    ok != 0 && info.nNumberOfLinks > 1
}

#[cfg(not(any(unix, windows)))]
fn has_multiple_links(_file: &File) -> bool {
    false
}

#[cfg(windows)]
fn validate_supported_source_security_metadata(path: &Path, metadata: &Metadata) -> Result<()> {
    use std::os::windows::fs::MetadataExt;

    validate_windows_file_attributes(metadata.file_attributes())?;
    validate_windows_stream_policy(path)
}

#[cfg(windows)]
fn validate_windows_file_attributes(attributes: u32) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ENCRYPTED;

    if attributes & FILE_ATTRIBUTE_ENCRYPTED != 0 {
        return Err(PluginError::unsupported_feature(
            "EFS-encrypted sources are outside the byte-preserving carrier filesystem policy",
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn validate_supported_source_security_metadata(_path: &Path, _metadata: &Metadata) -> Result<()> {
    Ok(())
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_os = "macos"))
))]
fn validate_supported_source_security_metadata(_path: &Path, _metadata: &Metadata) -> Result<()> {
    Err(PluginError::unsupported_feature(
        "filesystem access-control enumeration is not implemented on this Unix platform",
    ))
}

#[cfg(not(any(unix, windows)))]
fn validate_supported_source_security_metadata(_path: &Path, _metadata: &Metadata) -> Result<()> {
    Err(PluginError::unsupported_feature(
        "filesystem trust metadata cannot be enumerated on this platform",
    ))
}

#[cfg(windows)]
fn prepare_private_candidate_access_control(source: &Path, candidate: &Path) -> Result<()> {
    copy_windows_dacl(source, candidate)
}

#[cfg(target_os = "macos")]
fn prepare_private_candidate_access_control(_source: &Path, candidate: &Path) -> Result<()> {
    clear_macos_acl(candidate)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn prepare_private_candidate_access_control(_source: &Path, _candidate: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn prepare_private_candidate_access_control(_source: &Path, _candidate: &Path) -> Result<()> {
    Err(PluginError::unsupported_feature(
        "filesystem access control cannot be preserved on this platform",
    ))
}

#[cfg(windows)]
fn preserve_output_access_control(source: &Path, candidate: &Path) -> Result<()> {
    copy_windows_dacl(source, candidate)
}

#[cfg(target_os = "macos")]
fn preserve_output_access_control(source: &Path, candidate: &Path) -> Result<()> {
    copy_macos_acl(source, candidate)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn preserve_output_access_control(_source: &Path, _candidate: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn preserve_output_access_control(_source: &Path, _candidate: &Path) -> Result<()> {
    Err(PluginError::unsupported_feature(
        "filesystem access control cannot be preserved on this platform",
    ))
}

#[cfg(windows)]
fn preserve_output_security_metadata(
    source: &Path,
    _source_file: &File,
    candidate: &Path,
    _candidate_file: &File,
) -> Result<()> {
    copy_windows_zone_identifier(source, candidate)?;
    copy_windows_dacl(source, candidate)
}

#[cfg(windows)]
fn validate_windows_stream_policy(path: &Path) -> Result<()> {
    let streams = read_windows_stream_names(path)?;
    if streams.iter().any(|stream| {
        !stream.eq_ignore_ascii_case("::$DATA")
            && !stream.eq_ignore_ascii_case(":Zone.Identifier:$DATA")
    }) {
        return Err(PluginError::unsupported_feature(
            "source or sibling contains an alternate data stream outside the Zone.Identifier policy",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn read_windows_stream_names(path: &Path) -> Result<Vec<String>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        ERROR_HANDLE_EOF, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard,
        WIN32_FIND_STREAM_DATA,
    };

    struct FindHandle(HANDLE);
    impl Drop for FindHandle {
        fn drop(&mut self) {
            unsafe {
                FindClose(self.0);
            }
        }
    }

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut data = WIN32_FIND_STREAM_DATA::default();
    let handle = unsafe {
        FindFirstStreamW(
            wide.as_ptr(),
            FindStreamInfoStandard,
            (&raw mut data).cast(),
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_HANDLE_EOF as i32) {
            return Ok(Vec::new());
        }
        return Err(PluginError::invalid_argument(format!(
            "cannot enumerate file data streams: {error}"
        )));
    }
    let _handle = FindHandle(handle);
    let mut streams = Vec::new();
    loop {
        let end = data
            .cStreamName
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(data.cStreamName.len());
        streams.push(
            String::from_utf16(&data.cStreamName[..end]).map_err(|error| {
                PluginError::corrupt(format!("file stream name has invalid UTF-16: {error}"))
            })?,
        );
        data = WIN32_FIND_STREAM_DATA::default();
        if unsafe { FindNextStreamW(handle, (&raw mut data).cast()) } == 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code) if code == ERROR_HANDLE_EOF as i32 || code == ERROR_NO_MORE_FILES as i32
            ) {
                break;
            }
            return Err(PluginError::invalid_argument(format!(
                "cannot finish enumerating file data streams: {error}"
            )));
        }
    }
    streams.sort_unstable_by_key(|value| value.to_ascii_lowercase());
    Ok(streams)
}

#[cfg(windows)]
fn copy_windows_zone_identifier(source: &Path, candidate: &Path) -> Result<()> {
    fn stream_path(path: &Path) -> PathBuf {
        let mut value = path.as_os_str().to_os_string();
        value.push(":Zone.Identifier");
        PathBuf::from(value)
    }

    let Some(bytes) = read_windows_zone_identifier(source)? else {
        return Ok(());
    };
    let candidate_stream = stream_path(candidate);
    fs::write(candidate_stream, bytes).map_err(|error| {
        PluginError::invalid_argument(format!(
            "cannot preserve source Zone.Identifier on native sibling: {error}"
        ))
    })
}

#[cfg(windows)]
fn copy_windows_dacl(source: &Path, candidate: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::{
        SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let candidate_wide = wide(candidate);
    let (mut descriptor, protected) = read_windows_dacl_descriptor(source)?;
    let inheritance = if protected {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    if unsafe {
        SetFileSecurityW(
            candidate_wide.as_ptr(),
            DACL_SECURITY_INFORMATION | inheritance,
            descriptor.as_mut_ptr().cast(),
        )
    } == 0
    {
        return Err(PluginError::invalid_argument(format!(
            "cannot preserve source DACL on native sibling: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn read_windows_zone_identifier(path: &Path) -> Result<Option<Vec<u8>>> {
    const MAX_ZONE_IDENTIFIER_BYTES: usize = 64 * 1024;

    let mut stream = path.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    match fs::read(PathBuf::from(stream)) {
        Ok(bytes) if bytes.len() <= MAX_ZONE_IDENTIFIER_BYTES => Ok(Some(bytes)),
        Ok(_) => Err(resource_limit(format!(
            "Zone.Identifier exceeds {MAX_ZONE_IDENTIFIER_BYTES} bytes"
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PluginError::invalid_argument(format!(
            "cannot read Zone.Identifier: {error}"
        ))),
    }
}

#[cfg(windows)]
fn read_windows_dacl_descriptor(path: &Path) -> Result<(Vec<u8>, bool)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::{
        GetFileSecurityW, GetSecurityDescriptorControl, DACL_SECURITY_INFORMATION,
        SE_DACL_PROTECTED,
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut required = 0_u32;
    unsafe {
        GetFileSecurityW(
            wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
    }
    if required == 0 {
        return Err(PluginError::invalid_argument(format!(
            "cannot read file DACL: {}",
            io::Error::last_os_error()
        )));
    }
    let mut descriptor = vec![0_u8; usize::try_from(required).unwrap_or(0)];
    if descriptor.is_empty()
        || unsafe {
            GetFileSecurityW(
                wide.as_ptr(),
                DACL_SECURITY_INFORMATION,
                descriptor.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
    {
        return Err(PluginError::invalid_argument(format!(
            "cannot read file DACL: {}",
            io::Error::last_os_error()
        )));
    }
    let mut control = 0_u16;
    let mut revision = 0_u32;
    if unsafe {
        GetSecurityDescriptorControl(descriptor.as_mut_ptr().cast(), &mut control, &mut revision)
    } == 0
    {
        return Err(PluginError::invalid_argument(format!(
            "cannot inspect file DACL control: {}",
            io::Error::last_os_error()
        )));
    }
    Ok((descriptor, control & SE_DACL_PROTECTED != 0))
}

#[cfg(windows)]
fn windows_dacl_sddl(path: &Path) -> Result<(String, bool)> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

    let (mut descriptor, protected) = read_windows_dacl_descriptor(path)?;
    let mut sddl = std::ptr::null_mut();
    let mut length = 0_u32;
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.as_mut_ptr().cast(),
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut sddl,
            &mut length,
        )
    } == 0
        || sddl.is_null()
        || length == 0
    {
        return Err(PluginError::invalid_argument(format!(
            "cannot canonicalize file DACL: {}",
            io::Error::last_os_error()
        )));
    }
    let chars = unsafe { std::slice::from_raw_parts(sddl, usize::try_from(length).unwrap_or(0)) };
    let chars = &chars[..chars
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(chars.len())];
    let result = String::from_utf16(chars).map_err(|error| {
        PluginError::invalid_argument(format!("file DACL has invalid UTF-16 SDDL: {error}"))
    });
    unsafe {
        LocalFree(sddl.cast());
    }
    let result = result?;
    let ace_offset = result.find('(').unwrap_or(result.len());
    Ok((result[ace_offset..].to_string(), protected))
}

#[cfg(windows)]
fn security_metadata_matches(
    source: &Path,
    source_file: &File,
    target: &Path,
    target_file: &File,
) -> Result<bool> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;

    let source_metadata = source_file.metadata()?;
    let target_metadata = target_file.metadata()?;
    validate_supported_source_security_metadata(source, &source_metadata)?;
    validate_supported_source_security_metadata(target, &target_metadata)?;
    let source_readonly = source_metadata.file_attributes() & FILE_ATTRIBUTE_READONLY != 0;
    let target_readonly = target_metadata.file_attributes() & FILE_ATTRIBUTE_READONLY != 0;
    let modified_matches = source_metadata.modified()? == target_metadata.modified()?;
    let zone_matches =
        read_windows_zone_identifier(source)? == read_windows_zone_identifier(target)?;
    let streams_match = read_windows_stream_names(source)? == read_windows_stream_names(target)?;
    let dacl_matches = windows_dacl_sddl(source)? == windows_dacl_sddl(target)?;
    Ok(modified_matches
        && source_readonly == target_readonly
        && zone_matches
        && streams_match
        && dacl_matches)
}

#[cfg(target_os = "macos")]
mod macos_acl {
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::os::unix::ffi::OsStrExt;

    use super::*;

    const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;
    const COPYFILE_ACL: u32 = 1 << 0;
    const COPYFILE_NOFOLLOW_SRC: u32 = 1 << 18;
    const COPYFILE_NOFOLLOW_DST: u32 = 1 << 19;

    unsafe extern "C" {
        fn acl_free(value: *mut c_void) -> c_int;
        fn acl_get_file(path: *const c_char, kind: c_int) -> *mut c_void;
        fn acl_init(count: c_int) -> *mut c_void;
        fn acl_set_file(path: *const c_char, kind: c_int, acl: *mut c_void) -> c_int;
        fn acl_to_text(acl: *mut c_void, len: *mut libc::ssize_t) -> *mut c_char;
        fn copyfile(
            from: *const c_char,
            to: *const c_char,
            state: *mut c_void,
            flags: u32,
        ) -> c_int;
    }

    struct Acl(*mut c_void);

    impl Drop for Acl {
        fn drop(&mut self) {
            unsafe {
                acl_free(self.0);
            }
        }
    }

    fn c_path(path: &Path) -> Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| PluginError::invalid_argument("macOS ACL path contains an interior NUL"))
    }

    pub(super) fn clear(path: &Path) -> Result<()> {
        let path = c_path(path)?;
        let acl = unsafe { acl_init(0) };
        if acl.is_null() {
            return Err(PluginError::invalid_argument(format!(
                "cannot allocate an empty macOS ACL: {}",
                io::Error::last_os_error()
            )));
        }
        let acl = Acl(acl);
        if unsafe { acl_set_file(path.as_ptr(), ACL_TYPE_EXTENDED, acl.0) } != 0 {
            return Err(PluginError::invalid_argument(format!(
                "cannot make the private candidate ACL empty: {}",
                io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    pub(super) fn copy(source: &Path, candidate: &Path) -> Result<()> {
        let source = c_path(source)?;
        let candidate = c_path(candidate)?;
        let result = unsafe {
            copyfile(
                source.as_ptr(),
                candidate.as_ptr(),
                std::ptr::null_mut(),
                COPYFILE_ACL | COPYFILE_NOFOLLOW_SRC | COPYFILE_NOFOLLOW_DST,
            )
        };
        if result != 0 {
            return Err(PluginError::invalid_argument(format!(
                "cannot preserve the source macOS ACL: {}",
                io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    pub(super) fn read(path: &Path) -> Result<Vec<u8>> {
        let path = c_path(path)?;
        let acl = unsafe { acl_get_file(path.as_ptr(), ACL_TYPE_EXTENDED) };
        if acl.is_null() {
            let error = io::Error::last_os_error();
            // Darwin reports a regular file with no extended ACL as ENOENT.
            // Normalize only that absence marker; permission, filesystem, and
            // representation failures still fail closed below.
            if error.raw_os_error() == Some(libc::ENOENT) {
                return Ok(Vec::new());
            }
            return Err(PluginError::invalid_argument(format!(
                "cannot read the macOS ACL: {error}"
            )));
        }
        let acl = Acl(acl);
        let mut length = 0;
        let text = unsafe { acl_to_text(acl.0, &mut length) };
        if text.is_null() || length < 0 {
            return Err(PluginError::invalid_argument(format!(
                "cannot canonicalize the macOS ACL: {}",
                io::Error::last_os_error()
            )));
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(text.cast::<u8>(), usize::try_from(length).unwrap_or(0))
        }
        .to_vec();
        unsafe {
            acl_free(text.cast());
        }
        Ok(bytes)
    }
}

#[cfg(target_os = "macos")]
fn clear_macos_acl(path: &Path) -> Result<()> {
    macos_acl::clear(path)
}

#[cfg(target_os = "macos")]
fn copy_macos_acl(source: &Path, candidate: &Path) -> Result<()> {
    macos_acl::copy(source, candidate)
}

#[cfg(target_os = "macos")]
fn platform_acl_matches(source: &Path, target: &Path) -> Result<bool> {
    Ok(macos_acl::read(source)? == macos_acl::read(target)?)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_acl_matches(_source: &Path, _target: &Path) -> Result<bool> {
    Ok(true)
}

#[cfg(unix)]
fn security_metadata_matches(
    source: &Path,
    source_file: &File,
    target: &Path,
    target_file: &File,
) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    let source_metadata = source_file.metadata()?;
    let target_metadata = target_file.metadata()?;
    Ok(source_metadata.modified()? == target_metadata.modified()?
        && source_metadata.permissions().mode() == target_metadata.permissions().mode()
        && read_unix_xattrs(source_file)? == read_unix_xattrs(target_file)?
        && platform_acl_matches(source, target)?)
}

#[cfg(not(any(unix, windows)))]
fn security_metadata_matches(
    _source: &Path,
    _source_file: &File,
    _target: &Path,
    _target_file: &File,
) -> Result<bool> {
    Ok(false)
}

#[cfg(unix)]
fn preserve_output_security_metadata(
    _source: &Path,
    source_file: &File,
    _candidate: &Path,
    candidate_file: &File,
) -> Result<()> {
    use xattr::FileExt;

    let source_attributes = read_unix_xattrs(source_file)?;
    let candidate_attributes = read_unix_xattrs(candidate_file)?;
    for (name, _) in &candidate_attributes {
        if source_attributes
            .iter()
            .any(|(source_name, _)| source_name == name)
        {
            continue;
        }
        candidate_file.remove_xattr(name).map_err(|error| {
            PluginError::invalid_argument(format!(
                "cannot remove inherited extended attribute {name:?}: {error}"
            ))
        })?;
    }
    for (name, value) in &source_attributes {
        if candidate_attributes
            .iter()
            .any(|(candidate_name, candidate_value)| {
                candidate_name == name && candidate_value == value
            })
        {
            continue;
        }
        candidate_file.set_xattr(name, value).map_err(|error| {
            PluginError::invalid_argument(format!(
                "cannot preserve source extended attribute {name:?}: {error}"
            ))
        })?;
    }
    Ok(())
}

#[cfg(unix)]
fn read_unix_xattrs(file: &File) -> Result<Vec<(OsString, Vec<u8>)>> {
    use xattr::FileExt;

    const MAX_XATTR_COUNT: usize = 128;
    const MAX_XATTR_TOTAL_BYTES: usize = 8 * 1024 * 1024;

    let mut attributes = Vec::new();
    let mut count = 0_usize;
    let mut total = 0_usize;
    for name in file.list_xattr().map_err(|error| {
        PluginError::invalid_argument(format!("cannot enumerate extended attributes: {error}"))
    })? {
        count += 1;
        if count > MAX_XATTR_COUNT {
            return Err(resource_limit(format!(
                "source has more than {MAX_XATTR_COUNT} extended attributes"
            )));
        }
        let value = file
            .get_xattr(&name)
            .map_err(|error| {
                PluginError::invalid_argument(format!(
                    "cannot read extended attribute {name:?}: {error}"
                ))
            })?
            .ok_or_else(|| {
                PluginError::corrupt(format!(
                    "extended attribute {name:?} disappeared during inspection"
                ))
            })?;
        total = total
            .checked_add(value.len())
            .ok_or_else(|| resource_limit("extended attribute size overflowed".to_string()))?;
        if total > MAX_XATTR_TOTAL_BYTES {
            return Err(resource_limit(format!(
                "source extended attributes exceed {MAX_XATTR_TOTAL_BYTES} bytes"
            )));
        }
        attributes.push((name, value));
    }
    attributes.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    Ok(attributes)
}

#[cfg(not(any(unix, windows)))]
fn preserve_output_security_metadata(
    _source: &Path,
    _source_file: &File,
    _candidate: &Path,
    _candidate_file: &File,
) -> Result<()> {
    Err(PluginError::unsupported_feature(
        "filesystem trust metadata cannot be preserved on this platform",
    ))
}

#[cfg(unix)]
fn install_candidate(candidate: NamedTempFile, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| PluginError::internal("native sibling path has no parent"))?;
    let directory = File::open(parent)
        .map_err(|error| PluginError::internal(format!("cannot open output directory: {error}")))?;
    directory
        .sync_all()
        .map_err(|error| PluginError::internal(format!("cannot pre-sync output: {error}")))?;
    candidate.persist_noclobber(target).map_err(|error| {
        PluginError::internal(format!("atomic no-clobber output commit failed: {error}"))
    })?;
    directory
        .sync_all()
        .map_err(|error| PluginError::internal(format!("cannot sync output directory: {error}")))?;
    Ok(())
}

#[cfg(windows)]
fn install_candidate(candidate: NamedTempFile, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let candidate_path = candidate.into_temp_path();
    let target_wide = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let candidate_wide = candidate_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            candidate_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(PluginError::internal(format!(
            "atomic output commit failed: {}",
            io::Error::last_os_error()
        )));
    }
    drop(candidate_path);
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn install_candidate(candidate: NamedTempFile, target: &Path) -> Result<()> {
    candidate.persist_noclobber(target).map_err(|error| {
        PluginError::internal(format!("atomic no-clobber output commit failed: {error}"))
    })?;
    Ok(())
}

fn resource_limit(message: String) -> PluginError {
    PluginError::corrupt(format!("resource limit exceeded: {message}"))
}

#[cfg(all(test, windows))]
mod windows_security_tests {
    use super::*;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ENCRYPTED;

    #[test]
    fn efs_encryption_is_outside_the_admitted_filesystem_policy() {
        let error = validate_windows_file_attributes(FILE_ATTRIBUTE_ENCRYPTED)
            .expect_err("EFS must fail closed");

        assert_eq!(error.exit_code(), ExitCode::UnsupportedFeature);
    }

    #[test]
    fn retained_source_handle_blocks_path_rebinding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("source.cell");
        let displaced = dir.path().join("displaced.cell");
        fs::write(&source, b"original").expect("write source");

        let retained = open_source_without_reparse(&source).expect("retain source identity");
        fs::rename(&source, &displaced)
            .expect_err("retained source identity must deny rename or replacement");

        assert_eq!(
            fs::read(&source).expect("read retained source"),
            b"original"
        );
        assert!(!displaced.exists());
        drop(retained);
        fs::rename(&source, &displaced).expect("rename after releasing retained identity");
    }
}

#[cfg(all(test, unix))]
mod unix_security_tests {
    use super::*;

    #[test]
    fn xattrs_are_read_from_the_retained_file_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("source.cell");
        let displaced = dir.path().join("displaced.cell");
        fs::write(&source, b"original").expect("write original source");
        xattr::set(&source, "user.officecli.identity", b"original").expect("set original xattr");
        let retained = File::open(&source).expect("retain source identity");

        fs::rename(&source, &displaced).expect("displace original source");
        fs::write(&source, b"replacement").expect("write replacement source");
        xattr::set(&source, "user.officecli.identity", b"replacement")
            .expect("set replacement xattr");

        let attributes = read_unix_xattrs(&retained).expect("read retained xattrs");
        assert!(attributes.iter().any(|(name, value)| {
            name == "user.officecli.identity" && value.as_slice() == b"original"
        }));
        assert!(!attributes
            .iter()
            .any(|(_, value)| value.as_slice() == b"replacement"));
    }
}
