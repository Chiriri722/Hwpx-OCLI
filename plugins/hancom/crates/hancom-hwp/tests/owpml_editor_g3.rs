use std::io::{Cursor, Write};

use officecli_hwpx::owpml::editor::{
    copy_package, MutationPlan, PackageBaseline, SemanticExpectation,
};
use officecli_hwpx::owpml::model::{Block, Inline};
use officecli_hwpx::owpml::read_document_from;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const VERSION: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" tagetApplication="WORDPROCESSOR" major="5" minor="0" micro="5" buildNumber="0" xmlVersion="1.4" application="OfficeCLI" appVersion="0.1.0"/>"#;
const META_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?><odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;
const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#;
const HPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/><opf:item id="blob" href="BinData/blob.bin" media-type="application/octet-stream"/></opf:manifest><opf:spine><opf:itemref idref="header"/><opf:itemref idref="section0"/></opf:spine></opf:package>"#;
const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" version="1.4" secCnt="1"><hh:refList><hh:charProperties itemCnt="1"><hh:charPr id="0" height="1000"/></hh:charProperties><hh:paraProperties itemCnt="1"><hh:paraPr id="0"/></hh:paraProperties></hh:refList></hh:head>"#;

fn section(text: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p id="0" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>{text}</hp:t></hp:run><hp:linesegarray><hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray></hp:p></hs:sec>"#
    )
}

fn build_package(text: &str, binary: &[u8], binary_method: CompressionMethod) -> Vec<u8> {
    build_package_with_section_method(text, binary, binary_method, CompressionMethod::Deflated)
}

fn build_package_with_section_method(
    text: &str,
    binary: &[u8],
    binary_method: CompressionMethod,
    section_method: CompressionMethod,
) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        writer.set_comment("g3-fixture").expect("set ZIP comment");
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        for (name, contents, options) in [
            ("mimetype", b"application/hwp+zip".as_slice(), stored),
            ("version.xml", VERSION.as_bytes(), deflated),
            ("META-INF/manifest.xml", META_MANIFEST.as_bytes(), deflated),
            ("META-INF/container.xml", CONTAINER.as_bytes(), deflated),
            ("Contents/content.hpf", HPF.as_bytes(), deflated),
            ("Contents/header.xml", HEADER.as_bytes(), deflated),
        ] {
            writer
                .start_file(name, options)
                .expect("start fixture entry");
            writer.write_all(contents).expect("write fixture entry");
        }
        writer
            .start_file(
                "Contents/section0.xml",
                SimpleFileOptions::default().compression_method(section_method),
            )
            .expect("start section");
        writer
            .write_all(section(text).as_bytes())
            .expect("write section");
        writer
            .start_file(
                "BinData/blob.bin",
                SimpleFileOptions::default().compression_method(binary_method),
            )
            .expect("start binary");
        writer.write_all(binary).expect("write binary");
        writer.finish().expect("finish fixture package");
    }
    cursor.into_inner()
}

#[test]
fn noop_copy_preserves_snapshot_and_reopens_with_identical_semantics() {
    let source = build_package(
        "alpha",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");
    let candidate = copy_package(Cursor::new(source), Cursor::new(Vec::new()))
        .expect("raw package copy")
        .into_inner();

    let verified = baseline
        .verify_candidate(
            Cursor::new(candidate),
            &MutationPlan::no_op(),
            SemanticExpectation::Unchanged,
        )
        .expect("G3 no-op verification");

    assert_eq!(verified.snapshot(), baseline.snapshot());
    assert!(verified.document().is_none());
}

#[test]
fn g3_rejects_unplanned_payload_or_compression_changes() {
    let source = build_package(
        "alpha",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );
    let baseline = PackageBaseline::capture(Cursor::new(source)).expect("strict baseline");

    let changed_payload = build_package(
        "alpha",
        b"different-binary-payload",
        CompressionMethod::Deflated,
    );
    let error = baseline
        .verify_candidate(
            Cursor::new(changed_payload),
            &MutationPlan::no_op(),
            SemanticExpectation::Unchanged,
        )
        .expect_err("unplanned payload change must fail");
    assert!(error.message.contains("BinData/blob.bin"), "{error:?}");

    let recompressed = build_package("alpha", b"opaque-binary-payload", CompressionMethod::Stored);
    let error = baseline
        .verify_candidate(
            Cursor::new(recompressed),
            &MutationPlan::no_op(),
            SemanticExpectation::Unchanged,
        )
        .expect_err("unplanned raw representation change must fail");
    assert!(error.message.contains("BinData/blob.bin"), "{error:?}");
}

#[test]
fn g3_requires_each_planned_part_to_change() {
    let source = build_package(
        "alpha",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );
    let baseline = PackageBaseline::capture(Cursor::new(source.clone())).expect("strict baseline");
    let plan = MutationPlan::replace_existing(baseline.snapshot(), ["Contents/section0.xml"])
        .expect("valid plan");

    let error = baseline
        .verify_candidate(Cursor::new(source), &plan, SemanticExpectation::Unchanged)
        .expect_err("successful no-op mutation must fail");
    assert!(error.message.contains("did not change"), "{error:?}");
}

#[test]
fn g3_rejects_preserved_metadata_changes_even_for_a_planned_part() {
    let source = build_package(
        "alpha",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );
    let baseline = PackageBaseline::capture(Cursor::new(source)).expect("strict baseline");
    let plan = MutationPlan::replace_existing(baseline.snapshot(), ["Contents/section0.xml"])
        .expect("valid plan");
    let candidate = build_package_with_section_method(
        "beta",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
        CompressionMethod::Stored,
    );

    let error = baseline
        .verify_candidate(
            Cursor::new(candidate),
            &plan,
            SemanticExpectation::ExactDocument(
                &read_document_from(Cursor::new(build_package(
                    "beta",
                    b"opaque-binary-payload",
                    CompressionMethod::Deflated,
                )))
                .expect("expected known semantics"),
            ),
        )
        .expect_err("planned entries must preserve non-payload ZIP metadata");
    assert!(
        error.message.contains("preserved ZIP metadata"),
        "{error:?}"
    );
}

#[test]
fn g3_reopens_candidate_and_rejects_an_unexpected_known_semantic_delta() {
    let source = build_package(
        "alpha",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );
    let expected = read_document_from(Cursor::new(source.clone())).expect("known semantics");
    let baseline = PackageBaseline::capture(Cursor::new(source)).expect("strict baseline");
    let plan = MutationPlan::replace_existing(baseline.snapshot(), ["Contents/section0.xml"])
        .expect("valid plan");
    let candidate = build_package(
        "beta",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );

    let error = baseline
        .verify_candidate(
            Cursor::new(candidate),
            &plan,
            SemanticExpectation::ExactDocument(&expected),
        )
        .expect_err("unexpected semantic change must fail");
    assert!(error.message.contains("known semantics"), "{error:?}");
}

#[test]
fn g3_accepts_only_the_exact_requested_known_semantic_delta() {
    let source = build_package(
        "alpha",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );
    let mut expected = read_document_from(Cursor::new(source.clone())).expect("known semantics");
    let baseline = PackageBaseline::capture(Cursor::new(source)).expect("strict baseline");
    let plan = MutationPlan::replace_existing(baseline.snapshot(), ["Contents/section0.xml"])
        .expect("valid plan");
    let candidate = build_package(
        "beta",
        b"opaque-binary-payload",
        CompressionMethod::Deflated,
    );
    let Block::Paragraph(paragraph) = &mut expected.sections[0].blocks[0] else {
        panic!("fixture first block must be a paragraph");
    };
    let Inline::Text(run) = &mut paragraph.inlines[0] else {
        panic!("fixture first inline must be text");
    };
    run.text = "beta".to_owned();

    baseline
        .verify_candidate(
            Cursor::new(candidate),
            &plan,
            SemanticExpectation::ExactDocument(&expected),
        )
        .expect("exact semantic delta must pass G3");
}
