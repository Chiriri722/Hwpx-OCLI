//! Shared process entrypoint for the canonical binary and its legacy alias.

use std::io::{self, Write};
use std::process::ExitCode as ProcExit;

use officecli_hancom_core::heartbeat::{HeartbeatGuard, DEFAULT_HEARTBEAT_INTERVAL};

use crate::{error::ExitCode, escape_diagnostic_text, parse_args_os, run, Command};

pub fn main_entry() -> ProcExit {
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
                Err(error) => {
                    let _ = writeln!(err, "{}", escape_diagnostic_text(&error.to_string()));
                    error.exit_code()
                }
            }
        }
        Err(error) => {
            let _ = writeln!(err, "{}", escape_diagnostic_text(&error.to_string()));
            let _ = write!(err, "{}", crate::HELP_TEXT);
            error.exit_code()
        }
    };

    let _ = out.flush();
    let _ = err.flush();
    ProcExit::from(exit_byte(code))
}

fn exit_byte(code: ExitCode) -> u8 {
    u8::try_from(code.as_i32()).unwrap_or(70)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_exit_codes_fit_the_platform_byte() {
        assert_eq!(exit_byte(ExitCode::Success), 0);
        assert_eq!(exit_byte(ExitCode::CorruptInput), 2);
        assert_eq!(exit_byte(ExitCode::InternalError), 70);
    }
}
