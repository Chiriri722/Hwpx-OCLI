//! Installation/discovery contract for the two advertised source extensions.

#[cfg(unix)]
const UNIX_INSTALLER: &str = include_str!("../../../scripts/install.sh");
const WINDOWS_INSTALLER: &str = include_str!("../../../scripts/install.ps1");

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

#[test]
fn windows_installer_tracks_each_stage_before_copying_into_it() {
    let stage = WINDOWS_INSTALLER
        .find("$stage = Join-Path $target.Directory")
        .expect("install.ps1 stage path");
    let stage_tracking = WINDOWS_INSTALLER[stage..]
        .find("$staged += [PSCustomObject]")
        .map(|offset| stage + offset)
        .expect("install.ps1 must track a stage for cleanup");
    let copy = WINDOWS_INSTALLER[stage..]
        .find("Copy-Item -LiteralPath $builtBinary -Destination $stage")
        .map(|offset| stage + offset)
        .expect("install.ps1 stage copy");

    assert!(
        stage_tracking < copy,
        "a stage must be registered for cleanup before any fallible copy or validation"
    );
}

#[test]
fn windows_installer_never_deletes_recovery_backups_from_finally() {
    let transaction_finally = WINDOWS_INSTALLER
        .rfind("} finally {")
        .expect("install.ps1 transaction finally block");
    let finally_block = &WINDOWS_INSTALLER[transaction_finally..];

    assert!(
        !finally_block.contains("$record.Backup"),
        "rollback failure must leave its recovery backup intact"
    );
}

#[test]
fn windows_installer_creates_managed_directories_without_provider_path_expansion() {
    assert!(
        WINDOWS_INSTALLER.contains("[IO.Directory]::CreateDirectory($target.Directory)"),
        "managed directories must be created through the literal .NET path API"
    );
    assert!(
        !WINDOWS_INSTALLER.contains("New-Item -ItemType Directory -Force -Path $target.Directory"),
        "PowerShell provider path creation must not reinterpret HOME characters"
    );
}

#[cfg(windows)]
fn windows_installer_path() -> &'static std::path::Path {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/install.ps1"
    ))
}

#[cfg(windows)]
fn run_windows_installer(home: &std::path::Path, argument: &str) -> std::process::Output {
    run_windows_installer_at(windows_installer_path(), home, argument)
}

#[cfg(windows)]
fn run_windows_installer_at(
    installer: &std::path::Path,
    home: &std::path::Path,
    argument: &str,
) -> std::process::Output {
    std::process::Command::new("pwsh")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(installer)
        .arg(argument)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()
        .expect("run Windows installer")
}

#[cfg(windows)]
fn fake_windows_installer_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("fake Windows installer repo");
    let scripts = repo.path().join("scripts");
    let release = repo.path().join("target/release");
    std::fs::create_dir_all(&scripts).expect("create scripts dir");
    std::fs::create_dir_all(&release).expect("create release dir");
    std::fs::write(scripts.join("install.ps1"), WINDOWS_INSTALLER).expect("copy Windows installer");
    std::fs::copy(
        env!("CARGO_BIN_EXE_officecli-dump-reader-hwpx"),
        release.join("officecli-dump-reader-hwpx.exe"),
    )
    .expect("copy test plugin");
    repo
}

#[cfg(windows)]
fn create_windows_junction(link: &std::path::Path, target: &std::path::Path) {
    std::fs::create_dir_all(link.parent().expect("junction parent"))
        .expect("create junction parent");
    let output = std::process::Command::new("pwsh")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "New-Item -ItemType Junction -Path $env:HWPX_TEST_LINK -Target $env:HWPX_TEST_TARGET -ErrorAction Stop | Out-Null",
        ])
        .env("HWPX_TEST_LINK", link)
        .env("HWPX_TEST_TARGET", target)
        .output()
        .expect("create Windows junction");
    assert!(
        output.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
fn physical_path_for_junction(
    logical: &std::path::Path,
    junction: &std::path::Path,
    target: &std::path::Path,
) -> std::path::PathBuf {
    logical
        .strip_prefix(junction)
        .map_or_else(|_| logical.to_path_buf(), |suffix| target.join(suffix))
}

#[cfg(windows)]
#[test]
fn windows_uninstall_rejects_reparse_points_in_every_existing_managed_component() {
    const OLD_HWP: &[u8] = b"external HWP plugin must remain";
    const OLD_HWPX: &[u8] = b"external HWPX plugin must remain";
    let managed_components = [
        ".officecli",
        ".officecli/plugins",
        ".officecli/plugins/dump-reader",
        ".officecli/plugins/dump-reader/hwp",
        ".officecli/plugins/dump-reader/hwpx",
    ];

    for relative_junction in managed_components {
        let home = tempfile::tempdir().expect("temporary Windows home");
        let outside = tempfile::tempdir().expect("external Windows directory");
        let junction = home.path().join(relative_junction);
        create_windows_junction(&junction, outside.path());

        let root = home.path().join(".officecli/plugins/dump-reader");
        let logical_hwp = root.join("hwp/plugin.exe");
        let logical_hwpx = root.join("hwpx/plugin.exe");
        let physical_hwp = physical_path_for_junction(&logical_hwp, &junction, outside.path());
        let physical_hwpx = physical_path_for_junction(&logical_hwpx, &junction, outside.path());
        for (path, contents) in [(&physical_hwp, OLD_HWP), (&physical_hwpx, OLD_HWPX)] {
            std::fs::create_dir_all(path.parent().expect("plugin parent"))
                .expect("create plugin parent");
            std::fs::write(path, contents).expect("write protected plugin");
            std::fs::write(
                path.parent().expect("plugin parent").join("keep.txt"),
                b"keep",
            )
            .expect("write directory sentinel");
        }

        let output = run_windows_installer(home.path(), "-Uninstall");
        let hwp_after = std::fs::read(&physical_hwp).ok();
        let hwpx_after = std::fs::read(&physical_hwpx).ok();
        std::fs::remove_dir(&junction).expect("remove test junction without following it");

        assert!(
            !output.status.success(),
            "uninstall must reject junction component {relative_junction}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            hwp_after.as_deref(),
            Some(OLD_HWP),
            "HWP target changed through junction component {relative_junction}"
        );
        assert_eq!(
            hwpx_after.as_deref(),
            Some(OLD_HWPX),
            "HWPX target changed through junction component {relative_junction}"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_uninstall_rejects_non_directory_managed_components() {
    for relative_file in [
        ".officecli",
        ".officecli/plugins",
        ".officecli/plugins/dump-reader",
        ".officecli/plugins/dump-reader/hwp",
        ".officecli/plugins/dump-reader/hwpx",
    ] {
        let home = tempfile::tempdir().expect("temporary Windows home");
        let component = home.path().join(relative_file);
        std::fs::create_dir_all(component.parent().expect("component parent"))
            .expect("create component parent");
        std::fs::write(&component, b"not a directory").expect("write invalid component");

        let output = run_windows_installer(home.path(), "-Uninstall");
        assert!(
            !output.status.success(),
            "uninstall must reject non-directory component {relative_file}"
        );
        assert_eq!(
            std::fs::read(&component).expect("invalid component remains"),
            b"not a directory"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_uninstall_rejects_a_broken_ancestor_junction() {
    let home = tempfile::tempdir().expect("temporary Windows home");
    let outside = tempfile::tempdir().expect("external Windows directory");
    let junction = home.path().join(".officecli");
    create_windows_junction(&junction, outside.path());
    let outside_path = outside.path().to_path_buf();
    drop(outside);

    let output = run_windows_installer(home.path(), "-Uninstall");
    std::fs::remove_dir(&junction).expect("remove broken test junction");

    assert!(
        !output.status.success(),
        "uninstall must reject a broken ancestor junction to {}",
        outside_path.display()
    );
}

#[cfg(windows)]
#[test]
fn windows_uninstall_remains_idempotent_when_the_managed_tree_is_missing() {
    let home = tempfile::tempdir().expect("temporary Windows home");
    let output = run_windows_installer(home.path(), "-Uninstall");

    assert!(
        output.status.success(),
        "missing install tree must remain a safe no-op: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn windows_install_restores_the_first_target_when_the_second_commit_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;

    const OLD_HWP: &[u8] = b"known old HWP plugin";
    const OLD_HWPX: &[u8] = b"known old HWPX plugin";
    let repo = fake_windows_installer_repo();
    let installer = repo.path().join("scripts/install.ps1");
    let home = tempfile::tempdir().expect("temporary Windows home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let hwp = root.join("hwp/plugin.exe");
    let hwpx = root.join("hwpx/plugin.exe");
    std::fs::create_dir_all(hwp.parent().expect("HWP parent")).expect("create HWP dir");
    std::fs::create_dir_all(hwpx.parent().expect("HWPX parent")).expect("create HWPX dir");
    std::fs::write(&hwp, OLD_HWP).expect("write old HWP plugin");
    std::fs::write(&hwpx, OLD_HWPX).expect("write old HWPX plugin");

    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&hwpx)
        .expect("lock HWPX target against replacement");
    let output = run_windows_installer_at(&installer, home.path(), "-NoBuild");
    drop(lock);

    assert!(
        !output.status.success(),
        "locked second commit must fail installation"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to commit hwpx plugin"),
        "test must reach the locked second commit: {stderr}"
    );
    assert!(
        !stderr.contains("rollback incomplete"),
        "first target rollback unexpectedly failed: {stderr}"
    );
    assert_eq!(std::fs::read(&hwp).expect("restored HWP target"), OLD_HWP);
    assert_eq!(
        std::fs::read(&hwpx).expect("unchanged HWPX target"),
        OLD_HWPX
    );
    for directory in [hwp.parent().unwrap(), hwpx.parent().unwrap()] {
        let leftovers: Vec<_> = std::fs::read_dir(directory)
            .expect("read install directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "plugin.exe")
            .collect();
        assert!(
            leftovers.is_empty(),
            "successful rollback left artifacts in {}: {leftovers:?}",
            directory.display()
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_install_treats_brackets_in_home_as_literal_characters() {
    let repo = fake_windows_installer_repo();
    let installer = repo.path().join("scripts/install.ps1");
    let parent = tempfile::tempdir().expect("temporary Windows parent");
    let home = parent.path().join("home[with]brackets");
    std::fs::create_dir(&home).expect("create bracketed Windows home");

    let output = run_windows_installer_at(&installer, &home, "-NoBuild");

    assert!(
        output.status.success(),
        "bracketed HOME must install literally: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for extension in ["hwp", "hwpx"] {
        assert!(
            home.join(format!(
                ".officecli/plugins/dump-reader/{extension}/plugin.exe"
            ))
            .is_file(),
            "missing literal {extension} install target"
        );
    }
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
        std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/install.sh"
        )),
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
fn unix_installer_rejects_relative_home_for_every_action() {
    let repo = fake_installer_repo();
    let installer = repo.path().join("scripts/install.sh");

    for argument in ["--uninstall", "--print-env", "--no-build"] {
        let working = tempfile::tempdir().expect("temporary Unix working directory");
        let relative_home = std::path::Path::new("relative-home");
        let hwp = working
            .path()
            .join(relative_home)
            .join(".officecli/plugins/dump-reader/hwp/plugin");
        let hwpx = working
            .path()
            .join(relative_home)
            .join(".officecli/plugins/dump-reader/hwpx/plugin");
        std::fs::create_dir_all(hwp.parent().expect("HWP parent")).expect("create HWP dir");
        std::fs::create_dir_all(hwpx.parent().expect("HWPX parent")).expect("create HWPX dir");
        std::fs::write(&hwp, b"known old HWP plugin").expect("write old HWP plugin");
        std::fs::write(&hwpx, b"known old HWPX plugin").expect("write old HWPX plugin");

        let output = std::process::Command::new(&installer)
            .arg(argument)
            .current_dir(working.path())
            .env("HOME", relative_home)
            .output()
            .expect("run installer with relative HOME");

        assert!(
            !output.status.success(),
            "{argument} must reject relative HOME"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("HOME must be an absolute path"),
            "{argument} must explain the invalid HOME: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(&hwp).expect("HWP target remains"),
            b"known old HWP plugin"
        );
        assert_eq!(
            std::fs::read(&hwpx).expect("HWPX target remains"),
            b"known old HWPX plugin"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_help_does_not_require_an_absolute_home() {
    let output = std::process::Command::new(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/install.sh"
    )))
    .arg("--help")
    .env("HOME", "relative-home")
    .output()
    .expect("run Unix installer help");

    assert!(
        output.status.success(),
        "help must not depend on HOME: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("사용법:"));
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
fn unix_uninstall_rejects_symlinks_in_every_existing_managed_component() {
    use std::os::unix::fs::symlink;

    const OLD_HWP: &[u8] = b"external HWP plugin must remain";
    const OLD_HWPX: &[u8] = b"external HWPX plugin must remain";
    let managed_components = [
        ".officecli",
        ".officecli/plugins",
        ".officecli/plugins/dump-reader",
        ".officecli/plugins/dump-reader/hwp",
        ".officecli/plugins/dump-reader/hwpx",
    ];

    for relative_link in managed_components {
        let home = tempfile::tempdir().expect("temporary Unix home");
        let outside = tempfile::tempdir().expect("external Unix directory");
        let link = home.path().join(relative_link);
        std::fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
        symlink(outside.path(), &link).expect("create managed-path symlink");

        let root = home.path().join(".officecli/plugins/dump-reader");
        let logical_hwp = root.join("hwp/plugin");
        let logical_hwpx = root.join("hwpx/plugin");
        let physical = |logical: &std::path::Path| {
            logical.strip_prefix(&link).map_or_else(
                |_| logical.to_path_buf(),
                |suffix| outside.path().join(suffix),
            )
        };
        let physical_hwp = physical(&logical_hwp);
        let physical_hwpx = physical(&logical_hwpx);
        for (path, contents) in [(&physical_hwp, OLD_HWP), (&physical_hwpx, OLD_HWPX)] {
            std::fs::create_dir_all(path.parent().expect("plugin parent"))
                .expect("create plugin parent");
            std::fs::write(path, contents).expect("write protected plugin");
            std::fs::write(path.parent().expect("plugin parent").join("keep"), b"keep")
                .expect("write directory sentinel");
        }

        let output = run_unix_installer(home.path(), "--uninstall");
        let hwp_after = std::fs::read(&physical_hwp).ok();
        let hwpx_after = std::fs::read(&physical_hwpx).ok();
        std::fs::remove_file(&link).expect("remove test symlink without following it");

        assert!(
            !output.status.success(),
            "uninstall must reject symlink component {relative_link}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            hwp_after.as_deref(),
            Some(OLD_HWP),
            "HWP target changed through symlink component {relative_link}"
        );
        assert_eq!(
            hwpx_after.as_deref(),
            Some(OLD_HWPX),
            "HWPX target changed through symlink component {relative_link}"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_uninstall_rejects_non_directory_managed_components() {
    for relative_file in [
        ".officecli",
        ".officecli/plugins",
        ".officecli/plugins/dump-reader",
        ".officecli/plugins/dump-reader/hwp",
        ".officecli/plugins/dump-reader/hwpx",
    ] {
        let home = tempfile::tempdir().expect("temporary Unix home");
        let component = home.path().join(relative_file);
        std::fs::create_dir_all(component.parent().expect("component parent"))
            .expect("create component parent");
        std::fs::write(&component, b"not a directory").expect("write invalid component");

        let output = run_unix_installer(home.path(), "--uninstall");
        assert!(
            !output.status.success(),
            "uninstall must reject non-directory component {relative_file}"
        );
        assert_eq!(
            std::fs::read(&component).expect("invalid component remains"),
            b"not a directory"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_uninstall_rejects_a_broken_ancestor_symlink() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().expect("temporary Unix home");
    let link = home.path().join(".officecli");
    let missing_target = home.path().join("missing-link-target");
    symlink(&missing_target, &link).expect("create broken ancestor symlink");

    let output = run_unix_installer(home.path(), "--uninstall");
    std::fs::remove_file(&link).expect("remove broken test symlink");

    assert!(
        !output.status.success(),
        "uninstall must reject a broken ancestor symlink"
    );
}

#[cfg(unix)]
#[test]
fn unix_uninstall_preflights_both_targets_before_removing_either() {
    for invalid_extension in ["hwp", "hwpx"] {
        let home = tempfile::tempdir().expect("temporary Unix home");
        let root = home.path().join(".officecli/plugins/dump-reader");
        let hwp = root.join("hwp/plugin");
        let hwpx = root.join("hwpx/plugin");
        std::fs::create_dir_all(hwp.parent().expect("HWP parent")).expect("create HWP dir");
        std::fs::create_dir_all(hwpx.parent().expect("HWPX parent")).expect("create HWPX dir");
        let (invalid, valid) = if invalid_extension == "hwp" {
            (&hwp, &hwpx)
        } else {
            (&hwpx, &hwp)
        };
        std::fs::create_dir(invalid).expect("create invalid target directory");
        std::fs::write(invalid.join("keep"), b"keep").expect("write directory sentinel");
        std::fs::write(valid, b"known old peer plugin").expect("write valid peer plugin");

        let output = run_unix_installer(home.path(), "--uninstall");

        assert!(
            !output.status.success(),
            "{invalid_extension} directory target must fail uninstall preflight"
        );
        assert_eq!(
            std::fs::read(valid).expect("valid peer target remains"),
            b"known old peer plugin"
        );
        assert!(
            invalid.join("keep").exists(),
            "invalid target must remain untouched"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_install_preflights_both_targets_before_staging_or_backup() {
    for invalid_extension in ["hwp", "hwpx"] {
        let repo = fake_installer_repo();
        let home = tempfile::tempdir().expect("temporary Unix home");
        let root = home.path().join(".officecli/plugins/dump-reader");
        let hwp = root.join("hwp/plugin");
        let hwpx = root.join("hwpx/plugin");
        std::fs::create_dir_all(hwp.parent().expect("HWP parent")).expect("create HWP dir");
        std::fs::create_dir_all(hwpx.parent().expect("HWPX parent")).expect("create HWPX dir");
        let (invalid, valid) = if invalid_extension == "hwp" {
            (&hwp, &hwpx)
        } else {
            (&hwpx, &hwp)
        };
        std::fs::create_dir(invalid).expect("create invalid target directory");
        std::fs::write(invalid.join("keep"), b"keep").expect("write directory sentinel");
        std::fs::write(valid, b"known old peer plugin").expect("write valid peer plugin");

        let output = run_unix_installer_at(
            &repo.path().join("scripts/install.sh"),
            home.path(),
            "--no-build",
        );

        assert!(
            !output.status.success(),
            "{invalid_extension} directory target must fail install preflight"
        );
        assert_eq!(
            std::fs::read(valid).expect("valid peer target remains"),
            b"known old peer plugin"
        );
        assert!(
            invalid.join("keep").exists(),
            "invalid target must remain untouched"
        );
    }
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

#[cfg(unix)]
#[test]
fn unix_install_keeps_a_valid_install_when_backup_cleanup_fails() {
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
    let rm_wrapper = wrapper_dir.join("rm");
    std::fs::write(
        &rm_wrapper,
        b"#!/bin/sh\nfor arg do\n  case \"$arg\" in *.plugin.backup.*) exit 1 ;; esac\ndone\nexec /bin/rm \"$@\"\n",
    )
    .expect("write rm wrapper");
    std::fs::set_permissions(&rm_wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("make rm wrapper executable");

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(wrapper_dir).chain(std::env::split_paths(&current_path)),
    )
    .expect("compose test PATH");
    let output = std::process::Command::new(repo.path().join("scripts/install.sh"))
        .arg("--no-build")
        .env("HOME", home.path())
        .env("PATH", test_path)
        .output()
        .expect("run installer with failing backup cleanup");

    assert!(
        output.status.success(),
        "a valid committed install must survive backup cleanup failure: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("backup cleanup failed"),
        "cleanup failure must identify the preserved recovery artifact: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::fs::symlink_metadata(&hwp)
            .expect("installed HWP target")
            .file_type()
            .is_symlink(),
        "the newly committed HWP discovery link must remain installed"
    );
    assert!(
        std::fs::metadata(&hwpx)
            .expect("installed HWPX target")
            .permissions()
            .mode()
            & 0o111
            != 0,
        "the newly committed HWPX plugin must remain executable"
    );
    for directory in [hwp.parent().unwrap(), hwpx.parent().unwrap()] {
        let backups: Vec<_> = std::fs::read_dir(directory)
            .expect("read install directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".plugin.backup.")
            })
            .collect();
        assert_eq!(
            backups.len(),
            1,
            "failed cleanup must preserve one recovery backup in {}: {backups:?}",
            directory.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_rollback_continues_after_committed_target_removal_fails() {
    use std::os::unix::fs::PermissionsExt;

    const OLD_HWP: &[u8] = b"known old HWP plugin";
    const OLD_HWPX: &[u8] = b"known old HWPX plugin";
    let repo = fake_installer_repo();
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let hwp = root.join("hwp/plugin");
    let hwpx = root.join("hwpx/plugin");
    std::fs::create_dir_all(hwp.parent().expect("HWP parent")).expect("create HWP dir");
    std::fs::create_dir_all(hwpx.parent().expect("HWPX parent")).expect("create HWPX dir");
    std::fs::write(&hwp, OLD_HWP).expect("write old HWP plugin");
    std::fs::write(&hwpx, OLD_HWPX).expect("write old HWPX plugin");

    let wrapper_dir = repo.path().join("test-bin");
    std::fs::create_dir(&wrapper_dir).expect("create wrapper dir");
    let mv_wrapper = wrapper_dir.join("mv");
    std::fs::write(
        &mv_wrapper,
        b"#!/bin/sh\ndest=\nfor arg do dest=$arg; done\ncase \"$1\" in */.plugin-link.*) [ \"$dest\" = \"$FAIL_HWP_COMMIT_DEST\" ] && exit 1 ;; esac\nexec /bin/mv \"$@\"\n",
    )
    .expect("write mv wrapper");
    std::fs::set_permissions(&mv_wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("make mv wrapper executable");
    let rm_wrapper = wrapper_dir.join("rm");
    std::fs::write(
        &rm_wrapper,
        b"#!/bin/sh\nfor arg do\n  [ \"$arg\" = \"$FAIL_RM_TARGET\" ] && exit 1\ndone\nexec /bin/rm \"$@\"\n",
    )
    .expect("write rm wrapper");
    std::fs::set_permissions(&rm_wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("make rm wrapper executable");

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(wrapper_dir).chain(std::env::split_paths(&current_path)),
    )
    .expect("compose test PATH");
    let output = std::process::Command::new(repo.path().join("scripts/install.sh"))
        .arg("--no-build")
        .env("HOME", home.path())
        .env("PATH", test_path)
        .env("FAIL_HWP_COMMIT_DEST", &hwp)
        .env("FAIL_RM_TARGET", &hwpx)
        .output()
        .expect("run installer with failing commit and rollback removal");

    assert!(
        !output.status.success(),
        "injected HWP commit failure must fail installation"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("could not remove committed HWPX target during rollback"),
        "rollback must diagnose the failed removal: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(&hwp).expect("restored HWP target"), OLD_HWP);
    assert_eq!(
        std::fs::read(&hwpx).expect("restored HWPX target"),
        OLD_HWPX
    );
}
