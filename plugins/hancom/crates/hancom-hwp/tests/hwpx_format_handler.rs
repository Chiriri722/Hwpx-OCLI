use std::io::{BufRead, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use officecli_hwpx::format_handler::{format_handler_manifest, serve};
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const SECTION_PART: &str = "Contents/section0.xml";
const VERSION: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" tagetApplication="WORDPROCESSOR" major="5" minor="0" micro="5" buildNumber="0" xmlVersion="1.4" application="OfficeCLI" appVersion="0.1.0"/>"#;
const META_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?><odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;
const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#;
const HPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/></opf:manifest><opf:spine><opf:itemref idref="header"/><opf:itemref idref="section0"/></opf:spine></opf:package>"#;
const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" version="1.4" secCnt="1"><hh:refList><hh:charProperties itemCnt="1"><hh:charPr id="0" height="1000"/></hh:charProperties><hh:paraProperties itemCnt="1"><hh:paraPr id="0"/></hh:paraProperties></hh:refList></hh:head>"#;

fn section(first: &str, second: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p id="7" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>{first}</hp:t></hp:run><hp:linesegarray><hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray></hp:p><hp:p id="7" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>{second}</hp:t></hp:run><hp:linesegarray><hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray></hp:p></hs:sec>"#
    )
}

fn build_package(section_xml: &str) -> Vec<u8> {
    build_package_with_entries(section_xml, true)
}

fn build_permissive_package(section_xml: &str) -> Vec<u8> {
    build_package_with_entries(section_xml, false)
}

fn build_package_with_entries(section_xml: &str, strict_metadata: bool) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        writer.start_file("mimetype", stored).expect("mimetype");
        writer
            .write_all(b"application/hwp+zip")
            .expect("mimetype body");
        let mut entries = vec![
            ("Contents/content.hpf", HPF),
            ("Contents/header.xml", HEADER),
            (SECTION_PART, section_xml),
        ];
        if strict_metadata {
            entries.splice(
                0..0,
                [
                    ("version.xml", VERSION),
                    ("META-INF/manifest.xml", META_MANIFEST),
                    ("META-INF/container.xml", CONTAINER),
                ],
            );
        }
        for (name, body) in entries {
            writer.start_file(name, deflated).expect("fixture entry");
            writer.write_all(body.as_bytes()).expect("fixture body");
        }
        writer.finish().expect("finish fixture");
    }
    cursor.into_inner()
}

#[test]
fn dedicated_binary_prints_exactly_one_format_handler_manifest() {
    let output = Command::cargo_bin("officecli-hancom-hwpx")
        .expect("format-handler binary")
        .arg("--info")
        .output()
        .expect("run --info");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 manifest");
    assert_eq!(stdout.lines().count(), 1);
    let manifest: Value = serde_json::from_str(stdout.trim_end()).expect("manifest JSON");
    assert_eq!(manifest, format_handler_manifest());
}

fn read_section(path: &Path) -> String {
    let file = std::fs::File::open(path).expect("open package");
    let mut archive = ZipArchive::new(file).expect("open ZIP");
    let mut section = String::new();
    archive
        .by_name(SECTION_PART)
        .expect("section")
        .read_to_string(&mut section)
        .expect("read section");
    section
}

#[test]
fn mixed_text_remains_readable_without_expanding_the_writer() {
    for (markup, expected) in [
        ("LEFT<hp:tab/>RIGHT", "LEFT\tRIGHT"),
        ("LEFT<hp:tab></hp:tab>RIGHT", "LEFT\tRIGHT"),
        ("LEFT<hp:lineBreak/>RIGHT", "LEFT\u{b}RIGHT"),
        ("<![CDATA[LEFT & RIGHT]]>", "LEFT & RIGHT"),
        ("LEFT<!-- comment -->RIGHT", "LEFTRIGHT"),
        ("LEFT<x:tab xmlns:x=\"urn:foreign\"/>RIGHT", "LEFTRIGHT"),
    ] {
        for editable in [false, true] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("mixed.hwpx");
            let original = build_package(&section(markup, "TAIL"));
            std::fs::write(&path, &original).expect("fixture");
            let input = frame_lines(&[
                json!({"protocol":1,"msg_type":"open","editable":editable}),
                json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text"}}),
                json!({"protocol":1,"msg_type":"command","command":"get","args":{"path":"/document/section[1]/paragraph[1]","depth":1}}),
                json!({"protocol":1,"msg_type":"command","command":"query","args":{"selector":"text"}}),
                json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"forbidden"}}),
                json!({"protocol":1,"msg_type":"close"}),
            ]);
            let mut output = Vec::new();
            serve(&path, Cursor::new(input), &mut output).expect("serve");
            let replies = replies(output);
            assert_eq!(
                replies[1]["result"],
                format!("{expected}\nTAIL"),
                "{markup}, editable={editable}"
            );
            assert_eq!(replies[2]["result"]["text"], expected);
            assert_eq!(replies[2]["result"]["children"][0]["text"], expected);
            assert_eq!(replies[3]["result"][0]["text"], expected);
            assert_eq!(replies[3]["result"][0]["format"]["editable"], false);
            assert_eq!(
                replies[4]["error"]["code"],
                if editable {
                    "unsupported_feature"
                } else {
                    "unsupported_command"
                }
            );
            assert_eq!(std::fs::read(path).expect("source"), original);
        }
    }
}

#[test]
fn run_separators_survive_plain_text_edits_and_reopen() {
    for (control, separator) in [("tab", "\t"), ("lineBreak", "\u{b}")] {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("separator.hwpx");
        let markup = format!("LEFT</hp:t><hp:{control}/><hp:t>RIGHT");
        std::fs::write(&path, build_package(&section(&markup, "TAIL"))).expect("fixture");
        let mut output = Vec::new();
        serve(&path, Cursor::new(frame_lines(&[
            json!({"protocol":1,"msg_type":"open","editable":true}),
            json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text"}}),
            json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[2]"},"props":{"text":"CHANGED"}}),
            json!({"protocol":1,"msg_type":"save"}),
            json!({"protocol":1,"msg_type":"close"}),
        ])), &mut output).expect("serve");
        let response = replies(output);
        assert_eq!(response[1]["result"], format!("LEFT{separator}RIGHT\nTAIL"));
        assert!(
            response.iter().all(|reply| reply["msg_type"] == "ok"),
            "{response:#?}"
        );
        let mut output = Vec::new();
        serve(&path, Cursor::new(frame_lines(&[
            json!({"protocol":1,"msg_type":"open","editable":false}),
            json!({"protocol":1,"msg_type":"command","command":"get","args":{"path":"/document/section[1]/paragraph[1]"}}),
            json!({"protocol":1,"msg_type":"close"}),
        ])), &mut output).expect("reopen");
        assert_eq!(
            replies(output)[1]["result"]["text"],
            format!("LEFT{separator}CHANGED")
        );
    }
}

#[test]
fn query_type_aliases_match_bare_types_and_preserve_absolute_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("query.hwpx");
    std::fs::write(&path, build_package(&section("first", "second"))).expect("fixture");
    let mut frames = vec![json!({"protocol":1,"msg_type":"open","editable":false})];
    for kind in ["document", "section", "paragraph", "text"] {
        for selector in [kind.to_owned(), format!("//{kind}")] {
            frames.push(json!({"protocol":1,"msg_type":"command","command":"query","args":{"selector":selector}}));
        }
    }
    for selector in [
        "/document/section[1]/paragraph[1]/text[1]",
        "/document/missing",
        "//unknown",
        "//text[1]",
        "///text",
    ] {
        frames.push(json!({"protocol":1,"msg_type":"command","command":"query","args":{"selector":selector}}));
    }
    frames.push(json!({"protocol":1,"msg_type":"close"}));
    let mut output = Vec::new();
    serve(&path, Cursor::new(frame_lines(&frames)), &mut output).expect("serve");
    let replies = replies(output);
    for index in [1, 3, 5, 7] {
        assert_eq!(replies[index]["result"], replies[index + 1]["result"]);
        assert!(!replies[index]["result"]
            .as_array()
            .expect("nodes")
            .is_empty());
    }
    assert_eq!(replies[9]["result"][0]["text"], "first");
    assert_eq!(replies[10]["result"], json!([]));
    for reply in &replies[11..14] {
        assert_eq!(reply["error"]["code"], "invalid_argument");
    }
}

fn frame_lines(frames: &[Value]) -> String {
    frames
        .iter()
        .map(|frame| serde_json::to_string(frame).expect("serialize frame"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn replies(bytes: Vec<u8>) -> Vec<Value> {
    String::from_utf8(bytes)
        .expect("UTF-8 replies")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON reply"))
        .collect()
}

#[test]
fn structured_reads_preserve_sources_empty_cells_and_field_identity() {
    let xml = section("outer", "tail").replace("<hp:t>outer</hp:t>", r#"<hp:t>outer</hp:t>
<hp:tbl id="5" rowCnt="1" colCnt="2"><hp:tr><hp:tc name="entry"><hp:subList>
<hp:p id="7"><hp:run><hp:ctrl><hp:fieldBegin id="11" type="CLICK_HERE" name="same"/></hp:ctrl><hp:t></hp:t><hp:t/><hp:t>split</hp:t><hp:ctrl><hp:fieldEnd beginIDRef="11"/></hp:ctrl></hp:run></hp:p>
</hp:subList><hp:cellAddr rowAddr="0" colAddr="0"/><hp:cellSpan rowSpan="1" colSpan="2"/></hp:tc></hp:tr></hp:tbl>
<hp:ctrl><hp:footNote number="1" instId="9"><hp:subList><hp:p id="7"><hp:run><hp:t>note</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl>
<hp:ctrl><hp:fieldBegin id="12" type="CLICK_HERE" name="same"/><hp:fieldEnd beginIDRef="12"/></hp:ctrl>
<fake:tbl xmlns:fake="urn:foreign" rowCnt="99"/>"#);
    for editable in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("structure.hwpx");
        let original = build_package(&xml);
        std::fs::write(&path, &original).unwrap();
        let mut frames = vec![json!({"protocol":1,"msg_type":"open","editable":editable})];
        for kind in ["table", "cell", "note", "field", "paragraph", "text"] {
            frames.push(json!({"protocol":1,"msg_type":"command","command":"query","args":{"selector":format!("//{kind}")}}));
        }
        frames.push(json!({"protocol":1,"msg_type":"close"}));
        let mut output = Vec::new();
        serve(&path, Cursor::new(frame_lines(&frames)), &mut output).unwrap();
        let result = replies(output);
        assert!(result.iter().all(|r| r["msg_type"] == "ok"), "{result:#?}");
        assert_eq!(result[1]["result"].as_array().unwrap().len(), 1);
        let cell = &result[2]["result"][0];
        assert_eq!(cell["format"]["row"], 0);
        assert_eq!(cell["format"]["col_span"], 2);
        assert_eq!(
            cell["format"]["parent_path"],
            "/document/section[1]/table[1]"
        );
        assert_eq!(
            cell["format"]["paragraph_paths"],
            json!(["/document/section[1]/paragraph[2]"])
        );
        assert_eq!(cell["format"]["editable"], false);
        assert_eq!(
            cell["format"]["editability"]["candidate_target_paths"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(result[3]["result"][0]["format"]["note_kind"], "footnote");
        let fields = result[4]["result"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_ne!(fields[0]["path"], fields[1]["path"]);
        assert_eq!(fields[0]["format"]["name"], fields[1]["format"]["name"]);
        assert_eq!(fields[0]["format"]["range_status"], "matched");
        assert_eq!(
            fields[0]["format"]["text_paths"].as_array().unwrap().len(),
            3
        );
        let texts = result[6]["result"].as_array().unwrap();
        assert_eq!(
            texts.len(),
            6,
            "paired and self-closing empty nodes stay visible"
        );
        assert_eq!(texts[1]["format"]["editable"], editable);
        assert_eq!(texts[1]["format"]["editability"]["text_candidate"], true);
        assert_eq!(texts[2]["format"]["editability"]["text_candidate"], false);
        assert_eq!(texts[2]["text"], "");
        assert_eq!(texts[2]["format"]["editable"], false);
        assert_eq!(
            texts[3]["path"],
            "/document/section[1]/paragraph[2]/text[3]"
        );
        for row in &result[1..7] {
            for node in row["result"].as_array().unwrap() {
                let source = &node["format"]["source"];
                assert_eq!(source["part"], SECTION_PART);
                assert!(source["revision"].as_str().unwrap().starts_with("sha256:"));
                let start = source["byte_start"].as_u64().unwrap() as usize;
                let end = source["byte_end"].as_u64().unwrap() as usize;
                assert!(xml[start..end].starts_with("<hp:"));
                assert!(xml[start..end].ends_with('>'));
                assert!(node["format"]["editability"]["reason"].is_string());
            }
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        if editable {
            let mut output = Vec::new();
            serve(&path, Cursor::new(frame_lines(&[
                json!({"protocol":1,"msg_type":"open","editable":true}),
                json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[2]/text[1]"},"props":{"text":"채움 & 값"}}),
                json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[2]/text[2]"},"props":{"text":"forbidden"}}),
                json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[2]/text[3]"},"props":{"text":"separate"}}),
                json!({"protocol":1,"msg_type":"save"}),
                json!({"protocol":1,"msg_type":"close"}),
            ])), &mut output).unwrap();
            let saved = replies(output);
            assert_eq!(saved[2]["error"]["code"], "unsupported_feature");
            for i in [0, 1, 3, 4, 5] {
                assert_eq!(saved[i]["msg_type"], "ok", "{saved:#?}");
            }
            assert_eq!(
                read_section(&path),
                xml.replace("<hp:t></hp:t>", "<hp:t>채움 &amp; 값</hp:t>")
                    .replace("<hp:t>split</hp:t>", "<hp:t>separate</hp:t>")
            );
        }
    }
}

#[test]
fn guarded_edits_reject_stale_revision_without_partial_changes() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guarded.hwpx");
    let xml = section("", "second");
    let revision = format!(
        "sha256:{}",
        Sha256::digest(xml.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    std::fs::write(&path, build_package(&xml)).unwrap();
    let mut output = Vec::new();
    serve(&path, Cursor::new(frame_lines(&[
        json!({"protocol":1,"msg_type":"open","editable":true}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"filled","expected_revision":revision}}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[2]/text[1]"},"props":{"text":"wrong","expected_revision":revision}}),
        json!({"protocol":1,"msg_type":"command","command":"query","args":{"selector":"text"}}),
        json!({"protocol":1,"msg_type":"save"}),
        json!({"protocol":1,"msg_type":"close"}),
    ])), &mut output).unwrap();
    let result = replies(output);
    assert_eq!(result[1]["result"]["unsupported_properties"], json!([]));
    assert_eq!(result[2]["error"]["code"], "invalid_argument");
    assert_eq!(result[3]["result"][1]["text"], "second");
    assert_ne!(
        result[3]["result"][0]["format"]["source"]["revision"],
        revision
    );
    assert_eq!(result[4]["msg_type"], "ok", "{result:#?}");
    assert_eq!(
        read_section(&path),
        xml.replace("<hp:t></hp:t>", "<hp:t>filled</hp:t>")
    );
}

#[test]
fn unimplemented_views_are_explicit_and_chunk_reads_report_omissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("views.hwpx");
    std::fs::write(&path, build_package(&section("first", "second"))).unwrap();
    let mut output = Vec::new();
    serve(&path, Cursor::new(frame_lines(&[
        json!({"protocol":1,"msg_type":"open","editable":false}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"outline","format":"json"}}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"issues"}}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text","format":"json","max_lines":1}}),
        json!({"protocol":1,"msg_type":"close"}),
    ])), &mut output).unwrap();
    let result = replies(output);
    for row in &result[1..3] {
        assert_eq!(row["error"]["code"], "unsupported_feature");
    }
    assert_eq!(
        result[3]["result"]["paths"],
        json!(["/document/section[1]/paragraph[1]"])
    );
    assert_eq!(result[3]["result"]["total_lines"], 2);
    assert_eq!(result[3]["result"]["omitted_before"], 0);
    assert_eq!(result[3]["result"]["omitted_after"], 1);
}

#[test]
fn invalid_edit_preconditions_and_mixed_properties_never_apply_text() {
    for props in [
        json!({"text":"wrong","bold":"true"}),
        json!({"expected_revision":"sha256:missing-text"}),
        json!({"text":"wrong","expected_revision":42}),
        json!({"text":"wrong","expected_revision":null}),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid-edit.hwpx");
        let original = build_package(&section("first", "second"));
        std::fs::write(&path, &original).unwrap();
        let mut output = Vec::new();
        serve(&path, Cursor::new(frame_lines(&[
            json!({"protocol":1,"msg_type":"open","editable":true}),
            json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":props}),
            json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text"}}),
            json!({"protocol":1,"msg_type":"save"}),
            json!({"protocol":1,"msg_type":"close"}),
        ])), &mut output).unwrap();
        let result = replies(output);
        assert_eq!(
            result[1]["error"]["code"], "invalid_argument",
            "{result:#?}"
        );
        assert_eq!(result[2]["result"], "first\nsecond");
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

struct MutatingInput {
    inner: Cursor<Vec<u8>>,
    trigger_offset: u64,
    path: PathBuf,
    replacement: Vec<u8>,
    mutated: bool,
}

impl Read for MutatingInput {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.maybe_mutate()?;
        self.inner.read(buffer)
    }
}

impl BufRead for MutatingInput {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.maybe_mutate()?;
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

impl MutatingInput {
    fn maybe_mutate(&mut self) -> std::io::Result<()> {
        if !self.mutated && self.inner.position() >= self.trigger_offset {
            std::fs::write(&self.path, &self.replacement)?;
            self.mutated = true;
        }
        Ok(())
    }
}

#[test]
fn manifest_is_a_split_format_handler_with_honest_vocabulary() {
    let manifest = format_handler_manifest();
    assert_eq!(manifest["name"], "officecli-hancom-hwpx");
    assert_eq!(manifest["kinds"], json!(["format-handler"]));
    assert_eq!(manifest["extensions"], json!([".hwpx", ".owpml"]));
    assert!(manifest.get("target").is_none());
    assert_eq!(manifest["vocabulary"]["addable_types"], json!([]));
    assert_eq!(
        manifest["vocabulary"]["settable_props"]["text"],
        json!(["text", "expected_revision"])
    );
}

#[test]
fn open_frame_without_redundant_path_uses_the_cli_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cli-source.hwpx");
    std::fs::write(&path, build_package(&section("cli fallback", "second")))
        .expect("write fixture");
    let input = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","editable":false}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text"}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert_eq!(replies.len(), 3, "unexpected replies: {replies:#?}");
    assert!(
        replies.iter().all(|reply| reply["msg_type"] == "ok"),
        "unexpected replies: {replies:#?}"
    );
    assert_eq!(replies[1]["result"], "cli fallback\nsecond");
}

#[test]
fn open_frame_without_editable_defaults_to_read_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("readonly-default.hwpx");
    let original = build_package(&section("read only", "second"));
    std::fs::write(&path, &original).expect("write fixture");
    let input = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":path.canonicalize().expect("canonical")}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text"}}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"forbidden"}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    let commands = replies[0]["result"]["capabilities"]["commands"]
        .as_array()
        .expect("commands");
    assert!(!commands.contains(&json!("set")));
    assert!(!commands.contains(&json!("save")));
    assert_eq!(replies[1]["result"], "read only\nsecond");
    assert_eq!(replies[2]["error"]["code"], "unsupported_command");
    assert_eq!(std::fs::read(path).expect("read original"), original);
}

#[test]
fn open_frame_present_editable_keeps_strict_type_checks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("strict-editable.hwpx");
    std::fs::write(&path, build_package(&section("source", "second"))).expect("write fixture");
    let input = frame_lines(&[json!({
        "protocol":1,
        "msg_type":"open",
        "path":path.canonicalize().expect("canonical"),
        "editable":null
    })]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert_eq!(replies.len(), 1, "unexpected replies: {replies:#?}");
    assert_eq!(replies[0]["msg_type"], "error");
    assert_eq!(
        replies[0]["error"]["message"],
        "open.editable must be a boolean"
    );
}

#[test]
fn legacy_lifecycle_envelopes_preserve_explicit_editability_and_durable_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("legacy-open-args.hwpx");
    std::fs::write(&path, build_package(&section("before", "second"))).expect("write fixture");
    let input = frame_lines(&[
        json!({
            "protocol":1,
            "msg_type":"open",
            "args":{
                "path":path.canonicalize().expect("canonical"),
                "editable":true
            }
        }),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"legacy edited"}}),
        json!({"protocol":1,"msg_type":"command","command":"save","args":{}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert!(
        replies.iter().all(|reply| reply["msg_type"] == "ok"),
        "unexpected replies: {replies:#?}"
    );
    let commands = replies[0]["result"]["capabilities"]["commands"]
        .as_array()
        .expect("commands");
    assert!(commands.contains(&json!("set")));
    assert!(commands.contains(&json!("save")));
    assert!(read_section(&path).contains(">legacy edited<"));
}

#[test]
fn legacy_command_save_rejects_incomplete_or_extended_shapes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("strict-legacy-save.hwpx");
    std::fs::write(&path, build_package(&section("source", "second"))).expect("write fixture");
    let canonical = path.canonicalize().expect("canonical");

    for (label, save) in [
        (
            "missing args",
            json!({"protocol":1,"msg_type":"command","command":"save"}),
        ),
        (
            "null args",
            json!({"protocol":1,"msg_type":"command","command":"save","args":null}),
        ),
        (
            "nonempty args",
            json!({"protocol":1,"msg_type":"command","command":"save","args":{"unexpected":1}}),
        ),
        (
            "props",
            json!({"protocol":1,"msg_type":"command","command":"save","args":{},"props":{}}),
        ),
        (
            "extra field",
            json!({"protocol":1,"msg_type":"command","command":"save","args":{},"unexpected":1}),
        ),
    ] {
        let input = frame_lines(&[
            json!({"protocol":1,"msg_type":"open","path":canonical,"editable":true}),
            save,
            json!({"protocol":1,"msg_type":"close"}),
        ]);
        let mut output = Vec::new();
        serve(&path, Cursor::new(input), &mut output)
            .unwrap_or_else(|error| panic!("{label}: serve failed: {error}"));
        let replies = replies(output);
        assert_eq!(replies.len(), 3, "{label}: {replies:#?}");
        assert_eq!(replies[1]["msg_type"], "error", "{label}: {replies:#?}");
        assert_eq!(
            replies[1]["error"]["code"], "invalid_request",
            "{label}: {replies:#?}"
        );
    }
}

#[test]
fn legacy_nested_open_fields_reject_mixed_or_extended_shapes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("strict-legacy-open.hwpx");
    std::fs::write(&path, build_package(&section("source", "second"))).expect("write fixture");
    let canonical = path.canonicalize().expect("canonical");

    for (label, open) in [
        (
            "mixed",
            json!({
                "protocol":1,
                "msg_type":"open",
                "path":canonical,
                "editable":true,
                "args":{"path":canonical,"editable":true}
            }),
        ),
        (
            "extended",
            json!({
                "protocol":1,
                "msg_type":"open",
                "args":{"path":canonical,"editable":true,"unexpected":1}
            }),
        ),
        (
            "missing editable",
            json!({
                "protocol":1,
                "msg_type":"open",
                "args":{"path":canonical}
            }),
        ),
    ] {
        let mut output = Vec::new();
        serve(&path, Cursor::new(frame_lines(&[open])), &mut output)
            .unwrap_or_else(|error| panic!("{label}: serve failed: {error}"));
        let replies = replies(output);
        assert_eq!(
            replies.len(),
            1,
            "{label}: unexpected replies: {replies:#?}"
        );
        assert_eq!(replies[0]["msg_type"], "error", "{label}: {replies:#?}");
        assert_eq!(replies[0]["error"]["code"], "invalid_request", "{label}");
    }
}

#[test]
fn open_frame_present_path_keeps_strict_type_and_identity_checks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cli-source.hwpx");
    let other = dir.path().join("other.hwpx");
    let package = build_package(&section("source", "second"));
    std::fs::write(&path, &package).expect("write source");
    std::fs::write(&other, &package).expect("write other");

    for (label, open, expected) in [
        (
            "null",
            json!({"protocol":1,"msg_type":"open","path":null,"editable":false}),
            "path must be a string (received null)",
        ),
        (
            "different file",
            json!({"protocol":1,"msg_type":"open","path":other,"editable":false}),
            "open-handshake path does not match the CLI source path",
        ),
    ] {
        let mut output = Vec::new();
        serve(&path, Cursor::new(frame_lines(&[open])), &mut output)
            .unwrap_or_else(|error| panic!("{label}: serve failed: {error}"));
        let replies = replies(output);
        assert_eq!(
            replies.len(),
            1,
            "{label}: unexpected replies: {replies:#?}"
        );
        assert_eq!(replies[0]["msg_type"], "error", "{label}: {replies:#?}");
        assert_eq!(replies[0]["error"]["message"], expected, "{label}");
    }
}

#[test]
fn protocol_reads_edits_and_durably_reopens_the_saved_package() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("document.hwpx");
    std::fs::write(&path, build_package(&section("before", "second"))).expect("write fixture");
    let canonical = path.canonicalize().expect("canonical path");
    let text_path = "/document/section[1]/paragraph[1]/text[1]";
    let input = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":canonical,"editable":true}),
        json!({"protocol":1,"msg_type":"command","command":"get","args":{"path":"/document","depth":3}}),
        json!({"protocol":1,"msg_type":"command","command":"query","args":{"selector":"text"}}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text"}}),
        json!({"protocol":1,"msg_type":"command","command":"raw","args":{"part_path":SECTION_PART}}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":text_path},"props":{"text":"after & verified"}}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[2]/text[1]"},"props":{"text":"also changed"}}),
        json!({"protocol":1,"msg_type":"save"}),
        json!({"protocol":1,"msg_type":"command","command":"get","args":{"path":text_path,"depth":0}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert_eq!(replies.len(), 10);
    assert!(replies.iter().all(|reply| reply["protocol"] == 1));
    assert!(
        replies.iter().all(|reply| reply["msg_type"] == "ok"),
        "unexpected replies: {replies:#?}"
    );

    let commands = replies[0]["result"]["capabilities"]["commands"]
        .as_array()
        .expect("commands");
    for command in ["view", "get", "query", "validate", "raw", "set", "save"] {
        assert!(commands.contains(&json!(command)), "missing {command}");
    }
    for command in ["add", "remove", "move", "copy", "raw_set"] {
        assert!(
            !commands.contains(&json!(command)),
            "over-advertised {command}"
        );
    }
    assert_eq!(replies[1]["result"]["type"], "document");
    assert_eq!(replies[2]["result"].as_array().expect("query").len(), 2);
    assert_eq!(replies[3]["result"], "before\nsecond");
    assert!(replies[4]["result"]
        .as_str()
        .expect("raw")
        .contains("before"));
    assert_eq!(replies[5]["result"]["unsupported_properties"], json!([]));
    assert_eq!(replies[6]["result"]["unsupported_properties"], json!([]));
    assert!(replies[7]["result"].is_null());
    assert_eq!(replies[8]["result"]["text"], "after & verified");

    let saved = read_section(&path);
    assert!(saved.contains("after &amp; verified"));
    assert!(saved.contains(">also changed<"));
    assert!(!saved.contains(">before<"));
    assert_eq!(
        std::fs::read_dir(dir.path())
            .expect("read tempdir")
            .filter_map(Result::ok)
            .count(),
        1,
        "save left a temporary or accidental backup file"
    );
}

#[test]
fn python_writestr_metadata_survives_save_byte_for_byte() {
    // Python's `ZipFile.writestr(name, data)` on Windows records MS-DOS as the
    // producer and `0o600 << 16` without a regular-file type nibble. The
    // previous zip-crate re-synthesis could not reproduce that and rejected
    // the save; the raw COW writer must now keep every header byte.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("python-writestr.hwpx");
    let mut package = build_package(&section("before", "second"));
    for (_, central) in central_entries(&package) {
        package[central + 4..central + 6].copy_from_slice(&[20, 0]);
        package[central + 38..central + 42].copy_from_slice(&(0o600u32 << 16).to_le_bytes());
    }
    std::fs::write(&path, &package).expect("write fixture");
    let before = header_bytes(&package);

    let mut output = Vec::new();
    serve(&path, Cursor::new(frame_lines(&[
        json!({"protocol":1,"msg_type":"open","editable":true}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"after"}}),
        json!({"protocol":1,"msg_type":"save"}),
        json!({"protocol":1,"msg_type":"close"}),
    ])), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert!(
        replies.iter().all(|reply| reply["msg_type"] == "ok"),
        "{replies:#?}"
    );
    let saved = std::fs::read(&path).expect("saved package");
    assert!(read_section(&path).contains(">after<"));
    let after = header_bytes(&saved);
    assert_eq!(before.len(), after.len());
    for ((name, before), (_, after)) in before.iter().zip(&after) {
        if name == SECTION_PART {
            assert_eq!(masked_payload(before), masked_payload(after), "{name}");
        } else {
            assert_eq!(before, after, "{name}");
        }
    }
}

/// (local header offset, central record offset) for each entry.
fn central_entries(package: &[u8]) -> Vec<(usize, usize)> {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("open package");
    (0..archive.len())
        .map(|index| {
            let file = archive.by_index(index).expect("entry");
            (
                usize::try_from(file.header_start()).expect("offset"),
                usize::try_from(file.central_header_start()).expect("offset"),
            )
        })
        .collect()
}

/// Local fixed header and central fixed header (offset masked).
type RawHeaders = (Vec<u8>, Vec<u8>);

/// Entry name, local fixed header, and central fixed header (offset masked).
fn header_bytes(package: &[u8]) -> Vec<(String, RawHeaders)> {
    let names = {
        let archive = ZipArchive::new(Cursor::new(package)).expect("open package");
        archive.file_names().map(str::to_owned).collect::<Vec<_>>()
    };
    central_entries(package)
        .into_iter()
        .zip(names)
        .map(|((local, central), name)| {
            let mut central_fixed = package[central..central + 46].to_vec();
            central_fixed[42..46].fill(0);
            (name, (package[local..local + 30].to_vec(), central_fixed))
        })
        .collect()
}

fn masked_payload((local, central): &RawHeaders) -> RawHeaders {
    let mut local = local.clone();
    let mut central = central.clone();
    local[14..26].fill(0);
    central[16..28].fill(0);
    (local, central)
}

#[test]
fn picture_queries_resolve_only_exact_manifest_parts_and_survive_saves() {
    // Element shape observed in Hancom 2020-2024 packages (rhwp public samples
    // `test-image.hwpx`, `tb-img-03.hwpx`): `hp:pic` with a direct `hc:img`,
    // raw HWPUNIT `hp:orgSz`/`hp:curSz`/`hp:sz`, and an `hp:shapeComment`.
    let png = b"\x89PNG\r\n\x1a\nnot-a-real-image";
    let picture_section = r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core"><hp:p id="1" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>caption target</hp:t></hp:run></hp:p><hp:p id="2" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:pic id="100" zOrder="1"><hp:orgSz width="43440" height="25380"/><hp:curSz width="20304" height="16652"/><hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/><hp:sz width="20305" height="16652"/><hp:shapeComment>그림입니다. 원본 그림의 이름: 예시.png</hp:shapeComment></hp:pic><hp:pic id="101"><hc:img binaryItemIDRef="image9"/></hp:pic></hp:run></hp:p></hs:sec>"#;
    let hpf = |extra: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/><opf:item id="image1" href="BinData/image1.png" media-type="image/png" isEmbeded="1"/>{extra}</opf:manifest><opf:spine><opf:itemref idref="header"/><opf:itemref idref="section0"/></opf:spine></opf:package>"#
        )
    };
    let build = |hpf_xml: &str| {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut cursor);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            writer.start_file("mimetype", stored).unwrap();
            writer.write_all(b"application/hwp+zip").unwrap();
            for (name, body) in [
                ("version.xml", VERSION),
                ("META-INF/manifest.xml", META_MANIFEST),
                ("META-INF/container.xml", CONTAINER),
                ("Contents/content.hpf", hpf_xml),
                ("Contents/header.xml", HEADER),
                (SECTION_PART, picture_section),
            ] {
                writer.start_file(name, deflated).unwrap();
                writer.write_all(body.as_bytes()).unwrap();
            }
            writer.start_file("BinData/image1.png", stored).unwrap();
            writer.write_all(png).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    };
    let query = |path: &Path, editable: bool, extra: Vec<Value>| {
        let mut frames = vec![json!({"protocol":1,"msg_type":"open","editable":editable})];
        frames.extend(extra);
        frames.push(json!({"protocol":1,"msg_type":"command","command":"query","args":{"selector":"//picture"}}));
        frames.push(json!({"protocol":1,"msg_type":"close"}));
        let mut output = Vec::new();
        serve(path, Cursor::new(frame_lines(&frames)), &mut output).unwrap();
        replies(output)
    };

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pictures.hwpx");
    let original = build(&hpf(""));
    std::fs::write(&path, &original).unwrap();
    let result = query(&path, false, Vec::new());
    assert!(result.iter().all(|r| r["msg_type"] == "ok"), "{result:#?}");
    let pictures = result[1]["result"].as_array().unwrap();
    assert_eq!(pictures.len(), 2);
    let first = &pictures[0]["format"];
    assert_eq!(pictures[0]["path"], "/document/section[1]/picture[1]");
    assert_eq!(first["binary_status"], "resolved");
    assert_eq!(first["binary_item_id"], "image1");
    assert_eq!(first["binary_part"], "BinData/image1.png");
    assert_eq!(first["binary_size"], png.len());
    assert_eq!(first["media_type"], "image/png");
    assert_eq!(first["embedded"], true);
    assert_eq!(first["object_id"], "100");
    assert_eq!(
        first["original_size"],
        json!({"width":43440,"height":25380})
    );
    assert_eq!(first["current_size"], json!({"width":20304,"height":16652}));
    assert_eq!(first["size"], json!({"width":20305,"height":16652}));
    assert_eq!(
        first["shape_comment"],
        "그림입니다. 원본 그림의 이름: 예시.png"
    );
    assert_eq!(first["parent_path"], "/document/section[1]/paragraph[2]");
    assert_eq!(first["editable"], false);
    assert_eq!(first["editability"]["reason"], "read_only_session");
    let source = &first["source"];
    let range = source["byte_start"].as_u64().unwrap() as usize
        ..source["byte_end"].as_u64().unwrap() as usize;
    assert!(picture_section[range.clone()].starts_with("<hp:pic id=\"100\""));
    assert!(picture_section[range].ends_with("</hp:pic>"));
    assert_eq!(pictures[1]["format"]["binary_status"], "missing_item");
    assert!(pictures[1]["format"].get("binary_part").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), original);

    // An external link (isEmbeded="0", non-portable href) is reported, never resolved.
    let external = dir.path().join("external.hwpx");
    std::fs::write(
        &external,
        build(&hpf(
            r#"<opf:item id="image9" href="D:\pictures\" media-type="image/" isEmbeded="0"/>"#,
        )),
    )
    .unwrap();
    let result = query(&external, false, Vec::new());
    let linked = &result[1]["result"][1]["format"];
    assert_eq!(linked["binary_status"], "external", "{result:#?}");
    assert_eq!(linked["embedded"], false);
    assert!(linked.get("binary_part").is_none());

    // A text edit and save keep the picture metadata (catalog reloaded from the saved package).
    let result = query(
        &path,
        true,
        vec![
            json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"edited caption"}}),
            json!({"protocol":1,"msg_type":"save"}),
        ],
    );
    assert!(result.iter().all(|r| r["msg_type"] == "ok"), "{result:#?}");
    let after = &result[3]["result"][0]["format"];
    assert_eq!(after["binary_status"], "resolved");
    assert_eq!(after["binary_size"], png.len());
    assert_eq!(after["editability"]["reason"], "no_supported_text_targets");
    assert!(read_section(&path).contains(">edited caption<"));
}

#[test]
fn close_implicitly_saves_a_pending_supported_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("implicit.hwpx");
    std::fs::write(&path, build_package(&section("before", "second"))).expect("write fixture");
    let input = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":path.canonicalize().expect("canonical"),"editable":true}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[2]/text[1]"},"props":{"text":"implicit"}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert!(
        replies.iter().all(|reply| reply["msg_type"] == "ok"),
        "unexpected replies: {replies:#?}"
    );
    assert!(read_section(&path).contains(">implicit<"));
}

#[test]
fn read_only_session_never_advertises_or_accepts_mutation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("readonly.hwpx");
    let original = build_package(&section("before", "second"));
    std::fs::write(&path, &original).expect("write fixture");
    let input = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":path.canonicalize().expect("canonical"),"editable":false}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"forbidden"}}),
        json!({"protocol":1,"msg_type":"save"}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    let commands = replies[0]["result"]["capabilities"]["commands"]
        .as_array()
        .expect("commands");
    assert!(!commands.contains(&json!("set")));
    assert!(!commands.contains(&json!("save")));
    assert!(!replies[0]["result"]["capabilities"]["features"]
        .as_array()
        .expect("features")
        .contains(&json!("strict-g0-g3")));
    assert_eq!(replies[1]["error"]["code"], "unsupported_command");
    assert_eq!(replies[2]["error"]["code"], "unsupported_command");
    assert_eq!(std::fs::read(path).expect("read original"), original);
}

#[test]
fn view_honors_protocol_and_legacy_max_lines_spellings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("view-lines.hwpx");
    std::fs::write(&path, build_package(&section("first", "second"))).expect("write fixture");
    let input = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":path.canonicalize().expect("canonical"),"editable":false}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text","max_lines":1}}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text","max-lines":1}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert_eq!(replies[1]["result"], "first");
    assert_eq!(replies[2]["result"], "first");
}

#[test]
fn permissive_package_is_readable_but_cannot_cross_the_editable_gate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("permissive.hwpx");
    let original = build_permissive_package(&section("readable", "only"));
    std::fs::write(&path, &original).expect("write fixture");
    let canonical = path.canonicalize().expect("canonical");

    let read_only = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":canonical,"editable":false}),
        json!({"protocol":1,"msg_type":"command","command":"view","args":{"mode":"text"}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);
    let mut output = Vec::new();
    serve(&path, Cursor::new(read_only), &mut output).expect("read-only serve");
    let read_replies = replies(output);
    assert_eq!(read_replies.len(), 3);
    assert!(read_replies.iter().all(|reply| reply["msg_type"] == "ok"));
    assert_eq!(read_replies[1]["result"], "readable\nonly");

    let editable = frame_lines(&[json!({
        "protocol":1,
        "msg_type":"open",
        "path":path.canonicalize().expect("canonical"),
        "editable":true
    })]);
    let mut output = Vec::new();
    serve(&path, Cursor::new(editable), &mut output).expect("editable rejection envelope");
    let edit_replies = replies(output);
    assert_eq!(edit_replies.len(), 1);
    assert_eq!(edit_replies[0]["msg_type"], "error");
    assert_eq!(std::fs::read(path).expect("unchanged package"), original);
}

#[test]
fn unadvertised_topology_mutation_fails_without_a_successful_noop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("unsupported.hwpx");
    let original = build_package(&section("before", "second"));
    std::fs::write(&path, &original).expect("write fixture");
    let input = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":path.canonicalize().expect("canonical"),"editable":true}),
        json!({"protocol":1,"msg_type":"command","command":"add","args":{"parent_path":"/document","type":"paragraph"},"props":{"text":"nope"}}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);

    let mut output = Vec::new();
    serve(&path, Cursor::new(input), &mut output).expect("serve protocol");
    let replies = replies(output);
    assert_eq!(replies[1]["msg_type"], "error");
    assert_eq!(replies[1]["error"]["code"], "unsupported_command");
    assert_eq!(std::fs::read(path).expect("read original"), original);
}

#[test]
fn external_source_change_before_save_is_never_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("toctou.hwpx");
    let original = build_package(&section("before", "second"));
    let external = build_package(&section("external", "winner"));
    std::fs::write(&path, &original).expect("write fixture");
    let frames = frame_lines(&[
        json!({"protocol":1,"msg_type":"open","path":path.canonicalize().expect("canonical"),"editable":true}),
        json!({"protocol":1,"msg_type":"command","command":"set","args":{"path":"/document/section[1]/paragraph[1]/text[1]"},"props":{"text":"pending"}}),
        json!({"protocol":1,"msg_type":"save"}),
        json!({"protocol":1,"msg_type":"close"}),
    ]);
    let second_newline = frames
        .match_indices('\n')
        .nth(1)
        .map(|(index, _)| index + 1)
        .expect("two frames") as u64;
    let input = MutatingInput {
        inner: Cursor::new(frames.into_bytes()),
        trigger_offset: second_newline,
        path: path.clone(),
        replacement: external.clone(),
        mutated: false,
    };

    let mut output = Vec::new();
    serve(&path, input, &mut output).expect("serve protocol");
    let replies = replies(output);
    assert_eq!(replies[0]["msg_type"], "ok");
    assert_eq!(replies[1]["msg_type"], "ok");
    assert_eq!(replies[2]["msg_type"], "error");
    assert_eq!(replies[2]["error"]["code"], "internal_error");
    assert!(replies[2]["error"]["message"]
        .as_str()
        .expect("message")
        .contains("source changed"));
    assert_eq!(replies[3]["msg_type"], "error");
    assert_eq!(std::fs::read(&path).expect("read external"), external);
    assert_eq!(
        std::fs::read_dir(dir.path())
            .expect("read tempdir")
            .filter_map(Result::ok)
            .count(),
        1,
        "failed save left a temporary or backup file"
    );
}
