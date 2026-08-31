//! Hancom Show 12.0000 OOXML-carrier-subset dump-reader entry point.

fn main() -> std::process::ExitCode {
    officecli_hwpx::ooxml_carrier::main_entry(officecli_hwpx::ooxml_carrier::CarrierFamily::Show)
}
