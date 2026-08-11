//! 입력 파일의 포맷을 판별해 보고한다. 진단용.
//!
//! ```sh
//! cargo run --release --example detect -- 파일1.hwp 파일2.hwpx ...
//! ```
//!
//! 확장자가 실제 내용과 다른 파일을 찾을 때 쓴다. HWP 5.x는 버전과 보호 상태
//! (암호·DRM)까지 보고한다.

use officecli_hwpx::format::{detect_path, SourceFormat};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: detect <file>...");
        return std::process::ExitCode::from(64); // EX_USAGE
    }

    let mut failures = 0u8;
    for arg in &args {
        let path = std::path::Path::new(arg);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| arg.clone());

        match detect_path(path) {
            Ok(SourceFormat::Hwp5(info)) => {
                let notes = info.protection_notes();
                let protection = if notes.is_empty() {
                    "none".to_string()
                } else {
                    notes.join(",")
                };
                println!(
                    "  hwp5   {name}  version={} compressed={} protection={protection}",
                    info.version_string(),
                    info.compressed
                );
            }
            Ok(f) => println!("  {:6} {name}", f.label()),
            Err(e) => {
                println!("  error  {name}  {e}");
                failures = failures.saturating_add(1);
            }
        }
    }

    if failures > 0 {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}
