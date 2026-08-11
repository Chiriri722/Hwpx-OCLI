//! `officecli-dump-reader-hwpx` 진입점.
//!
//! 이름은 프로토콜 §3의 PATH 탐색 규약 `officecli-<kind>-<ext>`에 맞춘 것이다.

use std::io::{self, Write};
use std::process::ExitCode as ProcExit;

use officecli_hwpx::{error::ExitCode, parse_args, run};

fn main() -> ProcExit {
    // stdout은 JSONL 전용. LineWriter가 아니라 raw lock을 쓰고 emitter가
    // 명시적으로 flush한다(§2.1 "flushed individually").
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let args: Vec<String> = std::env::args().skip(1).collect();

    let code = match parse_args(&args) {
        Ok(cmd) => match run(cmd, &mut out, &mut err) {
            Ok(code) => code,
            Err(e) => {
                let _ = writeln!(err, "{e}");
                e.exit_code()
            }
        },
        Err(e) => {
            let _ = writeln!(err, "{e}");
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
