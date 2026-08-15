//! Installation/discovery contract for the two advertised source extensions.

#[cfg(unix)]
const UNIX_INSTALLER: &str = include_str!("../scripts/install.sh");
const WINDOWS_INSTALLER: &str = include_str!("../scripts/install.ps1");

#[test]
fn windows_installer_exposes_hwp_and_hwpx_environment_overrides() {
    assert!(
        WINDOWS_INSTALLER.contains("OFFICECLI_PLUGIN_DUMP_READER_HWPX ="),
        "install.ps1 must expose the HWPX discovery override"
    );
    assert!(
        WINDOWS_INSTALLER.contains("OFFICECLI_PLUGIN_DUMP_READER_HWP ="),
        "install.ps1 must expose the HWP discovery override"
    );
}

#[test]
fn windows_installer_manages_both_extension_directories() {
    for extension in ["hwp", "hwpx"] {
        let directory = format!("dump-reader\\{extension}\"");
        assert!(
            WINDOWS_INSTALLER.contains(&directory),
            "install.ps1 must manage {directory}"
        );
    }
}

#[test]
fn windows_uninstall_checks_reparse_directories_before_removing_targets() {
    let uninstall = WINDOWS_INSTALLER
        .find("if ($Uninstall)")
        .expect("install.ps1 uninstall branch");
    let uninstall_branch = &WINDOWS_INSTALLER[uninstall..];
    let guard = uninstall_branch
        .find("Assert-InstallDirectoryNotReparse $target.Directory")
        .expect("install.ps1 must call its reparse guard for each extension directory");
    let removal = uninstall_branch
        .find("Remove-Item -LiteralPath $target.Path -Force")
        .expect("install.ps1 target removal");
    assert!(
        guard < removal,
        "reparse directories must be rejected before uninstall addresses child paths"
    );
}

#[cfg(unix)]
fn run_unix_installer_at(
    installer: &std::path::Path,
    home: &std::path::Path,
    argument: &str,
) -> std::process::Output {
    std::process::Command::new(installer)
        .arg(argument)
        .env("HOME", home)
        .output()
        .expect("run Unix installer")
}

#[cfg(unix)]
fn run_unix_installer(home: &std::path::Path, argument: &str) -> std::process::Output {
    run_unix_installer_at(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/install.sh")),
        home,
        argument,
    )
}

#[cfg(unix)]
fn fake_installer_repo() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;

    let repo = tempfile::tempdir().expect("fake installer repo");
    let scripts = repo.path().join("scripts");
    let release = repo.path().join("target/release");
    std::fs::create_dir_all(&scripts).expect("create scripts dir");
    std::fs::create_dir_all(&release).expect("create release dir");

    let installer = scripts.join("install.sh");
    std::fs::write(&installer, UNIX_INSTALLER).expect("copy installer");
    std::fs::set_permissions(&installer, std::fs::Permissions::from_mode(0o755))
        .expect("make installer executable");

    let binary = release.join("officecli-dump-reader-hwpx");
    std::fs::write(
        &binary,
        b"#!/bin/sh\nprintf '%s\\n' '{\"name\":\"officecli-hwpx\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\".hwpx\",\".hwp\"],\"target\":\"docx\"}'\n",
    )
    .expect("write fake plugin");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("make fake plugin executable");
    repo
}

#[cfg(unix)]
#[test]
fn unix_print_env_registers_both_extensions() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = run_unix_installer(home.path(), "--print-env");
    assert!(
        output.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("UTF-8 installer output");
    assert!(
        stdout.contains("OFFICECLI_PLUGIN_DUMP_READER_HWPX="),
        "missing HWPX override: {stdout}"
    );
    assert!(
        stdout.contains("OFFICECLI_PLUGIN_DUMP_READER_HWP="),
        "missing HWP override: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn unix_uninstall_removes_both_extensions() {
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let hwp = root.join("hwp/plugin");
    let hwpx = root.join("hwpx/plugin");
    std::fs::create_dir_all(hwp.parent().expect("hwp parent")).expect("create hwp dir");
    std::fs::create_dir_all(hwpx.parent().expect("hwpx parent")).expect("create hwpx dir");
    std::fs::write(&hwp, b"old hwp plugin").expect("write hwp plugin");
    std::fs::write(&hwpx, b"old hwpx plugin").expect("write hwpx plugin");
    let unrelated = root.join("other/plugin");
    std::fs::create_dir_all(unrelated.parent().expect("other parent"))
        .expect("create unrelated dir");
    std::fs::write(&unrelated, b"unrelated plugin").expect("write unrelated plugin");

    let output = run_unix_installer(home.path(), "--uninstall");
    assert!(
        output.status.success(),
        "uninstall failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!hwp.exists(), "HWP plugin must be removed");
    assert!(!hwpx.exists(), "HWPX plugin must be removed");
    assert!(unrelated.exists(), "unrelated plugins must be preserved");

    let repeated = run_unix_installer(home.path(), "--uninstall");
    assert!(
        repeated.status.success(),
        "repeated uninstall must be idempotent: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert!(
        unrelated.exists(),
        "repeated uninstall must preserve unrelated plugins"
    );
}

#[cfg(unix)]
#[test]
fn unix_uninstall_does_not_follow_a_symlinked_extension_directory() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().expect("temporary home");
    let outside = tempfile::tempdir().expect("external directory");
    let outside_plugin = outside.path().join("plugin");
    std::fs::write(&outside_plugin, b"must remain outside plugin root")
        .expect("write external plugin");

    let root = home.path().join(".officecli/plugins/dump-reader");
    std::fs::create_dir_all(&root).expect("create plugin root");
    symlink(outside.path(), root.join("hwp")).expect("link HWP extension outside root");

    let output = run_unix_installer(home.path(), "--uninstall");
    assert!(
        !output.status.success(),
        "uninstall must fail closed for a symlinked extension directory"
    );
    assert!(
        outside_plugin.exists(),
        "uninstall must not delete through an extension-directory symlink"
    );
}

#[cfg(unix)]
#[test]
fn unix_install_places_an_executable_and_relative_hwp_link() {
    use std::os::unix::fs::PermissionsExt;

    let repo = fake_installer_repo();
    let home = tempfile::tempdir().expect("temporary home");
    let installer = repo.path().join("scripts/install.sh");
    let output = run_unix_installer_at(&installer, home.path(), "--no-build");
    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let root = home.path().join(".officecli/plugins/dump-reader");
    let hwp = root.join("hwp/plugin");
    let hwpx = root.join("hwpx/plugin");
    assert!(
        hwpx.exists(),
        "HWPX install path missing; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let hwpx_metadata = std::fs::metadata(&hwpx).expect("installed HWPX plugin");
    assert!(hwpx_metadata.is_file());
    assert_ne!(
        hwpx_metadata.permissions().mode() & 0o111,
        0,
        "HWPX plugin must be executable"
    );

    let hwp_metadata = std::fs::symlink_metadata(&hwp).expect("installed HWP link");
    assert!(hwp_metadata.file_type().is_symlink());
    assert_eq!(
        std::fs::read_link(&hwp).expect("HWP link target"),
        std::path::Path::new("../hwpx/plugin")
    );
    assert_eq!(
        std::fs::read(&hwp).expect("read through HWP link"),
        std::fs::read(&hwpx).expect("read HWPX plugin")
    );

    let repeated = run_unix_installer_at(&installer, home.path(), "--no-build");
    assert!(
        repeated.status.success(),
        "reinstall failed: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(
        std::fs::read_link(&hwp).expect("reinstalled HWP link target"),
        std::path::Path::new("../hwpx/plugin")
    );
    for directory in [root.join("hwp"), root.join("hwpx")] {
        let leftovers: Vec<_> = std::fs::read_dir(&directory)
            .expect("read install directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "plugin")
            .collect();
        assert!(
            leftovers.is_empty(),
            "reinstall left staging or backup files in {}: {leftovers:?}",
            directory.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_install_rolls_back_when_hwp_staging_fails() {
    use std::os::unix::fs::PermissionsExt;

    let repo = fake_installer_repo();
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let hwp_dir = root.join("hwp");
    let hwpx_dir = root.join("hwpx");
    std::fs::create_dir_all(&hwp_dir).expect("create HWP dir");
    std::fs::create_dir_all(&hwpx_dir).expect("create HWPX dir");
    let old_plugin = hwpx_dir.join("plugin");
    std::fs::write(&old_plugin, b"known old plugin").expect("write old plugin");
    std::fs::set_permissions(&hwp_dir, std::fs::Permissions::from_mode(0o500))
        .expect("make HWP dir unwritable");

    let installer = repo.path().join("scripts/install.sh");
    let output = run_unix_installer_at(&installer, home.path(), "--no-build");
    std::fs::set_permissions(&hwp_dir, std::fs::Permissions::from_mode(0o700))
        .expect("restore HWP dir permissions");

    assert!(
        !output.status.success(),
        "staging failure must fail install; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(&old_plugin).expect("old plugin remains"),
        b"known old plugin"
    );
    assert!(!hwp_dir.join("plugin").exists());
    for directory in [&hwp_dir, &hwpx_dir] {
        let leftovers: Vec<_> = std::fs::read_dir(directory)
            .expect("read install directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(".plugin")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "failed install left temporary files in {}: {leftovers:?}",
            directory.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_install_preserves_both_old_plugins_when_hwp_backup_move_fails() {
    use std::os::unix::fs::PermissionsExt;

    let repo = fake_installer_repo();
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let hwp = root.join("hwp/plugin");
    let hwpx = root.join("hwpx/plugin");
    std::fs::create_dir_all(hwp.parent().expect("HWP parent")).expect("create HWP dir");
    std::fs::create_dir_all(hwpx.parent().expect("HWPX parent")).expect("create HWPX dir");
    std::fs::write(&hwp, b"known old HWP plugin").expect("write old HWP plugin");
    std::fs::write(&hwpx, b"known old HWPX plugin").expect("write old HWPX plugin");

    let wrapper_dir = repo.path().join("test-bin");
    std::fs::create_dir(&wrapper_dir).expect("create wrapper dir");
    let mv_wrapper = wrapper_dir.join("mv");
    std::fs::write(
        &mv_wrapper,
        b"#!/bin/sh\nif [ \"$1\" = \"$FAIL_MV_SOURCE\" ]; then exit 1; fi\nexec /bin/mv \"$@\"\n",
    )
    .expect("write mv wrapper");
    std::fs::set_permissions(&mv_wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("make mv wrapper executable");

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(wrapper_dir.clone()).chain(std::env::split_paths(&current_path)),
    )
    .expect("compose test PATH");
    let installer = repo.path().join("scripts/install.sh");
    let output = std::process::Command::new(&installer)
        .arg("--no-build")
        .env("HOME", home.path())
        .env("PATH", test_path)
        .env("FAIL_MV_SOURCE", &hwp)
        .output()
        .expect("run installer with failing HWP backup move");

    assert!(
        !output.status.success(),
        "injected backup failure must fail install"
    );
    assert_eq!(
        std::fs::read(&hwp).expect("old HWP plugin remains"),
        b"known old HWP plugin"
    );
    assert_eq!(
        std::fs::read(&hwpx).expect("old HWPX plugin remains"),
        b"known old HWPX plugin"
    );
}
