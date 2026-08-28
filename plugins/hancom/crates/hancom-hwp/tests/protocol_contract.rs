//! 프로토콜 계약을 **실제 바이너리를 실행해서** 검증한다.
//!
//! 검증 대상은 `docs/01-protocol-contract.md`의 C1~C5.
//!
//! 시드 코드는 `#[test]` 안에서 `cargo run`을 호출했다. 그건 테스트 하네스가
//! cargo를 재귀 호출하는 것이라 파일 락 경합과 데드락 위험이 있다.
//! 여기서는 `assert_cmd`가 이미 빌드된 바이너리를 직접 실행한다.

mod common;

use assert_cmd::Command;
use common::*;
use serde_json::Value;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const BIN: &str = "officecli-dump-reader-hwpx";
const CANONICAL_BIN: &str = "officecli-hancom-hwp";
const HANCOM_NOTICE: &str =
    "본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.";

fn plugin() -> Command {
    Command::cargo_bin(BIN).expect("binary is built")
}

fn canonical_plugin() -> Command {
    Command::cargo_bin(CANONICAL_BIN).expect("canonical binary is built")
}

// ─────────────────────────── C1. --info 매니페스트 ───────────────────────────

fn info_manifest() -> Value {
    let out = plugin().arg("--info").assert().success();
    let stdout = &out.get_output().stdout;
    serde_json::from_slice(stdout).expect("--info must print exactly one JSON object")
}

#[test]
fn info_exits_zero_and_prints_single_json_object() {
    // §4: "MUST respond to `<plugin> --info` by printing a single JSON object
    //      to stdout and exiting 0"
    let out = plugin().arg("--info").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf-8");
    assert_eq!(
        stdout.lines().count(),
        1,
        "manifest must be a single line, got:\n{stdout}"
    );
    let v: Value = serde_json::from_str(stdout.trim()).expect("valid json");
    assert!(v.is_object(), "manifest must be an object");
}

#[test]
fn info_declares_required_fields() {
    // §4.1 필수 필드 표
    let m = info_manifest();
    for key in [
        "name",
        "version",
        "protocol",
        "kinds",
        "extensions",
        "idle_timeout_seconds",
        "runtime",
    ] {
        assert!(m.get(key).is_some(), "manifest missing required key: {key}");
    }
}

#[test]
fn info_declares_protocol_one() {
    // §4.1: v1 플러그인은 반드시 1. 불일치면 메인이 exit 5로 거부한다.
    assert_eq!(info_manifest()["protocol"], 1);
}

#[test]
fn canonical_binary_is_the_primary_hancom_hwp_entrypoint() {
    let out = canonical_plugin().arg("--info").assert().success();
    let manifest: Value = serde_json::from_slice(&out.get_output().stdout)
        .expect("canonical binary must print a manifest");
    assert_eq!(manifest["name"], "officecli-hancom-hwp");
}

#[test]
fn info_declares_dump_reader_kind_and_all_hancom_hwp_extensions() {
    let m = info_manifest();
    let kinds: Vec<&str> = m["kinds"]
        .as_array()
        .expect("kinds is array")
        .iter()
        .map(|v| v.as_str().expect("string"))
        .collect();
    assert!(kinds.contains(&"dump-reader"), "kinds = {kinds:?}");

    let exts: Vec<&str> = m["extensions"]
        .as_array()
        .expect("extensions is array")
        .iter()
        .map(|v| v.as_str().expect("string"))
        .collect();
    assert_eq!(
        exts,
        vec![".hwpx", ".owpml", ".hml", ".hwp"],
        "all advertised extensions must include a leading dot"
    );
}

#[test]
fn info_declares_valid_target_for_dump_reader() {
    // §4.1: dump-reader는 target 필수, docx/xlsx/pptx 중 하나
    let m = info_manifest();
    let t = m["target"].as_str().expect("target must exist and be a string");
    assert!(["docx", "xlsx", "pptx"].contains(&t), "target = {t}");
}

#[test]
fn info_idle_timeout_default_is_nonzero_integer() {
    // §4.2: default 필수, 양의 정수, 0 금지
    let m = info_manifest();
    let d = m["idle_timeout_seconds"]["default"]
        .as_u64()
        .expect("default must be an integer");
    assert!(d > 0, "0 is not allowed in the manifest");
}

#[test]
fn info_includes_required_hancom_notice() {
    let manifest = info_manifest();
    let description = manifest["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        description.contains(HANCOM_NOTICE),
        "--info description must include the required Hancom notice"
    );
}

#[test]
fn help_includes_required_hancom_notice() {
    let out = plugin().arg("--help").assert().success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).expect("utf-8 stderr");
    assert!(
        stderr.contains(HANCOM_NOTICE),
        "--help must include the required Hancom notice"
    );
}

#[test]
fn distributed_docs_include_required_hancom_notice() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for relative in ["README.md", "NOTICE"] {
        let text = std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        assert!(
            text.contains(HANCOM_NOTICE),
            "{relative} must include the required Hancom notice"
        );
    }
}

#[test]
fn info_does_not_declare_reserved_kinds() {
    // §2.4: engine / transformer는 v1에서 선언 금지
    let m = info_manifest();
    for k in m["kinds"].as_array().expect("array") {
        let s = k.as_str().expect("string");
        assert!(!["engine", "transformer"].contains(&s), "reserved kind: {s}");
    }
}

#[test]
fn info_uses_snake_case_keys_only() {
    // §5.5: 모든 JSON 키는 snake_case
    fn walk(v: &Value, path: &str) {
        if let Value::Object(map) = v {
            for (k, val) in map {
                assert!(
                    k.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "key not snake_case at {path}: {k}"
                );
                walk(val, &format!("{path}/{k}"));
            }
        }
    }
    walk(&info_manifest(), "");
}

#[test]
fn info_output_has_no_bom() {
    // §5.5: UTF-8 without BOM
    let out = plugin().arg("--info").assert().success();
    let bytes = &out.get_output().stdout;
    assert!(
        !bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
        "stdout must not start with a BOM"
    );
}

#[test]
fn info_output_uses_lf_not_crlf() {
    // §5.5: `\n` on all platforms
    let out = plugin().arg("--info").assert().success();
    assert!(
        !out.get_output().stdout.contains(&b'\r'),
        "stdout must not contain CR"
    );
}

// ─────────────────────────── C2/C3. dump 출력 ───────────────────────────

fn dump_stdout(builder: &HwpxBuilder) -> String {
    let (_dir, path) = builder.write_to_temp("sample.hwpx");
    let out = plugin()
        .arg("dump")
        .arg(&path)
        .assert()
        .success();
    String::from_utf8(out.get_output().stdout.clone()).expect("utf-8 stdout")
}

#[test]
fn dump_emits_one_json_object_per_line() {
    // §2.1: "one batch item per line"
    let stdout = dump_stdout(&simple_doc(&["첫 문단", "둘째 문단"]));
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "expected 2 items, got:\n{stdout}");
    for line in &lines {
        let v: Value = serde_json::from_str(line).expect("each line must be valid JSON");
        assert!(v.is_object(), "each line must be an object, got: {line}");
    }
}

#[test]
fn canonical_binary_reads_owpml_and_single_xml_hml() {
    let (_owpml_dir, owpml) = simple_doc(&["OWPML 확장자"])
        .write_to_temp("sample.owpml");
    let owpml_out = canonical_plugin()
        .arg("dump")
        .arg(&owpml)
        .assert()
        .success();
    assert!(
        String::from_utf8_lossy(&owpml_out.get_output().stdout).contains("OWPML 확장자")
    );

    let hml_dir = tempfile::tempdir().expect("tempdir");
    let hml = hml_dir.path().join("sample.hml");
    std::fs::write(
        &hml,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<HWPML Version="2.91"><HEAD SecCnt="1"><MAPPINGTABLE>
<CHARSHAPELIST Count="1"><CHARSHAPE Id="0" Height="1000"/></CHARSHAPELIST>
<PARASHAPELIST Count="1"><PARASHAPE Id="0" Align="Left"><PARAMARGIN/></PARASHAPE></PARASHAPELIST>
</MAPPINGTABLE></HEAD><BODY><SECTION Id="0"><P ParaShape="0">
<TEXT CharShape="0"><CHAR>HML 단일 XML 성공</CHAR></TEXT>
</P></SECTION></BODY></HWPML>"#
            .as_bytes(),
    )
    .expect("write hml");
    let hml_out = canonical_plugin()
        .arg("dump")
        .arg(&hml)
        .assert()
        .success();
    assert!(
        String::from_utf8_lossy(&hml_out.get_output().stdout).contains("HML 단일 XML 성공")
    );
}

#[test]
fn generic_xml_is_not_misclassified_as_hwpml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("not-hwpml.hml");
    std::fs::write(&path, b"<html><body>HWPML text only</body></html>").expect("write xml");
    let out = canonical_plugin().arg("dump").arg(path).assert().code(2);
    assert!(out.get_output().stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.get_output().stderr).contains("not an HWPML"));
}

#[test]
fn unsupported_hwpml_control_exits_three_without_partial_stdout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("unsupported-control.hml");
    std::fs::write(
        &path,
        r#"<HWPML Version="2.91"><BODY><SECTION>
          <P><TEXT><CHAR>이 문단도 부분 출력되면 안 됨</CHAR></TEXT></P>
          <P><TEXT><EQUATION><SCRIPT>x+y</SCRIPT></EQUATION></TEXT></P>
        </SECTION></BODY></HWPML>"#
            .as_bytes(),
    )
    .expect("write hml");

    let out = canonical_plugin().arg("dump").arg(path).assert().code(3);
    assert!(out.get_output().stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.get_output().stderr).contains("EQUATION")
    );
}

#[test]
fn dump_never_emits_top_level_array() {
    // §5.1: "A top-level JSON array on a single line is rejected with corrupt_batch"
    let stdout = dump_stdout(&simple_doc(&["문단"]));
    assert!(
        !stdout.trim_start().starts_with('['),
        "output must not be a top-level array:\n{stdout}"
    );
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).expect("json");
        assert!(!v.is_array(), "line must not be an array: {line}");
    }
}

#[test]
fn dump_lines_use_documented_batch_item_fields() {
    // wiki command-batch.md "Input Format" 표
    const ALLOWED: &[&str] = &[
        "command", "path", "parent", "type", "from", "index", "after", "before", "to",
        "path2", "props", "selector", "mode", "depth", "part", "xpath", "action", "xml",
    ];
    const COMMANDS: &[&str] = &[
        "get", "query", "set", "add", "remove", "move", "swap", "view", "raw", "raw-set",
        "validate",
    ];

    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let bold = b.char_pr(CharPr::bold());
    let pp = b.para_pr(ParaPr::centered());
    let mut body = para_with_runs(&pp, &[(&cp, "보통 "), (&bold, "굵게")]);
    body.push_str(&table(1, 2, &[CellSpec::new(0, 0, "가"), CellSpec::new(0, 1, "나")]));
    b.section(body);

    let stdout = dump_stdout(&b);
    assert!(!stdout.trim().is_empty(), "expected output");

    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).expect("json");
        let obj = v.as_object().expect("object");

        let cmd = obj
            .get("command")
            .and_then(|c| c.as_str())
            .expect("command is required");
        assert!(COMMANDS.contains(&cmd), "unknown command {cmd}: {line}");

        for k in obj.keys() {
            assert!(ALLOWED.contains(&k.as_str()), "undocumented field {k}: {line}");
        }
    }
}

#[test]
fn dump_add_items_carry_parent_and_type() {
    let stdout = dump_stdout(&simple_doc(&["문단"]));
    let v: Value = serde_json::from_str(stdout.lines().next().expect("a line")).expect("json");
    assert_eq!(v["command"], "add");
    assert_eq!(v["parent"], "/body");
    assert_eq!(v["type"], "paragraph");
    assert_eq!(v["props"]["text"], "문단");
}

#[test]
fn dump_never_emits_raw_newline_inside_prop_values() {
    // `\n`은 문단 경계로 해석되므로 값 안에 raw 개행이 있으면 문서가 어긋난다.
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    b.section(para_with_linebreak(&cp, &pp, "첫줄", "둘째줄"));

    let stdout = dump_stdout(&b);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).expect("json");
        if let Some(props) = v.get("props").and_then(|p| p.as_object()) {
            for (k, val) in props {
                if let Some(s) = val.as_str() {
                    assert!(!s.contains('\n'), "prop {k} contains raw newline: {s:?}");
                }
            }
        }
    }
    // 대신 soft break(\v)로 표현돼야 한다.
    assert!(
        stdout.contains("\\u000b") || stdout.contains('\u{000B}'),
        "line break must survive as a soft break:\n{stdout}"
    );
}

#[test]
fn dump_preserves_korean_text_as_utf8() {
    let stdout = dump_stdout(&simple_doc(&["한글 본문입니다"]));
    assert!(stdout.contains("한글 본문입니다"), "got:\n{stdout}");
}

#[test]
fn dump_stdout_has_no_bom_and_no_crlf() {
    // §5.5
    let (_dir, path) = simple_doc(&["문단"]).write_to_temp("a.hwpx");
    let out = plugin().arg("dump").arg(&path).assert().success();
    let bytes = &out.get_output().stdout;
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "no BOM allowed");
    assert!(!bytes.contains(&b'\r'), "no CR allowed");
    assert_eq!(bytes.last(), Some(&b'\n'), "last line must be terminated");
}

#[test]
fn dump_keeps_diagnostics_off_stdout() {
    // §5.1: 진단은 stderr 또는 --log-file. stdout은 JSONL 전용이다.
    let (_dir, path) = simple_doc(&["문단"]).write_to_temp("a.hwpx");
    let out = plugin().arg("dump").arg(&path).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf-8");
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<Value>(line)
            .unwrap_or_else(|e| panic!("non-JSON line on stdout: {line:?} ({e})"));
    }
    // 기본값에서는 요약이 stderr로 나간다.
    let stderr = String::from_utf8(out.get_output().stderr.clone()).expect("utf-8");
    assert!(stderr.contains("dumped"), "stderr summary missing: {stderr:?}");
}

#[test]
fn private_use_chars_are_reported_but_not_altered() {
    // 실측: 한글은 일부 특수문자를 PUA 코드포인트로 저장한다(U+F0854 등).
    // 매핑을 추측하지 않고 그대로 통과시키되, 사용자가 알 수 있게 알린다.
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    b.section(para(&cp, &pp, "특수문자 \u{F0854}표시\u{F0855} 끝"));
    let (_dir, path) = b.write_to_temp("pua.hwpx");

    let out = plugin().arg("dump").arg(&path).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf-8");
    let stderr = String::from_utf8(out.get_output().stderr.clone()).expect("utf-8");

    // 문자는 그대로 보존돼야 한다. 치환하면 정보가 사라진다.
    assert!(stdout.contains('\u{F0854}'), "PUA char must pass through");
    assert!(stdout.contains('\u{F0855}'));
    // 진단은 stderr로. stdout은 JSONL 전용이다.
    assert!(
        stderr.contains("private-use"),
        "expected a diagnostic, got: {stderr:?}"
    );
    assert!(stderr.contains('2'), "expected the count, got: {stderr:?}");
}

#[test]
fn documents_without_private_use_chars_get_no_such_note() {
    let (_dir, path) = simple_doc(&["보통 텍스트 『인용』"]).write_to_temp("plain.hwpx");
    let out = plugin().arg("dump").arg(&path).assert().success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).expect("utf-8");
    assert!(!stderr.contains("private-use"), "got: {stderr:?}");
}

#[test]
fn quiet_suppresses_stderr_summary() {
    // §5.4: `--quiet` suppresses non-error output
    let (_dir, path) = simple_doc(&["문단"]).write_to_temp("a.hwpx");
    let out = plugin()
        .arg("dump")
        .arg(&path)
        .arg("--quiet")
        .assert()
        .success();
    assert!(
        out.get_output().stderr.is_empty(),
        "--quiet must silence diagnostics"
    );
    assert!(!out.get_output().stdout.is_empty(), "JSONL must still flow");
}

#[test]
fn log_file_receives_diagnostics_instead_of_stderr() {
    // §5.4: `--log-file <path>` appends diagnostics there instead of stderr
    let (dir, path) = simple_doc(&["문단"]).write_to_temp("a.hwpx");
    let log = dir.path().join("plugin.log");
    let out = plugin()
        .arg("dump")
        .arg(&path)
        .arg("--log-file")
        .arg(&log)
        .assert()
        .success();
    assert!(
        out.get_output().stderr.is_empty(),
        "diagnostics must go to the log file, not stderr"
    );
    let contents = std::fs::read_to_string(&log).expect("log file written");
    assert!(contents.contains("dumped"), "log content: {contents:?}");
}

#[test]
fn log_file_cannot_overwrite_the_source() {
    let (_dir, path) = simple_doc(&["원본 보존"]).write_to_temp("source.hwpx");
    let before = std::fs::read(&path).expect("read source before");

    let output = plugin()
        .arg("dump")
        .arg(&path)
        .arg("--log-file")
        .arg(&path)
        .output()
        .expect("run plugin");

    assert!(!output.status.success(), "source/log alias must reject");
    assert_eq!(
        std::fs::read(&path).expect("read source after"),
        before,
        "the read-only source must remain byte-for-byte unchanged"
    );
}

#[cfg(unix)]
#[test]
fn log_file_symlink_to_source_is_rejected() {
    let (dir, path) = simple_doc(&["원본 보존"]).write_to_temp("source.hwpx");
    let log = dir.path().join("source.log");
    std::os::unix::fs::symlink(&path, &log).expect("create symlink");
    let before = std::fs::read(&path).expect("read source before");

    let output = plugin()
        .arg("dump")
        .arg(&path)
        .arg("--log-file")
        .arg(&log)
        .output()
        .expect("run plugin");

    assert!(!output.status.success(), "symlink alias must reject");
    assert_eq!(std::fs::read(&path).expect("read source after"), before);
}

#[cfg(any(unix, windows))]
#[test]
fn log_file_hard_link_to_source_is_rejected() {
    let (dir, path) = simple_doc(&["원본 보존"]).write_to_temp("source.hwpx");
    let log = dir.path().join("source.log");
    std::fs::hard_link(&path, &log).expect("create hard link");
    let before = std::fs::read(&path).expect("read source before");

    let output = plugin()
        .arg("dump")
        .arg(&path)
        .arg("--log-file")
        .arg(&log)
        .output()
        .expect("run plugin");

    assert!(!output.status.success(), "hard-link alias must reject");
    assert_eq!(std::fs::read(&path).expect("read source after"), before);
}

#[cfg(target_os = "linux")]
#[test]
fn dump_accepts_a_real_non_utf8_source_path() {
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().expect("create temp dir");
    let name = std::ffi::OsString::from_vec(b"non-utf8-\xff.hwpx".to_vec());
    let path = dir.path().join(name);
    std::fs::write(&path, simple_doc(&["비 UTF-8 경로"]).build()).expect("write HWPX");

    let output = plugin()
        .arg("dump")
        .arg(&path)
        .output()
        .expect("run plugin");

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(!output.stdout.is_empty(), "valid JSONL must be emitted");
    assert!(
        std::str::from_utf8(&output.stderr)
            .expect("diagnostics stay UTF-8")
            .contains("non-utf8-\u{fffd}.hwpx"),
        "lossy diagnostic must stay printable: {:?}",
        output.stderr
    );
}

#[cfg(unix)]
#[test]
fn diagnostic_path_control_characters_are_escaped() {
    let (_dir, path) = simple_doc(&["문단"]).write_to_temp("safe\nforged.hwpx");
    let output = plugin()
        .arg("dump")
        .arg(&path)
        .output()
        .expect("run plugin");

    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf-8 stderr");
    assert_eq!(
        stderr.lines().count(),
        1,
        "diagnostic must stay on one line"
    );
    assert!(stderr.contains("safe\\nforged.hwpx"), "got: {stderr:?}");
}

#[test]
fn log_file_open_failure_is_reported_on_stderr() {
    let (dir, path) = simple_doc(&["문단"]).write_to_temp("source.hwpx");
    let log = dir.path().join("missing").join("plugin.log");
    let output = plugin()
        .arg("dump")
        .arg(&path)
        .arg("--log-file")
        .arg(&log)
        .output()
        .expect("run plugin");

    assert!(
        output.status.success(),
        "diagnostic delivery failure must not discard valid JSONL"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf-8 stderr");
    assert!(stderr.contains("cannot write log file"), "got: {stderr:?}");
}

#[test]
fn media_dir_option_is_accepted() {
    // §5.1: `--media-dir <dir>`
    let (dir, path) = simple_doc(&["문단"]).write_to_temp("a.hwpx");
    let media = dir.path().join("media");
    std::fs::create_dir_all(&media).expect("mkdir");
    plugin()
        .arg("dump")
        .arg(&path)
        .arg("--media-dir")
        .arg(&media)
        .assert()
        .success();
}

// ─────────────────────────── C5. 종료코드 ───────────────────────────

// ─────────────── 포맷 판별 (docs/04-hwp-support-plan.md H2) ───────────────

/// 최소한의 HWP 5.x 파일. CFB + `FileHeader` 스트림.
///
/// 레이아웃 근거: `edwardkim/rhwp`(MIT) `src/parser/header.rs`.
fn minimal_hwp5(major: u8, minor: u8, flags: u32) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut header = vec![0u8; 256];
    header[..17].copy_from_slice(b"HWP Document File");
    // 32..36 = revision, build, minor, major
    header[32] = 0;
    header[33] = 0;
    header[34] = minor;
    header[35] = major;
    header[36..40].copy_from_slice(&flags.to_le_bytes());

    let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("cfb");
    {
        let mut st = comp.create_stream("/FileHeader").expect("stream");
        st.write_all(&header).expect("write");
    }
    comp.into_inner().into_inner()
}

#[cfg(unix)]
fn fake_converter(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("fake rhwp converter");
    std::fs::write(
        &path,
        r#"#!/bin/sh
set -eu
case "${MOCK_MODE:-}" in
  copy)
    printf '%s\0%s\0%s\0' "$1" "$2" "$3" > "$MOCK_ARGS"
    /bin/cp "$MOCK_HWPX" "$3"
    ;;
  nonzero)
    printf 'converter exploded\n' >&2
    exit 9
    ;;
  invalid)
    printf 'not hwpx' > "$3"
    ;;
  missing)
    exit 0
    ;;
  *)
    printf 'unknown fake converter mode\n' >&2
    exit 10
    ;;
esac
"#,
    )
    .expect("write script");
    let mut permissions = std::fs::metadata(&path)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("chmod script");
    path
}

#[cfg(windows)]
fn fake_converter(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("fake rhwp converter.exe");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("fake_converter.rs");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = std::process::Command::new(rustc)
        .arg("--edition=2021")
        .arg(source)
        .arg("-o")
        .arg(&path)
        .status()
        .expect("run rustc for fake converter");
    assert!(status.success(), "compile fake converter");
    path
}

#[cfg(unix)]
fn read_converter_args(path: &std::path::Path) -> Vec<std::ffi::OsString> {
    use std::os::unix::ffi::OsStringExt;
    std::fs::read(path)
        .expect("converter args")
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| std::ffi::OsString::from_vec(field.to_vec()))
        .collect()
}

#[cfg(windows)]
fn read_converter_args(path: &std::path::Path) -> Vec<std::ffi::OsString> {
    std::fs::read_to_string(path)
        .expect("converter args")
        .lines()
        .map(std::ffi::OsString::from)
        .collect()
}

#[test]
fn binary_hwp5_exits_three_with_actionable_message() {
    // §6.5: 3 = "Feature unsupported in this build".
    // 조용히 실패하거나 zip 오류로 오인시키지 않고 무엇을 해야 하는지 알린다.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("binary.hwp");
    std::fs::write(&path, minimal_hwp5(5, 1, 0x01)).expect("write");

    let out = plugin()
        .arg("dump")
        .arg(&path)
        .env_remove("OFFICECLI_HWPX_CONVERTER")
        .env("PATH", "")
        .env("HOME", dir.path())
        .assert()
        .code(3);
    assert!(
        out.get_output().stdout.is_empty(),
        "no JSONL for an unreadable format"
    );
    let stderr = String::from_utf8(out.get_output().stderr.clone()).expect("utf-8");
    assert!(stderr.contains("HWP 5"), "got: {stderr}");
    assert!(
        stderr.contains("5.1.0.0"),
        "version must be reported: {stderr}"
    );
    assert!(stderr.contains("rhwp"), "must say how to convert: {stderr}");
}

#[test]
fn configured_converter_turns_binary_hwp_into_jsonl_and_cleans_scratch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("binary source with spaces.hwp");
    std::fs::write(&source, minimal_hwp5(5, 1, 0x01)).expect("write source");
    let source_before = std::fs::read(&source).expect("read source");
    let modified_before = std::fs::metadata(&source)
        .expect("source metadata")
        .modified()
        .expect("source mtime");

    let (_fixture_dir, converted) =
        simple_doc(&["변환기 경유 성공"]).write_to_temp("converted fixture.hwpx");
    let args_log = dir.path().join("converter-args");
    let media_dir = dir.path().join("scratch");
    std::fs::create_dir(&media_dir).expect("scratch dir");
    let converter = fake_converter(dir.path());

    let out = plugin()
        .arg("dump")
        .arg(&source)
        .arg("--media-dir")
        .arg(&media_dir)
        .env("OFFICECLI_HWPX_CONVERTER", &converter)
        .env("MOCK_MODE", "copy")
        .env("MOCK_HWPX", &converted)
        .env("MOCK_ARGS", &args_log)
        .assert()
        .success();

    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf-8 JSONL");
    assert!(stdout.contains("변환기 경유 성공"), "got: {stdout}");
    assert_eq!(std::fs::read(&source).expect("source after"), source_before);
    assert_eq!(
        std::fs::metadata(&source)
            .expect("source metadata after")
            .modified()
            .expect("source mtime after"),
        modified_before,
        "converter bridge must not write the source"
    );
    assert_eq!(
        std::fs::read_dir(&media_dir).expect("read scratch").count(),
        0,
        "temporary conversion output must be removed"
    );

    let args = read_converter_args(&args_log);
    assert_eq!(args.len(), 3, "one subcommand and two path arguments");
    assert_eq!(args[0], "export-hwpx");
    assert_eq!(
        std::path::Path::new(&args[1]).file_name().unwrap(),
        "source.hwp"
    );
    assert_ne!(
        std::path::Path::new(&args[1]),
        source,
        "the converter must receive a private staged copy, not the source"
    );
    assert_eq!(
        std::path::Path::new(&args[2]).file_name().unwrap(),
        "converted.hwpx"
    );
    assert!(
        args.iter().all(|arg| arg.to_str().is_some()),
        "RHWP v0.8.4 requires UTF-8 argv"
    );
}

#[cfg(unix)]
#[test]
fn converter_wait_is_independent_of_an_ignored_sigchld() {
    use std::os::unix::process::CommandExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("binary.hwp");
    std::fs::write(&source, minimal_hwp5(5, 1, 0x01)).expect("write source");
    let (_fixture_dir, converted) =
        simple_doc(&["SIGCHLD 무시 환경 성공"]).write_to_temp("converted.hwpx");
    let args_log = dir.path().join("converter-args");
    let converter = fake_converter(dir.path());
    let executable = plugin().get_program().to_os_string();
    let mut command = std::process::Command::new(executable);
    command
        .arg("dump")
        .arg(&source)
        .env("OFFICECLI_HWPX_CONVERTER", &converter)
        .env("MOCK_MODE", "copy")
        .env("MOCK_HWPX", &converted)
        .env("MOCK_ARGS", &args_log);
    // SAFETY: this closure runs after fork and before exec in the new child.
    // `signal` is the only operation and changes only that child process.
    unsafe {
        command.pre_exec(|| {
            if libc::signal(libc::SIGCHLD, libc::SIG_IGN) == libc::SIG_ERR {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    let output = command.output().expect("run plugin with SIGCHLD ignored");
    assert!(
        output.status.success(),
        "plugin must not crash when SIGCHLD is inherited as ignored: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("SIGCHLD 무시 환경 성공"),
        "got: {output:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn binary_hwp_bridge_stages_non_utf8_source_and_media_paths() {
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"binary-\xff.hwp".to_vec()));
    let media_dir = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"scratch-\xfe".to_vec()));
    std::fs::write(&source, minimal_hwp5(5, 1, 0x01)).expect("write source");
    std::fs::create_dir(&media_dir).expect("create non-UTF-8 media dir");
    let source_before = std::fs::read(&source).expect("source before");

    let (_fixture_dir, converted) =
        simple_doc(&["비 UTF-8 브리지 성공"]).write_to_temp("converted.hwpx");
    let args_log = dir.path().join("converter-args");
    let converter = fake_converter(dir.path());
    let out = plugin()
        .arg("dump")
        .arg(&source)
        .arg("--media-dir")
        .arg(&media_dir)
        .env("OFFICECLI_HWPX_CONVERTER", &converter)
        .env("MOCK_MODE", "copy")
        .env("MOCK_HWPX", &converted)
        .env("MOCK_ARGS", &args_log)
        .assert()
        .success();

    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf-8 JSONL");
    assert!(stdout.contains("비 UTF-8 브리지 성공"), "got: {stdout}");
    assert_eq!(std::fs::read(&source).expect("source after"), source_before);
    assert_eq!(
        std::fs::read_dir(&media_dir)
            .expect("read non-UTF-8 media dir")
            .count(),
        0,
        "an unsafe media path must be left clean when system temp is used"
    );
    let args = read_converter_args(&args_log);
    assert!(
        args.iter().all(|arg| arg.to_str().is_some()),
        "RHWP must receive only UTF-8 staged paths: {args:?}"
    );
}

#[test]
fn converter_nonzero_exit_is_corrupt_input_and_keeps_stdout_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("binary.hwp");
    std::fs::write(&source, minimal_hwp5(5, 1, 0x01)).expect("write source");
    let converter = fake_converter(dir.path());

    let out = plugin()
        .arg("dump")
        .arg(&source)
        .env("OFFICECLI_HWPX_CONVERTER", &converter)
        .env("MOCK_MODE", "nonzero")
        .assert()
        .code(2);
    assert!(out.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("converter exploded"), "got: {stderr}");
    assert!(stderr.contains("status 9"), "got: {stderr}");
}

#[test]
fn converter_output_is_revalidated_as_hwpx() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("binary.hwp");
    std::fs::write(&source, minimal_hwp5(5, 1, 0x01)).expect("write source");
    let converter = fake_converter(dir.path());

    let out = plugin()
        .arg("dump")
        .arg(&source)
        .env("OFFICECLI_HWPX_CONVERTER", &converter)
        .env("MOCK_MODE", "invalid")
        .assert()
        .code(2);
    assert!(out.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("not HWPX"), "got: {stderr}");
}

#[test]
fn converter_success_without_an_output_is_corrupt_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("binary.hwp");
    std::fs::write(&source, minimal_hwp5(5, 1, 0x01)).expect("write source");
    let converter = fake_converter(dir.path());

    let out = plugin()
        .arg("dump")
        .arg(&source)
        .env("OFFICECLI_HWPX_CONVERTER", &converter)
        .env("MOCK_MODE", "missing")
        .assert()
        .code(2);
    assert!(out.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("did not create its output"),
        "got: {stderr}"
    );
}

#[test]
fn configured_converter_path_must_be_absolute() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("binary.hwp");
    std::fs::write(&source, minimal_hwp5(5, 1, 0x01)).expect("write source");

    let out = plugin()
        .arg("dump")
        .arg(&source)
        .env("OFFICECLI_HWPX_CONVERTER", "relative-rhwp")
        .env("PATH", "")
        .env("HOME", dir.path())
        .assert()
        .code(3);
    assert!(out.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("must be an absolute path"), "got: {stderr}");
}

#[test]
fn protected_hwp5_reports_the_protection() {
    // 암호/DRM 문서는 변환기도 실패할 수 있으므로 미리 알린다.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("locked.hwp");
    // 0x01 compressed | 0x02 encrypted
    std::fs::write(&path, minimal_hwp5(5, 0, 0x03)).expect("write");

    let out = plugin()
        .arg("dump")
        .arg(&path)
        .env_remove("OFFICECLI_HWPX_CONVERTER")
        .env("PATH", "")
        .env("HOME", dir.path())
        .env("USERPROFILE", dir.path())
        .assert()
        .code(3);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).expect("utf-8");
    assert!(stderr.contains("password-encrypted"), "got: {stderr}");
}

#[test]
fn extension_is_not_trusted_hwpx_named_hwp_still_works() {
    // 확장자를 믿지 않는다. HWPX 내용이면 확장자가 무엇이든 처리한다.
    let (_dir, path) = simple_doc(&["확장자가 틀린 문서"]).write_to_temp("mislabeled.hwp");
    let out = plugin().arg("dump").arg(&path).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf-8");
    assert!(stdout.contains("확장자가 틀린 문서"), "got: {stdout}");
}

#[test]
fn zip_that_is_not_hwpx_exits_two_naming_the_cause() {
    // `.docx`도 ZIP이다. "zip을 못 읽는다"가 아니라 원인을 말해야 한다.
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("actually_docx.hwpx");
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", opts).expect("start");
        zip.write_all(b"<w:document/>").expect("write");
        zip.finish().expect("finish");
    }
    std::fs::write(&path, buf.into_inner()).expect("write");

    let out = plugin().arg("dump").arg(&path).assert().code(2);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).expect("utf-8");
    assert!(stderr.contains("not an HWPX"), "got: {stderr}");
}

#[test]
fn corrupt_input_exits_two() {
    // §6.5: 2 = Corrupt input file
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("broken.hwpx");
    std::fs::write(&path, b"definitely not a zip archive").expect("write");

    let out = plugin().arg("dump").arg(&path).assert().code(2);
    assert!(
        out.get_output().stdout.is_empty(),
        "no JSONL should be emitted for corrupt input"
    );
}

#[test]
fn missing_file_exits_two() {
    plugin()
        .arg("dump")
        .arg("/nonexistent/path/to/file.hwpx")
        .assert()
        .code(2);
}

#[test]
fn zip_without_sections_exits_two() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    let (_dir, path) = b.write_to_temp("nosection.hwpx");
    plugin().arg("dump").arg(&path).assert().code(2);
}

#[test]
fn never_exits_with_host_reserved_code_six() {
    // §6.5: 6은 호스트가 부과한다. 플러그인은 어떤 경우에도 6을 내지 않는다.
    let dir = tempfile::tempdir().expect("tempdir");
    let broken = dir.path().join("broken.hwpx");
    std::fs::write(&broken, b"garbage").expect("write");

    let cases: Vec<Vec<String>> = vec![
        vec!["--info".into()],
        vec!["dump".into(), broken.display().to_string()],
        vec!["dump".into()],
        vec!["bogus-subcommand".into()],
        vec![
            "dump".into(),
            broken.display().to_string(),
            "--turbo".into(),
        ],
        vec![],
    ];
    for args in cases {
        let out = plugin().args(&args).assert();
        let code = out.get_output().status.code().expect("exited normally");
        assert_ne!(code, 6, "args {args:?} produced host-reserved exit code 6");
    }
}

#[test]
fn unknown_subcommand_fails_without_polluting_stdout() {
    let out = plugin().arg("export").assert().failure();
    assert!(
        out.get_output().stdout.is_empty(),
        "errors must not write to stdout"
    );
    assert!(
        !out.get_output().stderr.is_empty(),
        "error must be reported"
    );
}

#[test]
fn help_does_not_pollute_stdout() {
    // stdout은 JSONL/매니페스트 전용이다.
    let out = plugin().arg("--help").assert().success();
    assert!(out.get_output().stdout.is_empty(), "help must go to stderr");
}

#[test]
fn empty_document_exits_zero_with_no_output() {
    // 본문이 빈 섹션 하나. 실패가 아니라 빈 결과여야 한다.
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section("");
    let (_dir, path) = b.write_to_temp("empty.hwpx");
    let out = plugin().arg("dump").arg(&path).assert().success();
    assert!(out.get_output().stdout.is_empty());
}
