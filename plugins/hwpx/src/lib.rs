//! OfficeCLI `dump-reader` 플러그인 — HWPX(OWPML) → docx 명령.
//!
//! 계약: `docs/01-protocol-contract.md`

mod converter;
pub mod emit;
pub mod error;
pub mod format;
pub mod manifest;
pub mod owpml;

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use error::{ExitCode, PluginError, Result};

/// 파싱된 명령줄.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// `--info`
    Info,
    /// `dump <source> [--media-dir <dir>] [--log-file <path>] [--quiet]`
    Dump {
        source: PathBuf,
        media_dir: Option<PathBuf>,
        log_file: Option<PathBuf>,
        quiet: bool,
    },
    /// `--help` / `-h` / 인자 없음
    Help,
}

/// 프로토콜 §5.1/§5.4에 정의된 인자만 받는다.
pub fn parse_args<I, S>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parse_args_os(
        args.into_iter()
            .map(|argument| OsString::from(argument.as_ref())),
    )
}

/// OS 원시 인자를 파싱한다. 경로 값은 UTF-8 변환 없이 `PathBuf`로 보존한다.
pub fn parse_args_os<I, S>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect();
    let mut it = args.into_iter();

    let Some(first) = it.next() else {
        return Ok(Command::Help);
    };
    let first = first
        .to_str()
        .ok_or_else(|| PluginError::unsupported_command("subcommand must be valid UTF-8"))?;

    match first {
        "--info" => Ok(Command::Info),
        "--help" | "-h" => Ok(Command::Help),
        "--version" | "-V" => Ok(Command::Info),
        "dump" => {
            let mut source: Option<PathBuf> = None;
            let mut media_dir = None;
            let mut log_file = None;
            let mut quiet = false;

            while let Some(argument) = it.next() {
                if argument == OsStr::new("--media-dir") {
                    media_dir = Some(PathBuf::from(next_os_value(&mut it, "--media-dir")?));
                } else if argument == OsStr::new("--log-file") {
                    log_file = Some(PathBuf::from(next_os_value(&mut it, "--log-file")?));
                } else if argument == OsStr::new("--quiet") {
                    quiet = true;
                } else if argument
                    .to_str()
                    .is_some_and(|value| value.starts_with("--"))
                {
                    // 모르는 플래그는 조용히 무시하지 않고 알린다.
                    // 호스트가 새 플래그를 추가했다는 신호일 수 있다.
                    return Err(PluginError::invalid_argument(format!(
                        "unknown option for dump: {}",
                        argument.to_string_lossy()
                    )));
                } else {
                    let positional = PathBuf::from(argument);
                    if source.is_some() {
                        return Err(PluginError::invalid_argument(format!(
                            "unexpected extra argument: {}",
                            diagnostic_path(&positional)
                        )));
                    }
                    source = Some(positional);
                }
            }

            let source = source.ok_or_else(|| {
                PluginError::invalid_argument("dump requires a <source-file> argument")
            })?;

            Ok(Command::Dump {
                source,
                media_dir,
                log_file,
                quiet,
            })
        }
        other => Err(PluginError::unsupported_command(format!(
            "unknown subcommand: {other}"
        ))),
    }
}

fn next_os_value<I: Iterator<Item = OsString>>(it: &mut I, flag: &str) -> Result<OsString> {
    it.next()
        .ok_or_else(|| PluginError::invalid_argument(format!("{flag} requires a value")))
}

/// 터미널·한 줄 로그에 안전한 진단 문자열로 바꾼다.
///
/// 파일명에는 개행과 ESC 같은 제어 문자가 들어갈 수 있다. 진단 경계에서 이들을
/// 가시적인 이스케이프로 바꿔 로그 위조와 터미널 제어 시퀀스 실행을 막는다.
pub fn escape_diagnostic_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') {
            escaped.extend(ch.escape_default());
        } else {
            escaped.push(ch);
        }
    }
    escaped
}

fn diagnostic_path(path: &Path) -> String {
    escape_diagnostic_text(&path.to_string_lossy())
}

fn ensure_log_is_not_source(source: &Path, log: &Path) -> Result<()> {
    if paths_refer_to_same_file(source, log) {
        return Err(log_source_alias_error(source, log));
    }
    Ok(())
}

fn paths_refer_to_same_file(source: &Path, log: &Path) -> bool {
    if source == log {
        return true;
    }

    if let (Ok(source_path), Ok(log_path)) = (source.canonicalize(), log.canonicalize()) {
        if source_path == log_path {
            return true;
        }
    }

    let (Ok(source_file), Ok(log_file)) = (std::fs::File::open(source), std::fs::File::open(log))
    else {
        return false;
    };
    files_refer_to_same_file(&source_file, &log_file)
}

#[cfg(unix)]
fn files_refer_to_same_file(a: &std::fs::File, b: &std::fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(a), Ok(b)) = (a.metadata(), b.metadata()) else {
        return false;
    };
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(any(windows, test))]
fn same_file_identity(a: Option<(u32, u64)>, b: Option<(u32, u64)>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a == b)
}

#[cfg(windows)]
fn files_refer_to_same_file(a: &std::fs::File, b: &std::fs::File) -> bool {
    same_file_identity(windows_file_identity(a), windows_file_identity(b))
}

#[cfg(windows)]
fn windows_file_identity(file: &std::fs::File) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a valid live handle and `info` points to writable,
    // correctly sized storage for the duration of the call.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) };
    if ok == 0 {
        return None;
    }

    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Some((info.dwVolumeSerialNumber, index))
}

#[cfg(not(any(unix, windows)))]
fn files_refer_to_same_file(_a: &std::fs::File, _b: &std::fs::File) -> bool {
    false
}

fn opened_log_refers_to_source(source: &Path, log: &std::fs::File) -> bool {
    let Ok(source_file) = std::fs::File::open(source) else {
        return false;
    };
    files_refer_to_same_file(&source_file, log)
}

fn log_source_alias_error(source: &Path, log: &Path) -> PluginError {
    PluginError::invalid_argument(format!(
        "--log-file {} must not refer to source {}",
        diagnostic_path(log),
        diagnostic_path(source)
    ))
}

fn report_log_failure<E: Write>(stderr: &mut E, path: &Path, error: &std::io::Error) {
    let _ = writeln!(
        stderr,
        "warning: cannot write log file {}: {}",
        diagnostic_path(path),
        escape_diagnostic_text(&error.to_string())
    );
}

/// 바이너리 HWP를 만났을 때의 에러.
///
/// exit 3 (`unsupported_feature`, §6.5 "Feature unsupported in this build")로
/// 나간다. 조용히 실패하지 않고 무엇을 해야 하는지 알린다.
///
/// 변환기 브리지가 붙으면 이 경로는 사라진다 (`docs/04-hwp-support-plan.md` H3).
fn unsupported_binary_hwp(detected: &format::SourceFormat) -> PluginError {
    let mut msg = match detected {
        format::SourceFormat::Hwp5(info) => format!(
            "this is a binary HWP 5.x document (version {}), not HWPX",
            info.version_string()
        ),
        format::SourceFormat::Hwp3 => {
            "this is a binary HWP 3.0 document, not HWPX".to_string()
        }
        format::SourceFormat::Hwpx => {
            // 도달할 수 없다. needs_conversion()이 false인 경우다.
            "unexpected format state".to_string()
        }
    };

    if let format::SourceFormat::Hwp5(info) = detected {
        let notes = info.protection_notes();
        if !notes.is_empty() {
            msg.push_str(&format!(" [{}]", notes.join(", ")));
        }
    }

    msg.push_str(
        ". Binary HWP support needs the optional RHWP converter. Install RHWP v0.8.4+ \
         on PATH or set OFFICECLI_HWPX_CONVERTER to its absolute path. Manual conversion:\n  \
         rhwp export-hwpx <source> <target>.hwpx\n\
         (https://github.com/edwardkim/rhwp — MIT, prebuilt binaries available)",
    );

    PluginError {
        code: error::ErrorCode::UnsupportedFeature,
        message: msg,
    }
}

pub const HELP_TEXT: &str = concat!(
    "officecli-dump-reader-hwpx — OfficeCLI dump-reader plugin for HWPX\n",
    "\n",
    "USAGE:\n",
    "  officecli-dump-reader-hwpx --info\n",
    "  officecli-dump-reader-hwpx dump <source.hwpx> [--media-dir <dir>]\n",
    "                                                [--log-file <path>] [--quiet]\n",
    "\n",
    "The plugin writes one JSON BatchItem per line to stdout and exits.\n",
    "See docs/01-protocol-contract.md for the full contract.\n",
    "\n",
    "NOTICE:\n",
    "본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.\n",
);

/// 명령을 실행한다. stdout/stderr는 호출자가 넘긴다(테스트 용이성).
pub fn run<O: Write, E: Write>(cmd: Command, stdout: &mut O, stderr: &mut E) -> Result<ExitCode> {
    match cmd {
        Command::Info => {
            // §4: 단일 JSON 객체를 stdout에 쓰고 exit 0.
            writeln!(stdout, "{}", manifest::Manifest::default().to_json_line())?;
            stdout.flush()?;
            Ok(ExitCode::Success)
        }
        Command::Help => {
            // 도움말은 진단 출력이므로 stdout을 오염시키지 않는다.
            write!(stderr, "{HELP_TEXT}")?;
            stderr.flush()?;
            Ok(ExitCode::Success)
        }
        Command::Dump {
            source,
            media_dir,
            log_file,
            quiet,
        } => {
            // 로그 파일을 출력 전에 열고 실제 파일 정체성을 확인한다. 이렇게 해야
            // source와 같은 파일(직접 경로·심볼릭 링크·하드 링크)을 append로 열어
            // 성공적인 변환 뒤 조용히 손상시키는 일을 막을 수 있다.
            let mut diagnostic_log = None;
            if !quiet {
                if let Some(path) = log_file.as_deref() {
                    ensure_log_is_not_source(&source, path)?;
                    match std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        Ok(file) => {
                            if opened_log_refers_to_source(&source, &file) {
                                return Err(log_source_alias_error(&source, path));
                            }
                            diagnostic_log = Some(file);
                        }
                        Err(error) => report_log_failure(stderr, path, &error),
                    }
                }
            }

            // 확장자를 믿지 않고 매직 바이트로 판별한다.
            // `.hwp`인데 실제로는 HWPX인 파일이 흔하고 반대도 있다.
            let detected = format::detect_path(&source)?;

            let converted = if detected.needs_conversion() {
                match converter::convert_hwp_to_hwpx(&source, media_dir.as_deref())? {
                    Some(converted) => Some(converted),
                    None => return Err(unsupported_binary_hwp(&detected)),
                }
            } else {
                None
            };
            let readable_source = converted
                .as_ref()
                .map_or(source.as_path(), converter::ConvertedHwpx::path);

            let doc = owpml::read_document(readable_source)?;
            let count = emit::stream_document(&doc, stdout)?;

            // 진단은 stderr 또는 --log-file로. stdout은 JSONL 전용이다(§5.1).
            if !quiet {
                let mut msg = format!(
                    "dumped {} batch items from {}\n",
                    count,
                    diagnostic_path(&source)
                );
                // 한컴 사용자 정의 문자는 그대로 통과시키지만 알려준다.
                // 한컴 글꼴 밖에서는 빈 사각형으로 보이므로 사용자가 알아야 한다.
                let pua = doc.count_private_use_chars();
                if pua > 0 {
                    msg.push_str(&format!(
                        "note: {pua} private-use character(s) passed through unmapped \
                         (Hancom-specific glyphs; they may render as empty boxes outside \
                         Hancom fonts)\n"
                    ));
                }
                if let Some(mut file) = diagnostic_log {
                    if let Err(error) = file.write_all(msg.as_bytes()) {
                        if let Some(path) = log_file.as_deref() {
                            report_log_failure(stderr, path, &error);
                        }
                    }
                } else if log_file.is_none() {
                    let _ = stderr.write_all(msg.as_bytes());
                }
            }
            Ok(ExitCode::Success)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_info() {
        assert_eq!(parse_args(["--info"]).expect("ok"), Command::Info);
    }

    #[test]
    fn no_args_is_help() {
        let empty: [&str; 0] = [];
        assert_eq!(parse_args(empty).expect("ok"), Command::Help);
    }

    #[test]
    fn parses_dump_with_source() {
        let cmd = parse_args(["dump", "/tmp/a.hwpx"]).expect("ok");
        assert_eq!(
            cmd,
            Command::Dump {
                source: PathBuf::from("/tmp/a.hwpx"),
                media_dir: None,
                log_file: None,
                quiet: false,
            }
        );
    }

    #[test]
    fn parses_dump_options() {
        let cmd = parse_args([
            "dump",
            "/tmp/a.hwpx",
            "--media-dir",
            "/tmp/m",
            "--log-file",
            "/tmp/l.log",
            "--quiet",
        ])
        .expect("ok");
        assert_eq!(
            cmd,
            Command::Dump {
                source: PathBuf::from("/tmp/a.hwpx"),
                media_dir: Some(PathBuf::from("/tmp/m")),
                log_file: Some(PathBuf::from("/tmp/l.log")),
                quiet: true,
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn os_parser_preserves_non_utf8_source_path() {
        use std::os::unix::ffi::OsStringExt;

        let source = std::ffi::OsString::from_vec(b"/tmp/non-utf8-\xff.hwpx".to_vec());
        let cmd = parse_args_os([std::ffi::OsString::from("dump"), source.clone()])
            .expect("non-UTF-8 source is a valid path argument");
        assert_eq!(
            cmd,
            Command::Dump {
                source: PathBuf::from(source),
                media_dir: None,
                log_file: None,
                quiet: false,
            }
        );
    }

    #[test]
    fn file_identity_requires_matching_volume_and_index() {
        assert!(same_file_identity(Some((7, 11)), Some((7, 11))));
        assert!(!same_file_identity(Some((7, 11)), Some((8, 11))));
        assert!(!same_file_identity(Some((7, 11)), Some((7, 12))));
        assert!(!same_file_identity(Some((7, 11)), None));
    }

    #[test]
    fn dump_requires_source() {
        let e = parse_args(["dump"]).expect_err("must fail");
        assert_eq!(e.code, error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn flag_requiring_value_reports_missing_value() {
        let e = parse_args(["dump", "a.hwpx", "--media-dir"]).expect_err("must fail");
        assert_eq!(e.code, error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn unknown_subcommand_is_reported() {
        let e = parse_args(["export", "a.hwpx"]).expect_err("must fail");
        assert_eq!(e.code, error::ErrorCode::UnsupportedCommand);
    }

    #[test]
    fn unknown_option_is_reported() {
        let e = parse_args(["dump", "a.hwpx", "--turbo"]).expect_err("must fail");
        assert_eq!(e.code, error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn info_writes_manifest_to_stdout_only() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(Command::Info, &mut out, &mut err).expect("ok");
        assert_eq!(code, ExitCode::Success);
        assert!(err.is_empty(), "manifest must not touch stderr");
        let v: serde_json::Value =
            serde_json::from_slice(&out).expect("stdout is a single json object");
        assert!(v.is_object());
    }

    #[test]
    fn help_goes_to_stderr_not_stdout() {
        // stdout은 JSONL 전용이므로 도움말이 섞이면 안 된다.
        let mut out = Vec::new();
        let mut err = Vec::new();
        run(Command::Help, &mut out, &mut err).expect("ok");
        assert!(out.is_empty(), "help must not pollute stdout");
        assert!(!err.is_empty());
    }
}
