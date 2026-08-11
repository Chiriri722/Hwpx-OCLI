//! OfficeCLI `dump-reader` 플러그인 — HWPX(OWPML) → docx 명령.
//!
//! 계약: `docs/01-protocol-contract.md`

pub mod emit;
pub mod error;
pub mod format;
pub mod manifest;
pub mod owpml;

use std::io::Write;
use std::path::PathBuf;

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
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    let mut it = args.iter();

    let Some(first) = it.next() else {
        return Ok(Command::Help);
    };

    match first.as_str() {
        "--info" => Ok(Command::Info),
        "--help" | "-h" => Ok(Command::Help),
        "--version" | "-V" => Ok(Command::Info),
        "dump" => {
            let mut source: Option<PathBuf> = None;
            let mut media_dir = None;
            let mut log_file = None;
            let mut quiet = false;

            while let Some(a) = it.next() {
                match a.as_str() {
                    "--media-dir" => {
                        media_dir = Some(PathBuf::from(next_value(&mut it, "--media-dir")?));
                    }
                    "--log-file" => {
                        log_file = Some(PathBuf::from(next_value(&mut it, "--log-file")?));
                    }
                    "--quiet" => quiet = true,
                    other if other.starts_with("--") => {
                        // 모르는 플래그는 조용히 무시하지 않고 알린다.
                        // 호스트가 새 플래그를 추가했다는 신호일 수 있다.
                        return Err(PluginError::invalid_argument(format!(
                            "unknown option for dump: {other}"
                        )));
                    }
                    positional => {
                        if source.is_some() {
                            return Err(PluginError::invalid_argument(format!(
                                "unexpected extra argument: {positional}"
                            )));
                        }
                        source = Some(PathBuf::from(positional));
                    }
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

fn next_value<'a, I: Iterator<Item = &'a String>>(it: &mut I, flag: &str) -> Result<String> {
    it.next()
        .cloned()
        .ok_or_else(|| PluginError::invalid_argument(format!("{flag} requires a value")))
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
        ". This build reads HWPX only. Convert it first, e.g.:\n  \
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
            media_dir: _,
            log_file,
            quiet,
        } => {
            // 확장자를 믿지 않고 매직 바이트로 판별한다.
            // `.hwp`인데 실제로는 HWPX인 파일이 흔하고 반대도 있다.
            let detected = format::detect_path(&source)?;

            if detected.needs_conversion() {
                // HWP(바이너리)는 HWPX로 변환한 뒤에야 읽을 수 있다.
                // 변환기 브리지는 아직 없다 → §6.5의 3번(이 빌드에서 미지원).
                return Err(unsupported_binary_hwp(&detected));
            }

            let doc = owpml::read_document(&source)?;
            let count = emit::stream_document(&doc, stdout)?;

            // 진단은 stderr 또는 --log-file로. stdout은 JSONL 전용이다(§5.1).
            if !quiet {
                let mut msg = format!(
                    "dumped {} batch items from {}\n",
                    count,
                    source.display()
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
                match &log_file {
                    Some(p) => {
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(p)
                        {
                            let _ = f.write_all(msg.as_bytes());
                        }
                    }
                    None => {
                        let _ = stderr.write_all(msg.as_bytes());
                    }
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
