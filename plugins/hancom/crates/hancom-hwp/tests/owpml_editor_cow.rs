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
    build_package_with_extra_field(section_xml, false)
}

fn build_package_with_extra_field(section_xml: &str, add_extra_field: bool) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        writer.set_comment("cow-fixture").expect("set ZIP comment");
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        writer
            .start_file("mimetype", stored)
            .expect("start mimetype");
        writer
            .write_all(b"application/hwp+zip")
            .expect("write mimetype");

        let mut version_options = deflated.into_full_options();
        if add_extra_field {
            version_options
                .add_extra_data(0xbeef, b"opaque-extra", false)
                .expect("add vendor extra field");
        }
        writer
            .start_file("version.xml", version_options)
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
            .start_file(SECTION_PART, deflated)
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
    let source = build_package_with_extra_field(&source_section, true);
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
    .expect_err("unpreservable extra fields must fail before save verification");

    assert!(error.message.contains("ZIP extra fields"), "{error:?}");
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
