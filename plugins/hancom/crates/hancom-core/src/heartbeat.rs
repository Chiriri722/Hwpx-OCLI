//! OfficeCLI plugin watchdog heartbeat support.

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const HEARTBEAT_FRAME: &[u8] = b"{\"heartbeat\":true}\n";

pub struct HeartbeatGuard {
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HeartbeatGuard {
    pub fn start<W>(mut writer: W, interval: Duration) -> Self
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

        Self {
            stop: Some(stop),
            worker: Some(worker),
        }
    }
}

impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
