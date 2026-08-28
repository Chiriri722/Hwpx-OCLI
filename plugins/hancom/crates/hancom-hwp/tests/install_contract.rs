//! Installation/discovery contract for every advertised Hancom source extension.

#[cfg(unix)]
const UNIX_INSTALLER: &str = include_str!("../../../scripts/install.sh");
const WINDOWS_INSTALLER: &str = include_str!("../../../scripts/install.ps1");
const EXTENSIONS: [&str; 4] = ["hwp", "hwpx", "owpml", "hml"];
const CANONICAL_BINARY: &str = "officecli-hancom-hwp";

#[test]
fn windows_installer_exposes_every_environment_override() {
    for extension in EXTENSIONS {
        let variable = format!(
            "OFFICECLI_PLUGIN_DUMP_READER_{}",
            extension.to_ascii_uppercase()
        );
        assert!(
            WINDOWS_INSTALLER.contains(&variable),
            "install.ps1 must expose the {extension} discovery override"
        );
    }
}

#[test]
fn windows_installer_manages_every_extension_directory() {
    for extension in EXTENSIONS {
        let declaration = format!("Extension = \"{extension}\"");
        assert!(
            WINDOWS_INSTALLER.contains(&declaration),
            "install.ps1 must manage the {extension} directory"
        );
    }
}

#[test]
fn installers_use_the_canonical_binary_name() {
    assert!(
        WINDOWS_INSTALLER.contains("$binaryName = \"officecli-hancom-hwp.exe\""),
        "install.ps1 must install the canonical binary"
    );
    assert!(
        !WINDOWS_INSTALLER.contains("$binaryName = \"officecli-dump-reader-hwpx.exe\""),
        "install.ps1 must not source the legacy compatibility entry point"
    );
    #[cfg(unix)]
    {
        assert!(
            UNIX_INSTALLER.contains("BIN_NAME=\"officecli-hancom-hwp\""),
            "install.sh must install the canonical binary"
        );
        assert!(
            !UNIX_INSTALLER.contains("BIN_NAME=\"officecli-dump-reader-hwpx\""),
            "install.sh must not source the legacy compatibility entry point"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_installer_tracks_transaction_state_per_extension() {
    for state in ["STAGED=()", "BACKUPS=()", "HAD_EXISTING=()", "COMMITTED=()"] {
        assert!(
            UNIX_INSTALLER.contains(state),
            "install.sh must maintain {state} for all extension indexes"
        );
    }
    for legacy_scalar in [
        "STAGED_HWP=",
        "STAGED_HWPX=",
        "BACKUP_HWP=",
        "BACKUP_HWPX=",
        "COMMITTED_HWP=",
        "COMMITTED_HWPX=",
    ] {
        assert!(
            !UNIX_INSTALLER.contains(legacy_scalar),
            "install.sh must not encode transaction state in {legacy_scalar}"
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
        env!("CARGO_BIN_EXE_officecli-hancom-hwp"),
        release.join(format!("{CANONICAL_BINARY}.exe")),
    )
    .expect("copy test plugin");
    repo
}

#[cfg(windows)]
#[test]
fn windows_print_env_registers_every_extension_with_the_canonical_binary() {
    let home = tempfile::tempdir().expect("temporary Windows home");
    let output = run_windows_installer(home.path(), "-PrintEnv");
    assert!(
        output.status.success(),
        "print-env failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("UTF-8 installer output");
    let assignments: Vec<_> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(assignments.len(), EXTENSIONS.len());
    for extension in EXTENSIONS {
        let variable = format!(
            "OFFICECLI_PLUGIN_DUMP_READER_{}",
            extension.to_ascii_uppercase()
        );
        let assignment = assignments
            .iter()
            .find(|line| line.contains(&variable))
            .unwrap_or_else(|| panic!("missing {extension} override: {stdout}"));
        assert!(
            assignment.contains(CANONICAL_BINARY),
            "{extension} override must point at the canonical binary: {assignment}"
        );
    }
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
    let managed_components = [
        ".officecli",
        ".officecli/plugins",
        ".officecli/plugins/dump-reader",
        ".officecli/plugins/dump-reader/hwp",
        ".officecli/plugins/dump-reader/hwpx",
        ".officecli/plugins/dump-reader/owpml",
        ".officecli/plugins/dump-reader/hml",
    ];

    for relative_junction in managed_components {
        let home = tempfile::tempdir().expect("temporary Windows home");
        let outside = tempfile::tempdir().expect("external Windows directory");
        let junction = home.path().join(relative_junction);
        create_windows_junction(&junction, outside.path());

        let root = home.path().join(".officecli/plugins/dump-reader");
        let physical_targets: Vec<_> = EXTENSIONS
            .iter()
            .map(|extension| {
                let logical = root.join(extension).join("plugin.exe");
                (
                    *extension,
                    physical_path_for_junction(&logical, &junction, outside.path()),
                    format!("external {extension} plugin must remain"),
                )
            })
            .collect();
        for (_, path, contents) in &physical_targets {
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
        let contents_after: Vec<_> = physical_targets
            .iter()
            .map(|(_, path, _)| std::fs::read_to_string(path).ok())
            .collect();
        std::fs::remove_dir(&junction).expect("remove test junction without following it");

        assert!(
            !output.status.success(),
            "uninstall must reject junction component {relative_junction}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for ((extension, _, expected), actual) in physical_targets.iter().zip(&contents_after) {
            assert_eq!(
                actual.as_deref(),
                Some(expected.as_str()),
                "{extension} target changed through junction component {relative_junction}"
            );
        }
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
        ".officecli/plugins/dump-reader/owpml",
        ".officecli/plugins/dump-reader/hml",
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
fn windows_uninstall_removes_every_extension_and_preserves_unrelated_plugins() {
    let home = tempfile::tempdir().expect("temporary Windows home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let targets: Vec<_> = EXTENSIONS
        .iter()
        .map(|extension| root.join(extension).join("plugin.exe"))
        .collect();
    for target in &targets {
        std::fs::create_dir_all(target.parent().expect("extension parent"))
            .expect("create extension dir");
        std::fs::write(target, b"old plugin").expect("write old plugin");
    }
    let unrelated = root.join("other/plugin.exe");
    std::fs::create_dir_all(unrelated.parent().expect("unrelated parent"))
        .expect("create unrelated directory");
    std::fs::write(&unrelated, b"unrelated plugin").expect("write unrelated plugin");

    for attempt in 0..2 {
        let output = run_windows_installer(home.path(), "-Uninstall");
        assert!(
            output.status.success(),
            "uninstall attempt {attempt} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for (extension, target) in EXTENSIONS.iter().zip(&targets) {
            assert!(!target.exists(), "{extension} target must be removed");
        }
        assert!(unrelated.exists(), "unrelated plugin must remain");
    }
}

#[cfg(windows)]
#[test]
fn windows_install_restores_every_prior_target_when_a_later_commit_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;

    let repo = fake_windows_installer_repo();
    let installer = repo.path().join("scripts/install.ps1");
    let home = tempfile::tempdir().expect("temporary Windows home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let targets: Vec<_> = EXTENSIONS
        .iter()
        .map(|extension| {
            (
                *extension,
                root.join(extension).join("plugin.exe"),
                format!("known old {extension} plugin"),
            )
        })
        .collect();
    for (_, target, contents) in &targets {
        std::fs::create_dir_all(target.parent().expect("extension parent"))
            .expect("create extension dir");
        std::fs::write(target, contents).expect("write old plugin");
    }

    let locked = targets
        .iter()
        .find(|(extension, _, _)| *extension == "owpml")
        .expect("OWPML target");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked.1)
        .expect("lock OWPML target against replacement");
    let output = run_windows_installer_at(&installer, home.path(), "-NoBuild");
    drop(lock);

    assert!(
        !output.status.success(),
        "locked later commit must fail installation"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to commit owpml plugin"),
        "test must reach the locked later commit: {stderr}"
    );
    assert!(
        !stderr.contains("rollback incomplete"),
        "first target rollback unexpectedly failed: {stderr}"
    );
    for (extension, target, expected) in &targets {
        assert_eq!(
            std::fs::read_to_string(target).expect("restored target"),
            expected.as_str(),
            "{extension} target was not restored"
        );
    }
    for (_, target, _) in &targets {
        let directory = target.parent().expect("extension parent");
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
fn windows_install_migrates_a_legacy_hwpx_only_layout_idempotently() {
    const LEGACY_PLUGIN: &[u8] = b"legacy officecli-dump-reader-hwpx installation";

    let repo = fake_windows_installer_repo();
    let installer = repo.path().join("scripts/install.ps1");
    let source = repo.path().join("target/release/officecli-hancom-hwp.exe");
    let expected = std::fs::read(&source).expect("canonical source binary");
    let home = tempfile::tempdir().expect("temporary Windows home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let legacy = root.join("hwpx/plugin.exe");
    std::fs::create_dir_all(legacy.parent().expect("legacy HWPX parent"))
        .expect("create legacy HWPX directory");
    std::fs::write(&legacy, LEGACY_PLUGIN).expect("write legacy HWPX plugin");

    for attempt in 0..2 {
        let output = run_windows_installer_at(&installer, home.path(), "-NoBuild");
        assert!(
            output.status.success(),
            "migration attempt {attempt} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for extension in EXTENSIONS {
            let target = root.join(extension).join("plugin.exe");
            assert_eq!(
                std::fs::read(&target).expect("migrated plugin"),
                expected,
                "{extension} did not receive the canonical binary"
            );
            let leftovers: Vec<_> = std::fs::read_dir(target.parent().expect("extension parent"))
                .expect("read extension directory")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name() != "plugin.exe")
                .collect();
            assert!(
                leftovers.is_empty(),
                "migration left artifacts for {extension}: {leftovers:?}"
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_failed_migration_restores_the_legacy_hwpx_only_install() {
    use std::os::windows::fs::OpenOptionsExt;

    const LEGACY_PLUGIN: &[u8] = b"legacy officecli-dump-reader-hwpx installation";
    const CONFLICTING_OWPML: &[u8] = b"pre-existing OWPML plugin";

    let repo = fake_windows_installer_repo();
    let installer = repo.path().join("scripts/install.ps1");
    let home = tempfile::tempdir().expect("temporary Windows home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let legacy = root.join("hwpx/plugin.exe");
    let locked_target = root.join("owpml/plugin.exe");
    for target in [&legacy, &locked_target] {
        std::fs::create_dir_all(target.parent().expect("target parent"))
            .expect("create target directory");
    }
    std::fs::write(&legacy, LEGACY_PLUGIN).expect("write legacy HWPX plugin");
    std::fs::write(&locked_target, CONFLICTING_OWPML).expect("write OWPML plugin");

    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked_target)
        .expect("lock later migration target");
    let output = run_windows_installer_at(&installer, home.path(), "-NoBuild");
    drop(lock);

    assert!(!output.status.success(), "locked migration must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to commit owpml plugin"),
        "migration did not reach the injected failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(&legacy).expect("restored legacy HWPX plugin"),
        LEGACY_PLUGIN
    );
    assert_eq!(
        std::fs::read(&locked_target).expect("unchanged OWPML plugin"),
        CONFLICTING_OWPML
    );
    for extension in ["hwp", "hml"] {
        assert!(
            !root.join(extension).join("plugin.exe").exists(),
            "failed migration left a new {extension} target"
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
    for extension in EXTENSIONS {
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

    let binary = release.join(CANONICAL_BINARY);
    std::fs::write(
        &binary,
        b"#!/bin/sh\nprintf '%s\\n' '{\"name\":\"officecli-hancom-hwp\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\".hwpx\",\".owpml\",\".hml\",\".hwp\"],\"target\":\"docx\"}'\n",
    )
    .expect("write fake plugin");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("make fake plugin executable");
    repo
}

#[cfg(unix)]
#[test]
fn unix_print_env_registers_every_extension() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = run_unix_installer(home.path(), "--print-env");
    assert!(
        output.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("UTF-8 installer output");
    for extension in EXTENSIONS {
        let variable = format!(
            "OFFICECLI_PLUGIN_DUMP_READER_{}=",
            extension.to_ascii_uppercase()
        );
        assert!(
            stdout.contains(&variable),
            "missing {extension} override: {stdout}"
        );
        assert!(
            stdout.contains(CANONICAL_BINARY),
            "override must point at the canonical binary: {stdout}"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_installer_rejects_relative_home_for_every_action() {
    let repo = fake_installer_repo();
    let installer = repo.path().join("scripts/install.sh");

    for argument in ["--uninstall", "--print-env", "--no-build"] {
        let working = tempfile::tempdir().expect("temporary Unix working directory");
        let relative_home = std::path::Path::new("relative-home");
        let targets: Vec<_> = EXTENSIONS
            .iter()
            .map(|extension| {
                working
                    .path()
                    .join(relative_home)
                    .join(format!(".officecli/plugins/dump-reader/{extension}/plugin"))
            })
            .collect();
        for (extension, target) in EXTENSIONS.iter().zip(&targets) {
            std::fs::create_dir_all(target.parent().expect("extension parent"))
                .expect("create extension dir");
            std::fs::write(target, format!("known old {extension} plugin"))
                .expect("write old plugin");
        }

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
        for (extension, target) in EXTENSIONS.iter().zip(&targets) {
            assert_eq!(
                std::fs::read_to_string(target).expect("extension target remains"),
                format!("known old {extension} plugin")
            );
        }
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
fn unix_uninstall_removes_every_extension() {
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let targets: Vec<_> = EXTENSIONS
        .iter()
        .map(|extension| root.join(extension).join("plugin"))
        .collect();
    for (extension, target) in EXTENSIONS.iter().zip(&targets) {
        std::fs::create_dir_all(target.parent().expect("extension parent"))
            .expect("create extension dir");
        std::fs::write(target, format!("old {extension} plugin")).expect("write plugin");
    }
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
    for (extension, target) in EXTENSIONS.iter().zip(&targets) {
        assert!(!target.exists(), "{extension} plugin must be removed");
    }
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

    for extension in EXTENSIONS {
        let home = tempfile::tempdir().expect("temporary home");
        let outside = tempfile::tempdir().expect("external directory");
        let outside_plugin = outside.path().join("plugin");
        std::fs::write(&outside_plugin, b"must remain outside plugin root")
            .expect("write external plugin");

        let root = home.path().join(".officecli/plugins/dump-reader");
        std::fs::create_dir_all(&root).expect("create plugin root");
        symlink(outside.path(), root.join(extension)).expect("link extension outside plugin root");

        let output = run_unix_installer(home.path(), "--uninstall");
        assert!(
            !output.status.success(),
            "uninstall must fail closed for a symlinked {extension} directory"
        );
        assert!(
            outside_plugin.exists(),
            "uninstall must not delete through the {extension} directory symlink"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_uninstall_rejects_symlinks_in_every_existing_managed_component() {
    use std::os::unix::fs::symlink;

    let managed_components = [
        ".officecli",
        ".officecli/plugins",
        ".officecli/plugins/dump-reader",
        ".officecli/plugins/dump-reader/hwp",
        ".officecli/plugins/dump-reader/hwpx",
        ".officecli/plugins/dump-reader/owpml",
        ".officecli/plugins/dump-reader/hml",
    ];

    for relative_link in managed_components {
        let home = tempfile::tempdir().expect("temporary Unix home");
        let outside = tempfile::tempdir().expect("external Unix directory");
        let link = home.path().join(relative_link);
        std::fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
        symlink(outside.path(), &link).expect("create managed-path symlink");

        let root = home.path().join(".officecli/plugins/dump-reader");
        let physical = |logical: &std::path::Path| {
            logical.strip_prefix(&link).map_or_else(
                |_| logical.to_path_buf(),
                |suffix| outside.path().join(suffix),
            )
        };
        let physical_targets: Vec<_> = EXTENSIONS
            .iter()
            .map(|extension| {
                let logical = root.join(extension).join("plugin");
                (
                    *extension,
                    physical(&logical),
                    format!("external {extension} plugin must remain"),
                )
            })
            .collect();
        for (_, path, contents) in &physical_targets {
            std::fs::create_dir_all(path.parent().expect("plugin parent"))
                .expect("create plugin parent");
            std::fs::write(path, contents).expect("write protected plugin");
            std::fs::write(path.parent().expect("plugin parent").join("keep"), b"keep")
                .expect("write directory sentinel");
        }

        let output = run_unix_installer(home.path(), "--uninstall");
        let contents_after: Vec<_> = physical_targets
            .iter()
            .map(|(_, path, _)| std::fs::read_to_string(path).ok())
            .collect();
        std::fs::remove_file(&link).expect("remove test symlink without following it");

        assert!(
            !output.status.success(),
            "uninstall must reject symlink component {relative_link}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for ((extension, _, expected), actual) in physical_targets.iter().zip(&contents_after) {
            assert_eq!(
                actual.as_deref(),
                Some(expected.as_str()),
                "{extension} target changed through symlink component {relative_link}"
            );
        }
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
        ".officecli/plugins/dump-reader/owpml",
        ".officecli/plugins/dump-reader/hml",
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
fn unix_uninstall_preflights_every_target_before_removing_any() {
    for invalid_extension in EXTENSIONS {
        let home = tempfile::tempdir().expect("temporary Unix home");
        let root = home.path().join(".officecli/plugins/dump-reader");
        let targets: Vec<_> = EXTENSIONS
            .iter()
            .map(|extension| (*extension, root.join(extension).join("plugin")))
            .collect();
        for (extension, target) in &targets {
            std::fs::create_dir_all(target.parent().expect("extension parent"))
                .expect("create extension dir");
            if *extension == invalid_extension {
                std::fs::create_dir(target).expect("create invalid target directory");
                std::fs::write(target.join("keep"), b"keep").expect("write directory sentinel");
            } else {
                std::fs::write(target, format!("known old {extension} peer plugin"))
                    .expect("write valid peer plugin");
            }
        }

        let output = run_unix_installer(home.path(), "--uninstall");

        assert!(
            !output.status.success(),
            "{invalid_extension} directory target must fail uninstall preflight"
        );
        for (extension, target) in &targets {
            if *extension == invalid_extension {
                assert!(
                    target.join("keep").exists(),
                    "invalid target must remain untouched"
                );
            } else {
                assert_eq!(
                    std::fs::read_to_string(target).expect("valid peer target remains"),
                    format!("known old {extension} peer plugin")
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn unix_install_preflights_every_target_before_staging_or_backup() {
    for invalid_extension in EXTENSIONS {
        let repo = fake_installer_repo();
        let home = tempfile::tempdir().expect("temporary Unix home");
        let root = home.path().join(".officecli/plugins/dump-reader");
        let targets: Vec<_> = EXTENSIONS
            .iter()
            .map(|extension| (*extension, root.join(extension).join("plugin")))
            .collect();
        for (extension, target) in &targets {
            std::fs::create_dir_all(target.parent().expect("extension parent"))
                .expect("create extension dir");
            if *extension == invalid_extension {
                std::fs::create_dir(target).expect("create invalid target directory");
                std::fs::write(target.join("keep"), b"keep").expect("write directory sentinel");
            } else {
                std::fs::write(target, format!("known old {extension} peer plugin"))
                    .expect("write valid peer plugin");
            }
        }

        let output = run_unix_installer_at(
            &repo.path().join("scripts/install.sh"),
            home.path(),
            "--no-build",
        );

        assert!(
            !output.status.success(),
            "{invalid_extension} directory target must fail install preflight"
        );
        for (extension, target) in &targets {
            if *extension == invalid_extension {
                assert!(
                    target.join("keep").exists(),
                    "invalid target must remain untouched"
                );
            } else {
                assert_eq!(
                    std::fs::read_to_string(target).expect("valid peer target remains"),
                    format!("known old {extension} peer plugin")
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn unix_install_places_one_executable_and_relative_links_for_other_extensions() {
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

    for extension in ["hwp", "owpml", "hml"] {
        let link = root.join(extension).join("plugin");
        let metadata = std::fs::symlink_metadata(&link).expect("installed discovery link");
        assert!(
            metadata.file_type().is_symlink(),
            "{extension} target must be a symlink"
        );
        assert_eq!(
            std::fs::read_link(&link).expect("discovery link target"),
            std::path::Path::new("../hwpx/plugin")
        );
        assert_eq!(
            std::fs::read(&link).expect("read through discovery link"),
            std::fs::read(&hwpx).expect("read HWPX plugin")
        );
    }

    let repeated = run_unix_installer_at(&installer, home.path(), "--no-build");
    assert!(
        repeated.status.success(),
        "reinstall failed: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    for extension in ["hwp", "owpml", "hml"] {
        assert_eq!(
            std::fs::read_link(root.join(extension).join("plugin"))
                .expect("reinstalled discovery link target"),
            std::path::Path::new("../hwpx/plugin")
        );
    }
    for extension in EXTENSIONS {
        let directory = root.join(extension);
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
fn unix_install_migrates_a_legacy_hwpx_only_layout_idempotently() {
    use std::os::unix::fs::PermissionsExt;

    const LEGACY_PLUGIN: &[u8] = b"legacy officecli-dump-reader-hwpx installation";

    let repo = fake_installer_repo();
    let installer = repo.path().join("scripts/install.sh");
    let source = repo.path().join("target/release").join(CANONICAL_BINARY);
    let expected = std::fs::read(&source).expect("canonical source binary");
    let home = tempfile::tempdir().expect("temporary Unix home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let legacy = root.join("hwpx/plugin");
    std::fs::create_dir_all(legacy.parent().expect("legacy HWPX parent"))
        .expect("create legacy HWPX directory");
    std::fs::write(&legacy, LEGACY_PLUGIN).expect("write legacy HWPX plugin");

    for attempt in 0..2 {
        let output = run_unix_installer_at(&installer, home.path(), "--no-build");
        assert!(
            output.status.success(),
            "migration attempt {attempt} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let canonical = root.join("hwpx/plugin");
        let metadata = std::fs::metadata(&canonical).expect("migrated canonical plugin");
        assert_ne!(
            metadata.permissions().mode() & 0o111,
            0,
            "canonical plugin must remain executable"
        );
        assert_eq!(
            std::fs::read(&canonical).expect("canonical plugin"),
            expected
        );
        for extension in ["hwp", "owpml", "hml"] {
            let target = root.join(extension).join("plugin");
            assert_eq!(
                std::fs::read_link(&target).expect("migrated discovery link"),
                std::path::Path::new("../hwpx/plugin")
            );
            assert_eq!(
                std::fs::read(&target).expect("read through migrated link"),
                expected,
                "{extension} did not resolve to the canonical binary"
            );
        }
        for extension in EXTENSIONS {
            let directory = root.join(extension);
            let leftovers: Vec<_> = std::fs::read_dir(&directory)
                .expect("read extension directory")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name() != "plugin")
                .collect();
            assert!(
                leftovers.is_empty(),
                "migration left artifacts for {extension}: {leftovers:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn unix_failed_migration_restores_the_legacy_hwpx_only_install() {
    use std::os::unix::fs::PermissionsExt;

    const LEGACY_PLUGIN: &[u8] = b"legacy officecli-dump-reader-hwpx installation";

    let repo = fake_installer_repo();
    let home = tempfile::tempdir().expect("temporary Unix home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let legacy = root.join("hwpx/plugin");
    let owpml = root.join("owpml/plugin");
    std::fs::create_dir_all(legacy.parent().expect("legacy HWPX parent"))
        .expect("create legacy HWPX directory");
    std::fs::write(&legacy, LEGACY_PLUGIN).expect("write legacy HWPX plugin");

    let wrapper_dir = repo.path().join("migration-test-bin");
    std::fs::create_dir(&wrapper_dir).expect("create wrapper dir");
    let mv_wrapper = wrapper_dir.join("mv");
    std::fs::write(
        &mv_wrapper,
        b"#!/bin/sh\ndest=\nfor arg do dest=$arg; done\ncase \"$1\" in */.plugin-link.*) [ \"$dest\" = \"$FAIL_COMMIT_DEST\" ] && exit 1 ;; esac\nexec /bin/mv \"$@\"\n",
    )
    .expect("write mv wrapper");
    std::fs::set_permissions(&mv_wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("make mv wrapper executable");
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(wrapper_dir).chain(std::env::split_paths(&current_path)),
    )
    .expect("compose test PATH");

    let output = std::process::Command::new(repo.path().join("scripts/install.sh"))
        .arg("--no-build")
        .env("HOME", home.path())
        .env("PATH", test_path)
        .env("FAIL_COMMIT_DEST", &owpml)
        .output()
        .expect("run failing legacy migration");

    assert!(!output.status.success(), "injected migration must fail");
    assert_eq!(
        std::fs::read(&legacy).expect("restored legacy HWPX plugin"),
        LEGACY_PLUGIN
    );
    for extension in ["hwp", "owpml", "hml"] {
        let target = root.join(extension).join("plugin");
        assert!(
            std::fs::symlink_metadata(&target).is_err(),
            "failed migration left a new {extension} target"
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
    for extension in EXTENSIONS {
        let directory = root.join(extension);
        let leftovers: Vec<_> = std::fs::read_dir(&directory)
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

    let repo = fake_installer_repo();
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join(".officecli/plugins/dump-reader");
    let hwpx = root.join("hwpx/plugin");
    let owpml = root.join("owpml/plugin");
    let targets: Vec<_> = EXTENSIONS
        .iter()
        .map(|extension| {
            (
                *extension,
                root.join(extension).join("plugin"),
                format!("known old {extension} plugin"),
            )
        })
        .collect();
    for (_, target, contents) in &targets {
        std::fs::create_dir_all(target.parent().expect("extension parent"))
            .expect("create extension dir");
        std::fs::write(target, contents).expect("write old plugin");
    }

    let wrapper_dir = repo.path().join("test-bin");
    std::fs::create_dir(&wrapper_dir).expect("create wrapper dir");
    let mv_wrapper = wrapper_dir.join("mv");
    std::fs::write(
        &mv_wrapper,
        b"#!/bin/sh\ndest=\nfor arg do dest=$arg; done\ncase \"$1\" in */.plugin-link.*) [ \"$dest\" = \"$FAIL_COMMIT_DEST\" ] && exit 1 ;; esac\nexec /bin/mv \"$@\"\n",
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
        .env("FAIL_COMMIT_DEST", &owpml)
        .env("FAIL_RM_TARGET", &hwpx)
        .output()
        .expect("run installer with failing commit and rollback removal");

    assert!(
        !output.status.success(),
        "injected OWPML commit failure must fail installation"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("could not remove committed HWPX target during rollback"),
        "rollback must diagnose the failed removal: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (extension, target, expected) in &targets {
        assert_eq!(
            std::fs::read_to_string(target).expect("restored target"),
            expected.as_str(),
            "{extension} target was not restored"
        );
    }
}
