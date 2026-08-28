//! 입력 포맷 판별.
//!
//! **확장자를 믿지 않는다.** `.hwp`인데 실제로는 HWPX인 파일이 흔하고 반대도
//! 있다. 매직 바이트로 판별해야 잘못된 확장자에 강해진다.
//!
//! | 포맷 | 판별 |
//! |---|---|
//! | HWP 3.0 | 파일 선두 23바이트가 `HWP Document File V3.00` |
//! | HWP 5.x | CFB 시그니처 + `FileHeader` 스트림이 `HWP Document File`로 시작 |
//! | HWPX | ZIP 시그니처 + `mimetype`이 `application/hwp+zip` 또는 `Contents/section*.xml` 존재 |
//!
//! 판별 순서가 중요하다. CFB 파일의 `FileHeader` 스트림에도
//! `HWP Document File` 문자열이 들어 있으므로, HWP 3.0은 **파일 선두**에서만
//! 인정한다. CFB는 `D0 CF 11 E0`로 시작하므로 충돌하지 않는다.
//!
//! `FileHeader` 레이아웃 근거: `edwardkim/rhwp`(MIT) `src/parser/header.rs`.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{PluginError, Result};

pub const HWPX_MIMETYPE_ENTRY: &str = "mimetype";
pub const HWPX_MIMETYPE_VALUE: &str = "application/hwp+zip";
pub const HWPX_SECTION_PREFIX: &str = "Contents/section";
pub const MAX_MIMETYPE_BYTES: u64 = 4 * 1024;

/// CFB(Compound File Binary, OLE) 컨테이너 시그니처.
const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// ZIP 로컬 파일 헤더 시그니처.
const ZIP_SIGNATURE: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
/// ZIP 시그니처 계열의 세 번째 바이트.
///
/// `PK\x03\x04` 로컬 파일 헤더 외에도 `PK\x05\x06`(빈 아카이브,
/// end-of-central-directory가 첫 레코드)와 `PK\x07\x08`(분할 아카이브)이 있다.
/// 로컬 헤더만 보면 빈 ZIP을 놓친다.
const ZIP_THIRD_BYTES: [u8; 3] = [0x03, 0x05, 0x07];
/// HWP 5.x `FileHeader` 스트림 시그니처.
const HWP5_SIGNATURE: &[u8] = b"HWP Document File";
/// HWP 3.0 파일 선두 시그니처.
const HWP3_SIGNATURE: &[u8] = b"HWP Document File V3.00";
/// `FileHeader` 스트림 크기.
const FILE_HEADER_SIZE: usize = 256;
/// CFB 안의 `FileHeader` 스트림 경로.
const FILE_HEADER_PATH: &str = "/FileHeader";

/// HWP 5.x `FileHeader`의 속성 플래그와 버전.
///
/// 진단 메시지에 쓴다. 암호·DRM 문서는 변환기가 실패할 수 있으므로 미리 알린다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5Info {
    pub major: u8,
    pub minor: u8,
    pub build: u8,
    pub revision: u8,
    /// 레코드가 zlib 압축되어 있는지.
    pub compressed: bool,
    /// 암호로 보호됨.
    pub encrypted: bool,
    /// 배포용 문서 (읽기 제한).
    pub distribution: bool,
    pub drm: bool,
    pub public_key_encrypted: bool,
}

impl Hwp5Info {
    /// 사람이 읽는 버전 문자열. `5.1.1.0` 형태.
    pub fn version_string(&self) -> String {
        format!(
            "{}.{}.{}.{}",
            self.major, self.minor, self.build, self.revision
        )
    }

    /// 변환기가 실패할 가능성이 높은 보호 상태인지.
    pub fn is_protected(&self) -> bool {
        self.encrypted || self.drm || self.public_key_encrypted
    }

    /// 보호 상태를 사람이 읽는 목록으로.
    pub fn protection_notes(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.encrypted {
            v.push("password-encrypted");
        }
        if self.drm {
            v.push("DRM-protected");
        }
        if self.public_key_encrypted {
            v.push("public-key-encrypted");
        }
        if self.distribution {
            v.push("distribution-restricted");
        }
        v
    }

    /// `FileHeader` 스트림 256바이트를 해석한다.
    ///
    /// 레이아웃 (rhwp `src/parser/header.rs`):
    /// - `0..32`  시그니처 (`HWP Document File` + NULL 패딩)
    /// - `32..36` 버전. 바이트 순서는 **revision, build, minor, major**
    /// - `36..40` 속성 플래그 (u32 LE)
    /// - `40..44` 라이선스 (예약), `44..256` 예약
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 40 {
            return None;
        }
        // 시그니처는 NULL 패딩을 떼고 비교한다.
        let sig_area = &data[0..32];
        let end = sig_area.iter().position(|b| *b == 0).unwrap_or(32);
        if !sig_area[..end].starts_with(HWP5_SIGNATURE) {
            return None;
        }
        let flags = u32::from_le_bytes([data[36], data[37], data[38], data[39]]);
        Some(Self {
            revision: data[32],
            build: data[33],
            minor: data[34],
            major: data[35],
            compressed: flags & 0x01 != 0,
            encrypted: flags & 0x02 != 0,
            distribution: flags & 0x04 != 0,
            drm: flags & 0x10 != 0,
            public_key_encrypted: flags & 0x100 != 0,
        })
    }
}

/// 판별된 입력 포맷.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceFormat {
    /// HWPX (ZIP + OWPML). 우리 파서가 직접 처리한다.
    Hwpx,
    /// HWP 5.x (CFB). 외부 변환기가 필요하다.
    Hwp5(Hwp5Info),
    /// HWP 3.0 (평문). 외부 변환기가 필요하다.
    Hwp3,
}

impl SourceFormat {
    /// HWPX로 변환하는 단계가 필요한지.
    pub fn needs_conversion(&self) -> bool {
        !matches!(self, Self::Hwpx)
    }

    /// 진단용 짧은 이름.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hwpx => "hwpx",
            Self::Hwp5(_) => "hwp5",
            Self::Hwp3 => "hwp3",
        }
    }
}

/// 파일 경로로 판별한다.
pub fn detect_path(path: &Path) -> Result<SourceFormat> {
    let file = std::fs::File::open(path)
        .map_err(|e| PluginError::corrupt(format!("cannot open {}: {e}", path.display())))?;
    detect_reader(std::io::BufReader::new(file))
}

/// 리더로 판별한다. 필요한 만큼만 읽고 되돌린다.
pub fn detect_reader<R: Read + Seek>(mut reader: R) -> Result<SourceFormat> {
    let mut head = [0u8; 32];
    let read = read_up_to(&mut reader, &mut head)?;
    let head = &head[..read];

    // 1. HWP 3.0 — 파일 선두에서만 인정한다.
    if head.starts_with(HWP3_SIGNATURE) {
        return Ok(SourceFormat::Hwp3);
    }

    // 2. CFB → HWP 5.x
    if head.starts_with(&CFB_SIGNATURE) {
        reader.seek(SeekFrom::Start(0))?;
        return detect_cfb(reader);
    }

    // 3. ZIP → HWPX 인지 확인
    if is_zip_like(head) {
        reader.seek(SeekFrom::Start(0))?;
        return detect_zip(reader);
    }

    Err(PluginError::corrupt(format!(
        "unrecognized format (first bytes: {})",
        hex_prefix(head, 8)
    )))
}

/// CFB 컨테이너에서 `FileHeader` 스트림을 읽어 HWP 5.x인지 확인한다.
///
/// `.doc`/`.xls`도 CFB이므로 시그니처만으로는 부족하다.
fn detect_cfb<R: Read + Seek>(reader: R) -> Result<SourceFormat> {
    let mut comp = cfb::CompoundFile::open(reader)
        .map_err(|e| PluginError::corrupt(format!("not a readable CFB container: {e}")))?;

    let mut stream = comp.open_stream(FILE_HEADER_PATH).map_err(|_| {
        PluginError::corrupt(
            "CFB container without a FileHeader stream — not an HWP document \
             (a .doc or .xls file uses the same container)",
        )
    })?;

    let mut buf = vec![0u8; FILE_HEADER_SIZE];
    let read = read_up_to(&mut stream, &mut buf)?;
    buf.truncate(read);

    match Hwp5Info::parse(&buf) {
        Some(info) => Ok(SourceFormat::Hwp5(info)),
        None => Err(PluginError::corrupt(
            "FileHeader stream does not carry the HWP signature",
        )),
    }
}

/// ZIP이 HWPX인지 확인한다. `.docx`/`.xlsx`도 ZIP이므로 내용을 봐야 한다.
fn detect_zip<R: Read + Seek>(reader: R) -> Result<SourceFormat> {
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| PluginError::corrupt(format!("not a readable zip container: {e}")))?;

    // mimetype이 있으면 그것이 가장 확실하다.
    if let Ok(mut f) = archive.by_name(HWPX_MIMETYPE_ENTRY) {
        let mut bytes = Vec::new();
        f.by_ref()
            .take(MAX_MIMETYPE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MIMETYPE_BYTES {
            return Err(PluginError::corrupt(
                "resource limit exceeded: mimetype entry is too large",
            ));
        }
        let s = String::from_utf8_lossy(&bytes);
        let s = s.trim();
        if s == HWPX_MIMETYPE_VALUE {
            return Ok(SourceFormat::Hwpx);
        }
        if !s.is_empty() {
            return Err(PluginError::corrupt(format!(
                "zip container with unexpected mimetype {s:?} (expected {:?})",
                HWPX_MIMETYPE_VALUE
            )));
        }
    }

    // mimetype을 빠뜨린 생성기가 있다. 섹션 XML 존재로 판정한다.
    let has_section = archive
        .file_names()
        .any(|n| n.starts_with(HWPX_SECTION_PREFIX) && n.ends_with(".xml"));
    if has_section {
        return Ok(SourceFormat::Hwpx);
    }

    Err(PluginError::corrupt(
        "zip container without Contents/section*.xml — not an HWPX document \
         (a .docx or .xlsx file uses the same container)",
    ))
}

/// ZIP 시그니처 계열인지. 빈 아카이브·분할 아카이브도 인정한다.
fn is_zip_like(head: &[u8]) -> bool {
    head.len() >= 4
        && head[0] == ZIP_SIGNATURE[0]
        && head[1] == ZIP_SIGNATURE[1]
        && ZIP_THIRD_BYTES.contains(&head[2])
}

/// EOF를 오류로 보지 않고 읽은 만큼 돌려준다.
fn read_up_to<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0usize;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(PluginError::corrupt(format!("read error: {e}"))),
        }
    }
    Ok(filled)
}

fn hex_prefix(bytes: &[u8], n: usize) -> String {
    bytes
        .iter()
        .take(n)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// 최소한의 HWPX ZIP.
    fn hwpx_bytes(mimetype: Option<&str>, with_section: bool) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            if let Some(m) = mimetype {
                zip.start_file("mimetype", opts).expect("start");
                zip.write_all(m.as_bytes()).expect("write");
            }
            if with_section {
                zip.start_file("Contents/section0.xml", opts)
                    .expect("start");
                zip.write_all(b"<hs:sec/>").expect("write");
            }
            zip.finish().expect("finish");
        }
        cursor.into_inner()
    }

    /// 최소한의 CFB + FileHeader 스트림.
    fn cfb_bytes(signature: &[u8], version: [u8; 4], flags: u32) -> Vec<u8> {
        let mut header = vec![0u8; FILE_HEADER_SIZE];
        let n = signature.len().min(32);
        header[..n].copy_from_slice(&signature[..n]);
        header[32..36].copy_from_slice(&version);
        header[36..40].copy_from_slice(&flags.to_le_bytes());

        let cursor = Cursor::new(Vec::new());
        let mut comp = cfb::CompoundFile::create(cursor).expect("create cfb");
        {
            let mut s = comp.create_stream(FILE_HEADER_PATH).expect("stream");
            s.write_all(&header).expect("write header");
        }
        comp.into_inner().into_inner()
    }

    /// FileHeader가 없는 CFB (.doc 흉내).
    fn cfb_without_file_header() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut comp = cfb::CompoundFile::create(cursor).expect("create cfb");
        {
            let mut s = comp.create_stream("/WordDocument").expect("stream");
            s.write_all(b"not hwp").expect("write");
        }
        comp.into_inner().into_inner()
    }

    #[test]
    fn detects_hwpx_by_mimetype() {
        let b = hwpx_bytes(Some("application/hwp+zip"), true);
        assert_eq!(
            detect_reader(Cursor::new(b)).expect("detects"),
            SourceFormat::Hwpx
        );
    }

    #[test]
    fn detects_hwpx_without_mimetype_via_section() {
        // mimetype을 빠뜨리는 생성기가 있다.
        let b = hwpx_bytes(None, true);
        assert_eq!(
            detect_reader(Cursor::new(b)).expect("detects"),
            SourceFormat::Hwpx
        );
    }

    #[test]
    fn rejects_zip_that_is_not_hwpx() {
        // .docx 도 ZIP이다. 섹션도 mimetype도 없으면 거부해야 한다.
        let b = hwpx_bytes(None, false);
        let e = detect_reader(Cursor::new(b)).expect_err("must reject");
        assert!(e.message.contains("not an HWPX"), "got: {}", e.message);
    }

    #[test]
    fn rejects_zip_with_wrong_mimetype() {
        let b = hwpx_bytes(Some("application/epub+zip"), true);
        let e = detect_reader(Cursor::new(b)).expect_err("must reject");
        assert!(
            e.message.contains("unexpected mimetype"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn detects_hwp5_and_reads_version_and_flags() {
        // 버전 바이트 순서는 revision, build, minor, major (rhwp header.rs)
        // 0x01 압축 + 0x02 암호
        let b = cfb_bytes(HWP5_SIGNATURE, [0, 1, 1, 5], 0x03);
        let f = detect_reader(Cursor::new(b)).expect("detects");
        let SourceFormat::Hwp5(info) = f else {
            panic!("expected hwp5, got {f:?}");
        };
        assert_eq!(info.version_string(), "5.1.1.0");
        assert!(info.compressed);
        assert!(info.encrypted);
        assert!(!info.drm);
        assert!(info.is_protected());
        assert!(info.protection_notes().contains(&"password-encrypted"));
    }

    #[test]
    fn hwp5_without_protection_flags_is_not_protected() {
        let b = cfb_bytes(HWP5_SIGNATURE, [0, 0, 0, 5], 0x01);
        let SourceFormat::Hwp5(info) = detect_reader(Cursor::new(b)).expect("detects") else {
            panic!("expected hwp5");
        };
        assert!(info.compressed);
        assert!(!info.is_protected());
        assert!(info.protection_notes().is_empty());
        assert_eq!(info.version_string(), "5.0.0.0");
    }

    #[test]
    fn rejects_cfb_without_file_header() {
        // .doc / .xls 도 CFB다.
        let e = detect_reader(Cursor::new(cfb_without_file_header())).expect_err("must reject");
        assert!(
            e.message.contains("not an HWP document"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn rejects_cfb_with_wrong_signature() {
        let b = cfb_bytes(b"SOMETHING ELSE", [0, 0, 0, 5], 0);
        let e = detect_reader(Cursor::new(b)).expect_err("must reject");
        assert!(e.message.contains("HWP signature"), "got: {}", e.message);
    }

    #[test]
    fn detects_hwp3_only_at_the_start_of_the_file() {
        let mut b = HWP3_SIGNATURE.to_vec();
        b.extend_from_slice(&[0u8; 64]);
        assert_eq!(
            detect_reader(Cursor::new(b)).expect("detects"),
            SourceFormat::Hwp3
        );

        // 앞에 다른 바이트가 붙으면 HWP3이 아니다.
        let mut shifted = vec![0u8, 1, 2, 3];
        shifted.extend_from_slice(HWP3_SIGNATURE);
        assert!(detect_reader(Cursor::new(shifted)).is_err());
    }

    #[test]
    fn hwp5_file_header_string_does_not_trip_hwp3_detection() {
        // CFB 파일의 FileHeader 스트림에도 "HWP Document File"이 들어 있다.
        // 파일 선두에서만 HWP3을 인정하므로 충돌하지 않아야 한다.
        let b = cfb_bytes(HWP5_SIGNATURE, [0, 0, 0, 5], 0);
        assert!(matches!(
            detect_reader(Cursor::new(b)).expect("detects"),
            SourceFormat::Hwp5(_)
        ));
    }

    #[test]
    fn rejects_unrecognized_bytes_with_a_hex_hint() {
        let e =
            detect_reader(Cursor::new(b"this is plain text".to_vec())).expect_err("must reject");
        assert!(
            e.message.contains("unrecognized format"),
            "got: {}",
            e.message
        );
        // 진단에 실제 바이트를 남겨야 원인 추적이 된다.
        assert!(e.message.contains("74 68 69 73"), "got: {}", e.message);
    }

    #[test]
    fn handles_files_shorter_than_the_probe() {
        let e = detect_reader(Cursor::new(b"PK".to_vec())).expect_err("must reject");
        assert!(!e.message.is_empty());
        assert!(detect_reader(Cursor::new(Vec::new())).is_err());
    }

    #[test]
    fn needs_conversion_only_for_binary_formats() {
        assert!(!SourceFormat::Hwpx.needs_conversion());
        assert!(SourceFormat::Hwp3.needs_conversion());
        let b = cfb_bytes(HWP5_SIGNATURE, [0, 0, 0, 5], 0);
        let f = detect_reader(Cursor::new(b)).expect("detects");
        assert!(f.needs_conversion());
        assert_eq!(f.label(), "hwp5");
    }
}
