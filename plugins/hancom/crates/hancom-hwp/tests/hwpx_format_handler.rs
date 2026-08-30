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
        json!(["text"])
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
