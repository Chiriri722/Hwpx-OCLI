//! Long-lived protocol-v1 format-handler for package-preserving HWPX edits.
//!
//! The session deliberately exposes a closed surface: semantic reads, raw XML
//! reads, and direct `hp:p/hp:run/hp:t` text replacement. Topology-changing
//! verbs stay absent from capabilities and fail with `unsupported_command`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use serde::Serialize;
use serde_json::{json, Map, Value};
use tempfile::NamedTempFile;
use zip::ZipArchive;

use crate::error::{ExitCode, PluginError, Result};
use crate::owpml::conformance::validate_output_package;
use crate::owpml::editor::{
    replace_text_node, rewrite_and_verify, ExactTextExpectation, MutationPlan, PackageBaseline,
    PackageSnapshot, SemanticExpectation, TextNodeSelector,
};
use crate::owpml::package::{Package, MAX_XML_ENTRY_BYTES};

const PROTOCOL_VERSION: u64 = 1;
const FORMAT_HANDLER_NAME: &str = "officecli-hancom-hwpx";
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const PARAGRAPH_NAMESPACE: &[u8] = b"http://www.hancom.co.kr/hwpml/2011/paragraph";
const HELP_TEXT: &str = concat!(
    "officecli-hancom-hwpx — package-preserving HWPX format-handler\n",
    "\n",
    "Usage:\n",
    "  officecli-hancom-hwpx --info\n",
    "  officecli-hancom-hwpx open <file>\n",
);

/// Manifest for the split HWPX/OWPML format-handler entry point.
pub fn format_handler_manifest() -> Value {
    json!({
        "name": FORMAT_HANDLER_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL_VERSION,
        "kinds": ["format-handler"],
        "extensions": [".hwpx", ".owpml"],
        "runtime": "rust",
        "idle_timeout_seconds": {
            "default": 60,
            "verbs": { "open": 30, "save": 120 }
        },
        "description": "Package-preserving Hancom HWPX/OWPML format-handler with a closed text-edit subset. 본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.",
        "license": "MIT",
        "supports": [
            "package-preserving",
            "strict-g0-g3",
            "text-node-edit"
        ],
        "vocabulary": vocabulary()
    })
}

pub fn format_handler_manifest_line() -> String {
    serde_json::to_string(&format_handler_manifest()).expect("static manifest is serializable")
}

fn vocabulary() -> Value {
    json!({
        "addable_types": [],
        "settable_props": { "text": ["text"] },
        "path_segments": ["document", "section", "paragraph", "text"]
    })
}

/// Format-handler process entry point used by the dedicated binary.
pub fn main_entry() -> std::process::ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr();
    let result = run_args(args, &mut stdout, &mut stderr);
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
    let _ = stdout.flush();
    let _ = stderr.flush();
    std::process::ExitCode::from(u8::try_from(code.as_i32()).unwrap_or(1))
}

fn run_args<O: Write, E: Write>(args: Vec<OsString>, stdout: &mut O, stderr: &mut E) -> Result<()> {
    match args.as_slice() {
        [arg] if arg == OsStr::new("--info") || arg == OsStr::new("--version") => {
            writeln!(stdout, "{}", format_handler_manifest_line())?;
            stdout.flush()?;
            Ok(())
        }
        [] => {
            write!(stderr, "{HELP_TEXT}")?;
            Ok(())
        }
        [arg] if arg == OsStr::new("--help") || arg == OsStr::new("-h") => {
            write!(stderr, "{HELP_TEXT}")?;
            Ok(())
        }
        [command, path] if command == OsStr::new("open") => {
            let stdin = io::stdin();
            serve(Path::new(path), stdin.lock(), stdout)
        }
        [command, ..] if command == OsStr::new("open") => Err(PluginError::invalid_argument(
            "open requires exactly one <file> argument",
        )),
        _ => Err(PluginError::unsupported_command(
            "expected --info, --help, or open <file>",
        )),
    }
}

/// Serve one document session over protocol-v1 JSONL.
pub fn serve<R: BufRead, W: Write>(path: &Path, mut input: R, output: &mut W) -> Result<()> {
    let first = match read_request(&mut input) {
        Ok(Some(request)) => request,
        Ok(None) => {
            return Err(PluginError::corrupt(
                "format-handler stdin closed before the open handshake",
            ));
        }
        Err(error) => {
            write_error(output, WireError::from(error))?;
            return Ok(());
        }
    };

    let (request_path, editable) = match parse_open(&first) {
        Ok(open) => open,
        Err(error) => {
            write_error(output, error)?;
            return Ok(());
        }
    };
    let canonical_cli_path = fs::canonicalize(path).map_err(|error| {
        PluginError::corrupt(format!(
            "cannot open HWPX source {}: {error}",
            path.display()
        ))
    })?;
    if let Some(request_path) = request_path {
        let canonical_request_path = fs::canonicalize(&request_path).map_err(|error| {
            PluginError::corrupt(format!(
                "cannot resolve open-handshake path {}: {error}",
                request_path.display()
            ))
        })?;
        if canonical_request_path != canonical_cli_path {
            write_error(
                output,
                WireError::invalid_argument(
                    "open-handshake path does not match the CLI source path",
                ),
            )?;
            return Ok(());
        }
    }

    let mut session = match HwpxSession::open(canonical_cli_path, editable) {
        Ok(session) => session,
        Err(error) => {
            write_error(output, WireError::from(error))?;
            return Ok(());
        }
    };
    write_ok(output, session.open_result())?;

    loop {
        let request = match read_request(&mut input) {
            Ok(Some(request)) => request,
            Ok(None) => {
                if session.dirty() {
                    return Err(PluginError::corrupt(
                        "format-handler stdin closed with unsaved mutations; source was not replaced",
                    ));
                }
                return Err(PluginError::corrupt(
                    "format-handler stdin closed before a close frame",
                ));
            }
            Err(error) => {
                write_error(output, WireError::from(error))?;
                continue;
            }
        };

        let should_close = request
            .get("msg_type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "close");
        let reply = session.dispatch(&request);
        match reply {
            Ok(result) => write_ok(output, result)?,
            Err(error) => write_error(output, error)?,
        }
        if should_close {
            break;
        }
    }
    Ok(())
}

fn read_request<R: BufRead>(input: &mut R) -> Result<Option<Value>> {
    let Some(mut line) = read_jsonl_frame(input)? else {
        return Ok(None);
    };
    if line.ends_with(b"\r\n") {
        return Err(PluginError::invalid_argument(
            "protocol frames must use LF rather than CRLF",
        ));
    }
    debug_assert_eq!(line.last(), Some(&b'\n'));
    line.pop();
    if line.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(PluginError::invalid_argument(
            "protocol frames must not contain a UTF-8 BOM",
        ));
    }
    let line = std::str::from_utf8(&line).map_err(|error| {
        PluginError::invalid_argument(format!("JSONL frame is not valid UTF-8: {error}"))
    })?;
    let value = serde_json::from_str::<Value>(line)
        .map_err(|error| PluginError::invalid_argument(format!("invalid JSONL frame: {error}")))?;
    if !value.is_object() {
        return Err(PluginError::invalid_argument(
            "protocol frame must be a JSON object",
        ));
    }
    Ok(Some(value))
}

fn read_jsonl_frame<R: BufRead>(input: &mut R) -> Result<Option<Vec<u8>>> {
    let mut frame = Vec::with_capacity(4096);
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            if frame.is_empty() {
                return Ok(None);
            }
            return Err(PluginError::invalid_argument(
                "protocol frame ended before its LF terminator",
            ));
        }

        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            let consumed = newline + 1;
            if frame.len().saturating_add(consumed) > MAX_FRAME_BYTES {
                input.consume(consumed);
                return Err(frame_too_large());
            }
            frame.extend_from_slice(&available[..consumed]);
            input.consume(consumed);
            return Ok(Some(frame));
        }

        let consumed = available.len();
        if frame.len().saturating_add(consumed) > MAX_FRAME_BYTES {
            input.consume(consumed);
            drain_through_lf(input)?;
            return Err(frame_too_large());
        }
        frame.extend_from_slice(available);
        input.consume(consumed);
    }
}

fn drain_through_lf<R: BufRead>(input: &mut R) -> Result<()> {
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |newline| newline + 1);
        let found_newline = consumed <= available.len() && available[consumed - 1] == b'\n';
        input.consume(consumed);
        if found_newline {
            return Ok(());
        }
    }
}

fn frame_too_large() -> PluginError {
    PluginError::invalid_argument(format!("JSONL frame exceeds {MAX_FRAME_BYTES} bytes"))
}

fn parse_open(request: &Value) -> std::result::Result<(Option<PathBuf>, bool), WireError> {
    validate_protocol(request)?;
    if string_field(request, "msg_type")? != "open" {
        return Err(WireError::invalid_request(
            "the first protocol frame must have msg_type=open",
        ));
    }
    // Hosts predating the protocol-v1 lifecycle fix placed the two open-only
    // fields inside `args`. Accept exactly that known shape, but never merge
    // it with canonical top-level fields or ignore additions: either behavior
    // would make precedence and future protocol evolution ambiguous.
    let legacy_args = match request.get("args") {
        None => None,
        Some(Value::Object(args)) => {
            if request.get("path").is_some() || request.get("editable").is_some() {
                return Err(WireError::invalid_request(
                    "open frame cannot mix top-level lifecycle fields with legacy args",
                ));
            }
            if args.len() != 2 || !args.contains_key("path") || !args.contains_key("editable") {
                return Err(WireError::invalid_request(
                    "legacy open.args must contain exactly path and editable",
                ));
            }
            Some(args)
        }
        Some(_) => {
            return Err(WireError::invalid_request(
                "legacy open.args must be an object",
            ));
        }
    };

    // The process-level `open <file>` argument is the authoritative source
    // identity. Protocol-v1 hosts normally repeat that value in the first
    // JSONL frame, but released hosts have also omitted the redundant field.
    // Accept only true absence for compatibility: a present value must remain
    // a string and is canonicalized and identity-checked by `serve` above.
    let path_value = legacy_args
        .and_then(|args| args.get("path"))
        .or_else(|| request.get("path"));
    let path = match path_value {
        None => None,
        Some(Value::String(value)) => Some(PathBuf::from(value)),
        received => {
            return Err(WireError::invalid_request(format!(
                "path must be a string (received {})",
                json_value_kind(received)
            )));
        }
    };
    // A released host may omit this hint for read-only commands such as view.
    // Absence must never grant write access: default to a read-only session,
    // while preserving strict validation whenever the field is present.
    let editable_value = legacy_args
        .and_then(|args| args.get("editable"))
        .or_else(|| request.get("editable"));
    let editable = match editable_value {
        None => false,
        Some(Value::Bool(editable)) => *editable,
        Some(_) => {
            return Err(WireError::invalid_request(
                "open.editable must be a boolean",
            ));
        }
    };
    Ok((path, editable))
}

fn validate_protocol(request: &Value) -> std::result::Result<(), WireError> {
    match request.get("protocol").and_then(Value::as_u64) {
        Some(PROTOCOL_VERSION) => Ok(()),
        _ => Err(WireError::new(
            "protocol_mismatch",
            "format-handler requires protocol version 1",
        )),
    }
}

fn string_field<'a>(value: &'a Value, name: &str) -> std::result::Result<&'a str, WireError> {
    match value.get(name) {
        Some(Value::String(value)) => Ok(value),
        received => Err(WireError::invalid_request(format!(
            "{name} must be a string (received {})",
            json_value_kind(received)
        ))),
    }
}

fn json_value_kind(value: Option<&Value>) -> &'static str {
    match value {
        None => "missing",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

fn object_field<'a>(
    value: &'a Value,
    name: &str,
) -> std::result::Result<Option<&'a Map<String, Value>>, WireError> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(object)) => Ok(Some(object)),
        Some(_) => Err(WireError::invalid_request(format!(
            "{name} must be a JSON object"
        ))),
    }
}

fn write_ok<W: Write>(output: &mut W, result: Value) -> Result<()> {
    serde_json::to_writer(
        &mut *output,
        &json!({"protocol": PROTOCOL_VERSION, "msg_type": "ok", "result": result}),
    )
    .map_err(|error| PluginError::internal(format!("cannot serialize protocol reply: {error}")))?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn write_error<W: Write>(output: &mut W, error: WireError) -> Result<()> {
    serde_json::to_writer(
        &mut *output,
        &json!({
            "protocol": PROTOCOL_VERSION,
            "msg_type": "error",
            "error": {"code": error.code, "message": error.message}
        }),
    )
    .map_err(|failure| {
        PluginError::internal(format!("cannot serialize protocol error reply: {failure}"))
    })?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

#[derive(Debug)]
struct WireError {
    code: String,
    message: String,
}

impl WireError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new("invalid_argument", message)
    }

    fn unsupported_command(message: impl Into<String>) -> Self {
        Self::new("unsupported_command", message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }
}

impl From<PluginError> for WireError {
    fn from(error: PluginError) -> Self {
        Self::new(error.code.as_str(), error.message)
    }
}

struct HwpxSession {
    path: PathBuf,
    editable: bool,
    baseline: Option<PackageBaseline>,
    section_paths: Vec<String>,
    source_parts: BTreeMap<String, Vec<u8>>,
    replacements: BTreeMap<String, Vec<u8>>,
    pending: BTreeMap<String, PendingText>,
    index: Vec<SectionIndex>,
}

#[derive(Clone)]
struct PendingText {
    part: String,
    selector: TextNodeSelector,
    expected: String,
}

impl HwpxSession {
    fn open(path: PathBuf, editable: bool) -> Result<Self> {
        // The ordinary reader intentionally remains more permissive than the
        // writer profile. Only an editable session needs a strict G0-G2
        // baseline and may advertise set/save.
        let baseline = if editable {
            Some(PackageBaseline::capture(BufReader::new(File::open(
                &path,
            )?))?)
        } else {
            None
        };
        let package = Package::open(BufReader::new(File::open(&path)?))?;
        let section_paths = package.section_paths().to_vec();
        drop(package);
        if section_paths.is_empty() {
            return Err(PluginError::corrupt("HWPX package has no section parts"));
        }
        let source_parts = load_section_parts(&path, &section_paths)?;
        let index = build_index(&section_paths, &source_parts)?;
        Ok(Self {
            path,
            editable,
            baseline,
            section_paths,
            source_parts,
            replacements: BTreeMap::new(),
            pending: BTreeMap::new(),
            index,
        })
    }

    fn dirty(&self) -> bool {
        !self.replacements.is_empty()
    }

    fn open_result(&self) -> Value {
        let mut commands = vec!["view", "get", "query", "validate", "raw"];
        let mut features = vec!["package-preserving"];
        if self.editable {
            commands.extend(["set", "save"]);
            features.extend(["strict-g0-g3", "save", "text-node-edit"]);
        }
        json!({
            "capabilities": {
                "commands": commands,
                "features": features
            },
            "vocabulary": vocabulary()
        })
    }

    fn dispatch(&mut self, request: &Value) -> std::result::Result<Value, WireError> {
        validate_protocol(request)?;
        match string_field(request, "msg_type")? {
            "command" => self.command(request),
            "save" => {
                if !self.editable {
                    return Err(WireError::unsupported_command(
                        "read-only HWPX sessions do not implement save",
                    ));
                }
                self.save().map_err(WireError::from)?;
                Ok(Value::Null)
            }
            "close" => {
                if self.dirty() {
                    if !self.editable {
                        return Err(WireError::unsupported_command(
                            "read-only HWPX session contains an impossible mutation",
                        ));
                    }
                    self.save().map_err(WireError::from)?;
                }
                Ok(Value::Null)
            }
            "ping" => Ok(Value::Null),
            "open" => Err(WireError::invalid_request(
                "a format-handler session accepts exactly one open frame",
            )),
            other => Err(WireError::invalid_request(format!(
                "unknown msg_type {other:?}"
            ))),
        }
    }

    fn command(&mut self, request: &Value) -> std::result::Result<Value, WireError> {
        let command = string_field(request, "command")?;
        let args = object_field(request, "args")?.cloned().unwrap_or_default();
        let props = object_field(request, "props")?.cloned().unwrap_or_default();
        match command {
            "view" => self.view(&args),
            "get" => self.get(&args),
            "query" => self.query(&args),
            "validate" => self.validate(),
            "raw" => self.raw(&args),
            "set" if self.editable => self.set(&args, &props),
            "set" => Err(WireError::unsupported_command(
                "read-only HWPX sessions do not implement set",
            )),
            "save" => Err(WireError::invalid_request(
                "save uses msg_type=save rather than a command envelope",
            )),
            "add" | "remove" | "move" | "copy" | "raw_set" | "add_part" | "extract_binary" => {
                Err(WireError::unsupported_command(format!(
                    "HWPX closed edit subset does not implement {command}"
                )))
            }
            _ => Err(WireError::unsupported_command(format!(
                "unknown or unimplemented HWPX command {command:?}"
            ))),
        }
    }

    fn document_tree(&self) -> DocumentNode {
        let sections = self
            .index
            .iter()
            .enumerate()
            .map(|(section_index, section)| section.node(section_index))
            .collect::<Vec<_>>();
        DocumentNode::branch("/document", "document", sections)
    }

    fn get(&self, args: &Map<String, Value>) -> std::result::Result<Value, WireError> {
        let path = map_string(args, "path")?;
        let depth = map_usize_or(args, "depth", 1)?;
        let root = self.document_tree();
        let node = find_node(&root, path)
            .cloned()
            .ok_or_else(|| WireError::not_found(format!("HWPX path {path:?} does not exist")))?;
        serde_json::to_value(node.truncated(depth))
            .map_err(|error| WireError::new("internal_error", error.to_string()))
    }

    fn query(&self, args: &Map<String, Value>) -> std::result::Result<Value, WireError> {
        let selector = map_string(args, "selector")?;
        let normalized = selector.strip_prefix("//").unwrap_or(selector);
        let root = self.document_tree();
        let mut nodes = Vec::new();
        flatten_nodes(&root, &mut nodes);
        let selected = if selector.starts_with('/') {
            nodes
                .into_iter()
                .filter(|node| node.path == selector)
                .cloned()
                .collect::<Vec<_>>()
        } else if ["document", "section", "paragraph", "text"].contains(&normalized) {
            nodes
                .into_iter()
                .filter(|node| node.kind == normalized)
                .map(|node| node.clone().truncated(0))
                .collect::<Vec<_>>()
        } else {
            return Err(WireError::invalid_argument(format!(
                "unsupported HWPX selector {selector:?}; use a path or document/section/paragraph/text"
            )));
        };
        serde_json::to_value(selected)
            .map_err(|error| WireError::new("internal_error", error.to_string()))
    }

    fn view(&self, args: &Map<String, Value>) -> std::result::Result<Value, WireError> {
        let mode = args.get("mode").and_then(Value::as_str).unwrap_or("text");
        let paragraphs = self.paragraph_summaries();
        match mode {
            "text" => {
                let lines = slice_lines(paragraphs, args)?;
                let text = lines
                    .iter()
                    .map(|(_, text)| text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if args.get("format").and_then(Value::as_str) == Some("json") {
                    Ok(
                        json!({"text": text, "lines": lines.iter().map(|(_, line)| line).collect::<Vec<_>>() }),
                    )
                } else {
                    Ok(Value::String(text))
                }
            }
            "annotated" => {
                let lines = slice_lines(paragraphs, args)?;
                Ok(Value::String(
                    lines
                        .into_iter()
                        .map(|(path, text)| format!("{path}: {text}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ))
            }
            "outline" => {
                if args.get("format").and_then(Value::as_str) == Some("json") {
                    Ok(json!({"items": []}))
                } else {
                    Ok(Value::String(String::new()))
                }
            }
            "stats" => {
                let paragraph_count = paragraphs.len();
                let text_count = self
                    .index
                    .iter()
                    .flat_map(|section| &section.paragraphs)
                    .map(|paragraph| paragraph.texts.len())
                    .sum::<usize>();
                let stats = json!({
                    "sections": self.index.len(),
                    "paragraphs": paragraph_count,
                    "text_nodes": text_count
                });
                if args.get("format").and_then(Value::as_str) == Some("json") {
                    Ok(stats)
                } else {
                    Ok(Value::String(format!(
                        "sections: {}\nparagraphs: {paragraph_count}\ntext_nodes: {text_count}",
                        self.index.len()
                    )))
                }
            }
            "issues" => Ok(json!([])),
            _ => Err(WireError::unsupported_command(format!(
                "HWPX view mode {mode:?} is not implemented"
            ))),
        }
    }

    fn paragraph_summaries(&self) -> Vec<(String, String)> {
        self.index
            .iter()
            .enumerate()
            .flat_map(|(section_index, section)| {
                section.paragraphs.iter().map(move |paragraph| {
                    (
                        paragraph_path(section_index, paragraph.ordinal),
                        paragraph
                            .texts
                            .iter()
                            .map(|text| text.value.as_str())
                            .collect::<String>(),
                    )
                })
            })
            .collect()
    }

    fn validate(&self) -> std::result::Result<Value, WireError> {
        let result = if self.dirty() {
            self.verified_candidate_bytes().map(|_| ())
        } else {
            validate_output_package(BufReader::new(
                File::open(&self.path).map_err(PluginError::from)?,
            ))
        };
        Ok(match result {
            Ok(()) => json!([]),
            Err(error) => json!([{
                "error_type": error.code.as_str(),
                "description": error.message,
                "path": "/document",
                "part": null
            }]),
        })
    }

    fn raw(&self, args: &Map<String, Value>) -> std::result::Result<Value, WireError> {
        if args.contains_key("start_row")
            || args.contains_key("end_row")
            || args.contains_key("cols")
        {
            return Err(WireError::new(
                "unsupported_feature",
                "HWPX raw reads do not implement spreadsheet row/column slicing",
            ));
        }
        let requested = map_string(args, "part_path")?;
        let part = normalize_part_name(requested)?;
        self.verify_source_unchanged().map_err(WireError::from)?;
        let bytes = self.read_part_current(&part).map_err(WireError::from)?;
        let text = String::from_utf8(bytes).map_err(|_| {
            WireError::new(
                "unsupported_feature",
                format!("package part {part:?} is not UTF-8 text"),
            )
        })?;
        Ok(Value::String(text))
    }

    fn set(
        &mut self,
        args: &Map<String, Value>,
        props: &Map<String, Value>,
    ) -> std::result::Result<Value, WireError> {
        let path = map_string(args, "path")?;
        let unsupported = props
            .keys()
            .filter(|key| key.as_str() != "text")
            .cloned()
            .collect::<Vec<_>>();
        let replacement = props.get("text").and_then(Value::as_str);
        let Some(replacement) = replacement else {
            if props.contains_key("text") {
                return Err(WireError::invalid_argument(
                    "text property must be a string",
                ));
            }
            if props.is_empty() {
                return Err(WireError::invalid_argument(
                    "HWPX set requires at least one property",
                ));
            }
            return Ok(json!({"unsupported_properties": unsupported}));
        };
        let target = self.find_text_target(path).cloned().ok_or_else(|| {
            WireError::not_found(format!("editable HWPX text path {path:?} does not exist"))
        })?;
        if !target.editable {
            return Err(WireError::new(
                "unsupported_feature",
                "target paragraph is outside the direct plain hp:p/hp:run/hp:t subset",
            ));
        }
        let current_part = self
            .read_part_current(&target.part)
            .map_err(WireError::from)?;
        let patched =
            replace_text_node(&current_part, &target.selector, &target.value, replacement)
                .map_err(WireError::from)?;

        let source_part = self.source_parts.get(&target.part).ok_or_else(|| {
            WireError::new(
                "internal_error",
                "editable section source bytes are missing",
            )
        })?;
        if &patched == source_part {
            self.replacements.remove(&target.part);
            self.pending
                .retain(|_, pending| pending.part != target.part);
        } else {
            self.replacements.insert(target.part.clone(), patched);
            self.pending.insert(
                path.to_owned(),
                PendingText {
                    part: target.part,
                    selector: target.selector,
                    expected: replacement.to_owned(),
                },
            );
        }
        self.index = build_index(&self.section_paths, &self.current_section_parts())
            .map_err(WireError::from)?;
        Ok(json!({"unsupported_properties": unsupported}))
    }

    fn find_text_target(&self, path: &str) -> Option<&TextIndex> {
        self.index
            .iter()
            .flat_map(|section| &section.paragraphs)
            .flat_map(|paragraph| &paragraph.texts)
            .find(|target| target.path == path)
    }

    fn current_section_parts(&self) -> BTreeMap<String, Vec<u8>> {
        self.source_parts
            .iter()
            .map(|(part, source)| {
                (
                    part.clone(),
                    self.replacements
                        .get(part)
                        .cloned()
                        .unwrap_or_else(|| source.clone()),
                )
            })
            .collect()
    }

    fn read_part_current(&self, part: &str) -> Result<Vec<u8>> {
        if let Some(replacement) = self.replacements.get(part) {
            return Ok(replacement.clone());
        }
        read_zip_part(&self.path, part)
    }

    fn verify_source_unchanged(&self) -> Result<()> {
        let Some(baseline) = &self.baseline else {
            return Ok(());
        };
        let current = PackageSnapshot::capture(BufReader::new(File::open(&self.path)?))?;
        if &current != baseline.snapshot() {
            return Err(PluginError::internal(
                "HWPX source changed outside this format-handler session",
            ));
        }
        Ok(())
    }

    fn semantic_expectations(&self) -> Vec<ExactTextExpectation<'_>> {
        self.pending
            .values()
            .map(|pending| ExactTextExpectation {
                part: &pending.part,
                selector: &pending.selector,
                expected: &pending.expected,
            })
            .collect()
    }

    fn verified_candidate_bytes(&self) -> Result<Vec<u8>> {
        if !self.dirty() {
            self.verify_source_unchanged()?;
            return fs::read(&self.path).map_err(PluginError::from);
        }
        let baseline = self.baseline.as_ref().ok_or_else(|| {
            PluginError::internal("dirty HWPX session has no strict editable baseline")
        })?;
        let plan = MutationPlan::replace_exact(baseline.snapshot(), &self.replacements)?;
        let expectations = self.semantic_expectations();
        let source = BufReader::new(File::open(&self.path)?);
        let destination = Cursor::new(Vec::new());
        let (candidate, _) = rewrite_and_verify(
            baseline,
            source,
            destination,
            &plan,
            &self.replacements,
            SemanticExpectation::ExactTexts(&expectations),
        )?;
        Ok(candidate.into_inner())
    }

    fn save(&mut self) -> Result<()> {
        if !self.dirty() {
            return self.verify_source_unchanged();
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| PluginError::internal("HWPX source path has no parent directory"))?;
        let temporary = tempfile::Builder::new()
            .prefix(".officecli-hwpx-")
            .suffix(".tmp")
            .tempfile_in(parent)
            .map_err(|error| {
                PluginError::internal(format!("cannot create sibling save candidate: {error}"))
            })?;

        let baseline = self.baseline.as_ref().ok_or_else(|| {
            PluginError::internal("dirty HWPX session has no strict editable baseline")
        })?;
        let plan = MutationPlan::replace_exact(baseline.snapshot(), &self.replacements)?;
        let expectations = self.semantic_expectations();
        let source = BufReader::new(File::open(&self.path)?);
        let destination = temporary.reopen().map_err(|error| {
            PluginError::internal(format!("cannot reopen save candidate: {error}"))
        })?;
        let (mut candidate, _) = rewrite_and_verify(
            baseline,
            source,
            destination,
            &plan,
            &self.replacements,
            SemanticExpectation::ExactTexts(&expectations),
        )?;
        candidate.flush().map_err(|error| {
            PluginError::internal(format!("cannot flush save candidate: {error}"))
        })?;
        candidate.sync_all().map_err(|error| {
            PluginError::internal(format!("cannot durably flush save candidate: {error}"))
        })?;
        drop(candidate);

        let next_baseline =
            PackageBaseline::capture(BufReader::new(temporary.reopen().map_err(|error| {
                PluginError::internal(format!("cannot inspect save candidate: {error}"))
            })?))?;
        let next_source_parts = load_section_parts(temporary.path(), &self.section_paths)?;
        let next_index = build_index(&self.section_paths, &next_source_parts)?;

        copy_source_permissions(&self.path, &temporary)?;
        temporary.as_file().sync_all().map_err(|error| {
            PluginError::internal(format!("cannot durably flush save permissions: {error}"))
        })?;
        // Candidate construction and independent G3 reopening can take long
        // enough for another writer to replace the source. Re-snapshot at the
        // commit boundary so no change during that verification interval is
        // overwritten. The final OS rename/replace remains the only commit.
        self.verify_source_unchanged()?;
        replace_atomically(temporary, &self.path)?;
        self.baseline = Some(next_baseline);
        self.source_parts = next_source_parts;
        self.replacements.clear();
        self.pending.clear();
        self.index = next_index;
        Ok(())
    }
}

fn map_string<'a>(
    map: &'a Map<String, Value>,
    name: &str,
) -> std::result::Result<&'a str, WireError> {
    map.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| WireError::invalid_argument(format!("args.{name} must be a string")))
}

fn map_usize_or(
    map: &Map<String, Value>,
    name: &str,
    default: usize,
) -> std::result::Result<usize, WireError> {
    let Some(value) = map.get(name) else {
        return Ok(default);
    };
    let value = value.as_u64().ok_or_else(|| {
        WireError::invalid_argument(format!("args.{name} must be a non-negative integer"))
    })?;
    usize::try_from(value)
        .map_err(|_| WireError::invalid_argument(format!("args.{name} is too large")))
}

fn slice_lines(
    lines: Vec<(String, String)>,
    args: &Map<String, Value>,
) -> std::result::Result<Vec<(String, String)>, WireError> {
    let start = map_usize_or(args, "start", 1)?;
    if start == 0 {
        return Err(WireError::invalid_argument("view start is one-based"));
    }
    let end = match args.get("end") {
        Some(_) => map_usize_or(args, "end", lines.len())?,
        None => lines.len(),
    };
    if end == 0 {
        return Err(WireError::invalid_argument("view end is one-based"));
    }
    if end < start.saturating_sub(1) {
        return Err(WireError::invalid_argument("view end precedes start"));
    }
    let max_lines = match (args.get("max_lines"), args.get("max-lines")) {
        (Some(_), Some(_)) => {
            return Err(WireError::invalid_argument(
                "view accepts only one of max_lines or legacy max-lines",
            ));
        }
        (Some(_), None) => Some(map_usize_or(args, "max_lines", lines.len())?),
        (None, Some(_)) => Some(map_usize_or(args, "max-lines", lines.len())?),
        (None, None) => None,
    };
    let skip = start - 1;
    let take = end.saturating_sub(skip);
    let selected = lines.into_iter().skip(skip).take(take);
    Ok(match max_lines {
        Some(limit) => selected.take(limit).collect(),
        None => selected.collect(),
    })
}

fn normalize_part_name(path: &str) -> std::result::Result<String, WireError> {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() || path.contains('\\') || path.contains('\0') {
        return Err(WireError::invalid_argument(
            "invalid HWPX package part path",
        ));
    }
    if Path::new(path)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(WireError::invalid_argument(
            "HWPX package part path must be relative and traversal-free",
        ));
    }
    Ok(path.to_owned())
}

fn load_section_parts(path: &Path, sections: &[String]) -> Result<BTreeMap<String, Vec<u8>>> {
    sections
        .iter()
        .map(|section| Ok((section.clone(), read_zip_part(path, section)?)))
        .collect()
}

fn read_zip_part(path: &Path, part: &str) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let mut entry = archive.by_name(part).map_err(|error| {
        PluginError::invalid_argument(format!("package part {part:?} does not exist: {error}"))
    })?;
    if entry.size() > MAX_XML_ENTRY_BYTES {
        return Err(PluginError::unsupported_feature(format!(
            "package part {part:?} exceeds {MAX_XML_ENTRY_BYTES} bytes"
        )));
    }
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(MAX_XML_ENTRY_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_XML_ENTRY_BYTES {
        return Err(PluginError::unsupported_feature(format!(
            "package part {part:?} exceeded {MAX_XML_ENTRY_BYTES} bytes while reading"
        )));
    }
    Ok(bytes)
}

#[derive(Clone, Debug)]
struct SectionIndex {
    part: String,
    paragraphs: Vec<ParagraphIndex>,
}

#[derive(Clone, Debug)]
struct ParagraphIndex {
    ordinal: usize,
    id: Option<String>,
    editable: bool,
    next_text_ordinal: usize,
    texts: Vec<TextIndex>,
}

#[derive(Clone, Debug)]
struct TextIndex {
    path: String,
    part: String,
    selector: TextNodeSelector,
    value: String,
    editable: bool,
    ordinal: usize,
}

impl SectionIndex {
    fn node(&self, section_index: usize) -> DocumentNode {
        let path = section_path(section_index);
        let children = self
            .paragraphs
            .iter()
            .map(|paragraph| paragraph.node(section_index))
            .collect::<Vec<_>>();
        let mut node = DocumentNode::branch(path, "section", children);
        node.format
            .insert("part".to_owned(), Value::String(self.part.clone()));
        node
    }
}

impl ParagraphIndex {
    fn node(&self, section_index: usize) -> DocumentNode {
        let path = paragraph_path(section_index, self.ordinal);
        let text = self
            .texts
            .iter()
            .map(|text| text.value.as_str())
            .collect::<String>();
        let children = self
            .texts
            .iter()
            .map(|target| target.node())
            .collect::<Vec<_>>();
        let mut node = DocumentNode::branch(path, "paragraph", children);
        node.text = Some(text);
        node.format
            .insert("editable".to_owned(), Value::Bool(self.editable));
        if let Some(id) = &self.id {
            node.format
                .insert("paragraph_id".to_owned(), Value::String(id.clone()));
        }
        node
    }
}

impl TextIndex {
    fn node(&self) -> DocumentNode {
        let mut node = DocumentNode::leaf(&self.path, "text", self.value.clone());
        node.format
            .insert("editable".to_owned(), Value::Bool(self.editable));
        node.format.insert(
            "text_ordinal".to_owned(),
            Value::from(u64::try_from(self.ordinal).unwrap_or(u64::MAX)),
        );
        node
    }
}

#[derive(Clone, Debug, Serialize)]
struct DocumentNode {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<String>,
    #[serde(rename = "childCount")]
    child_count: usize,
    format: BTreeMap<String, Value>,
    children: Vec<DocumentNode>,
}

impl DocumentNode {
    fn branch(path: impl Into<String>, kind: impl Into<String>, children: Vec<Self>) -> Self {
        Self {
            path: path.into(),
            kind: kind.into(),
            text: None,
            preview: None,
            style: None,
            child_count: children.len(),
            format: BTreeMap::new(),
            children,
        }
    }

    fn leaf(path: impl Into<String>, kind: impl Into<String>, text: String) -> Self {
        Self {
            path: path.into(),
            kind: kind.into(),
            preview: Some(text.chars().take(80).collect()),
            text: Some(text),
            style: None,
            child_count: 0,
            format: BTreeMap::new(),
            children: Vec::new(),
        }
    }

    fn truncated(mut self, depth: usize) -> Self {
        if depth == 0 {
            self.children.clear();
        } else {
            self.children = self
                .children
                .into_iter()
                .map(|child| child.truncated(depth - 1))
                .collect();
        }
        self
    }
}

fn find_node<'a>(node: &'a DocumentNode, path: &str) -> Option<&'a DocumentNode> {
    if node.path == path {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, path))
}

fn flatten_nodes<'a>(node: &'a DocumentNode, output: &mut Vec<&'a DocumentNode>) {
    output.push(node);
    for child in &node.children {
        flatten_nodes(child, output);
    }
}

fn section_path(section: usize) -> String {
    format!("/document/section[{}]", section.saturating_add(1))
}

fn paragraph_path(section: usize, paragraph: usize) -> String {
    format!(
        "{}/paragraph[{}]",
        section_path(section),
        paragraph.saturating_add(1)
    )
}

fn text_path(section: usize, paragraph: usize, text: usize) -> String {
    format!(
        "{}/text[{}]",
        paragraph_path(section, paragraph),
        text.saturating_add(1)
    )
}

fn build_index(
    section_paths: &[String],
    parts: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<SectionIndex>> {
    section_paths
        .iter()
        .enumerate()
        .map(|(section_index, part)| {
            let xml = parts.get(part).ok_or_else(|| {
                PluginError::internal(format!("section bytes are missing for {part:?}"))
            })?;
            scan_section(section_index, part, xml)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenElement {
    Paragraph(usize),
    Run,
    Text,
    Other,
}

struct ActiveText {
    paragraph: usize,
    ordinal: usize,
    open_depth: usize,
    value: String,
    plain: bool,
}

fn scan_section(section_index: usize, part: &str, xml: &[u8]) -> Result<SectionIndex> {
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut stack = Vec::new();
    let mut paragraphs = Vec::<ParagraphIndex>::new();
    let mut active_text: Option<ActiveText> = None;

    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => {
                if let Some(active) = active_text.as_mut() {
                    active.plain = false;
                }
                let element = paragraph_element(&reader, &event)?;
                let open = match element {
                    ParagraphElement::Paragraph => {
                        if let Some(OpenElement::Paragraph(parent)) = stack
                            .iter()
                            .rev()
                            .find(|element| matches!(element, OpenElement::Paragraph(_)))
                        {
                            paragraphs[*parent].editable = false;
                        }
                        let ordinal = paragraphs.len();
                        paragraphs.push(ParagraphIndex {
                            ordinal,
                            id: exact_attribute(&event, b"id")?,
                            editable: true,
                            next_text_ordinal: 0,
                            texts: Vec::new(),
                        });
                        OpenElement::Paragraph(ordinal)
                    }
                    ParagraphElement::Run => {
                        if let Some(paragraph) = nearest_paragraph(&stack) {
                            let direct = matches!(stack.last(), Some(OpenElement::Paragraph(index)) if *index == paragraph);
                            if !direct {
                                paragraphs[paragraph].editable = false;
                            }
                        }
                        OpenElement::Run
                    }
                    ParagraphElement::Text => {
                        if let Some(paragraph) = nearest_paragraph(&stack) {
                            let direct = matches!(
                                stack.as_slice(),
                                [.., OpenElement::Paragraph(index), OpenElement::Run] if *index == paragraph
                            );
                            if !direct {
                                paragraphs[paragraph].editable = false;
                            } else {
                                let ordinal = paragraphs[paragraph].next_text_ordinal;
                                paragraphs[paragraph].next_text_ordinal =
                                    ordinal.checked_add(1).ok_or_else(|| {
                                        PluginError::unsupported_feature(
                                            "HWPX text ordinal overflowed",
                                        )
                                    })?;
                                active_text = Some(ActiveText {
                                    paragraph,
                                    ordinal,
                                    open_depth: stack.len(),
                                    value: String::new(),
                                    plain: true,
                                });
                            }
                        }
                        OpenElement::Text
                    }
                    ParagraphElement::Other => OpenElement::Other,
                };
                stack.push(open);
            }
            Event::Empty(event) => {
                if let Some(active) = active_text.as_mut() {
                    active.plain = false;
                }
                match paragraph_element(&reader, &event)? {
                    ParagraphElement::Paragraph => {
                        if let Some(parent) = nearest_paragraph(&stack) {
                            paragraphs[parent].editable = false;
                        }
                        let ordinal = paragraphs.len();
                        paragraphs.push(ParagraphIndex {
                            ordinal,
                            id: exact_attribute(&event, b"id")?,
                            editable: true,
                            next_text_ordinal: 0,
                            texts: Vec::new(),
                        });
                    }
                    ParagraphElement::Run => {
                        if let Some(paragraph) = nearest_paragraph(&stack) {
                            let direct = matches!(stack.last(), Some(OpenElement::Paragraph(index)) if *index == paragraph);
                            if !direct {
                                paragraphs[paragraph].editable = false;
                            }
                        }
                    }
                    ParagraphElement::Text => {
                        if let Some(paragraph) = nearest_paragraph(&stack) {
                            let direct = matches!(
                                stack.as_slice(),
                                [.., OpenElement::Paragraph(index), OpenElement::Run] if *index == paragraph
                            );
                            if !direct {
                                paragraphs[paragraph].editable = false;
                            } else {
                                paragraphs[paragraph].next_text_ordinal = paragraphs[paragraph]
                                    .next_text_ordinal
                                    .checked_add(1)
                                    .ok_or_else(|| {
                                        PluginError::unsupported_feature(
                                            "HWPX text ordinal overflowed",
                                        )
                                    })?;
                            }
                        }
                    }
                    ParagraphElement::Other => {}
                }
            }
            Event::Text(text) => {
                if let Some(active) = active_text.as_mut() {
                    active.value.push_str(&text.decode()?);
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(active) = active_text.as_mut() {
                    active.value.push_str(&resolve_reference(&reference)?);
                }
            }
            Event::CData(_) | Event::Comment(_) | Event::PI(_) if active_text.is_some() => {
                active_text.as_mut().expect("checked").plain = false;
            }
            Event::DocType(_) => {
                return Err(PluginError::corrupt(
                    "HWPX section XML must not contain a document type declaration",
                ));
            }
            Event::End(_) => {
                let closing_depth = stack
                    .len()
                    .checked_sub(1)
                    .ok_or_else(|| PluginError::corrupt("HWPX XML closing-element underflow"))?;
                if active_text
                    .as_ref()
                    .is_some_and(|active| active.open_depth == closing_depth)
                {
                    let active = active_text.take().expect("active text checked");
                    if active.plain {
                        let paragraph = &mut paragraphs[active.paragraph];
                        let selector = match paragraph.id.as_deref() {
                            Some(id) if !id.is_empty() => TextNodeSelector::at_paragraph_with_id(
                                paragraph.ordinal,
                                id,
                                active.ordinal,
                            )?,
                            _ => TextNodeSelector::at_paragraph(paragraph.ordinal, active.ordinal),
                        };
                        paragraph.texts.push(TextIndex {
                            path: text_path(section_index, paragraph.ordinal, active.ordinal),
                            part: part.to_owned(),
                            selector,
                            value: active.value,
                            editable: true,
                            ordinal: active.ordinal,
                        });
                    }
                }
                stack.pop();
            }
            Event::Decl(_) if !stack.is_empty() => {
                return Err(PluginError::corrupt(
                    "HWPX XML declaration must appear before document content",
                ));
            }
            Event::Eof => {
                if !stack.is_empty() || active_text.is_some() {
                    return Err(PluginError::corrupt(
                        "HWPX section XML ended before all elements were closed",
                    ));
                }
                break;
            }
            _ => {}
        }
        buffer.clear();
    }

    for paragraph in &mut paragraphs {
        for text in &mut paragraph.texts {
            text.editable = paragraph.editable;
        }
    }
    Ok(SectionIndex {
        part: part.to_owned(),
        paragraphs,
    })
}

fn nearest_paragraph(stack: &[OpenElement]) -> Option<usize> {
    stack.iter().rev().find_map(|element| match element {
        OpenElement::Paragraph(index) => Some(*index),
        _ => None,
    })
}

#[derive(Clone, Copy)]
enum ParagraphElement {
    Paragraph,
    Run,
    Text,
    Other,
}

fn paragraph_element(reader: &NsReader<&[u8]>, event: &BytesStart<'_>) -> Result<ParagraphElement> {
    let (namespace, local_name) = reader.resolver().resolve_element(event.name());
    match namespace {
        ResolveResult::Bound(namespace) if namespace.as_ref() == PARAGRAPH_NAMESPACE => {
            Ok(match local_name.as_ref() {
                b"p" => ParagraphElement::Paragraph,
                b"run" => ParagraphElement::Run,
                b"t" => ParagraphElement::Text,
                _ => ParagraphElement::Other,
            })
        }
        ResolveResult::Unknown(prefix) => Err(PluginError::corrupt(format!(
            "HWPX XML uses undeclared namespace prefix {:?}",
            String::from_utf8_lossy(&prefix)
        ))),
        _ => Ok(ParagraphElement::Other),
    }
}

fn exact_attribute(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    let mut value = None;
    for attribute in event.attributes() {
        let attribute = attribute?;
        if attribute.key.as_ref() == name {
            if value.is_some() {
                return Err(PluginError::corrupt(format!(
                    "HWPX XML repeats attribute {:?}",
                    String::from_utf8_lossy(name)
                )));
            }
            value = Some(
                attribute
                    .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                    .map_err(|error| {
                        PluginError::corrupt(format!(
                            "HWPX XML attribute cannot be normalized: {error}"
                        ))
                    })?
                    .into_owned(),
            );
        }
    }
    Ok(value)
}

fn resolve_reference(reference: &BytesRef<'_>) -> Result<String> {
    if let Some(character) = reference.resolve_char_ref().map_err(|error| {
        PluginError::corrupt(format!(
            "HWPX text contains an invalid character reference: {error}"
        ))
    })? {
        return Ok(character.to_string());
    }
    let name = reference.decode()?;
    quick_xml::escape::resolve_predefined_entity(&name)
        .map(str::to_owned)
        .ok_or_else(|| {
            PluginError::corrupt(format!(
                "HWPX text contains unsupported entity reference &{name};"
            ))
        })
}

fn copy_source_permissions(source: &Path, candidate: &NamedTempFile) -> Result<()> {
    let permissions = fs::metadata(source)?.permissions();
    candidate.as_file().set_permissions(permissions)?;
    Ok(())
}

#[cfg(windows)]
fn replace_atomically(candidate: NamedTempFile, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

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
    let replaced = unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            candidate_wide.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced == 0 {
        let replace_error = io::Error::last_os_error();
        // ReplaceFileW also tries to merge WRITE_DAC metadata and can be
        // denied in otherwise writable sandbox/inherited-ACL directories.
        // ERROR_ACCESS_DENIED guarantees both original names are unchanged,
        // so a same-directory MoveFileExW replacement remains a safe atomic
        // fallback. Other ReplaceFileW errors can describe partially moved
        // names and therefore fail closed without a second operation.
        if replace_error.raw_os_error() == Some(5) {
            let moved = unsafe {
                MoveFileExW(
                    candidate_wide.as_ptr(),
                    target_wide.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if moved != 0 {
                drop(candidate_path);
                return Ok(());
            }
            return Err(PluginError::internal(format!(
                "atomic HWPX replacement fallback failed after ReplaceFileW was denied: {}",
                io::Error::last_os_error()
            )));
        }
        return Err(PluginError::internal(format!(
            "atomic HWPX replacement failed: {replace_error}"
        )));
    }
    drop(candidate_path);
    Ok(())
}

#[cfg(unix)]
fn replace_atomically(candidate: NamedTempFile, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| PluginError::internal("HWPX source path has no parent directory"))?;
    let directory = File::open(parent).map_err(|error| {
        PluginError::internal(format!(
            "cannot open HWPX parent directory for sync: {error}"
        ))
    })?;
    directory.sync_all().map_err(|error| {
        PluginError::internal(format!("cannot pre-sync HWPX parent directory: {error}"))
    })?;
    let candidate_path = candidate.into_temp_path();
    fs::rename(&candidate_path, target).map_err(|error| {
        PluginError::internal(format!("atomic HWPX replacement failed: {error}"))
    })?;
    directory.sync_all().map_err(|error| {
        PluginError::internal(format!("cannot sync HWPX parent directory: {error}"))
    })?;
    drop(candidate_path);
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn replace_atomically(candidate: NamedTempFile, target: &Path) -> Result<()> {
    let candidate_path = candidate.into_temp_path();
    fs::rename(&candidate_path, target).map_err(|error| {
        PluginError::internal(format!("atomic HWPX replacement failed: {error}"))
    })?;
    drop(candidate_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_frames_require_an_lf_terminator() {
        let frame = br#"{"protocol":1,"msg_type":"close"}"#;
        let error = read_request(&mut Cursor::new(frame)).expect_err("unterminated frame");
        assert_eq!(error.code.as_str(), "invalid_argument");
        assert!(error.to_string().contains("LF"));
    }

    #[test]
    fn oversized_jsonl_frame_is_drained_before_the_next_request() {
        let mut input = vec![b'x'; MAX_FRAME_BYTES];
        input.extend_from_slice(b"x\n{\"protocol\":1,\"msg_type\":\"close\"}\n");
        let mut input = BufReader::with_capacity(97, Cursor::new(input));

        let error = read_request(&mut input).expect_err("oversized frame");
        assert_eq!(error.code.as_str(), "invalid_argument");
        let request = read_request(&mut input)
            .expect("next request")
            .expect("request after oversized frame");
        assert_eq!(request["msg_type"], "close");
    }

    #[test]
    fn invalid_utf8_frame_is_a_protocol_argument_error() {
        let error = read_request(&mut Cursor::new([0xff, b'\n']))
            .expect_err("invalid UTF-8 protocol frame");
        assert_eq!(error.code.as_str(), "invalid_argument");
    }

    #[test]
    fn string_fields_report_the_received_json_kind() {
        for (value, expected) in [
            (json!({}), "missing"),
            (json!({ "path": null }), "null"),
            (json!({ "path": { "value": "x" } }), "object"),
        ] {
            let error = string_field(&value, "path").expect_err("non-string path");
            assert!(
                error.message.contains(expected),
                "expected {expected:?} in {:?}",
                error.message
            );
        }
    }

    #[test]
    fn section_scan_rejects_unclosed_xml() {
        let xml = br#"<hp:section xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p><hp:run><hp:t>x</hp:t>"#;
        let error = scan_section(0, "Contents/section0.xml", xml).expect_err("unclosed XML");
        assert_eq!(error.code.as_str(), "corrupt_input");
    }

    #[test]
    fn view_end_zero_is_rejected() {
        let args =
            serde_json::from_value::<Map<String, Value>>(json!({ "end": 0 })).expect("object map");
        let error = slice_lines(vec![("/x".into(), "x".into())], &args)
            .expect_err("end zero must not mean an empty success");
        assert_eq!(error.code, "invalid_argument");
    }

    #[test]
    fn part_paths_reject_traversal_and_windows_separators() {
        assert!(normalize_part_name("Contents/section0.xml").is_ok());
        assert!(normalize_part_name("/Contents/section0.xml").is_ok());
        for invalid in ["", "../x", "Contents/../x", "C:\\x", "//server/x"] {
            assert!(
                normalize_part_name(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn wire_errors_retain_core_error_codes() {
        for error in [
            PluginError::invalid_argument("x"),
            PluginError::unsupported_command("x"),
            PluginError::unsupported_feature("x"),
            PluginError::corrupt("x"),
            PluginError::internal("x"),
        ] {
            let expected = error.code.as_str();
            assert_eq!(WireError::from(error).code, expected);
        }
        assert_eq!(
            crate::error::ErrorCode::InvalidArgument.as_str(),
            "invalid_argument"
        );
    }
}
