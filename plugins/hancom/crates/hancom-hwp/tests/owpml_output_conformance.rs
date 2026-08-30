use std::io::{Cursor, Write};

use officecli_hwpx::error::ErrorCode;
use officecli_hwpx::owpml::conformance::validate_output_package;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const VERSION: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" tagetApplication="WORDPROCESSOR" major="5" minor="0" micro="5" buildNumber="0" xmlVersion="1.4" application="OfficeCLI" appVersion="0.1.0"/>"#;
const META_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?><odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;
const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#;
const HPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/></opf:manifest><opf:spine><opf:itemref idref="header"/><opf:itemref idref="section0"/></opf:spine></opf:package>"#;
const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" version="1.4" secCnt="1"><hh:refList/></hh:head>"#;
const SECTION: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section"/>"#;

#[derive(Clone)]
struct Entry<'a> {
    name: &'a str,
    contents: &'a [u8],
    method: CompressionMethod,
    unix_permissions: Option<u32>,
    symlink_target: Option<&'a str>,
}

impl<'a> Entry<'a> {
    fn stored(name: &'a str, contents: &'a str) -> Self {
        Self {
            name,
            contents: contents.as_bytes(),
            method: CompressionMethod::Stored,
            unix_permissions: None,
            symlink_target: None,
        }
    }

    fn deflated(name: &'a str, contents: &'a str) -> Self {
        Self {
            name,
            contents: contents.as_bytes(),
            method: CompressionMethod::Deflated,
            unix_permissions: None,
            symlink_target: None,
        }
    }
}

fn canonical_entries() -> Vec<Entry<'static>> {
    vec![
        Entry::stored("mimetype", "application/hwp+zip"),
        Entry::deflated("version.xml", VERSION),
        Entry::deflated("META-INF/manifest.xml", META_MANIFEST),
        Entry::deflated("META-INF/container.xml", CONTAINER),
        Entry::deflated("Contents/content.hpf", HPF),
        Entry::deflated("Contents/header.xml", HEADER),
        Entry::deflated("Contents/section0.xml", SECTION),
    ]
}

fn build(entries: &[Entry<'_>]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        for entry in entries {
            let mut options = SimpleFileOptions::default().compression_method(entry.method);
            if let Some(mode) = entry.unix_permissions {
                options = options.unix_permissions(mode);
            }
            if let Some(target) = entry.symlink_target {
                writer
                    .add_symlink(entry.name, target, options)
                    .expect("add fixture symlink");
                continue;
            }
            writer
                .start_file(entry.name, options)
                .expect("start fixture entry");
            writer
                .write_all(entry.contents)
                .expect("write fixture entry");
        }
        writer.finish().expect("finish fixture archive");
    }
    cursor.into_inner()
}

fn strict_error(entries: &[Entry<'_>]) -> officecli_hwpx::error::PluginError {
    validate_output_package(Cursor::new(build(entries))).expect_err("strict gate must reject")
}

#[test]
fn canonical_output_package_passes_strict_gate() {
    validate_output_package(Cursor::new(build(&canonical_entries())))
        .expect("canonical writer output");
}

#[test]
fn mimetype_must_be_first_stored_and_byte_exact() {
    let mut not_first = canonical_entries();
    not_first.swap(0, 1);
    assert!(strict_error(&not_first).message.contains("first entry"));

    let mut compressed = canonical_entries();
    compressed[0].method = CompressionMethod::Deflated;
    assert!(strict_error(&compressed).message.contains("stored"));

    let mut padded = canonical_entries();
    padded[0] = Entry::stored("mimetype", "application/hwp+zip\n");
    assert!(strict_error(&padded).message.contains("exactly"));
}

#[test]
fn required_package_parts_must_all_exist() {
    for required in [
        "version.xml",
        "META-INF/manifest.xml",
        "META-INF/container.xml",
        "Contents/content.hpf",
        "Contents/header.xml",
        "Contents/section0.xml",
    ] {
        let entries: Vec<_> = canonical_entries()
            .into_iter()
            .filter(|entry| entry.name != required)
            .collect();
        let error = strict_error(&entries);
        assert_eq!(error.code, ErrorCode::CorruptInput, "{required}");
        assert!(error.message.contains(required), "{required}: {error:?}");
    }
}

#[test]
fn package_paths_must_be_portable_and_unambiguous() {
    for unsafe_name in [
        "../escape.xml",
        "/absolute.xml",
        "Contents\\backslash.xml",
        "C:/drive.xml",
        "Contents/file.xml:stream",
        "Contents/./dot.xml",
        "Contents/trailing-dot.",
        "Contents/trailing-space ",
        "Contents/CON",
    ] {
        let mut entries = canonical_entries();
        entries.push(Entry::deflated(unsafe_name, "<root/>"));
        let error = strict_error(&entries);
        assert!(error.message.contains("unsafe"), "{unsafe_name}: {error:?}");
    }

    let mut colliding = canonical_entries();
    colliding.push(Entry::stored("BinData/a.bin", "one"));
    colliding.push(Entry::stored("bindata/A.bin", "two"));
    assert!(strict_error(&colliding)
        .message
        .contains("case-insensitive"));

    let mut unicode_colliding = canonical_entries();
    unicode_colliding.push(Entry::stored("BinData/Ä.bin", "one"));
    unicode_colliding.push(Entry::stored("BinData/ä.bin", "two"));
    assert!(strict_error(&unicode_colliding)
        .message
        .contains("case-insensitive"));
}

#[test]
fn symlink_entries_are_not_writer_output() {
    let mut entries = canonical_entries();
    entries.push(Entry {
        name: "BinData/link",
        contents: b"../outside",
        method: CompressionMethod::Stored,
        unix_permissions: Some(0o777),
        symlink_target: Some("../outside"),
    });
    assert!(strict_error(&entries).message.contains("symbolic link"));
}

#[test]
fn every_xml_part_must_be_safe_and_well_formed() {
    for invalid_xml in [
        "<root>",
        "<!DOCTYPE root SYSTEM \"file:///etc/passwd\"><root/>",
        "<?unsafe instruction?><root/>",
        "<root>&external;</root>",
        "<root value=\"&external;\"/>",
        "<root>&#x110000;</root>",
        "<root xmlns:x=\"&external;\"><x:item/></root>",
        "<?xml version=\"1.1\" encoding=\"UTF-8\"?><root/>",
        "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><root/>",
        "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>",
    ] {
        let mut entries = canonical_entries();
        entries.push(Entry::deflated("Contents/extra.xml", invalid_xml));
        let error = strict_error(&entries);
        assert!(
            error.message.contains("Contents/extra.xml"),
            "{invalid_xml}: {error:?}"
        );
    }
}

#[test]
fn container_must_select_the_canonical_hpf_rootfile() {
    let other_container = CONTAINER.replace("Contents/content.hpf", "Contents/other.hpf");
    let mut entries = canonical_entries();
    entries[3] = Entry::deflated("META-INF/container.xml", &other_container);
    let error = strict_error(&entries);
    assert!(error.message.contains("rootfile"), "{error:?}");
}

#[test]
fn every_container_rootfile_must_resolve() {
    let container = CONTAINER.replace(
        "</ocf:rootfiles>",
        r#"<ocf:rootfile full-path="Preview/missing.txt" media-type="text/plain"/></ocf:rootfiles>"#,
    );
    let mut entries = canonical_entries();
    entries[3] = Entry::deflated("META-INF/container.xml", &container);
    let error = strict_error(&entries);
    assert!(error.message.contains("Preview/missing.txt"), "{error:?}");
}

#[test]
fn topology_elements_must_be_in_their_canonical_parents() {
    let container = CONTAINER
        .replace("<ocf:rootfiles>", "<ocf:metadata>")
        .replace("</ocf:rootfiles>", "</ocf:metadata>");
    let mut misplaced_rootfile = canonical_entries();
    misplaced_rootfile[3] = Entry::deflated("META-INF/container.xml", &container);
    let error = strict_error(&misplaced_rootfile);
    assert!(error.message.contains("rootfiles"), "{error:?}");

    let hpf = HPF
        .replace("<opf:manifest>", "<opf:metadata>")
        .replace("</opf:manifest>", "</opf:metadata>");
    let mut misplaced_items = canonical_entries();
    misplaced_items[4] = Entry::deflated("Contents/content.hpf", &hpf);
    let error = strict_error(&misplaced_items);
    assert!(error.message.contains("manifest"), "{error:?}");

    let hpf = HPF
        .replace("<opf:spine>", "<opf:bindings>")
        .replace("</opf:spine>", "</opf:bindings>");
    let mut misplaced_itemrefs = canonical_entries();
    misplaced_itemrefs[4] = Entry::deflated("Contents/content.hpf", &hpf);
    let error = strict_error(&misplaced_itemrefs);
    assert!(error.message.contains("spine"), "{error:?}");
}

#[test]
fn manifest_hrefs_and_spine_ids_must_resolve() {
    let missing_part_hpf = HPF.replace(
        "</opf:manifest>",
        r#"<opf:item id="missing-part" href="BinData/missing.bin" media-type="application/octet-stream"/></opf:manifest>"#,
    );
    let mut missing_part = canonical_entries();
    missing_part[4] = Entry::deflated("Contents/content.hpf", &missing_part_hpf);
    assert!(strict_error(&missing_part)
        .message
        .contains("missing archive entry"));

    let missing_id_hpf = HPF.replace("idref=\"section0\"", "idref=\"missing\"");
    let mut missing_id = canonical_entries();
    missing_id[4] = Entry::deflated("Contents/content.hpf", &missing_id_hpf);
    assert!(strict_error(&missing_id)
        .message
        .contains("unknown manifest id"));
}

#[test]
fn section_manifest_spine_and_header_count_must_agree() {
    let section_one = SECTION;
    let hpf_with_unreferenced_section = HPF.replace(
        "</opf:manifest>",
        r#"<opf:item id="section1" href="Contents/section1.xml" media-type="application/xml"/></opf:manifest>"#,
    );
    let mut unreferenced = canonical_entries();
    unreferenced[4] = Entry::deflated("Contents/content.hpf", &hpf_with_unreferenced_section);
    unreferenced.push(Entry::deflated("Contents/section1.xml", section_one));
    let error = strict_error(&unreferenced);
    assert!(error.message.contains("spine section set"), "{error:?}");

    let mismatched_header = HEADER.replace("secCnt=\"1\"", "secCnt=\"2\"");
    let mut wrong_count = canonical_entries();
    wrong_count[5] = Entry::deflated("Contents/header.xml", &mismatched_header);
    let error = strict_error(&wrong_count);
    assert!(error.message.contains("secCnt"), "{error:?}");

    let section_one_hpf = HPF
        .replace("id=\"section0\"", "id=\"section1\"")
        .replace("idref=\"section0\"", "idref=\"section1\"")
        .replace("Contents/section0.xml", "Contents/section1.xml");
    let mut non_contiguous = canonical_entries();
    non_contiguous[4] = Entry::deflated("Contents/content.hpf", &section_one_hpf);
    non_contiguous[6] = Entry::deflated("Contents/section1.xml", SECTION);
    let error = strict_error(&non_contiguous);
    assert!(error.message.contains("contiguous"), "{error:?}");

    let duplicate_section_ref_hpf = HPF.replace(
        "</opf:spine>",
        r#"<opf:itemref idref="section0"/></opf:spine>"#,
    );
    let mut duplicate_section_ref = canonical_entries();
    duplicate_section_ref[4] = Entry::deflated("Contents/content.hpf", &duplicate_section_ref_hpf);
    let error = strict_error(&duplicate_section_ref);
    assert!(error.message.contains("more than once"), "{error:?}");

    let noncanonical_section_hpf = HPF.replace("section0.xml", "section00.xml");
    let mut noncanonical_section = canonical_entries();
    noncanonical_section[4] = Entry::deflated("Contents/content.hpf", &noncanonical_section_hpf);
    noncanonical_section[6] = Entry::deflated("Contents/section00.xml", SECTION);
    let error = strict_error(&noncanonical_section);
    assert!(
        error.message.contains("canonical section path"),
        "{error:?}"
    );
}

#[test]
fn archive_must_start_with_the_physical_mimetype_local_header() {
    let package = build(&canonical_entries());
    let mut prefixed = b"not-a-zip-preamble".to_vec();
    prefixed.extend(package);
    let error =
        validate_output_package(Cursor::new(prefixed)).expect_err("preamble must be rejected");
    assert!(error.message.contains("physical ZIP entry"), "{error:?}");
}

#[test]
fn all_entry_payloads_are_checksum_verified() {
    let mut entries = canonical_entries();
    entries.push(Entry::stored(
        "BinData/checksum.bin",
        "unique-checksum-payload",
    ));
    let mut bytes = build(&entries);
    let needle = b"unique-checksum-payload";
    let offset = bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("stored payload offset");
    bytes[offset] ^= 0x01;

    let error = validate_output_package(Cursor::new(bytes)).expect_err("CRC mismatch must reject");
    assert!(error.message.contains("checksum.bin"), "{error:?}");
}

#[test]
fn writer_output_cannot_exceed_reader_resource_limits() {
    const MAX_HPF_BYTES: usize = 4 * 1024 * 1024;

    let oversized_hpf = format!("{HPF}{}", " ".repeat(MAX_HPF_BYTES + 1 - HPF.len()));
    let mut oversized = canonical_entries();
    oversized[4] = Entry::stored("Contents/content.hpf", &oversized_hpf);
    let error = strict_error(&oversized);
    assert!(error.message.contains("content.hpf"), "{error:?}");
    assert!(error.message.contains("4194304"), "{error:?}");

    let extra_manifest_items = (0..4097)
        .map(|index| {
            format!(
                r#"<opf:item id="extra-{index}" href="Contents/header.xml" media-type="application/xml"/>"#
            )
        })
        .collect::<String>();
    let manifest_heavy_hpf = HPF.replace(
        "</opf:manifest>",
        &format!("{extra_manifest_items}</opf:manifest>"),
    );
    let mut manifest_heavy = canonical_entries();
    manifest_heavy[4] = Entry::deflated("Contents/content.hpf", &manifest_heavy_hpf);
    let error = strict_error(&manifest_heavy);
    assert!(error.message.contains("manifest item"), "{error:?}");
    assert!(error.message.contains("4096"), "{error:?}");

    let extra_spine_items = r#"<opf:itemref idref="header"/>"#.repeat(2049);
    let spine_heavy_hpf = HPF.replace("</opf:spine>", &format!("{extra_spine_items}</opf:spine>"));
    let mut spine_heavy = canonical_entries();
    spine_heavy[4] = Entry::deflated("Contents/content.hpf", &spine_heavy_hpf);
    let error = strict_error(&spine_heavy);
    assert!(error.message.contains("spine item"), "{error:?}");
    assert!(error.message.contains("2048"), "{error:?}");

    let excessive_section_count = HEADER.replace("secCnt=\"1\"", "secCnt=\"1025\"");
    let mut too_many_sections = canonical_entries();
    too_many_sections[5] = Entry::deflated("Contents/header.xml", &excessive_section_count);
    let error = strict_error(&too_many_sections);
    assert!(error.message.contains("section count"), "{error:?}");
    assert!(error.message.contains("1024"), "{error:?}");
}
