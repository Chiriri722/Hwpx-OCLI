use std::io::{self, Cursor, Write};
use std::sync::mpsc;
use std::time::Duration;

use officecli_hancom_core::budget::ResourceBudget;
use officecli_hancom_core::container::{detect_reader, SourceFormat};
use officecli_hancom_core::diagnostics::escape_diagnostic_text;
use officecli_hancom_core::heartbeat::{HeartbeatGuard, HEARTBEAT_FRAME};
use officecli_hancom_core::model::{hwpunit_to_point, hwpunit_to_twip};

#[test]
fn container_detection_is_shared_without_trusting_extensions() {
    let input = Cursor::new(b"HWP Document File V3.00\0payload".to_vec());
    assert_eq!(
        detect_reader(input).expect("detect HWP 3"),
        SourceFormat::Hwp3
    );
}

#[test]
fn document_units_are_shared_at_the_protocol_boundary() {
    assert_eq!(hwpunit_to_twip(7_200), 1_440);
    assert_eq!(hwpunit_to_point(7_200), 72.0);
}

#[test]
fn resource_budget_rejects_over_limit_without_committing_usage() {
    let mut budget = ResourceBudget::new("embedded image bytes", 5);
    budget.consume(3).expect("first allocation");
    budget.consume(2).expect("exact limit");

    let error = budget.consume(1).expect_err("over-limit allocation");
    assert!(error.message.contains("embedded image bytes"));
    assert_eq!(
        budget.used(),
        5,
        "a rejected allocation must not mutate usage"
    );
}

#[test]
fn diagnostics_escape_terminal_and_line_control_characters() {
    assert_eq!(
        escape_diagnostic_text("line\n\u{1b}[31m\u{2028}"),
        "line\\n\\u{1b}[31m\\u{2028}"
    );
}

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
fn heartbeat_uses_the_host_watchdog_frame() {
    let (tx, rx) = mpsc::channel();
    let guard = HeartbeatGuard::start(ChannelWriter(tx), Duration::from_millis(1));
    let frame = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("heartbeat must arrive");
    drop(guard);

    assert_eq!(frame, HEARTBEAT_FRAME);
    assert_eq!(frame, b"{\"heartbeat\":true}\n");
}
