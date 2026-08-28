//! Optional binary HWP → HWPX converter boundary.
//!
//! RHWP is deliberately kept out-of-process. The child receives three separate
//! OS-native arguments (no shell), writes only inside a private temporary
//! directory, and is killed after a fixed total budget.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::{mem::size_of, os::windows::io::AsRawHandle};
#[cfg(windows)]
use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
#[cfg(windows)]
use std::{os::windows::io::FromRawHandle, os::windows::io::OwnedHandle};

#[cfg(not(windows))]
use tempfile::{Builder, TempDir};
#[cfg(windows)]
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
#[cfg(windows)]
use windows_sys::Wdk::Storage::FileSystem::{
    NtCreateFile, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT,
};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    LocalFree, RtlNtStatusToDosError, ERROR_PROCESS_ABORTED, HANDLE, INVALID_HANDLE_VALUE,
    OBJ_CASE_INSENSITIVE, STATUS_OBJECT_NAME_COLLISION, UNICODE_STRING,
};
#[cfg(windows)]
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
#[cfg(windows)]
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
#[cfg(windows)]
use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
    OPEN_EXISTING, SYNCHRONIZE,
};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(windows)]
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use crate::error::{PluginError, Result};
use crate::format::{self, SourceFormat};

const CONVERTER_ENV: &str = "OFFICECLI_HWPX_CONVERTER";
const CONVERTER_TIMEOUT: Duration = Duration::from_secs(120);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(windows)]
const PROCESS_TREE_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const STDERR_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_BINARY_HWP_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_CONVERTER_STDERR_BYTES: usize = 8 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;

/// Keeps the private directory alive until the converted document is parsed.
pub(crate) struct ConvertedHwpx {
    _scratch: ConverterScratch,
    path: PathBuf,
}

#[cfg(not(windows))]
#[derive(Debug)]
struct ConverterScratch(TempDir);

#[cfg(not(windows))]
impl ConverterScratch {
    fn path(&self) -> &Path {
        self.0.path()
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct ConverterScratch {
    path: PathBuf,
    directory: Option<OwnedHandle>,
    root: Option<OwnedHandle>,
}

#[cfg(windows)]
impl ConverterScratch {
    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(windows)]
impl Drop for ConverterScratch {
    fn drop(&mut self) {
        // Release the no-FILE_SHARE_DELETE handle before removing our own
        // directory. Until this point it prevents replacement by the parent.
        drop(self.directory.take());
        let started = Instant::now();
        loop {
            match std::fs::remove_dir_all(&self.path) {
                Ok(()) => break,
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(_) if started.elapsed() < PROCESS_TREE_DRAIN_TIMEOUT => {
                    thread::sleep(CHILD_POLL_INTERVAL);
                }
                Err(_) => break,
            }
        }
        drop(self.root.take());
    }
}

impl ConvertedHwpx {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// Converts a binary HWP when an explicit or installed RHWP executable exists.
///
/// `Ok(None)` means the optional runtime dependency is unavailable. Callers
/// preserve the protocol's actionable exit-3 behavior for that case.
pub(crate) fn convert_hwp_to_hwpx(
    source: &Path,
    media_dir: Option<&Path>,
) -> Result<Option<ConvertedHwpx>> {
    let Some(converter) = find_converter()? else {
        return Ok(None);
    };

    let scratch = create_converter_scratch(media_dir)?;
    let staged_source = scratch.path().join("source.hwp");
    let output = scratch.path().join("converted.hwpx");
    stage_source_with_limit(source, &staged_source, MAX_BINARY_HWP_SOURCE_BYTES)?;

    run_converter(&converter, &staged_source, &output, CONVERTER_TIMEOUT)?;
    validate_output(&output)?;

    Ok(Some(ConvertedHwpx {
        _scratch: scratch,
        path: output,
    }))
}

fn stage_source_with_limit(source: &Path, staged: &Path, limit: u64) -> Result<()> {
    let reader = std::fs::File::open(source).map_err(|error| {
        PluginError::corrupt(format!("cannot open the HWP source for staging: {error}"))
    })?;
    let metadata = reader.metadata().map_err(|error| {
        PluginError::corrupt(format!(
            "cannot inspect the HWP source for staging: {error}"
        ))
    })?;
    if !metadata.is_file() {
        return Err(PluginError::corrupt(
            "HWP conversion requires a regular source file",
        ));
    }
    if metadata.len() > limit {
        return Err(PluginError::corrupt(format!(
            "HWP source exceeds the {limit}-byte conversion copy budget"
        )));
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut writer = options.open(staged).map_err(|error| {
        PluginError::corrupt(format!(
            "cannot create the staged HWP source in the private conversion directory: {error}"
        ))
    })?;
    let read_limit = limit.saturating_add(1);
    let copied = io::copy(&mut reader.take(read_limit), &mut writer).map_err(|error| {
        PluginError::corrupt(format!(
            "cannot copy the HWP source into the private conversion directory: {error}"
        ))
    })?;
    if copied > limit {
        return Err(PluginError::corrupt(format!(
            "HWP source exceeds the {limit}-byte conversion copy budget"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_converter_scratch(media_dir: Option<&Path>) -> Result<ConverterScratch> {
    if let Some(directory) = media_dir {
        let mut builder = Builder::new();
        builder.prefix("officecli-hwpx-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let scratch = builder.tempdir_in(directory).map_err(|error| {
            PluginError::unsupported_feature(format!(
                "cannot create a private HWP conversion directory: {error}"
            ))
        })?;
        if scratch.path().to_str().is_some() {
            return Ok(ConverterScratch(scratch));
        }
        // RHWP v0.8.4 collects argv with `std::env::args()`, which rejects
        // non-UTF-8 paths on Unix. Fall back to a converter-safe system temp
        // root instead of exposing the original or media path to that limit.
        drop(scratch);
    }

    let mut builder = Builder::new();
    builder.prefix("officecli-hwpx-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let scratch = builder.tempdir().map_err(|error| {
        PluginError::unsupported_feature(format!(
            "cannot create a private HWP conversion directory: {error}"
        ))
    })?;
    if scratch.path().to_str().is_none() {
        return Err(PluginError::unsupported_feature(
            "HWP conversion requires a UTF-8 temporary directory for RHWP v0.8.4",
        ));
    }
    Ok(ConverterScratch(scratch))
}

#[cfg(windows)]
fn create_converter_scratch(_media_dir: Option<&Path>) -> Result<ConverterScratch> {
    // A caller-supplied Windows media root may be shared or contain mutable
    // junctions. Use the canonical OS user-temp root and an atomic protected
    // child instead; the protocol says the plugin *may* use --media-dir.
    let scratch = create_private_windows_scratch(&std::env::temp_dir())?;
    if scratch.path().to_str().is_none() {
        return Err(PluginError::unsupported_feature(
            "HWP conversion requires a Unicode temporary directory for RHWP v0.8.4",
        ));
    }
    Ok(scratch)
}

#[cfg(windows)]
fn create_private_windows_scratch(root: &Path) -> Result<ConverterScratch> {
    const ATTEMPTS: usize = 32;
    let root = root.canonicalize().map_err(|error| {
        PluginError::unsupported_feature(format!(
            "cannot canonicalize the Windows HWP conversion scratch root: {error}"
        ))
    })?;
    if !root.is_dir() {
        return Err(PluginError::unsupported_feature(
            "Windows HWP conversion scratch root is not a directory",
        ));
    }
    // Protected DACL: only the object owner and LocalSystem receive full
    // control, inherited by staged input and converter output. RHWP runs with
    // the same user token and therefore retains access.
    let sddl: Vec<u16> = OsStr::new("D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)\0")
        .encode_wide()
        .collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    };
    if converted == 0 {
        return Err(PluginError::unsupported_feature(format!(
            "cannot create the private Windows HWP conversion DACL: {}",
            io::Error::last_os_error()
        )));
    }
    let descriptor = LocalSecurityDescriptor(descriptor);

    let mut wide_root: Vec<u16> = root.as_os_str().encode_wide().collect();
    wide_root.push(0);
    let root_raw = unsafe {
        CreateFileW(
            wide_root.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if root_raw == INVALID_HANDLE_VALUE {
        return Err(PluginError::unsupported_feature(format!(
            "cannot open the HWP conversion scratch root securely: {}",
            io::Error::last_os_error()
        )));
    }
    let root_handle = unsafe { OwnedHandle::from_raw_handle(root_raw.cast()) };
    let mut root_tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let root_inspected = unsafe {
        GetFileInformationByHandleEx(
            root_raw,
            FileAttributeTagInfo,
            (&raw mut root_tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>())
                .expect("Windows attribute tag info fits in u32"),
        )
    };
    if root_inspected == 0 || root_tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        let detail = if root_inspected == 0 {
            io::Error::last_os_error().to_string()
        } else {
            "scratch root is a reparse point".to_owned()
        };
        return Err(PluginError::unsupported_feature(format!(
            "cannot verify the HWP conversion scratch root: {detail}"
        )));
    }

    for _ in 0..ATTEMPTS {
        let mut random = [0_u8; 16];
        let status = unsafe {
            BCryptGenRandom(
                null_mut(),
                random.as_mut_ptr(),
                u32::try_from(random.len()).expect("random buffer length fits in u32"),
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            let code = unsafe { RtlNtStatusToDosError(status) };
            return Err(PluginError::unsupported_feature(format!(
                "cannot generate a private HWP conversion directory name: {}",
                io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
            )));
        }

        let mut name = String::with_capacity("officecli-hwpx-".len() + random.len() * 2);
        name.push_str("officecli-hwpx-");
        for byte in random {
            use std::fmt::Write;
            write!(&mut name, "{byte:02x}").expect("writing to String cannot fail");
        }
        let mut wide_name: Vec<u16> = OsStr::new(&name).encode_wide().collect();
        let name_bytes = wide_name
            .len()
            .checked_mul(size_of::<u16>())
            .and_then(|length| u16::try_from(length).ok())
            .expect("fixed scratch directory name fits in UNICODE_STRING");
        let unicode_name = UNICODE_STRING {
            Length: name_bytes,
            MaximumLength: name_bytes,
            Buffer: wide_name.as_mut_ptr(),
        };
        let object_attributes = OBJECT_ATTRIBUTES {
            Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>())
                .expect("Windows object attributes fit in u32"),
            RootDirectory: root_raw,
            ObjectName: &unicode_name,
            Attributes: OBJ_CASE_INSENSITIVE,
            SecurityDescriptor: descriptor.0.cast(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut io_status = IO_STATUS_BLOCK::default();
        let mut raw: HANDLE = null_mut();
        let status = unsafe {
            NtCreateFile(
                &mut raw,
                FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                &object_attributes,
                &mut io_status,
                std::ptr::null(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_CREATE,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                std::ptr::null(),
                0,
            )
        };
        if status == 0 {
            let directory = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
            return Ok(ConverterScratch {
                path: root.join(name),
                directory: Some(directory),
                root: Some(root_handle),
            });
        }
        if status == STATUS_OBJECT_NAME_COLLISION {
            continue;
        }
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(PluginError::unsupported_feature(format!(
            "cannot atomically create and lock the private HWP conversion directory: {}",
            io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
        )));
    }

    Err(PluginError::unsupported_feature(
        "cannot allocate a unique private HWP conversion directory after 32 attempts",
    ))
}

#[cfg(windows)]
struct LocalSecurityDescriptor(PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn find_converter() -> Result<Option<PathBuf>> {
    find_converter_from(
        std::env::var_os(CONVERTER_ENV),
        std::env::var_os("PATH"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    )
}

fn find_converter_from(
    configured: Option<OsString>,
    path_value: Option<OsString>,
    home: Option<OsString>,
) -> Result<Option<PathBuf>> {
    if let Some(configured) = configured.filter(|value| !value.is_empty()) {
        let path = PathBuf::from(configured);
        if !path.is_absolute() {
            return Err(PluginError::unsupported_feature(format!(
                "{CONVERTER_ENV} must be an absolute path"
            )));
        }
        if path.to_str().is_none() {
            return Err(PluginError::unsupported_feature(format!(
                "{CONVERTER_ENV} must be a Unicode path because RHWP v0.8.4 collects argv as strings"
            )));
        }
        if !is_executable_file(&path) {
            return Err(PluginError::unsupported_feature(format!(
                "{CONVERTER_ENV} does not name an executable file"
            )));
        }
        return Ok(Some(path));
    }

    if let Some(path_value) = path_value {
        for directory in std::env::split_paths(&path_value) {
            // An empty or relative PATH entry searches the current directory.
            // Do not turn an untrusted working directory into code execution.
            if !directory.is_absolute() {
                continue;
            }
            for name in converter_names() {
                let candidate = directory.join(name);
                if is_executable_file(&candidate) {
                    return Ok(Some(candidate));
                }
            }
        }
    }

    if let Some(home) = home {
        let home = PathBuf::from(home);
        if home.is_absolute() {
            for name in converter_names() {
                let candidate = home.join(".local").join("rhwp").join(name);
                if is_executable_file(&candidate) {
                    return Ok(Some(candidate));
                }
            }
        }
    }

    Ok(None)
}

#[cfg(windows)]
fn converter_names() -> &'static [&'static str] {
    &["rhwp.exe", "rhwp"]
}

#[cfg(not(windows))]
fn converter_names() -> &'static [&'static str] {
    &["rhwp"]
}

fn is_executable_file(path: &Path) -> bool {
    // RHWP v0.8.4 calls `std::env::args()`, which includes argv[0]. Do not
    // select an executable path that would make RHWP panic before conversion.
    if path.to_str().is_none() {
        return false;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn run_converter(converter: &Path, source: &Path, output: &Path, timeout: Duration) -> Result<()> {
    prepare_converter_waiting()?;
    let mut command = Command::new(converter);
    command
        .arg(OsStr::new("export-hwpx"))
        .arg(source.as_os_str())
        .arg(output.as_os_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    #[cfg(windows)]
    let converter_job = WindowsJob::new().map_err(|error| {
        PluginError::unsupported_feature(format!(
            "cannot create a secure Windows Job Object for the HWP converter: {error}"
        ))
    })?;

    let mut child = command.spawn().map_err(|error| {
        PluginError::unsupported_feature(format!(
            "cannot start the configured HWP converter: {error}"
        ))
    })?;
    #[cfg(windows)]
    if let Err(error) = converter_job.assign(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(PluginError::unsupported_feature(format!(
            "cannot contain the HWP converter in a Windows Job Object: {error}"
        )));
    }

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PluginError::internal("converter stderr pipe was not created"))?;
    let stderr_capture = StderrCapture::start(stderr);
    let converter_process_group = child.id();

    let wait_outcome = match wait_for_child(&mut child, timeout) {
        Ok(true) => ChildWaitOutcome::Exited,
        Ok(false) => ChildWaitOutcome::TimedOut,
        Err(error) => ChildWaitOutcome::Failed(error),
    };

    // Keep the direct Unix child unreaped while its process group is killed.
    // Its PID/PGID cannot be reused in this interval. On Windows, terminate the
    // whole Job and wait for active-process count zero before scratch cleanup.
    terminate_converter(
        &mut child,
        #[cfg(windows)]
        &converter_job,
    );
    #[cfg(windows)]
    let tree_cleanup = converter_job
        .wait_empty(PROCESS_TREE_DRAIN_TIMEOUT)
        .map_err(|error| {
            PluginError::corrupt(format!("cannot observe HWP converter Job cleanup: {error}"))
        })
        .and_then(|empty| {
            if empty {
                Ok(())
            } else {
                Err(PluginError::corrupt(
                    "HWP converter process tree did not terminate within 2 seconds",
                ))
            }
        });

    let captured_result = stderr_capture.finish(converter_process_group);
    let status_result = child.wait();
    #[cfg(windows)]
    drop(converter_job);
    let captured = captured_result?;
    let status = status_result
        .map_err(|error| PluginError::corrupt(format!("cannot reap the HWP converter: {error}")))?;
    #[cfg(windows)]
    tree_cleanup?;

    match wait_outcome {
        ChildWaitOutcome::Exited => {}
        ChildWaitOutcome::TimedOut => {
            return Err(PluginError::corrupt(format!(
                "HWP converter timed out after {} seconds{}",
                timeout.as_secs_f64(),
                diagnostic_suffix(&captured)
            )));
        }
        ChildWaitOutcome::Failed(error) => {
            return Err(PluginError::corrupt(format!(
                "cannot wait for the HWP converter: {error}{}",
                diagnostic_suffix(&captured)
            )));
        }
    }

    if !status.success() {
        let status = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| status.to_string());
        return Err(PluginError::corrupt(format!(
            "HWP converter exited with status {status}{}",
            diagnostic_suffix(&captured)
        )));
    }

    Ok(())
}

enum ChildWaitOutcome {
    Exited,
    TimedOut,
    Failed(io::Error),
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<bool> {
    let started = Instant::now();
    loop {
        if child_has_exited_without_reaping(child)? {
            return Ok(true);
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(false);
        }
        thread::sleep(remaining.min(CHILD_POLL_INTERVAL));
    }
}

#[cfg(unix)]
fn child_has_exited_without_reaping(child: &mut Child) -> io::Result<bool> {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.si_pid() } != 0)
}

#[cfg(not(unix))]
fn child_has_exited_without_reaping(child: &mut Child) -> io::Result<bool> {
    Ok(child.try_wait()?.is_some())
}

#[cfg(unix)]
fn prepare_converter_waiting() -> Result<()> {
    // POSIX preserves SIG_IGN and the blocked signal mask across exec. The
    // plugin does not rely on SIGCHLD delivery (it polls waitpid via
    // Child::try_wait), but SIG_IGN may auto-reap children before their status
    // can be observed. Reset only this standalone plugin process to SIG_DFL;
    // a blocked mask is safe for the polling implementation.
    let mut existing = unsafe { std::mem::zeroed::<libc::sigaction>() };
    let query = unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut existing) };
    if query != 0 {
        return Err(PluginError::unsupported_feature(format!(
            "cannot inspect the SIGCHLD boundary for the HWP converter: {}",
            io::Error::last_os_error()
        )));
    }
    if existing.sa_sigaction != libc::SIG_IGN && existing.sa_flags & libc::SA_NOCLDWAIT == 0 {
        return Ok(());
    }

    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = libc::SIG_DFL;
    action.sa_flags = 0;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    let result = unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) };
    if result == 0 {
        Ok(())
    } else {
        Err(PluginError::unsupported_feature(format!(
            "cannot establish a safe SIGCHLD boundary for the HWP converter: {}",
            io::Error::last_os_error()
        )))
    }
}

#[cfg(not(unix))]
fn prepare_converter_waiting() -> Result<()> {
    Ok(())
}

fn terminate_converter(child: &mut Child, #[cfg(windows)] converter_job: &WindowsJob) {
    #[cfg(windows)]
    let _ = converter_job.terminate();
    terminate_converter_process_group(child.id());
    let _ = child.kill();
}

#[cfg(unix)]
fn terminate_converter_process_group(process_group: u32) {
    if let Ok(process_group) = i32::try_from(process_group) {
        // The command is placed in its own process group before spawn. Killing
        // the group prevents a helper that inherited stderr from outliving the
        // converter deadline and keeping the diagnostic pipe open forever.
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn terminate_converter_process_group(_process_group: u32) {}

#[cfg(windows)]
struct WindowsJob(OwnedHandle);

#[cfg(windows)]
impl WindowsJob {
    fn new() -> std::io::Result<Self> {
        // SAFETY: null arguments request default security attributes and an
        // unnamed Job Object. A non-null return is a new owned HANDLE.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let this = Self(unsafe { OwnedHandle::from_raw_handle(raw.cast()) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let length = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .expect("Windows Job Object limit structure fits in u32");
        // SAFETY: `this` owns a valid Job Object and `limits` is live and has
        // the exact size required by JobObjectExtendedLimitInformation.
        let ok = unsafe {
            SetInformationJobObject(
                this.raw(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                length,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(this)
    }

    fn raw(&self) -> HANDLE {
        self.0.as_raw_handle().cast()
    }

    fn assign(&self, child: &Child) -> std::io::Result<()> {
        // SAFETY: both borrowed handles remain valid for the duration of the
        // call and their ownership stays with `self` and `child`.
        let ok = unsafe { AssignProcessToJobObject(self.raw(), child.as_raw_handle().cast()) };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn terminate(&self) -> std::io::Result<()> {
        // SAFETY: `self` owns a valid Job Object handle.
        let ok = unsafe { TerminateJobObject(self.raw(), ERROR_PROCESS_ABORTED) };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn active_processes(&self) -> std::io::Result<u32> {
        use windows_sys::Win32::System::JobObjects::{
            JobObjectBasicAccountingInformation, QueryInformationJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        };

        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let ok = unsafe {
            QueryInformationJobObject(
                self.raw(),
                JobObjectBasicAccountingInformation,
                (&raw mut accounting).cast(),
                u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
                    .expect("Windows Job accounting structure fits in u32"),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(accounting.ActiveProcesses)
        }
    }

    fn wait_empty(&self, timeout: Duration) -> std::io::Result<bool> {
        let started = Instant::now();
        loop {
            if self.active_processes()? == 0 {
                return Ok(true);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(false);
            }
            thread::sleep(remaining.min(CHILD_POLL_INTERVAL));
        }
    }
}

#[derive(Clone)]
struct CapturedStderr {
    tail: Vec<u8>,
    truncated: bool,
}

impl CapturedStderr {
    fn empty() -> Self {
        Self {
            tail: Vec::with_capacity(MAX_CONVERTER_STDERR_BYTES),
            truncated: false,
        }
    }

    fn append(&mut self, bytes: &[u8], limit: usize) {
        if bytes.len() >= limit {
            self.tail.clear();
            self.tail.extend_from_slice(&bytes[bytes.len() - limit..]);
            self.truncated = true;
            return;
        }
        if self.tail.len() + bytes.len() > limit {
            let remove = self.tail.len() + bytes.len() - limit;
            self.tail.drain(..remove);
            self.truncated = true;
        }
        self.tail.extend_from_slice(bytes);
    }
}

struct StderrCaptureState {
    captured: CapturedStderr,
    error: Option<String>,
    finished: bool,
}

struct StderrCapture {
    worker: Option<thread::JoinHandle<()>>,
    shared: Arc<(Mutex<StderrCaptureState>, Condvar)>,
}

impl StderrCapture {
    fn start(mut stderr: ChildStderr) -> Self {
        let shared = Arc::new((
            Mutex::new(StderrCaptureState {
                captured: CapturedStderr::empty(),
                error: None,
                finished: false,
            }),
            Condvar::new(),
        ));
        let worker_state = Arc::clone(&shared);
        let worker = thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        let (lock, _) = &*worker_state;
                        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                        state
                            .captured
                            .append(&buffer[..read], MAX_CONVERTER_STDERR_BYTES);
                    }
                    Err(error) => {
                        let (lock, condition) = &*worker_state;
                        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                        state.error = Some(error.to_string());
                        state.finished = true;
                        condition.notify_all();
                        return;
                    }
                }
            }

            let (lock, condition) = &*worker_state;
            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
            state.finished = true;
            condition.notify_all();
        });
        Self {
            worker: Some(worker),
            shared,
        }
    }

    fn wait_finished(&self, timeout: Duration) -> Result<bool> {
        let (lock, condition) = &*self.shared;
        let state = lock
            .lock()
            .map_err(|_| PluginError::internal("converter stderr state was poisoned"))?;
        let (state, _) = condition
            .wait_timeout_while(state, timeout, |state| !state.finished)
            .map_err(|_| PluginError::internal("converter stderr state was poisoned"))?;
        Ok(state.finished)
    }

    fn finish(mut self, converter_process_group: u32) -> Result<CapturedStderr> {
        if !self.wait_finished(STDERR_DRAIN_TIMEOUT)? {
            terminate_converter_process_group(converter_process_group);
        }
        let finished = self.wait_finished(STDERR_DRAIN_TIMEOUT)?;
        let (captured, error) = {
            let (lock, _) = &*self.shared;
            let state = lock
                .lock()
                .map_err(|_| PluginError::internal("converter stderr state was poisoned"))?;
            let mut captured = state.captured.clone();
            if !finished {
                captured.truncated = true;
            }
            (captured, state.error.clone())
        };

        if finished {
            self.worker
                .take()
                .expect("stderr worker exists")
                .join()
                .map_err(|_| PluginError::internal("converter stderr reader panicked"))?;
        }
        // Dropping an unfinished JoinHandle detaches it. The reader owns only a
        // bounded tail and can no longer delay the plugin process.
        if let Some(error) = error {
            return Err(PluginError::corrupt(format!(
                "cannot read converter stderr: {error}"
            )));
        }
        Ok(captured)
    }
}

#[cfg(test)]
fn capture_tail<R: Read>(mut reader: R, limit: usize) -> io::Result<CapturedStderr> {
    let mut captured = CapturedStderr::empty();
    let mut buffer = [0_u8; 4096];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        captured.append(&buffer[..read], limit);
    }

    Ok(captured)
}

fn diagnostic_suffix(captured: &CapturedStderr) -> String {
    if captured.tail.is_empty() {
        return String::new();
    }

    let raw = String::from_utf8_lossy(&captured.tail);
    let escaped = crate::escape_diagnostic_text(raw.trim());
    let escaped = bounded_utf8_tail(&escaped, MAX_DIAGNOSTIC_BYTES);
    let truncation = if captured.truncated {
        " (tail only)"
    } else {
        ""
    };
    format!("; converter stderr{truncation}: {escaped}")
}

fn bounded_utf8_tail(value: &str, limit: usize) -> &str {
    if value.len() <= limit {
        return value;
    }
    let mut start = value.len() - limit;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

fn validate_output(output: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(output).map_err(|error| {
        PluginError::corrupt(format!(
            "HWP converter succeeded but did not create its output: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(PluginError::corrupt(
            "HWP converter output is not a regular HWPX file",
        ));
    }

    let detected = format::detect_path(output).map_err(|error| {
        PluginError::corrupt(format!(
            "HWP converter output is not HWPX: {}",
            error.message
        ))
    })?;
    if !matches!(detected, SourceFormat::Hwpx) {
        return Err(PluginError::corrupt(format!(
            "HWP converter output is not HWPX (detected {})",
            detected.label()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stderr_capture_keeps_only_a_bounded_tail() {
        let input = vec![b'x'; MAX_CONVERTER_STDERR_BYTES * 3];
        let captured =
            capture_tail(Cursor::new(input), MAX_CONVERTER_STDERR_BYTES).expect("capture stderr");
        assert!(captured.truncated);
        assert_eq!(captured.tail.len(), MAX_CONVERTER_STDERR_BYTES);
        assert!(diagnostic_suffix(&captured).len() <= MAX_DIAGNOSTIC_BYTES + 64);
    }

    #[test]
    fn staging_rejects_a_source_that_exceeds_its_copy_budget() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("source.hwp");
        let staged = dir.path().join("staged.hwp");
        std::fs::write(&source, b"four").expect("source");

        let error = stage_source_with_limit(&source, &staged, 3)
            .expect_err("oversized source must not be staged");
        assert!(error.message.contains("copy budget"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn child_polling_does_not_depend_on_sigchld_delivery() {
        let mut signals = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        unsafe {
            libc::sigemptyset(&mut signals);
            libc::sigaddset(&mut signals, libc::SIGCHLD);
        }
        let block_result =
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut()) };
        assert_eq!(block_result, 0, "block SIGCHLD");

        let mut child = Command::new("/usr/bin/true").spawn().expect("spawn true");
        let result = wait_for_child(&mut child, Duration::from_secs(2));

        let restore_result =
            unsafe { libc::pthread_sigmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut()) };
        assert_eq!(restore_result, 0, "restore SIGCHLD mask");
        assert!(result.expect("poll child"), "child must complete");
        assert!(child.wait().expect("reap child").success(), "child status");
    }

    #[cfg(not(windows))]
    #[test]
    fn unusable_media_directory_is_an_unsupported_runtime_boundary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let not_a_directory = dir.path().join("media-file");
        std::fs::write(&not_a_directory, b"not a directory").expect("media-file");

        let error = create_converter_scratch(Some(&not_a_directory))
            .expect_err("an unusable media directory must fail closed");
        assert_eq!(error.code, crate::error::ErrorCode::UnsupportedFeature);
        assert_eq!(
            error.exit_code(),
            crate::error::ExitCode::UnsupportedFeature
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_ignores_an_untrusted_media_root_for_conversion_scratch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let not_a_directory = dir.path().join("media-file");
        std::fs::write(&not_a_directory, b"not a directory").expect("media-file");

        let scratch = create_converter_scratch(Some(&not_a_directory))
            .expect("Windows must use protected user-temp scratch instead");
        let expected_root = std::env::temp_dir()
            .canonicalize()
            .expect("canonical user temp root");
        assert!(scratch.path().starts_with(expected_root));
        assert!(!scratch.path().starts_with(&not_a_directory));
    }

    #[cfg(unix)]
    #[test]
    fn conversion_scratch_and_staged_source_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = create_converter_scratch(None).expect("scratch");
        let scratch_mode = std::fs::metadata(scratch.path())
            .expect("scratch metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(scratch_mode, 0o700, "scratch must not expose HWP data");

        let source = scratch.path().parent().unwrap().join("source-fixture.hwp");
        std::fs::write(&source, b"private source").expect("source");
        let staged = scratch.path().join("source.hwp");
        stage_source_with_limit(&source, &staged, 1024).expect("stage source");
        let staged_mode = std::fs::metadata(&staged)
            .expect("staged metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(staged_mode, 0o600, "staged source must be owner-only");
        std::fs::remove_file(source).expect("remove source fixture");
    }

    #[cfg(windows)]
    #[test]
    fn windows_conversion_scratch_has_a_protected_dacl() {
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            GetSecurityDescriptorControl, DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        };

        let scratch = create_converter_scratch(None).expect("scratch");
        let mut wide: Vec<u16> = scratch.path().as_os_str().encode_wide().collect();
        wide.push(0);
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let result = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(
            result,
            0,
            "read scratch DACL: {}",
            io::Error::from_raw_os_error(result as i32)
        );
        let _descriptor = LocalSecurityDescriptor(descriptor);
        let mut control = 0_u16;
        let mut revision = 0_u32;
        let inspected =
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
        assert_ne!(inspected, 0, "inspect scratch DACL control");
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "scratch DACL must not inherit access from a shared media directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn converter_timeout_kills_the_child() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let converter = dir.path().join("slow converter");
        std::fs::write(&converter, "#!/bin/sh\nexec /bin/sleep 5\n").expect("script");
        let mut permissions = std::fs::metadata(&converter)
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&converter, permissions).expect("chmod");

        let started = std::time::Instant::now();
        let error = run_converter(
            &converter,
            &dir.path().join("source.hwp"),
            &dir.path().join("output.hwpx"),
            Duration::from_millis(50),
        )
        .expect_err("converter must time out");
        assert!(error.message.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn converter_does_not_wait_for_a_detached_stderr_inheritor() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let converter = dir.path().join("converter with detached helper");
        std::fs::write(&converter, "#!/bin/sh\n/bin/sleep 5 >&2 &\nexit 0\n").expect("script");
        let mut permissions = std::fs::metadata(&converter)
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&converter, permissions).expect("chmod");

        let started = std::time::Instant::now();
        run_converter(
            &converter,
            &dir.path().join("source.hwp"),
            &dir.path().join("output.hwpx"),
            Duration::from_secs(2),
        )
        .expect("the direct converter exited successfully");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "an inherited stderr pipe must not extend the converter deadline: {elapsed:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_converter_does_not_leave_a_silent_descendant_alive() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let converter = dir.path().join("converter with silent helper");
        std::fs::write(
            &converter,
            "#!/bin/sh\n( exec 2>/dev/null; /bin/sleep 0.25; : > \"$3.marker\" ) &\nexit 0\n",
        )
        .expect("script");
        let mut permissions = std::fs::metadata(&converter)
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&converter, permissions).expect("chmod");
        let output = dir.path().join("output.hwpx");

        run_converter(
            &converter,
            &dir.path().join("source.hwp"),
            &output,
            Duration::from_secs(2),
        )
        .expect("the direct converter exited successfully");
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !dir.path().join("output.hwpx.marker").exists(),
            "a converter descendant survived the process boundary"
        );
    }

    #[cfg(windows)]
    #[test]
    fn converter_timeout_terminates_the_windows_process_tree() {
        use windows_sys::Win32::Foundation::{
            CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0, WAIT_TIMEOUT,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let converter = dir.path().join("tree converter.exe");
        let source_code = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("support")
            .join("fake_converter.rs");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let status = Command::new(rustc)
            .arg("--edition=2021")
            .arg(source_code)
            .arg("-o")
            .arg(&converter)
            .status()
            .expect("run rustc for process-tree helper");
        assert!(status.success(), "compile process-tree helper");

        let source = dir.path().join("tree-mode.hwp");
        let output = dir.path().join("output.hwpx");
        let started = std::time::Instant::now();
        let error = run_converter(&converter, &source, &output, Duration::from_millis(800))
            .expect_err("converter tree must time out");
        assert!(error.message.contains("timed out"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "stderr inheritance must not delay the timeout"
        );

        let descendant_pid: u32 = std::fs::read_to_string(output.with_extension("pid"))
            .expect("descendant pid")
            .trim()
            .parse()
            .expect("numeric descendant pid");
        // SAFETY: OpenProcess creates a borrowed process handle for a numeric
        // PID; if the process object is already gone, null is also success for
        // this assertion. A non-null handle is closed exactly once below.
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, descendant_pid) };
        if process.is_null() {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                i32::try_from(ERROR_INVALID_PARAMETER).ok(),
                "OpenProcess failed for an unexpected reason"
            );
        } else {
            let wait = unsafe { WaitForSingleObject(process, 2_000) };
            unsafe {
                CloseHandle(process);
            }
            assert_ne!(wait, WAIT_TIMEOUT, "converter descendant remained alive");
            assert_eq!(wait, WAIT_OBJECT_0, "unexpected process wait result");
        }
    }

    #[test]
    fn relative_path_entries_are_not_converter_search_roots() {
        let path = std::env::join_paths([Path::new("relative")]).expect("join PATH");
        assert!(find_converter_from(None, Some(path), None)
            .expect("lookup")
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn explicit_non_utf8_converter_path_is_rejected_as_unsupported() {
        use std::os::unix::ffi::OsStringExt;

        let configured = OsString::from_vec(b"/tmp/rhwp-\xff".to_vec());
        let error = find_converter_from(Some(configured), None, None)
            .expect_err("RHWP cannot collect a non-UTF-8 argv[0]");
        assert_eq!(error.code, crate::error::ErrorCode::UnsupportedFeature);
        assert!(error.message.contains("Unicode path"), "{error}");
    }
}
