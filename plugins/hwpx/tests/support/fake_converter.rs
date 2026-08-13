//! Standalone Windows test helper compiled by `protocol_contract.rs`.
//!
//! This lives below `tests/support` so Cargo does not treat it as another
//! integration-test crate. The Windows runner has `rustc` available already.

use std::ffi::OsString;
use std::path::PathBuf;

fn required_env(name: &str) -> OsString {
    std::env::var_os(name).unwrap_or_else(|| panic!("missing {name}"))
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).and_then(|arg| arg.to_str()) == Some("--tree-child") {
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }
    assert_eq!(args.len(), 4, "expected subcommand, source, and output");
    let output = PathBuf::from(&args[3]);
    if PathBuf::from(&args[2])
        .file_name()
        .and_then(|name| name.to_str())
        == Some("tree-mode.hwp")
    {
        // Let the caller assign this process to its Job Object, then create a
        // descendant that inherits stderr and would keep the pipe open.
        std::thread::sleep(std::time::Duration::from_millis(250));
        let child = std::process::Command::new(std::env::current_exe().expect("current exe"))
            .arg("--tree-child")
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("spawn tree child");
        std::fs::write(output.with_extension("pid"), child.id().to_string())
            .expect("write descendant pid");
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }

    match std::env::var("MOCK_MODE").as_deref() {
        Ok("copy") => {
            let log = PathBuf::from(required_env("MOCK_ARGS"));
            let rendered = args[1..]
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(log, format!("{rendered}\n")).expect("write argv log");
            std::fs::copy(PathBuf::from(required_env("MOCK_HWPX")), output)
                .expect("copy HWPX output");
        }
        Ok("nonzero") => {
            eprintln!("converter exploded");
            std::process::exit(9);
        }
        Ok("invalid") => std::fs::write(output, b"not hwpx").expect("write invalid output"),
        Ok("missing") => {}
        mode => panic!("unknown fake converter mode: {mode:?}"),
    }
}
