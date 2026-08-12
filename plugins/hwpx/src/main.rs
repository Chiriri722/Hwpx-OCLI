//! `officecli-dump-reader-hwpx` 진입점.
//!
//! 이름은 프로토콜 §3의 PATH 탐색 규약 `officecli-<kind>-<ext>`에 맞춘 것이다.

use std::io::{self, Write};
use std::process::ExitCode as ProcExit;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use officecli_hwpx::{error::ExitCode, escape_diagnostic_text, parse_args_os, run, Command};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const HEARTBEAT_FRAME: &[u8] = b"{\"heartbeat\":true}\n";

struct HeartbeatGuard {
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn start_heartbeat<W>(mut writer: W, interval: Duration) -> HeartbeatGuard
where
    W: Write + Send + 'static,
{
    let (stop, stopped) = mpsc::channel();
    let worker = thread::spawn(move || {
        while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
            if writer
                .write_all(HEARTBEAT_FRAME)
                .and_then(|_| writer.flush())
                .is_err()
            {
                break;
            }
        }
    });

    HeartbeatGuard {
        stop: Some(stop),
        worker: Some(worker),
    }
}

fn main() -> ProcExit {
    // stdout은 JSONL 전용. LineWriter가 아니라 raw lock을 쓰고 emitter가
    // 명시적으로 flush한다(§2.1 "flushed individually").
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut err = io::stderr();

    let code = match parse_args_os(std::env::args_os().skip(1)) {
        Ok(cmd) => {
            let heartbeat = matches!(&cmd, Command::Dump { .. })
                .then(|| start_heartbeat(io::stderr(), HEARTBEAT_INTERVAL));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    struct ChannelWriter(mpsc::Sender<Vec<u8>>);

    impl Write for ChannelWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .send(buf.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "receiver closed"))?;
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn heartbeat_emits_the_host_watchdog_frame() {
        let (tx, rx) = mpsc::channel();
        let guard = start_heartbeat(ChannelWriter(tx), Duration::from_millis(1));
        let frame = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("heartbeat must arrive before the timeout");
        drop(guard);

        assert_eq!(frame, b"{\"heartbeat\":true}\n");
    }
}
