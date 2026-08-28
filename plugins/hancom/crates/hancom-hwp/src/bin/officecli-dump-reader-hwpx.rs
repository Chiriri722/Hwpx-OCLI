//! `officecli-dump-reader-hwpx` 진입점.
//!
//! 이름은 프로토콜 §3의 PATH 탐색 규약 `officecli-<kind>-<ext>`에 맞춘 것이다.

use std::io::{self, Write};
use std::process::ExitCode as ProcExit;

use officecli_hancom_core::heartbeat::{HeartbeatGuard, DEFAULT_HEARTBEAT_INTERVAL};
use officecli_hwpx::{error::ExitCode, escape_diagnostic_text, parse_args_os, run, Command};

fn main() -> ProcExit {
    // stdout은 JSONL 전용. LineWriter가 아니라 raw lock을 쓰고 emitter가
    // 명시적으로 flush한다(§2.1 "flushed individually").
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut err = io::stderr();

    let code = match parse_args_os(std::env::args_os().skip(1)) {
        Ok(cmd) => {
            let heartbeat = matches!(&cmd, Command::Dump { .. })
                .then(|| HeartbeatGuard::start(io::stderr(), DEFAULT_HEARTBEAT_INTERVAL));
            let result = run(cmd, &mut out, &mut err);
            drop(heartbeat);
            match result {
                Ok(code) => code,
                Err(e) => {
                    let _ = writeln!(err, "{}", escape_diagnostic_text(&e.to_string()));
                    e.exit_code()
                }
            }
        }
        Err(e) => {
            let _ = writeln!(err, "{}", escape_diagnostic_text(&e.to_string()));
            let _ = write!(err, "{}", officecli_hwpx::HELP_TEXT);
            e.exit_code()
        }
    };

    let _ = out.flush();
    let _ = err.flush();

    // ExitCode::from(u8)은 0-255를 그대로 전달한다.
    ProcExit::from(exit_byte(code))
}

fn exit_byte(code: ExitCode) -> u8 {
    let v = code.as_i32();
    u8::try_from(v).unwrap_or(70)
}
