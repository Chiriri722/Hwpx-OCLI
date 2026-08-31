use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Write};
use std::time::{Duration, SystemTime};

use officecli_hwpx::error::ErrorCode;
use officecli_hwpx::ooxml_carrier::{bridge_ooxml, carrier_manifest, run_args, CarrierFamily};
use serde_json::json;
use zip::write::{FullFileOptions, SimpleFileOptions};
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const PACKAGE_RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const APP_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const CORE_PROPERTIES_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const THUMBNAIL_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
const APP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const HANCOM_PUBLIC_SPEC_NOTICE: &str =
    "본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.";

#[derive(Clone, Copy)]
struct Fixture<'a> {
    family: CarrierFamily,
    application: &'a str,
    app_version: &'a str,
    main_xml: Option<&'a str>,
    app_xml: Option<&'a str>,
    content_types_xml: Option<&'a str>,
    relationships_xml: Option<&'a str>,
}

impl Fixture<'_> {
    fn valid(family: CarrierFamily) -> Self {
        match family {
            CarrierFamily::Cell => Self {
                family,
                application: "Cell",
                app_version: "12.0300",
                main_xml: None,
                app_xml: None,
                content_types_xml: None,
                relationships_xml: None,
            },
            CarrierFamily::Show => Self {
                family,
                application: "Show",
                app_version: "12.0000",
                main_xml: None,
                app_xml: None,
                content_types_xml: None,
                relationships_xml: None,
            },
        }
    }
}

fn main_part(family: CarrierFamily) -> (&'static str, &'static str, &'static str) {
    match family {
        CarrierFamily::Cell => (
            "xl/workbook.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fileVersion appName="HCell" lastEdited="12.0"/><calcPr xmlns:hs="http://schemas.haansoft.com/office/spreadsheet/8.0" hs:hclCalcId="1"/></workbook>"#,
        ),
        CarrierFamily::Show => (
            "ppt/presentation.xml",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
            r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#,
        ),
    }
}

fn build_fixture(
    fixture: Fixture<'_>,
    extra_entries: &[(&str, &[u8], CompressionMethod, Option<u32>)],
) -> Vec<u8> {
    build_fixture_with_timestamp_quirk(fixture, extra_entries, false)
}

fn build_fixture_with_timestamp_quirk(
    fixture: Fixture<'_>,
    extra_entries: &[(&str, &[u8], CompressionMethod, Option<u32>)],
    timestamp_quirk: bool,
) -> Vec<u8> {
    let (main_path, main_content_type, default_main) = main_part(fixture.family);
    let default_content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_content_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );
    let content_types = fixture
        .content_types_xml
        .unwrap_or(default_content_types.as_str());
    let default_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/></Relationships>"#
    );
    let relationships = fixture
        .relationships_xml
        .unwrap_or(default_relationships.as_str());
    let default_app = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Properties xmlns="{APP_NS}"><Application>{}</Application><AppVersion>{}</AppVersion></Properties>"#,
        fixture.application, fixture.app_version
    );

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, contents) in [
            ("[Content_Types].xml", content_types.as_bytes()),
            ("_rels/.rels", relationships.as_bytes()),
            (
                "docProps/app.xml",
                fixture.app_xml.unwrap_or(&default_app).as_bytes(),
            ),
            (
                main_path,
                fixture.main_xml.unwrap_or(default_main).as_bytes(),
            ),
        ] {
            if timestamp_quirk && name == "[Content_Types].xml" {
                let mut options =
                    FullFileOptions::default().compression_method(CompressionMethod::Deflated);
                let mut timestamp = [0_u8; 13];
                timestamp[0] = 0b0000_0111;
                options
                    .add_extra_data(0x5455, timestamp, false)
                    .expect("add valid timestamp test field");
                writer
                    .start_file(name, options)
                    .expect("start timestamp fixture entry");
            } else {
                writer
                    .start_file(name, deflated)
                    .expect("start fixture entry");
            }
            writer.write_all(contents).expect("write fixture entry");
        }
        for (name, contents, method, mode) in extra_entries {
            let mut options = SimpleFileOptions::default().compression_method(*method);
            if let Some(mode) = mode {
                options = options.unix_permissions(*mode);
                if mode & 0o170000 == 0o120000 {
                    writer
                        .add_symlink(
                            *name,
                            std::str::from_utf8(contents).expect("UTF-8 symlink target"),
                            options,
                        )
                        .expect("add extra symlink");
                    continue;
                }
            }
            writer
                .start_file(*name, options)
                .expect("start extra entry");
            writer.write_all(contents).expect("write extra entry");
        }
        writer.finish().expect("finish fixture");
    }
    let mut bytes = cursor.into_inner();
    if timestamp_quirk {
        let marker = [0x55, 0x54, 0x0D, 0x00, 0x07];
        let offsets = bytes
            .windows(marker.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == marker).then_some(offset))
            .collect::<Vec<_>>();
        assert_eq!(offsets.len(), 2, "local and central timestamp markers");
        for offset in offsets {
            bytes[offset + 4] = 0b0000_0010;
        }
    }
    bytes
}

#[test]
fn manifests_route_each_extension_to_its_native_ooxml_family() {
    assert_eq!(
        carrier_manifest(CarrierFamily::Cell),
        json!({
            "name": "officecli-hancom-cell",
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": 1,
            "kinds": ["dump-reader"],
            "extensions": [".cell"],
            "target": "xlsx",
            "runtime": "rust",
            "idle_timeout_seconds": {
                "default": 60,
                "verbs": { "dump": 30 }
            },
            "description": format!(
                "Validated byte-preserving bridge for the evidence-backed Hancom Cell 12.0300 OOXML carrier subset. {HANCOM_PUBLIC_SPEC_NOTICE}"
            ),
            "license": "MIT",
            "supports": ["hancom-cell-12.0300-ooxml-carrier-subset", "byte-preserving", "direct-native"]
        })
    );
    assert_eq!(
        carrier_manifest(CarrierFamily::Show)["target"],
        json!("pptx")
    );
    assert_eq!(
        carrier_manifest(CarrierFamily::Show)["extensions"],
        json!([".show"])
    );
    assert_eq!(
        carrier_manifest(CarrierFamily::Show)["supports"],
        json!([
            "hancom-show-12.0000-ooxml-carrier-subset",
            "byte-preserving",
            "direct-native"
        ])
    );
}

#[test]
fn carrier_help_includes_required_hancom_public_spec_notice() {
    for family in [CarrierFamily::Cell, CarrierFamily::Show] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        run_args(
            family,
            vec![OsString::from("--help")],
            &mut stdout,
            &mut stderr,
        )
        .expect("carrier help");

        assert!(stdout.is_empty());
        let help = String::from_utf8(stderr).expect("UTF-8 carrier help");
        assert!(help.contains(HANCOM_PUBLIC_SPEC_NOTICE));
        let exact_profile = match family {
            CarrierFamily::Cell => "Cell 12.0300 OOXML carrier subset",
            CarrierFamily::Show => "Show 12.0000 OOXML carrier subset",
        };
        assert!(help.contains(exact_profile), "help omitted {exact_profile}");
        assert!(!help.contains(" v12 OOXML carrier bridge"));
    }
}

#[test]
fn dump_cli_accepts_common_protocol_options() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("survey.cell");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &[]),
    )
    .expect("write source");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    run_args(
        CarrierFamily::Cell,
        vec![
            OsString::from("dump"),
            source.as_os_str().to_owned(),
            OsString::from("--media-dir"),
            dir.path().join("media").into_os_string(),
            OsString::from("--log-file"),
            dir.path().join("carrier.log").into_os_string(),
            OsString::from("--quiet"),
        ],
        &mut stdout,
        &mut stderr,
    )
    .expect("common options");

    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert!(dir.path().join("survey.xlsx").is_file());
}

#[test]
fn dump_cli_rejects_unknown_options_and_wrong_source_extensions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("survey.show");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &[]),
    )
    .expect("write source");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let unknown = run_args(
        CarrierFamily::Cell,
        vec![
            OsString::from("dump"),
            source.as_os_str().to_owned(),
            OsString::from("--future-option"),
        ],
        &mut stdout,
        &mut stderr,
    )
    .expect_err("unknown option");
    assert_eq!(unknown.code, ErrorCode::InvalidArgument);

    let wrong_extension =
        bridge_ooxml(&source, CarrierFamily::Cell).expect_err("wrong source extension");
    assert_eq!(wrong_extension.code, ErrorCode::InvalidArgument);
    assert!(!dir.path().join("survey.xlsx").exists());
}

#[test]
fn namespace_confused_required_children_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, main_type, _) = main_part(CarrierFamily::Cell);

    let relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}" xmlns:evil="urn:evil"><evil:Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/></Relationships>"#
    );
    let source = dir.path().join("relationship.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                relationships_xml: Some(&relationships),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write relationship fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("foreign Relationship namespace")
            .code,
        ErrorCode::CorruptInput
    );

    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}" xmlns:evil="urn:evil"><evil:Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );
    let source = dir.path().join("content-types.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                content_types_xml: Some(&content_types),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write content-types fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("foreign Override namespace")
            .code,
        ErrorCode::CorruptInput
    );

    let app_xml = format!(
        r#"<Properties xmlns="{APP_NS}" xmlns:evil="urn:evil"><evil:Application>Cell</evil:Application><evil:AppVersion>12.0300</evil:AppVersion></Properties>"#
    );
    let source = dir.path().join("application.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                app_xml: Some(&app_xml),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write application fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("foreign app-properties namespace")
            .code,
        ErrorCode::CorruptInput
    );

    let main_xml = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:evil="urn:evil"><evil:fileVersion appName="HCell"/><calcPr xmlns:hs="http://schemas.haansoft.com/office/spreadsheet/8.0" hs:hclCalcId="1"/></workbook>"#;
    let source = dir.path().join("file-version.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                main_xml: Some(main_xml),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write main fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("foreign fileVersion namespace")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn relationship_and_content_type_metadata_reject_unexpected_attributes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, main_type, _) = main_part(CarrierFamily::Cell);
    let relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}" xml:base="https://example.invalid/"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/></Relationships>"#
    );
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" unexpected="true"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );

    for (name, fixture) in [
        (
            "relationship-xml-base.cell",
            Fixture {
                relationships_xml: Some(&relationships),
                ..Fixture::valid(CarrierFamily::Cell)
            },
        ),
        (
            "content-type-extra-attribute.cell",
            Fixture {
                content_types_xml: Some(&content_types),
                ..Fixture::valid(CarrierFamily::Cell)
            },
        ),
    ] {
        let source = dir.path().join(name);
        fs::write(&source, build_fixture(fixture, &[])).expect("write metadata fixture");

        let error =
            bridge_ooxml(&source, CarrierFamily::Cell).expect_err("unexpected metadata attribute");
        assert_eq!(error.code, ErrorCode::CorruptInput, "{name}");
        assert!(!source.with_extension("xlsx").exists(), "{name}");
    }
}

#[test]
fn duplicate_producer_fields_and_uppercase_xml_payloads_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let duplicate_app = format!(
        r#"<Properties xmlns="{APP_NS}"><Application>Cell</Application><Application/><AppVersion>12.0300</AppVersion></Properties>"#
    );
    let source = dir.path().join("duplicate.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                app_xml: Some(&duplicate_app),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write duplicate app fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("duplicate producer field")
            .code,
        ErrorCode::CorruptInput
    );

    let source = dir.path().join("uppercase-xml.show");
    let dtd = br#"<!DOCTYPE x [<!ENTITY payload "expanded">]><x>&payload;</x>"#;
    let extras = [(
        "custom/payload.XML",
        dtd.as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Show), &extras),
    )
    .expect("write uppercase XML fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("uppercase XML DTD")
            .code,
        ErrorCode::CorruptInput
    );

    let source = dir.path().join("bad-character-reference.cell");
    let invalid_reference = [(
        "xl/invalid.XML",
        b"<x>&#bogus;</x>".as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &invalid_reference),
    )
    .expect("write invalid character reference fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("invalid character reference")
            .code,
        ErrorCode::CorruptInput
    );

    let source = dir.path().join("undeclared-prefix.cell");
    let undeclared_prefix = [(
        "xl/undeclared.xml",
        b"<missing:root/>".as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &undeclared_prefix),
    )
    .expect("write undeclared namespace fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("undeclared namespace prefix")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn unknown_attribute_entities_and_misplaced_declarations_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");

    let source = dir.path().join("unknown-attribute-entity.cell");
    let unknown_attribute_entity = [(
        "xl/unknown-attribute-entity.xml",
        br#"<x value="&bogus;"/>"#.as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(
            Fixture::valid(CarrierFamily::Cell),
            &unknown_attribute_entity,
        ),
    )
    .expect("write unknown attribute entity fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("unknown attribute entity")
            .code,
        ErrorCode::CorruptInput
    );

    let source = dir.path().join("misplaced-declaration.show");
    let misplaced_declaration = [(
        "ppt/misplaced-declaration.xml",
        br#"<x/><?xml version="1.0"?>"#.as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Show), &misplaced_declaration),
    )
    .expect("write misplaced XML declaration fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("misplaced XML declaration")
            .code,
        ErrorCode::CorruptInput
    );

    let source = dir.path().join("encoding-mismatch.cell");
    let encoding_mismatch = [(
        "xl/encoding-mismatch.xml",
        br#"<?xml version="1.0" encoding="UTF-16"?><x/>"#.as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &encoding_mismatch),
    )
    .expect("write XML encoding mismatch fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("XML declaration encoding mismatch")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn reserved_namespace_rebindings_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, xml) in [
        (
            "wrong-xml-prefix.cell",
            br#"<x xmlns:xml="urn:not-the-xml-namespace"/>"#.as_slice(),
        ),
        (
            "declared-xmlns-prefix.cell",
            br#"<x xmlns:xmlns="urn:not-allowed"/>"#.as_slice(),
        ),
        (
            "reserved-xmlns-uri.cell",
            br#"<x xmlns:p="http://www.w3.org/2000/xmlns/"/>"#.as_slice(),
        ),
    ] {
        let source = dir.path().join(name);
        let extra = [(
            "xl/reserved-namespace.xml",
            xml,
            CompressionMethod::Stored,
            None,
        )];
        fs::write(
            &source,
            build_fixture(Fixture::valid(CarrierFamily::Cell), &extra),
        )
        .expect("write reserved namespace fixture");

        assert_eq!(
            bridge_ooxml(&source, CarrierFamily::Cell)
                .expect_err("reserved namespace binding")
                .code,
            ErrorCode::CorruptInput,
            "{name}"
        );
    }
}

#[test]
fn invalid_utf8_and_raw_xml_control_characters_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, xml) in [
        ("invalid-utf8.cell", b"<x>\xff</x>".as_slice()),
        ("raw-control.cell", b"<x>\x01</x>".as_slice()),
    ] {
        let source = dir.path().join(name);
        let extra = [("xl/invalid-text.xml", xml, CompressionMethod::Stored, None)];
        fs::write(
            &source,
            build_fixture(Fixture::valid(CarrierFamily::Cell), &extra),
        )
        .expect("write invalid XML text fixture");

        assert_eq!(
            bridge_ooxml(&source, CarrierFamily::Cell)
                .expect_err("invalid XML text")
                .code,
            ErrorCode::CorruptInput,
            "{name}"
        );
    }
}

#[test]
fn expanded_name_duplicate_attributes_and_attribute_floods_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("duplicate-expanded-attribute.cell");
    let duplicate_expanded_attribute = [(
        "xl/duplicate-expanded-attribute.xml",
        br#"<x xmlns:a="urn:same" xmlns:b="urn:same" a:value="1" b:value="2"/>"#.as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(
            Fixture::valid(CarrierFamily::Cell),
            &duplicate_expanded_attribute,
        ),
    )
    .expect("write duplicate expanded attribute fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("duplicate expanded attribute")
            .code,
        ErrorCode::CorruptInput
    );

    let source = dir.path().join("attribute-flood.show");
    let attributes = (0..1025)
        .map(|index| format!(" a{index}=\"x\""))
        .collect::<String>();
    let flooded_xml = format!("<x{attributes}/>");
    let flood = [(
        "ppt/attribute-flood.xml",
        flooded_xml.as_bytes(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Show), &flood),
    )
    .expect("write attribute flood fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("attribute flood")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn duplicate_relationship_ids_and_dangling_internal_targets_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, _, _) = main_part(CarrierFamily::Cell);
    let duplicate_ids = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="same" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="same" Type="{APP_REL}" Target="docProps/app.xml"/></Relationships>"#
    );
    let source = dir.path().join("duplicate-relationship-id.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                relationships_xml: Some(&duplicate_ids),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write duplicate relationship ID fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("duplicate relationship ID")
            .code,
        ErrorCode::CorruptInput
    );

    let source = dir.path().join("dangling-relationship.show");
    let dangling_relationship = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/missing.png"/></Relationships>"#;
    let extras = [
        (
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#
                .as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            dangling_relationship.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Show), &extras),
    )
    .expect("write dangling relationship fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("dangling internal relationship target")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn opc_identity_and_required_root_relationships_are_unambiguous() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, main_type, default_main) = main_part(CarrierFamily::Cell);
    let app_type = "application/vnd.openxmlformats-officedocument.extended-properties+xml";

    let dual_office_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="rId2" Type="{OFFICE_REL}" Target="xl/alternate.xml"/><Relationship Id="rId3" Type="{APP_REL}" Target="docProps/app.xml"/></Relationships>"#
    );
    let dual_office_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/xl/alternate.xml" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="{app_type}"/></Types>"#
    );
    let alternate = [(
        "xl/alternate.xml",
        default_main.as_bytes(),
        CompressionMethod::Stored,
        None,
    )];
    let source = dir.path().join("dual-office-root.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                relationships_xml: Some(&dual_office_relationships),
                content_types_xml: Some(&dual_office_types),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &alternate,
        ),
    )
    .expect("write dual office root fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("dual officeDocument roots")
            .code,
        ErrorCode::CorruptInput
    );

    let dual_app_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/><Relationship Id="rId3" Type="{APP_REL}" Target="docProps/app2.xml"/></Relationships>"#
    );
    let dual_app_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="{app_type}"/><Override PartName="/docProps/app2.xml" ContentType="{app_type}"/></Types>"#
    );
    let app2 = [(
        "docProps/app2.xml",
        format!(
            r#"<Properties xmlns="{APP_NS}"><Application>Cell</Application><AppVersion>12.0300</AppVersion></Properties>"#
        ),
    )];
    let app2_entries = [(
        app2[0].0,
        app2[0].1.as_bytes(),
        CompressionMethod::Stored,
        None,
    )];
    let source = dir.path().join("dual-app-properties.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                relationships_xml: Some(&dual_app_relationships),
                content_types_xml: Some(&dual_app_types),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &app2_entries,
        ),
    )
    .expect("write dual app properties fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("dual extended-properties relationships")
            .code,
        ErrorCode::CorruptInput
    );

    let case_alias = [(
        "XL/workbook.xml",
        default_main.as_bytes(),
        CompressionMethod::Stored,
        None,
    )];
    let source = dir.path().join("case-alias.cell");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &case_alias),
    )
    .expect("write case alias fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("case-only part alias")
            .code,
        ErrorCode::CorruptInput
    );

    let encoded_target = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="%78l/workbook.xml"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/></Relationships>"#
    );
    let source = dir.path().join("percent-encoded-target.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                relationships_xml: Some(&encoded_target),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write percent-encoded target fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("percent-encoded relationship target")
            .code,
        ErrorCode::CorruptInput
    );

    let root_image_relationship = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/root.png"/></Relationships>"#
    );
    let root_image_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="{app_type}"/></Types>"#
    );
    let root_image = [(
        "media/root.png",
        b"not a parsed image".as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    let source = dir.path().join("root-image-relationship.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                relationships_xml: Some(&root_image_relationship),
                content_types_xml: Some(&root_image_types),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &root_image,
        ),
    )
    .expect("write package-level image relationship fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("document relationship at package level")
            .code,
        ErrorCode::CorruptInput
    );

    let nested_app_relationship = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rIdApp" Type="{APP_REL}" Target="../docProps/app.xml"/></Relationships>"#
    );
    let nested_app = [(
        "xl/_rels/workbook.xml.rels",
        nested_app_relationship.as_bytes(),
        CompressionMethod::Stored,
        None,
    )];
    let source = dir.path().join("nested-app-relationship.cell");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &nested_app),
    )
    .expect("write nested app relationship fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("package relationship below a part")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn package_metadata_relationships_are_singletons() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, main_type, _) = main_part(CarrierFamily::Cell);
    let relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/><Relationship Id="rId3" Type="{CORE_PROPERTIES_REL}" Target="docProps/core.xml"/><Relationship Id="rId4" Type="{CORE_PROPERTIES_REL}" Target="docProps/core2.xml"/></Relationships>"#
    );
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/core2.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#
    );
    let core = br#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"/>"#;
    let extras = [
        (
            "docProps/core.xml",
            core.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "docProps/core2.xml",
            core.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    let source = dir.path().join("duplicate-core-properties.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                relationships_xml: Some(&relationships),
                content_types_xml: Some(&content_types),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &extras,
        ),
    )
    .expect("write duplicate core-properties fixture");

    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("duplicate package metadata relationship")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn dangling_main_sheet_and_slide_relationship_ids_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cell_main = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><fileVersion appName="HCell"/><calcPr xmlns:hs="http://schemas.haansoft.com/office/spreadsheet/8.0" hs:hclCalcId="1"/><sheets><sheet name="Sheet1" sheetId="1" r:id="missing"/></sheets></workbook>"#;
    let source = dir.path().join("dangling-sheet.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                main_xml: Some(cell_main),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write dangling sheet fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("dangling workbook relationship ID")
            .code,
        ErrorCode::CorruptInput
    );

    let show_main = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="missing"/></p:sldIdLst></p:presentation>"#;
    let source = dir.path().join("dangling-slide.show");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                main_xml: Some(show_main),
                ..Fixture::valid(CarrierFamily::Show)
            },
            &[],
        ),
    )
    .expect("write dangling slide fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("dangling presentation relationship ID")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn sheet_and_slide_relationship_ids_require_their_canonical_collection_parent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let worksheet_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdSheet" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let misplaced_sheet = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><fileVersion appName="HCell"/><calcPr xmlns:hs="http://schemas.haansoft.com/office/spreadsheet/8.0" hs:hclCalcId="1"/><extLst><sheet name="Sheet1" sheetId="1" r:id="rIdSheet"/></extLst></workbook>"#;
    let cell_extras = [
        (
            "xl/_rels/workbook.xml.rels",
            worksheet_relationships.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#
                .as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    let source = dir.path().join("misplaced-sheet.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                main_xml: Some(misplaced_sheet),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &cell_extras,
        ),
    )
    .expect("write misplaced sheet fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("sheet outside sheets")
            .code,
        ErrorCode::CorruptInput
    );

    let slide_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdSlide" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let misplaced_slide = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:extLst><p:sldId id="256" r:id="rIdSlide"/></p:extLst></p:presentation>"#;
    let show_extras = [
        (
            "ppt/_rels/presentation.xml.rels",
            slide_relationships.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#
                .as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    let source = dir.path().join("misplaced-slide.show");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                main_xml: Some(misplaced_slide),
                ..Fixture::valid(CarrierFamily::Show)
            },
            &show_extras,
        ),
    )
    .expect("write misplaced slide fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("slide outside sldIdLst")
            .code,
        ErrorCode::CorruptInput
    );
}

#[test]
fn active_parts_and_non_hyperlink_external_relationships_are_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("hidden-vba.cell");
    let active_part = [(
        "xl/vbaProject.bin",
        b"not executable in the test".as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &active_part),
    )
    .expect("write hidden active part fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("hidden VBA part")
            .code,
        ErrorCode::UnsupportedFeature
    );

    let (main_path, main_type, _) = main_part(CarrierFamily::Cell);
    let active_content_type = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/><Override PartName="/custom/payload.bin" ContentType="application/vnd.ms-office.vbaProject"/></Types>"#
    );
    let source = dir.path().join("disguised-active-type.cell");
    let disguised_part = [(
        "custom/payload.bin",
        b"disguised active payload".as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(
            Fixture {
                content_types_xml: Some(&active_content_type),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &disguised_part,
        ),
    )
    .expect("write active content type fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("active content type")
            .code,
        ErrorCode::UnsupportedFeature
    );

    let source = dir.path().join("external-image.show");
    let external_image = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="file:///outside.png" TargetMode="External"/></Relationships>"#;
    let extras = [
        (
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#
                .as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            external_image.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Show), &extras),
    )
    .expect("write external image fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("external non-hyperlink relationship")
            .code,
        ErrorCode::UnsupportedFeature
    );
}

#[test]
fn unreachable_parts_and_unobserved_content_types_are_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("orphan-payload.cell");
    let orphan = [(
        "custom/payload.bin",
        b"unreachable payload".as_slice(),
        CompressionMethod::Stored,
        None,
    )];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &orphan),
    )
    .expect("write unreachable part fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("unreachable package part")
            .code,
        ErrorCode::UnsupportedFeature
    );

    let (main_path, main_type, _) = main_part(CarrierFamily::Cell);
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/><Override PartName="/custom/payload.bin" ContentType="application/octet-stream"/></Types>"#
    );
    let root_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}" Target="{main_path}"/><Relationship Id="rId2" Type="{APP_REL}" Target="docProps/app.xml"/><Relationship Id="rId3" Type="{THUMBNAIL_REL}" Target="custom/payload.bin"/></Relationships>"#
    );
    let source = dir.path().join("unobserved-content-type.cell");
    fs::write(
        &source,
        build_fixture(
            Fixture {
                content_types_xml: Some(&content_types),
                relationships_xml: Some(&root_relationships),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &orphan,
        ),
    )
    .expect("write unobserved content type fixture");
    assert_eq!(
        bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("unobserved content type")
            .code,
        ErrorCode::UnsupportedFeature
    );
}

#[test]
fn every_reachable_part_requires_an_unambiguous_content_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("untyped.cell");
    let workbook_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/untyped.bin"/></Relationships>"#
    );
    let extras = [
        (
            "xl/_rels/workbook.xml.rels",
            workbook_relationships.as_bytes(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "xl/media/untyped.bin",
            b"untyped payload".as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &extras),
    )
    .expect("write untyped part fixture");

    let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("untyped part");

    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(!dir.path().join("untyped.xlsx").exists());
}

#[test]
fn duplicate_and_missing_content_type_declarations_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, main_type, _) = main_part(CarrierFamily::Cell);
    let duplicate = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="RELS" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );
    let missing = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/><Override PartName="/ghost.xml" ContentType="application/xml"/></Types>"#
    );
    for (name, content_types) in [
        ("duplicate-content-type.cell", duplicate.as_str()),
        ("missing-content-type-part.cell", missing.as_str()),
    ] {
        let source = dir.path().join(name);
        fs::write(
            &source,
            build_fixture(
                Fixture {
                    content_types_xml: Some(content_types),
                    ..Fixture::valid(CarrierFamily::Cell)
                },
                &[],
            ),
        )
        .expect("write content type fixture");

        let error = bridge_ooxml(&source, CarrierFamily::Cell)
            .expect_err("ambiguous content type declaration");
        assert_eq!(error.code, ErrorCode::CorruptInput, "{name}");
    }
}

#[test]
fn https_hyperlinks_are_the_only_supported_external_relationships() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("https-hyperlink.show");
    let hyperlink = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/watch" TargetMode="External"/></Relationships>"#;
    let presentation = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
    let presentation_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let extras = [
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_relationships.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#
                .as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            hyperlink.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    let bytes = build_fixture(
        Fixture {
            main_xml: Some(presentation),
            ..Fixture::valid(CarrierFamily::Show)
        },
        &extras,
    );
    fs::write(&source, &bytes).expect("write HTTPS hyperlink fixture");

    let target = bridge_ooxml(&source, CarrierFamily::Show).expect("HTTPS hyperlink carrier");

    assert_eq!(fs::read(target).expect("read target"), bytes);
}

#[test]
fn show_gif_image_parts_are_supported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, main_type, _) = main_part(CarrierFamily::Show);
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );
    let presentation = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
    let presentation_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let image_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdGif" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.gif"/></Relationships>"#;
    let slide = br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:pic><p:blipFill><a:blip r:embed="rIdGif"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>"#;

    for (label, version_byte) in [("87a", b'7'), ("89a", b'9')] {
        let source = dir.path().join(format!("gif-image-{label}.show"));
        let mut gif = [
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c,
            0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00,
            0x3b,
        ];
        gif[4] = version_byte;
        let extras = [
            (
                "ppt/_rels/presentation.xml.rels",
                presentation_relationships.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/slides/slide1.xml",
                slide.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                image_relationships.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/media/image1.gif",
                gif.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
        ];
        let bytes = build_fixture(
            Fixture {
                main_xml: Some(presentation),
                content_types_xml: Some(&content_types),
                ..Fixture::valid(CarrierFamily::Show)
            },
            &extras,
        );
        fs::write(&source, &bytes).expect("write GIF image fixture");

        let target = bridge_ooxml(&source, CarrierFamily::Show).expect("GIF image carrier");

        assert_eq!(fs::read(target).expect("read target"), bytes);
    }
}

#[test]
fn cell_gif_default_mapping_remains_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("gif-mapping.cell");
    let (main_path, main_type, _) = main_part(CarrierFamily::Cell);
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );
    fs::write(
        &source,
        build_fixture(
            Fixture {
                content_types_xml: Some(&content_types),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write Cell GIF mapping fixture");

    let error = bridge_ooxml(&source, CarrierFamily::Cell)
        .expect_err("Cell GIF mapping must stay outside the verified profile");

    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(!dir.path().join("gif-mapping.xlsx").exists());
}

#[test]
fn show_gif_override_remains_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, main_type, _) = main_part(CarrierFamily::Show);
    let presentation = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
    let presentation_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let image_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdGif" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.gif"/></Relationships>"#;
    let slide = br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:pic><p:blipFill><a:blip r:embed="rIdGif"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>"#;
    let gif = [
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ];
    for (label, override_type) in [("gif", "image/gif"), ("png", "image/png")] {
        let source = dir.path().join(format!("gif-{label}-override.show"));
        let content_types = format!(
            r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/><Override PartName="/ppt/media/image1.gif" ContentType="{override_type}"/></Types>"#
        );
        let extras = [
            (
                "ppt/_rels/presentation.xml.rels",
                presentation_relationships.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/slides/slide1.xml",
                slide.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                image_relationships.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/media/image1.gif",
                gif.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
        ];
        fs::write(
            &source,
            build_fixture(
                Fixture {
                    main_xml: Some(presentation),
                    content_types_xml: Some(&content_types),
                    ..Fixture::valid(CarrierFamily::Show)
                },
                &extras,
            ),
        )
        .expect("write Show GIF override fixture");

        let error = bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("Show GIF override must stay outside the verified profile");

        assert_eq!(error.code, ErrorCode::UnsupportedFeature);
        assert!(!source.with_extension("pptx").exists());
    }
}

#[test]
fn show_gif_parts_require_a_verified_payload_and_relationship_role() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (main_path, main_type, _) = main_part(CarrierFamily::Show);
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );
    let presentation = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
    let presentation_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let referenced_slide = br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:pic><p:blipFill><a:blip r:embed="rIdGif"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>"#;
    let unreferenced_slide =
        br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#;
    let misplaced_reference = br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:extLst r:embed="rIdGif"/></p:sld>"#;
    let valid_gif = [
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ];
    let invalid_gif = b"not-a-gif";
    let empty_relationships =
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

    for (label, slide, relationship_type, gif_part, gif_target, gif, outgoing_relationships) in [
        (
            "invalid-payload",
            referenced_slide.as_slice(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
            "ppt/media/image1.gif",
            "../media/image1.gif",
            invalid_gif.as_slice(),
            false,
        ),
        (
            "unreferenced",
            unreferenced_slide.as_slice(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
            "ppt/media/image1.gif",
            "../media/image1.gif",
            valid_gif.as_slice(),
            false,
        ),
        (
            "audio-relationship",
            referenced_slide.as_slice(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/audio",
            "ppt/media/image1.gif",
            "../media/image1.gif",
            valid_gif.as_slice(),
            false,
        ),
        (
            "misplaced-reference",
            misplaced_reference.as_slice(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
            "ppt/media/image1.gif",
            "../media/image1.gif",
            valid_gif.as_slice(),
            false,
        ),
        (
            "outgoing-relationships",
            referenced_slide.as_slice(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
            "ppt/media/image1.gif",
            "../media/image1.gif",
            valid_gif.as_slice(),
            true,
        ),
        (
            "empty-media-stem",
            referenced_slide.as_slice(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
            "ppt/media/.gif",
            "../media/.gif",
            valid_gif.as_slice(),
            false,
        ),
    ] {
        let source = dir.path().join(format!("gif-{label}.show"));
        let image_relationships = format!(
            r#"<Relationships xmlns="{PACKAGE_RELS_NS}"><Relationship Id="rIdGif" Type="{relationship_type}" Target="{gif_target}"/></Relationships>"#
        );
        let mut extras = vec![
            (
                "ppt/_rels/presentation.xml.rels",
                presentation_relationships.as_slice(),
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/slides/slide1.xml",
                slide,
                CompressionMethod::Stored,
                None,
            ),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                image_relationships.as_bytes(),
                CompressionMethod::Stored,
                None,
            ),
            (gif_part, gif, CompressionMethod::Stored, None),
        ];
        if outgoing_relationships {
            extras.push((
                "ppt/media/_rels/image1.gif.rels",
                empty_relationships.as_slice(),
                CompressionMethod::Stored,
                None,
            ));
        }
        fs::write(
            &source,
            build_fixture(
                Fixture {
                    main_xml: Some(presentation),
                    content_types_xml: Some(&content_types),
                    ..Fixture::valid(CarrierFamily::Show)
                },
                &extras,
            ),
        )
        .expect("write Show GIF boundary fixture");

        let error = bridge_ooxml(&source, CarrierFamily::Show)
            .expect_err("unverified Show GIF part must be rejected");

        assert_eq!(error.code, ErrorCode::UnsupportedFeature, "{label}");
        assert!(!source.with_extension("pptx").exists(), "{label}");
    }
}

#[test]
fn show_gif_parts_require_a_slide_relationship_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("gif-from-layout.show");
    let (main_path, main_type, _) = main_part(CarrierFamily::Show);
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/{main_path}" ContentType="{main_type}"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
    );
    let presentation = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
    let presentation_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let slide_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdLayout" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#;
    let layout_relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdGif" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.gif"/></Relationships>"#;
    let layout = br#"<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:pic><p:blipFill><a:blip r:embed="rIdGif"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sldLayout>"#;
    let gif = [
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ];
    let extras = [
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_relationships.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#
                .as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            slide_relationships.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slideLayouts/slideLayout1.xml",
            layout.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_relationships.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
        (
            "ppt/media/image1.gif",
            gif.as_slice(),
            CompressionMethod::Stored,
            None,
        ),
    ];
    fs::write(
        &source,
        build_fixture(
            Fixture {
                main_xml: Some(presentation),
                content_types_xml: Some(&content_types),
                ..Fixture::valid(CarrierFamily::Show)
            },
            &extras,
        ),
    )
    .expect("write non-slide GIF source fixture");

    let error = bridge_ooxml(&source, CarrierFamily::Show)
        .expect_err("GIF image relationships from a layout remain unsupported");

    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(!source.with_extension("pptx").exists());
}

#[test]
fn validated_cell_is_copied_byte_for_byte_to_xlsx_sibling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("survey.cell");
    let bytes = build_fixture(Fixture::valid(CarrierFamily::Cell), &[]);
    fs::write(&source, &bytes).expect("write source");

    let target = bridge_ooxml(&source, CarrierFamily::Cell).expect("bridge");

    assert_eq!(target, dir.path().join("survey.xlsx"));
    assert_eq!(fs::read(&source).expect("source"), bytes);
    assert_eq!(fs::read(&target).expect("target"), bytes);
}

#[test]
fn validated_show_is_copied_byte_for_byte_to_pptx_sibling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("lesson.show");
    let bytes = build_fixture(Fixture::valid(CarrierFamily::Show), &[]);
    fs::write(&source, &bytes).expect("write source");

    let target = bridge_ooxml(&source, CarrierFamily::Show).expect("bridge");

    assert_eq!(target, dir.path().join("lesson.pptx"));
    assert_eq!(fs::read(&target).expect("target"), bytes);
}

#[test]
fn hancom_show_timestamp_quirk_is_ignored_only_in_the_validation_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("quirk.show");
    let bytes = build_fixture_with_timestamp_quirk(Fixture::valid(CarrierFamily::Show), &[], true);
    assert!(
        ZipArchive::new(Cursor::new(&bytes)).is_err(),
        "the fixture must reproduce zip-rs rejection before compatibility handling"
    );
    fs::write(&source, &bytes).expect("write source");

    let target = bridge_ooxml(&source, CarrierFamily::Show).expect("bridge timestamp quirk");

    assert_eq!(fs::read(&source).expect("source"), bytes);
    assert_eq!(fs::read(&target).expect("target"), bytes);
}

#[test]
fn timestamp_compatibility_rejects_non_hancom_shapes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("malformed-timestamp.show");
    let mut bytes =
        build_fixture_with_timestamp_quirk(Fixture::valid(CarrierFamily::Show), &[], true);
    let marker = [0x55, 0x54, 0x0D, 0x00, 0x02];
    let offsets = bytes
        .windows(marker.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == marker).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 2, "local and central timestamp markers");
    for offset in offsets {
        bytes[offset + 9] ^= 1;
    }
    fs::write(&source, bytes).expect("write source");

    let error = bridge_ooxml(&source, CarrierFamily::Show).expect_err("malformed timestamp");

    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(!dir.path().join("malformed-timestamp.pptx").exists());
}

#[test]
fn unobserved_zip_extra_fields_are_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("unknown-extra.show");
    let mut bytes =
        build_fixture_with_timestamp_quirk(Fixture::valid(CarrierFamily::Show), &[], true);
    let marker = [0x55, 0x54, 0x0D, 0x00, 0x02];
    let offsets = bytes
        .windows(marker.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == marker).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 2, "local and central timestamp markers");
    for offset in offsets {
        bytes[offset] = 0xFE;
        bytes[offset + 1] = 0xCA;
    }
    fs::write(&source, bytes).expect("write unknown extra-field fixture");

    let error = bridge_ooxml(&source, CarrierFamily::Show).expect_err("unknown extra field");

    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(!dir.path().join("unknown-extra.pptx").exists());
}

#[test]
fn local_and_central_zip_headers_must_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("header-mismatch.cell");
    let mut bytes = build_fixture(Fixture::valid(CarrierFamily::Cell), &[]);
    let eocd = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("EOCD");
    let central = u32::from_le_bytes(
        bytes[eocd + 16..eocd + 20]
            .try_into()
            .expect("central offset"),
    ) as usize;
    assert_eq!(&bytes[central..central + 4], b"PK\x01\x02");
    let local = u32::from_le_bytes(
        bytes[central + 42..central + 46]
            .try_into()
            .expect("local offset"),
    ) as usize;
    bytes[local + 6] ^= 0x02;
    fs::write(&source, bytes).expect("write mismatched header fixture");

    let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("header mismatch");

    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(!dir.path().join("header-mismatch.xlsx").exists());
}

#[test]
fn a_valid_opposite_ooxml_family_is_unsupported_not_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("wrong.cell");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Show), &[]),
    )
    .expect("write source");

    let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("wrong family");

    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(!dir.path().join("wrong.xlsx").exists());
}

#[test]
fn unverified_producer_versions_fail_closed_as_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, application, version) in [
        ("foreign.cell", "Microsoft Excel", "16.0000"),
        ("older.cell", "Cell", "11.0000"),
        ("unobserved.cell", "Cell", "12.9999"),
        ("malformed.cell", "Cell", "12.evil"),
    ] {
        let source = dir.path().join(name);
        let fixture = Fixture {
            application,
            app_version: version,
            ..Fixture::valid(CarrierFamily::Cell)
        };
        fs::write(&source, build_fixture(fixture, &[])).expect("write source");
        let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("unsupported producer");
        assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    }
}

#[test]
fn duplicate_cell_main_markers_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("duplicate-markers.cell");
    let main_xml = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fileVersion appName="HCell"/><fileVersion appName="HCell"/><calcPr xmlns:hs="http://schemas.haansoft.com/office/spreadsheet/8.0" hs:hclCalcId="1"/></workbook>"#;
    fs::write(
        &source,
        build_fixture(
            Fixture {
                main_xml: Some(main_xml),
                ..Fixture::valid(CarrierFamily::Cell)
            },
            &[],
        ),
    )
    .expect("write duplicate marker fixture");

    let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("duplicate markers");

    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(!dir.path().join("duplicate-markers.xlsx").exists());
}

#[test]
fn oversized_sources_are_rejected_before_container_parsing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("oversized.cell");
    let file = fs::File::create(&source).expect("create sparse source");
    file.set_len(512 * 1024 * 1024 + 1)
        .expect("extend sparse source");
    drop(file);

    let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("source size limit");

    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("resource limit exceeded"));
    assert!(!dir.path().join("oversized.xlsx").exists());
}

#[test]
fn legacy_or_unknown_containers_are_reported_as_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, bytes) in [
        (
            "legacy.cell",
            vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        ),
        ("unknown.cell", b"not a known Hancom container".to_vec()),
    ] {
        let source = dir.path().join(name);
        fs::write(&source, bytes).expect("write source");
        let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("unsupported container");
        assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    }
}

#[test]
fn a_claimed_zip_that_cannot_be_opened_is_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("broken.show");
    fs::write(&source, b"PK\x03\x04truncated").expect("write source");

    let error = bridge_ooxml(&source, CarrierFamily::Show).expect_err("corrupt ZIP");

    assert_eq!(error.code, ErrorCode::CorruptInput);
}

#[test]
fn unsafe_or_colliding_zip_entry_names_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, extra_name) in [
        ("traversal.cell", "../escape.xml"),
        ("backslash.cell", r"xl\escape.xml"),
        ("collision.cell", "XL/WORKBOOK.XML"),
        ("percent.cell", "xl/%77orkbook.xml"),
        ("fragment.cell", "xl/payload.xml#shadow"),
        ("query.cell", "xl/payload.xml?shadow"),
        ("unicode.cell", "xl/문서.xml"),
    ] {
        let source = dir.path().join(name);
        let extras = [(extra_name, b"x".as_slice(), CompressionMethod::Stored, None)];
        fs::write(
            &source,
            build_fixture(Fixture::valid(CarrierFamily::Cell), &extras),
        )
        .expect("write source");
        let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("unsafe entry");
        assert_eq!(error.code, ErrorCode::CorruptInput, "{extra_name}");
    }
}

#[test]
fn symlink_entries_and_extreme_expansion_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let symlink_source = dir.path().join("symlink.show");
    let symlink = [(
        "ppt/link",
        b"../outside".as_slice(),
        CompressionMethod::Stored,
        Some(0o120777),
    )];
    fs::write(
        &symlink_source,
        build_fixture(Fixture::valid(CarrierFamily::Show), &symlink),
    )
    .expect("write source");
    let error = bridge_ooxml(&symlink_source, CarrierFamily::Show).expect_err("ZIP symlink");
    assert_eq!(error.code, ErrorCode::CorruptInput);

    let bomb_source = dir.path().join("bomb.cell");
    let zeros = vec![0_u8; 4 * 1024 * 1024];
    let bomb = [(
        "xl/bomb.bin",
        zeros.as_slice(),
        CompressionMethod::Deflated,
        None,
    )];
    fs::write(
        &bomb_source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &bomb),
    )
    .expect("write source");
    let error = bridge_ooxml(&bomb_source, CarrierFamily::Cell).expect_err("ZIP bomb");
    assert_eq!(error.code, ErrorCode::CorruptInput);
}

#[test]
fn dtds_and_incomplete_ooxml_packages_are_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dtd_source = dir.path().join("dtd.show");
    let app_xml = format!(
        r#"<?xml version="1.0"?><!DOCTYPE Properties [<!ENTITY x "Show">]><Properties xmlns="{APP_NS}"><Application>&x;</Application><AppVersion>12.0000</AppVersion></Properties>"#
    );
    let fixture = Fixture {
        app_xml: Some(&app_xml),
        ..Fixture::valid(CarrierFamily::Show)
    };
    fs::write(&dtd_source, build_fixture(fixture, &[])).expect("write source");
    let error = bridge_ooxml(&dtd_source, CarrierFamily::Show).expect_err("DTD");
    assert_eq!(error.code, ErrorCode::CorruptInput);

    let bad_main_source = dir.path().join("bad-main.cell");
    let fixture = Fixture {
        main_xml: Some("<not-a-workbook/>"),
        ..Fixture::valid(CarrierFamily::Cell)
    };
    fs::write(&bad_main_source, build_fixture(fixture, &[])).expect("write source");
    let error = bridge_ooxml(&bad_main_source, CarrierFamily::Cell).expect_err("bad main part");
    assert_eq!(error.code, ErrorCode::CorruptInput);
}

#[test]
fn a_target_hardlink_to_the_source_is_never_replaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("alias.cell");
    let target = dir.path().join("alias.xlsx");
    let bytes = build_fixture(Fixture::valid(CarrierFamily::Cell), &[]);
    fs::write(&source, &bytes).expect("write source");
    fs::hard_link(&source, &target).expect("hardlink");

    let error = bridge_ooxml(&source, CarrierFamily::Cell).expect_err("source alias");

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(fs::read(&source).expect("source"), bytes);
    assert_eq!(fs::read(&target).expect("target"), bytes);
}

#[test]
fn a_different_preexisting_native_sibling_is_never_replaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("paired.cell");
    let target = dir.path().join("paired.xlsx");
    let source_bytes = build_fixture(Fixture::valid(CarrierFamily::Cell), &[]);
    let target_bytes = b"independently published native workbook";
    fs::write(&source, &source_bytes).expect("write source");
    fs::write(&target, target_bytes).expect("write preexisting sibling");

    let error = bridge_ooxml(&source, CarrierFamily::Cell)
        .expect_err("different preexisting sibling must conflict");

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(fs::read(&source).expect("source"), source_bytes);
    assert_eq!(fs::read(&target).expect("target"), target_bytes);
}

#[test]
fn a_cached_native_sibling_with_a_different_mtime_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("cached.cell");
    let target = dir.path().join("cached.xlsx");
    let bytes = build_fixture(Fixture::valid(CarrierFamily::Cell), &[]);
    fs::write(&source, &bytes).expect("write source");
    fs::write(&target, &bytes).expect("write cached target");
    let stale = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    fs::File::options()
        .write(true)
        .open(&target)
        .expect("open cached target")
        .set_times(fs::FileTimes::new().set_modified(stale))
        .expect("set cached target mtime");
    assert_ne!(
        fs::metadata(&source)
            .expect("source metadata")
            .modified()
            .ok(),
        fs::metadata(&target)
            .expect("target metadata")
            .modified()
            .ok(),
        "test setup requires distinct mtimes"
    );

    let error = bridge_ooxml(&source, CarrierFamily::Cell)
        .expect_err("cached target with stale mtime must conflict");

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(fs::read(&target).expect("target remains intact"), bytes);
}

#[cfg(windows)]
#[test]
#[allow(clippy::permissions_set_readonly_false)] // This test cleanup is Windows-only.
fn windows_zone_identifier_is_preserved_on_the_native_sibling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("downloaded.cell");
    let target = dir.path().join("downloaded.xlsx");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &[]),
    )
    .expect("write source");
    let mut source_zone = source.as_os_str().to_os_string();
    source_zone.push(":Zone.Identifier");
    let zone = b"[ZoneTransfer]\r\nZoneId=3\r\n";
    fs::write(&source_zone, zone).expect("write source Zone.Identifier");
    let mut source_permissions = fs::metadata(&source)
        .expect("source metadata")
        .permissions();
    source_permissions.set_readonly(true);
    fs::set_permissions(&source, source_permissions).expect("make source read-only");

    bridge_ooxml(&source, CarrierFamily::Cell).expect("bridge");

    let mut target_zone = target.as_os_str().to_os_string();
    target_zone.push(":Zone.Identifier");
    assert_eq!(fs::read(target_zone).expect("target Zone.Identifier"), zone);
    assert!(
        fs::metadata(&target)
            .expect("target metadata")
            .permissions()
            .readonly(),
        "target did not preserve the source read-only flag"
    );
    for path in [&source, &target] {
        let mut permissions = fs::metadata(path).expect("cleanup metadata").permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).expect("clear read-only flag for cleanup");
    }

    let source = dir.path().join("cached.cell");
    let target = dir.path().join("cached.xlsx");
    let bytes = build_fixture(Fixture::valid(CarrierFamily::Cell), &[]);
    fs::write(&source, &bytes).expect("write cached source");
    fs::write(&target, &bytes).expect("write cached target without MOTW");
    let mut source_zone = source.as_os_str().to_os_string();
    source_zone.push(":Zone.Identifier");
    fs::write(source_zone, zone).expect("write cached source Zone.Identifier");

    let error = bridge_ooxml(&source, CarrierFamily::Cell)
        .expect_err("cached target without source trust marker");

    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[cfg(windows)]
#[test]
fn windows_unlisted_alternate_data_streams_fail_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("streamed.cell");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &[]),
    )
    .expect("write source");
    let mut private_stream = source.as_os_str().to_os_string();
    private_stream.push(":private-policy");
    fs::write(private_stream, b"must not disappear").expect("write alternate data stream");

    let error = bridge_ooxml(&source, CarrierFamily::Cell)
        .expect_err("unlisted alternate data stream must fail closed");

    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(!source.with_extension("xlsx").exists());
}

#[cfg(unix)]
#[test]
fn unix_extended_attributes_are_preserved_and_checked_on_cached_siblings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("downloaded.cell");
    let target = dir.path().join("downloaded.xlsx");
    let bytes = build_fixture(Fixture::valid(CarrierFamily::Cell), &[]);
    fs::write(&source, &bytes).expect("write source");
    let attribute = if cfg!(target_os = "macos") {
        "com.openai.officecli.test-trust"
    } else {
        "user.officecli.test-trust"
    };
    xattr::set(&source, attribute, b"restricted").expect("set source xattr");

    bridge_ooxml(&source, CarrierFamily::Cell).expect("bridge");

    assert_eq!(
        xattr::get(&target, attribute).expect("target xattr"),
        Some(b"restricted".to_vec())
    );

    fs::remove_file(&target).expect("remove generated target");
    fs::write(&target, &bytes).expect("write cached target without xattr");
    let error =
        bridge_ooxml(&source, CarrierFamily::Cell).expect_err("cached target without source xattr");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_extended_acl_is_preserved_before_publication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("acl.cell");
    let target = dir.path().join("acl.xlsx");
    fs::write(
        &source,
        build_fixture(Fixture::valid(CarrierFamily::Cell), &[]),
    )
    .expect("write source");
    let status = std::process::Command::new("chmod")
        .args([
            "+a",
            "everyone deny write",
            source.to_str().expect("UTF-8 path"),
        ])
        .status()
        .expect("run chmod");
    assert!(status.success(), "test setup must add a macOS ACL");

    bridge_ooxml(&source, CarrierFamily::Cell).expect("bridge ACL-bearing source");

    assert!(target.exists());
    for path in [&source, &target] {
        let status = std::process::Command::new("chmod")
            .args(["-N", path.to_str().expect("UTF-8 path")])
            .status()
            .expect("clear ACL");
        assert!(status.success(), "test cleanup must clear the macOS ACL");
    }
}
