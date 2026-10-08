use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use officecli_hwpx::owpml::editor::{
    read_text_node, replace_text_node, rewrite_and_verify, MutationPlan, PackageBaseline,
    SemanticExpectation, TextNodeSelector,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const SECTION_PART: &str = "Contents/section0.xml";
const VERSION: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" tagetApplication="WORDPROCESSOR" major="5" minor="0" micro="5" buildNumber="0" xmlVersion="1.4" application="OfficeCLI" appVersion="0.1.0"/>"#;
const META_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?><odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;
const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#;
const HPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/><opf:item id="blob" href="BinData/blob.bin" media-type="application/octet-stream"/></opf:manifest><opf:spine><opf:itemref idref="header"/><opf:itemref idref="section0"/></opf:spine></opf:package>"#;
const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" version="1.4" secCnt="1"><hh:refList><hh:charProperties itemCnt="1"><hh:charPr id="0" height="1000"/></hh:charProperties><hh:paraProperties itemCnt="1"><hh:paraPr id="0"/></hh:paraProperties></hh:refList></hh:head>"#;

fn section(text: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p id="7" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>{text}</hp:t></hp:run><hp:linesegarray><hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray></hp:p></hs:sec>"#
    )
}

fn build_package(section_xml: &str) -> Vec<u8> {
    build_package_with_extra_field_on(section_xml, None)
}

fn build_package_with_extra_field_on(section_xml: &str, extra_part: Option<&str>) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        writer.set_comment("cow-fixture").expect("set ZIP comment");
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let options_for = |name: &str| {
            let mut options = deflated.into_full_options();
            if extra_part == Some(name) {
                options
                    .add_extra_data(0xbeef, b"opaque-extra", false)
                    .expect("add vendor extra field");
            }
            options
        };

        writer
            .start_file("mimetype", stored)
            .expect("start mimetype");
        writer
            .write_all(b"application/hwp+zip")
            .expect("write mimetype");

        writer
            .start_file("version.xml", options_for("version.xml"))
            .expect("start version");
        writer.write_all(VERSION.as_bytes()).expect("write version");

        for (name, contents) in [
            ("META-INF/manifest.xml", META_MANIFEST.as_bytes()),
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("Contents/content.hpf", HPF.as_bytes()),
            ("Contents/header.xml", HEADER.as_bytes()),
        ] {
            writer
                .start_file(name, deflated)
                .expect("start fixture entry");
            writer.write_all(contents).expect("write fixture entry");
        }
        writer
            .start_file(SECTION_PART, options_for(SECTION_PART))
            .expect("start section");
        writer
            .write_all(section_xml.as_bytes())
            .expect("write section");
        writer
            .start_file("BinData/blob.bin", stored)
            .expect("start binary");
        writer
            .write_all(b"opaque-binary-payload")
            .expect("write binary");
        writer.finish().expect("finish fixture package");
    }
    cursor.into_inner()
}

fn read_part(package: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("open package");
    let mut file = archive.by_name(name).expect("find part");
    let mut body = Vec::new();
    file.read_to_end(&mut body).expect("read part");
    body
}

fn central_versions(package: &[u8]) -> Vec<[u8; 2]> {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("open package");
    let offsets = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .expect("read entry")
                .central_header_start() as usize
                + 4
        })
        .collect::<Vec<_>>();
    drop(archive);
    offsets
        .into_iter()
        .map(|offset| [package[offset], package[offset + 1]])
        .collect()
}

fn set_non_default_central_versions(package: &mut [u8]) {
    let mut archive = ZipArchive::new(Cursor::new(&*package)).expect("open package");
    let offsets = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .expect("read entry")
                .central_header_start() as usize
                + 4
        })
        .collect::<Vec<_>>();
    drop(archive);
    for offset in offsets {
        package[offset] = 23;
        package[offset + 1] = 3;
    }
}

#[test]
fn surgical_text_edit_changes_only_the_selected_inner_bytes() {
    let original = r#"<?xml version="1.0" encoding="UTF-8"?>
<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
  <hp:p id="7"><hp:run><hp:t>alpha &amp; beta</hp:t></hp:run></hp:p>
  <hp:p id="8"><hp:run><hp:t>untouched</hp:t></hp:run></hp:p>
</hs:sec>"#;
    let selector =
        TextNodeSelector::at_paragraph_with_id(0, "7", 0).expect("valid ordinal selector");

    let updated = replace_text_node(
        original.as_bytes(),
        &selector,
        "alpha & beta",
        "gamma < delta & 끝",
    )
    .expect("surgical replacement");
    let expected = original.replacen("alpha &amp; beta", "gamma &lt; delta &amp; 끝", 1);

    assert_eq!(updated, expected.as_bytes());
}

#[test]
fn ordinal_selector_disambiguates_repeated_hancom_paragraph_ids() {
    let original = r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p id="2147483648"><hp:run><hp:t>first</hp:t></hp:run></hp:p><hp:p id="2147483648"><hp:run><hp:t>second</hp:t></hp:run></hp:p></hs:sec>"#;
    let selector =
        TextNodeSelector::at_paragraph_with_id(1, "2147483648", 0).expect("valid ordinal selector");

    let updated = replace_text_node(original.as_bytes(), &selector, "second", "changed")
        .expect("target the second sentinel-id paragraph");

    assert_eq!(
        updated,
        original.replacen(">second<", ">changed<", 1).as_bytes()
    );
    assert_eq!(
        read_text_node(&updated, &selector).expect("read updated target"),
        "changed"
    );

    let stale_selector =
        TextNodeSelector::at_paragraph_with_id(1, "7", 0).expect("syntactically valid selector");
    let error = replace_text_node(original.as_bytes(), &stale_selector, "second", "changed")
        .expect_err("paragraph id precondition must fail");
    assert!(error.message.contains("expected \"7\""), "{error:?}");
}

#[test]
fn surgical_text_edit_rejects_noops_wrong_preconditions_and_ambiguous_ids() {
    let original = section("alpha");
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");

    let error = replace_text_node(original.as_bytes(), &selector, "wrong", "beta")
        .expect_err("stale expected text must fail");
    assert!(error.message.contains("expected text"), "{error:?}");

    let error = replace_text_node(original.as_bytes(), &selector, "alpha", "alpha")
        .expect_err("successful no-op edit must fail");
    assert!(error.message.contains("no-op"), "{error:?}");

    let error = replace_text_node(original.as_bytes(), &selector, "alpha", "bad\u{1}text")
        .expect_err("XML-forbidden replacement characters must fail");
    assert!(error.message.contains("XML 1.0-forbidden"), "{error:?}");

    let duplicate = original.replacen(
        "</hs:sec>",
        r#"<hp:p id="7"><hp:run><hp:t>other</hp:t></hp:run></hp:p></hs:sec>"#,
        1,
    );
    let error = replace_text_node(duplicate.as_bytes(), &selector, "alpha", "beta")
        .expect_err("duplicate paragraph ids must fail closed");
    assert!(
        error.message.contains("more than one paragraph"),
        "{error:?}"
    );
}

#[test]
fn surgical_text_edit_rejects_namespace_confusion() {
    let confused = r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="urn:not-hancom"><hp:p id="7"><hp:run><hp:t>alpha</hp:t></hp:run></hp:p></hs:sec>"#;
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");

    let error = replace_text_node(confused.as_bytes(), &selector, "alpha", "beta")
        .expect_err("wrong paragraph namespace must not resolve");

    assert!(error.message.contains("does not contain"), "{error:?}");
}

#[test]
fn surgical_text_edit_rejects_non_plain_target_content() {
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");
    for xml in [section("<![CDATA[alpha]]>"), section("alpha<hp:tab/>beta")] {
        let error = replace_text_node(xml.as_bytes(), &selector, "alpha", "beta")
            .expect_err("non-plain target must fail closed");
        assert!(error.message.contains("plain text"), "{error:?}");
    }

    let wrong_parent = r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p id="7"><hp:t>alpha</hp:t></hp:p></hs:sec>"#;
    let error = replace_text_node(wrong_parent.as_bytes(), &selector, "alpha", "beta")
        .expect_err("hp:t outside a direct hp:run must fail closed");
    assert!(error.message.contains("directly under hp:run"), "{error:?}");
}

#[test]
fn cow_rewrites_only_an_exact_planned_part_and_passes_g3() {
    let source_section = section("alpha");
    let source = build_package(&source_section);
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");
    let replacement = replace_text_node(source_section.as_bytes(), &selector, "alpha", "beta")
        .expect("surgical replacement");
    let replacements = BTreeMap::from([(SECTION_PART.to_owned(), replacement.clone())]);
    let plan = MutationPlan::replace_exact(baseline.snapshot(), &replacements)
        .expect("exact replacement plan");

    let (candidate, verified) = rewrite_and_verify(
        &baseline,
        Cursor::new(source),
        Cursor::new(Vec::new()),
        &plan,
        &replacements,
        SemanticExpectation::ExactText {
            part: SECTION_PART,
            selector: &selector,
            expected: "beta",
        },
    )
    .expect("verified COW candidate");

    assert_eq!(read_part(candidate.get_ref(), SECTION_PART), replacement);
    assert!(verified.document().is_none());
}

#[test]
fn cow_noop_uses_the_exact_raw_package_path() {
    let source = build_package(&section("alpha"));
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");

    let (_, verified) = rewrite_and_verify(
        &baseline,
        Cursor::new(source),
        Cursor::new(Vec::new()),
        &MutationPlan::no_op(),
        &BTreeMap::new(),
        SemanticExpectation::Unchanged,
    )
    .expect("verified no-op candidate");

    assert_eq!(verified.snapshot(), baseline.snapshot());
}

#[test]
fn cow_rejects_inexact_plans_key_mismatches_and_changed_sources() {
    let source_section = section("alpha");
    let source = build_package(&source_section);
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");
    let replacement = replace_text_node(source_section.as_bytes(), &selector, "alpha", "beta")
        .expect("surgical replacement");
    let replacements = BTreeMap::from([(SECTION_PART.to_owned(), replacement)]);
    let exact = MutationPlan::replace_exact(baseline.snapshot(), &replacements)
        .expect("exact replacement plan");
    let inexact = MutationPlan::replace_existing(baseline.snapshot(), [SECTION_PART])
        .expect("name-only plan");

    let error = rewrite_and_verify(
        &baseline,
        Cursor::new(source.clone()),
        Cursor::new(Vec::new()),
        &exact,
        &BTreeMap::new(),
        SemanticExpectation::ExactText {
            part: SECTION_PART,
            selector: &selector,
            expected: "beta",
        },
    )
    .expect_err("missing replacement key must fail");
    assert!(error.message.contains("replacement keys"), "{error:?}");

    let error = rewrite_and_verify(
        &baseline,
        Cursor::new(source.clone()),
        Cursor::new(Vec::new()),
        &inexact,
        &replacements,
        SemanticExpectation::ExactText {
            part: SECTION_PART,
            selector: &selector,
            expected: "beta",
        },
    )
    .expect_err("writer requires exact content hashes");
    assert!(error.message.contains("exact replacement"), "{error:?}");

    let changed_source = build_package(&section("changed-before-save"));
    let error = rewrite_and_verify(
        &baseline,
        Cursor::new(changed_source),
        Cursor::new(Vec::new()),
        &MutationPlan::no_op(),
        &BTreeMap::new(),
        SemanticExpectation::Unchanged,
    )
    .expect_err("TOCTOU source change must fail");
    assert!(
        error.message.contains("changed since session open"),
        "{error:?}"
    );
}

#[test]
fn g3_rejects_semantically_matching_but_byte_tampered_replacements() {
    let source = build_package(&section("alpha"));
    let baseline = PackageBaseline::capture(Cursor::new(source)).expect("strict baseline");
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");
    let replacement = section("beta").into_bytes();
    let replacements = BTreeMap::from([(SECTION_PART.to_owned(), replacement)]);
    let plan = MutationPlan::replace_exact(baseline.snapshot(), &replacements)
        .expect("exact replacement plan");
    let tampered = section("beta").replacen("</hs:sec>", "<!--tamper--></hs:sec>", 1);
    let candidate = build_package(&tampered);

    let error = baseline
        .verify_candidate(
            Cursor::new(candidate),
            &plan,
            SemanticExpectation::ExactText {
                part: SECTION_PART,
                selector: &selector,
                expected: "beta",
            },
        )
        .expect_err("matching target text cannot hide other part changes");
    assert!(error.message.contains("exact replacement"), "{error:?}");
}

#[test]
fn cow_fails_closed_when_zip_extra_fields_cannot_be_preserved() {
    let source_section = section("alpha");
    let source = build_package_with_extra_field_on(&source_section, Some(SECTION_PART));
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");
    let replacement = replace_text_node(source_section.as_bytes(), &selector, "alpha", "beta")
        .expect("surgical replacement");
    let replacements = BTreeMap::from([(SECTION_PART.to_owned(), replacement)]);
    let plan = MutationPlan::replace_exact(baseline.snapshot(), &replacements)
        .expect("exact replacement plan");

    let error = rewrite_and_verify(
        &baseline,
        Cursor::new(source),
        Cursor::new(Vec::new()),
        &plan,
        &replacements,
        SemanticExpectation::ExactText {
            part: SECTION_PART,
            selector: &selector,
            expected: "beta",
        },
    )
    .expect_err("unpreservable extra fields on a replaced part must fail before save verification");

    assert!(error.message.contains("ZIP extra fields"), "{error:?}");
}

#[test]
fn cow_preserves_extra_fields_on_unchanged_parts_byte_for_byte() {
    let source_section = section("alpha");
    let source = build_package_with_extra_field_on(&source_section, Some("version.xml"));
    let (candidate, replacement) = edit_section(&source, &source_section);

    assert_eq!(read_part(&candidate, SECTION_PART), replacement);
    assert_eq!(
        raw_headers(&candidate, "version.xml"),
        raw_headers(&source, "version.xml")
    );
}

/// Replace the fixture paragraph text and return the verified candidate bytes.
fn edit_section(source: &[u8], source_section: &str) -> (Vec<u8>, Vec<u8>) {
    let baseline = PackageBaseline::capture(Cursor::new(source.to_vec())).expect("strict baseline");
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");
    let replacement = replace_text_node(source_section.as_bytes(), &selector, "alpha", "beta")
        .expect("surgical replacement");
    let replacements = BTreeMap::from([(SECTION_PART.to_owned(), replacement.clone())]);
    let plan = MutationPlan::replace_exact(baseline.snapshot(), &replacements)
        .expect("exact replacement plan");
    let (candidate, _) = rewrite_and_verify(
        &baseline,
        Cursor::new(source.to_vec()),
        Cursor::new(Vec::new()),
        &plan,
        &replacements,
        SemanticExpectation::ExactText {
            part: SECTION_PART,
            selector: &selector,
            expected: "beta",
        },
    )
    .expect("verified COW candidate");
    (candidate.into_inner(), replacement)
}

/// Local header (fixed part, name, extra) and central record with the offset
/// and, when requested, CRC-32/sizes masked.
fn raw_headers(package: &[u8], name: &str) -> (Vec<u8>, Vec<u8>) {
    raw_headers_masking_payload(package, name, false)
}

fn raw_headers_masking_payload(
    package: &[u8],
    name: &str,
    mask_payload: bool,
) -> (Vec<u8>, Vec<u8>) {
    let (local_start, central_start) = header_offsets(package, name);
    let local_len =
        30 + usize::from(u16::from_le_bytes([
            package[local_start + 26],
            package[local_start + 27],
        ])) + usize::from(u16::from_le_bytes([
            package[local_start + 28],
            package[local_start + 29],
        ]));
    let central_len = 46
        + [28, 30, 32]
            .into_iter()
            .map(|field| {
                usize::from(u16::from_le_bytes([
                    package[central_start + field],
                    package[central_start + field + 1],
                ]))
            })
            .sum::<usize>();
    let mut local = package[local_start..local_start + local_len].to_vec();
    let mut central = package[central_start..central_start + central_len].to_vec();
    central[42..46].fill(0);
    if mask_payload {
        local[14..26].fill(0);
        central[16..28].fill(0);
    }
    (local, central)
}

fn header_offsets(package: &[u8], name: &str) -> (usize, usize) {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("open package");
    let file = archive.by_name(name).expect("find part");
    (
        usize::try_from(file.header_start()).expect("offset"),
        usize::try_from(file.central_header_start()).expect("offset"),
    )
}

/// Rewrite header metadata that the `zip` writer cannot reproduce itself.
fn set_producer_metadata(package: &mut [u8], name: &str, external: u32, flags: Option<u16>) {
    let (local, central) = header_offsets(package, name);
    // version made by: MS-DOS/FAT, 2.0 (as Python's zipfile writes on Windows).
    package[central + 4..central + 6].copy_from_slice(&[20, 0]);
    package[central + 38..central + 42].copy_from_slice(&external.to_le_bytes());
    if let Some(flags) = flags {
        package[local + 6..local + 8].copy_from_slice(&flags.to_le_bytes());
        package[central + 8..central + 10].copy_from_slice(&flags.to_le_bytes());
    }
}

#[test]
fn cow_preserves_producer_zip_metadata_that_the_zip_writer_normalizes() {
    let source_section = section("alpha");
    let mut source = build_package(&source_section);
    let names = [
        "mimetype",
        "version.xml",
        "META-INF/manifest.xml",
        "META-INF/container.xml",
        "Contents/content.hpf",
        "Contents/header.xml",
        SECTION_PART,
        "BinData/blob.bin",
    ];
    for name in names {
        // Python's default `0o600 << 16` lacks the regular-file type nibble;
        // Hancom 2020 sets the DOS archive bit (0x20) and deflate option flag 0x4.
        let (external, flags) = match name {
            "mimetype" => (0o600 << 16, None),
            "BinData/blob.bin" => ((0o100600 << 16) | 0x20, None),
            _ => ((0o100600 << 16) | 0x20, Some(0x0004)),
        };
        set_producer_metadata(&mut source, name, external, flags);
    }
    let before = names.map(|name| raw_headers_masking_payload(&source, name, name == SECTION_PART));

    let (candidate, replacement) = edit_section(&source, &source_section);

    assert_eq!(read_part(&candidate, SECTION_PART), replacement);
    let after =
        names.map(|name| raw_headers_masking_payload(&candidate, name, name == SECTION_PART));
    for ((name, before), after) in names.iter().zip(before).zip(after) {
        assert_eq!(before, after, "{name} header metadata changed");
    }
    for name in names.iter().filter(|name| **name != SECTION_PART) {
        assert_eq!(read_part(&candidate, name), read_part(&source, name));
    }
}

#[test]
fn cow_preserves_data_descriptor_layouts() {
    let source_section = section("alpha");
    let archive = ZipArchive::new(Cursor::new(build_package(&source_section))).expect("fixture");
    let mut stream = ZipWriter::new_stream(Vec::new());
    stream.set_comment("cow-fixture").expect("set ZIP comment");
    let mut archive = archive;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("fixture entry");
        let options = SimpleFileOptions::default().compression_method(file.compression());
        let name = file.name().to_owned();
        let mut body = Vec::new();
        file.read_to_end(&mut body).expect("fixture body");
        stream
            .start_file(name, options)
            .expect("start streamed entry");
        stream.write_all(&body).expect("write streamed entry");
    }
    let source = stream.finish().expect("finish stream").into_inner();
    let (local, _) = header_offsets(&source, SECTION_PART);
    let section_flags = u16::from_le_bytes([source[local + 6], source[local + 7]]);
    assert_ne!(
        section_flags & 0x0008,
        0,
        "fixture must use data descriptors"
    );

    let (candidate, replacement) = edit_section(&source, &source_section);

    assert_eq!(read_part(&candidate, SECTION_PART), replacement);
    assert_eq!(
        raw_headers(&candidate, "BinData/blob.bin"),
        raw_headers(&source, "BinData/blob.bin")
    );
    let (candidate_local, _) = raw_headers(&candidate, SECTION_PART);
    let (source_local, _) = raw_headers(&source, SECTION_PART);
    assert_eq!(
        candidate_local, source_local,
        "descriptor-mode local header must keep its zero placeholders"
    );
}

#[test]
fn g3_rejects_raw_metadata_changes_that_decoded_fields_cannot_see() {
    let source = build_package(&section("alpha"));
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");
    for (label, patch) in [
        ("DOS attribute bit", (38usize, 0x20u8)),
        ("general-purpose option flag", (8, 0x04)),
        ("internal attributes", (36, 0x01)),
    ] {
        let mut candidate = source.clone();
        let (_, central) = header_offsets(&candidate, "version.xml");
        candidate[central + patch.0] ^= patch.1;
        let error = baseline
            .verify_candidate(
                Cursor::new(candidate),
                &MutationPlan::no_op(),
                SemanticExpectation::Unchanged,
            )
            .expect_err(label);
        assert!(error.message.contains("version.xml"), "{label}: {error:?}");
    }
}

#[test]
fn cow_restores_non_default_version_made_by_metadata() {
    let source_section = section("alpha");
    let mut source = build_package(&source_section);
    set_non_default_central_versions(&mut source);
    let source_versions = central_versions(&source);
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");
    let selector = TextNodeSelector::new("7", 0).expect("valid selector");
    let replacement = replace_text_node(source_section.as_bytes(), &selector, "alpha", "beta")
        .expect("surgical replacement");
    let replacements = BTreeMap::from([(SECTION_PART.to_owned(), replacement)]);
    let plan = MutationPlan::replace_exact(baseline.snapshot(), &replacements)
        .expect("exact replacement plan");

    let (candidate, _) = rewrite_and_verify(
        &baseline,
        Cursor::new(source),
        Cursor::new(Vec::new()),
        &plan,
        &replacements,
        SemanticExpectation::ExactText {
            part: SECTION_PART,
            selector: &selector,
            expected: "beta",
        },
    )
    .expect("verified COW candidate");

    assert_eq!(central_versions(candidate.get_ref()), source_versions);
}
