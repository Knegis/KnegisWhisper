//! Windows-only uninstall coordinator. No renderer, network, or database startup.
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Security::{Authorization::*, Credentials::*, *},
        Storage::FileSystem::*,
        System::{
            Com::CoTaskMemFree, Diagnostics::ToolHelp::*, Pipes::*, Registry::*, Threading::*,
        },
        UI::{Controls::*, Shell::*, WindowsAndMessaging::*},
    },
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const APP: &str = "KnegisWhisper";
const BUNDLE: &str = "com.knegis.knegiswhisper";
const HELPER: &str = "knegiswhisper-uninstall.exe";
fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
fn fail<T>(s: impl Into<String>) -> Result<T> {
    Err(std::io::Error::other(s.into()).into())
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
fn folder(id: &windows::core::GUID) -> Result<PathBuf> {
    unsafe {
        let value = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
        let result = value.to_string();
        CoTaskMemFree(Some(value.0.cast()));
        Ok(PathBuf::from(result?))
    }
}
fn current_sid() -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let token = Handle(token);
        let mut needed = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
        let result = text.to_string();
        let _ = LocalFree(HLOCAL(text.0.cast()));
        Ok(result?)
    }
}
pub fn show_error(message: &str) {
    let message = wide(format!("Uninstall did not complete. Some requested data may already have been removed. The application has been kept where possible.\n\n{message}\n\nClose the listed programs or correct the problem, then retry. Deletion cannot be undone."));
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            windows::core::w!("KnegisWhisper uninstall"),
            MB_OK | MB_ICONERROR,
        );
    }
}
fn option(instruction: &str, description: &str, checkbox: &str) -> Result<Option<bool>> {
    let instruction = wide(instruction);
    let description = wide(description);
    let checkbox = wide(checkbox);
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
        dwCommonButtons: TDCBF_OK_BUTTON | TDCBF_CANCEL_BUTTON,
        pszWindowTitle: windows::core::w!("Uninstall KnegisWhisper"),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(description.as_ptr()),
        pszVerificationText: PCWSTR(checkbox.as_ptr()),
        ..Default::default()
    };
    let mut button = 0;
    let mut selected = BOOL(0);
    unsafe {
        TaskDialogIndirect(&config, Some(&mut button), None, Some(&mut selected))?;
    }
    Ok((button == IDOK.0).then_some(selected.as_bool()))
}
fn confirm(personal: bool, shared: bool) -> bool {
    let text = wide(format!("Remove KnegisWhisper?\n\nYour notes, recordings, settings, downloads and credentials: {}\nShared downloads and temporary audio for all users: {}\n\nSelected data deletion is permanent. Exported copies stay where you saved them. Windows will ask for administrator access before anything is deleted.", if personal { "DELETE" } else { "KEEP" }, if shared { "DELETE" } else { "KEEP" }));
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            windows::core::w!("Confirm uninstall"),
            MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDOK
    }
}
fn retry<T>(interactive: bool, mut operation: impl FnMut() -> Result<T>) -> Result<T> {
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                if !interactive {
                    return Err(error);
                }
                let text = wide(format!("{error}\n\nClose any listed programs or correct the problem, then retry. Cancel stops uninstall; data already deleted cannot be restored."));
                if unsafe {
                    MessageBoxW(
                        None,
                        PCWSTR(text.as_ptr()),
                        windows::core::w!("Uninstall needs attention"),
                        MB_RETRYCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
                    )
                } != IDRETRY
                {
                    return Err(error);
                }
            }
        }
    }
}
fn elevated(process: HANDLE) -> Result<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token)?;
        let token = Handle(token);
        let mut info = TOKEN_ELEVATION::default();
        let mut needed = 0;
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&mut info as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of_val(&info) as u32,
            &mut needed,
        )?;
        Ok(info.TokenIsElevated != 0)
    }
}
fn authenticate_process(pid: u32) -> Result<Handle> {
    unsafe {
        let process = Handle(OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )?);
        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )?;
        buffer.truncate(size as usize);
        let path = PathBuf::from(String::from_utf16(&buffer)?).canonicalize()?;
        if path != std::env::current_exe()?.canonicalize()? {
            return fail("Unexpected uninstall peer executable");
        }
        Ok(process)
    }
}
/// Pin ancestors against rename/write and reject every reparse point. Deletion
/// uses the opened object, never a second path lookup after validation.
fn open_pinned(path: &Path, deleting: bool) -> Result<File> {
    let file = OpenOptions::new()
        // Attribute-only opens do not enforce the sharing exclusion on NTFS.
        // FILE_GENERIC_READ makes the no-write/no-delete share mode effective.
        .access_mode(FILE_GENERIC_READ.0 | if deleting { DELETE.0 } else { 0 })
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return fail(format!("Refusing reparse point: {}", path.display()));
    }
    Ok(file)
}
fn erase_node(path: &Path) -> Result<()> {
    let file = match open_pinned(path, true) {
        Ok(f) => f,
        Err(e) => match std::fs::symlink_metadata(path) {
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            _ => return Err(e),
        },
    };
    if file.metadata()?.is_dir() {
        for item in std::fs::read_dir(path)? {
            erase_node(&item?.path())?;
        }
    }
    let disposition = FILE_DISPOSITION_INFO {
        DeleteFile: true.into(),
    };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of_val(&disposition) as u32,
        )?;
    }
    Ok(())
}
fn erase_beneath(parent: &Path, leaf: &str) -> Result<()> {
    if !parent.is_absolute()
        || leaf.is_empty()
        || leaf.contains(['/', '\\', ':'])
        || matches!(leaf, "." | "..")
    {
        return fail("Invalid cleanup boundary");
    }
    let mut ancestors = Vec::new();
    let mut cumulative = PathBuf::new();
    for component in parent.components() {
        cumulative.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        if matches!(component, std::path::Component::ParentDir) {
            return fail("Invalid cleanup ancestor");
        }
        match std::fs::symlink_metadata(&cumulative) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        ancestors.push(open_pinned(&cumulative, false)?);
    }
    erase_node(&parent.join(leaf))
        .map_err(|e| format!("Could not remove {}: {e}", parent.join(leaf).display()).into())
}
fn belongs_to_app(target: &str, username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 128
        && username
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && target == format!("{username}.{BUNDLE}")
}
fn erase_credentials() -> Result<()> {
    erase_matching_credentials(belongs_to_app)
}
fn erase_matching_credentials(matches: impl Fn(&str, &str) -> bool) -> Result<()> {
    unsafe {
        let mut count = 0;
        let mut entries = std::ptr::null_mut();
        if let Err(e) = CredEnumerateW(
            PCWSTR::null(),
            CRED_ENUMERATE_FLAGS(0),
            &mut count,
            &mut entries,
        ) {
            if e.code() == ERROR_NOT_FOUND.to_hresult() {
                return Ok(());
            }
            return Err(e.into());
        }
        struct Entries(*mut *mut CREDENTIALW);
        impl Drop for Entries {
            fn drop(&mut self) {
                unsafe {
                    CredFree(self.0.cast());
                }
            }
        }
        let _guard = Entries(entries);
        // Windows only supports prefix filters. Inspect target/user metadata;
        // never dereference any credential blob or log credential metadata.
        for entry in std::slice::from_raw_parts(entries, count as usize) {
            let entry = &**entry;
            if entry.Type == CRED_TYPE_GENERIC
                && !entry.TargetName.is_null()
                && !entry.UserName.is_null()
                && matches(&entry.TargetName.to_string()?, &entry.UserName.to_string()?)
            {
                CredDeleteW(PCWSTR(entry.TargetName.0), CRED_TYPE_GENERIC, 0)?;
            }
        }
    }
    Ok(())
}
fn erase_personal() -> Result<()> {
    let roaming = folder(&FOLDERID_RoamingAppData)?;
    let local = folder(&FOLDERID_LocalAppData)?;
    let home = folder(&FOLDERID_Profile)?;
    erase_beneath(&roaming, APP)?;
    erase_beneath(&roaming, BUNDLE)?;
    erase_beneath(&local, BUNDLE)?;
    erase_beneath(&home.join(".knegiswhisper"), "local-api.json")?;
    erase_credentials()?;
    // Only the initiating account's exact app values. Never delete either
    // registry key, which contains unrelated applications' startup settings.
    for key in [
        windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        windows::core::w!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run"
        ),
    ] {
        let result = unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key, windows::core::w!("KnegisWhisper"))
        };
        if result != ERROR_FILE_NOT_FOUND && result != ERROR_PATH_NOT_FOUND {
            result.ok()?;
        }
    }
    Ok(())
}
fn erase_shared() -> Result<()> {
    erase_beneath(&folder(&FOLDERID_ProgramData)?.join(APP), "cache")
}
fn require_idle() -> Result<()> {
    unsafe {
        let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?);
        let mut item = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        Process32FirstW(snapshot.0, &mut item)?;
        loop {
            let end = item
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(item.szExeFile.len());
            let name =
                String::from_utf16_lossy(item.szExeFile.get(..end).ok_or("Invalid process name")?)
                    .to_lowercase();
            if name == "knegiswhisper-desktop.exe"
                || name == "whisper-server.exe"
                || name == "llama-server.exe"
                || name.starts_with("sherpa-onnx")
                || name == "ffmpeg.exe"
            {
                return fail(format!(
                    "Close {name} (process {}) before uninstalling. No process was terminated.",
                    item.th32ProcessID
                ));
            }
            if let Err(e) = Process32NextW(snapshot.0, &mut item) {
                if e.code() == ERROR_NO_MORE_FILES.to_hresult() {
                    break;
                }
                return Err(e.into());
            }
        }
    }
    Ok(())
}
fn installation() -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.file_name().and_then(|s| s.to_str()) != Some(HELPER) {
        return fail("Unrecognized uninstall helper");
    }
    let dir = exe
        .parent()
        .ok_or("No installation directory")?
        .to_path_buf();
    // Bind the elevated action to the machine's registered installation. No
    // caller-supplied profile, installation path or executable is accepted.
    let mut location = vec![0u16; 32768];
    let mut bytes = (location.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            windows::core::w!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\KnegisWhisper"
            ),
            windows::core::w!("InstallLocation"),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(location.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()?;
    }
    let text = String::from_utf16(
        &location
            .into_iter()
            .take_while(|c| *c != 0)
            .collect::<Vec<_>>(),
    )?;
    let registered = PathBuf::from(text.trim_matches('"'));
    if !registered.is_absolute() || registered.canonicalize()? != dir {
        return fail("Helper does not match the registered installation");
    }
    if !dir.join("uninstall.exe").is_file() {
        return fail("Run the helper from an installed KnegisWhisper directory");
    }
    // NSIS and shortcut comparisons use the normal DOS path stored at install
    // time, not the extended \\?\ spelling returned by Rust canonicalize().
    Ok(registered)
}
fn elevate(params: &str) -> Result<Handle> {
    let exe = wide(std::env::current_exe()?);
    let params = wide(params);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: windows::core::w!("runas"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info)?;
    }
    if info.hProcess.is_invalid() {
        return fail("Elevation returned no process handle");
    }
    Ok(Handle(info.hProcess))
}
fn pipe_server() -> Result<(File, String)> {
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random).map_err(|e| std::io::Error::other(e.to_string()))?;
    let name = format!(
        "\\\\.\\pipe\\KnegisWhisper-uninstall-{}",
        random
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>()
    );
    let sddl = wide(format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{})",
        current_sid()?
    ));
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let path = wide(&name);
        let handle = CreateNamedPipeW(
            PCWSTR(path.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            Some(&attributes),
        );
        let _ = LocalFree(HLOCAL(descriptor.0));
        if handle == INVALID_HANDLE_VALUE {
            return Err(windows::core::Error::from_win32().into());
        }
        Ok((File::from_raw_handle(handle.0), name))
    }
}
fn accept(pipe: &File, child: &Handle) -> Result<()> {
    let start = Instant::now();
    let handle = HANDLE(pipe.as_raw_handle());
    loop {
        unsafe {
            match ConnectNamedPipe(handle, None) {
                Ok(()) => break,
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => break,
                Err(e)
                    if e.code() == ERROR_PIPE_LISTENING.to_hresult()
                        || e.code() == ERROR_NO_DATA.to_hresult() => {}
                Err(e) => return Err(e.into()),
            }
            if WaitForSingleObject(child.0, 0) == WAIT_OBJECT_0 {
                return fail("Elevated helper exited before connecting");
            }
        }
        if start.elapsed() > Duration::from_secs(60) {
            return fail("Timed out waiting for elevated helper");
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    unsafe {
        let mut pid = 0;
        GetNamedPipeClientProcessId(handle, &mut pid)?;
        if pid != GetProcessId(child.0) {
            return fail("Unexpected cleanup process");
        }
        let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
        SetNamedPipeHandleState(handle, Some(&mode), None, None)?;
    }
    Ok(())
}
fn run_nsis(dir: &Path, quiet: bool) -> Result<bool> {
    require_idle()?;
    let status = Command::new(dir.join("uninstall.exe"))
        .arg(if quiet { "/S" } else { "/P" })
        .raw_arg(format!("_?={}", dir.display()))
        .status()?;
    if status.code() != Some(0) && status.code() != Some(3010) {
        return fail(format!("Application removal failed with {status}"));
    }
    if dir.join("knegiswhisper-desktop.exe").exists() {
        return fail("The application executable remains after uninstall");
    }
    Ok(status.code() == Some(3010))
}
fn broker(pipe_name: &str, parent: u32, quiet: bool) -> Result<()> {
    if pipe_name
        .strip_prefix("\\\\.\\pipe\\KnegisWhisper-uninstall-")
        .is_none_or(|s| s.len() != 32 || !s.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return fail("Invalid uninstall session");
    }
    let dir = installation()?;
    if !unsafe { elevated(GetCurrentProcess())? } {
        return fail("Cleanup broker requires administrator access");
    }
    let _parent = authenticate_process(parent)?;
    if !quiet && elevated(_parent.0)? {
        return fail("Personal cleanup requires an unelevated initiating process");
    }
    // Never let a rogue pipe server impersonate this elevated client, including
    // the interval before its process identity has been checked.
    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0)
        .open(pipe_name)?;
    unsafe {
        let mut pid = 0;
        GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut pid)?;
        if pid != parent {
            return fail("Unexpected initiating process");
        }
    }
    let result = (|| -> Result<bool> {
        retry(!quiet, require_idle)?;
        pipe.write_all(b"R")?;
        let mut choice = [0];
        pipe.read_exact(&mut choice)?;
        match choice[0] {
            b'S' if !quiet => retry(true, erase_shared)?,
            b'K' => {}
            _ => return fail("Canceled uninstall session"),
        }
        run_nsis(&dir, quiet)
    })();
    match result {
        Ok(reboot) => {
            pipe.write_all(if reboot { b"B" } else { b"D" })?;
        }
        Err(e) => {
            pipe.write_all(format!("E{e}").as_bytes())?;
        }
    }
    Ok(())
}
pub fn run() -> Result<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--broker") {
        let [_, name, parent, mode] = args.as_slice() else {
            return fail("Invalid broker arguments");
        };
        if !matches!(mode.as_str(), "quiet" | "interactive") {
            return fail("Invalid broker arguments");
        }
        return broker(name, parent.parse()?, mode == "quiet").map(|()| 0);
    }
    if args.iter().any(|a| a != "--quiet") {
        return fail("Unsupported uninstall option; unattended data deletion is not supported");
    }
    let quiet = !args.is_empty();
    installation()?;
    // Never guess which profile an already-elevated launch intended. In
    // particular, an alternate administrator's profile must not be selected.
    if !quiet && unsafe { elevated(GetCurrentProcess())? } {
        return fail("Open the uninstall helper without Run as administrator, from the Windows account whose data you want to remove. An already-elevated launch cannot safely identify the initiating account.");
    }
    retry(!quiet, require_idle)?;
    let (personal, shared) = if quiet {
        (false, false)
    } else {
        let Some(personal) = option("Your personal data", "By default your data stays for a later reinstall. Select the option only to permanently remove this Windows account's notes, recordings, settings, downloads and saved credentials.", "Delete my KnegisWhisper data")? else { return Ok(0); };
        let Some(shared) = option("Shared downloads and temporary audio", "The ProgramData cache may be used by other Windows accounts. Deleting it affects those users too. It has no per-user ownership records.", "Delete shared downloads and temporary audio for all users")? else { return Ok(0); };
        if !confirm(personal, shared) {
            return Ok(0);
        }
        (personal, shared)
    };
    let (mut pipe, name) = pipe_server()?;
    let child = elevate(&format!(
        "--broker {name} {} {}",
        std::process::id(),
        if quiet { "quiet" } else { "interactive" }
    ))?;
    accept(&pipe, &child)?;
    let mut ready = [0];
    pipe.read_exact(&mut ready)?;
    if ready != *b"R" {
        return fail("Elevated cleanup preflight failed; no personal data was removed");
    }
    if personal {
        retry(true, erase_personal)?;
    }
    pipe.write_all(if shared { b"S" } else { b"K" })?;
    let mut result = String::new();
    pipe.take(8192).read_to_string(&mut result)?;
    if result != "D" && result != "B" {
        return fail(format!(
            "Cleanup did not complete: {}",
            result.trim_start_matches('E')
        ));
    }
    if !quiet {
        let text = if result == "B" {
            windows::core::w!("Uninstalled. Restart Windows to remove the uninstall helper and remaining installer files.")
        } else {
            windows::core::w!(
                "KnegisWhisper was uninstalled. Data you chose to keep remains on this computer."
            )
        };
        unsafe {
            MessageBoxW(None, text, windows::core::w!("KnegisWhisper"), MB_OK);
        }
    }
    Ok(if result == "B" { 3010 } else { 0 })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    #[test]
    fn credential_scope_is_exact() {
        assert!(belongs_to_app("openai.com.knegis.knegiswhisper", "openai"));
        assert!(belongs_to_app(
            "retired-slot.com.knegis.knegiswhisper",
            "retired-slot"
        ));
        for (target, user) in [
            ("openai.com.knegis.knegiswhisper.other", "openai"),
            ("x.com.knegis.knegiswhisper", "openai"),
            (".com.knegis.knegiswhisper", ""),
            ("OpenAI.com.knegis.knegiswhisper", "OpenAI"),
        ] {
            assert!(!belongs_to_app(target, user));
        }
    }
    #[test]
    fn deletion_is_bounded_and_repeatable() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("owned")).unwrap();
        std::fs::write(temp.path().join("owned/file"), b"fiction").unwrap();
        std::fs::write(temp.path().join("sibling"), b"keep").unwrap();
        erase_beneath(temp.path(), "owned").unwrap();
        erase_beneath(temp.path(), "owned").unwrap();
        assert_eq!(std::fs::read(temp.path().join("sibling")).unwrap(), b"keep");
        for invalid in ["", "..", "../sibling", "a/b", "C:", "a\\b"] {
            assert!(erase_beneath(temp.path(), invalid).is_err());
        }
    }
    #[test]
    fn locked_file_is_an_error_then_retry_succeeds() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("locked");
        std::fs::write(&path, b"keep").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(erase_beneath(temp.path(), "locked").is_err());
        assert!(path.exists());
        drop(lock);
        erase_beneath(temp.path(), "locked").unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn non_ascii_tree_and_missing_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let owned = temp.path().join("åäö日本");
        std::fs::create_dir_all(owned.join("nested")).unwrap();
        std::fs::write(owned.join("nested/fixture"), b"fiction").unwrap();
        erase_beneath(temp.path(), "åäö日本").unwrap();
        assert!(!owned.exists());
        erase_beneath(&temp.path().join("absent/parent"), "child").unwrap();
        assert!(erase_beneath(Path::new("relative"), "child").is_err());
    }
    #[test]
    fn junctions_at_leaf_and_ancestor_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("sentinel"), b"keep").unwrap();
        let link = temp.path().join("redirect");
        // Creating a junction is allowed without Developer Mode or elevation.
        let output = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:KW_JUNCTION -Target $env:KW_TARGET -ErrorAction Stop | Out-Null"])
            .env("KW_JUNCTION", &link).env("KW_TARGET", outside.path()).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(erase_beneath(temp.path(), "redirect").is_err());
        assert!(erase_beneath(&link, "sentinel").is_err());
        assert_eq!(
            std::fs::read(outside.path().join("sentinel")).unwrap(),
            b"keep"
        );
        // The recursive traversal must reject it too, even after earlier
        // entries have been removed; a dangling junction is still a refusal.
        let owned = temp.path().join("owned");
        std::fs::create_dir(&owned).unwrap();
        let nested = owned.join("redirect");
        std::fs::rename(&link, &nested).unwrap();
        assert!(erase_beneath(temp.path(), "owned").is_err());
        assert_eq!(
            std::fs::read(outside.path().join("sentinel")).unwrap(),
            b"keep"
        );
        std::fs::rename(&nested, &link).unwrap();
        outside.close().unwrap();
        assert!(erase_beneath(temp.path(), "redirect").is_err());
        std::fs::remove_dir(&link).unwrap();
    }
    #[test]
    fn readonly_file_reports_failure_without_false_success() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("readonly");
        std::fs::write(&path, b"fixture").unwrap();
        let name = wide(&path);
        unsafe {
            SetFileAttributesW(PCWSTR(name.as_ptr()), FILE_ATTRIBUTE_READONLY).unwrap();
        }
        let failed = erase_beneath(temp.path(), "readonly").is_err();
        let retained = path.exists();
        unsafe {
            SetFileAttributesW(PCWSTR(name.as_ptr()), FILE_ATTRIBUTE_NORMAL).unwrap();
        }
        assert!(failed && retained);
        erase_beneath(temp.path(), "readonly").unwrap();
    }
    #[test]
    fn ancestor_handle_prevents_retargeting() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned");
        std::fs::create_dir(&path).unwrap();
        let pinned = open_pinned(&path, false).unwrap();
        assert!(std::fs::rename(&path, temp.path().join("moved")).is_err());
        drop(pinned);
        std::fs::rename(&path, temp.path().join("moved")).unwrap();
    }
    #[test]
    fn pipe_client_fixture() {
        let Ok(name) = std::env::var("KW_TEST_UNINSTALL_PIPE") else {
            return;
        };
        let parent: u32 = std::env::var("KW_TEST_UNINSTALL_PARENT")
            .unwrap()
            .parse()
            .unwrap();
        let _peer = authenticate_process(parent).unwrap();
        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
            .unwrap();
        let mut byte = [0];
        pipe.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"T");
        pipe.write_all(b"ok").unwrap();
    }
    #[test]
    fn pipe_authenticates_expected_child_and_transfers_only_fixture_data() {
        let (mut pipe, name) = pipe_server().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "windows::tests::pipe_client_fixture"])
            .env("KW_TEST_UNINSTALL_PIPE", name)
            .env("KW_TEST_UNINSTALL_PARENT", std::process::id().to_string())
            .spawn()
            .unwrap();
        let handle = unsafe {
            Handle(
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    child.id(),
                )
                .unwrap(),
            )
        };
        accept(&pipe, &handle).unwrap();
        pipe.write_all(b"T").unwrap();
        let mut reply = [0; 2];
        pipe.read_exact(&mut reply).unwrap();
        assert_eq!(reply, *b"ok");
        assert!(child.wait().unwrap().success());
    }
    #[test]
    fn peer_with_different_executable_is_rejected() {
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 20"])
            .spawn()
            .unwrap();
        assert!(authenticate_process(child.id()).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    #[ignore = "Requires an interactive Windows credential session; uses only uniquely named fake credentials"]
    fn native_credentials_remove_only_exact_app_targets() {
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random).unwrap();
        let prefix = format!(
            "uninstall-test-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        struct Fixtures(Vec<Vec<u16>>);
        impl Drop for Fixtures {
            fn drop(&mut self) {
                for target in &self.0 {
                    unsafe {
                        let _ = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0);
                    }
                }
            }
        }
        let mut fixtures = Fixtures(Vec::new());
        let names = [
            format!("{prefix}-active"),
            format!("{prefix}-obsolete"),
            format!("{prefix}-unrelated"),
        ];
        for (index, name) in names.iter().enumerate() {
            let target = wide(format!(
                "{name}.{BUNDLE}{}",
                if index == 2 { ".other" } else { "" }
            ));
            let user = wide(name);
            let mut blob = b"fictional-uninstall-test".to_vec();
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(target.as_ptr().cast_mut()),
                UserName: PWSTR(user.as_ptr().cast_mut()),
                Persist: CRED_PERSIST_SESSION,
                CredentialBlobSize: blob.len() as u32,
                CredentialBlob: blob.as_mut_ptr(),
                ..Default::default()
            };
            fixtures.0.push(target.clone());
            unsafe {
                CredWriteW(&credential, 0).unwrap();
            }
        }
        // The additional random prefix confines this probe even if a developer
        // has real app credentials. Production uses only belongs_to_app.
        erase_matching_credentials(|target, user| {
            user.starts_with(&prefix) && belongs_to_app(target, user)
        })
        .unwrap();
        for (index, target) in fixtures.0.iter().enumerate() {
            let mut entry = std::ptr::null_mut();
            let result =
                unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0, &mut entry) };
            if result.is_ok() {
                unsafe {
                    CredFree(entry.cast());
                }
            }
            if index == 2 {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.unwrap_err().code(), ERROR_NOT_FOUND.to_hresult());
            }
        }
    }
}
