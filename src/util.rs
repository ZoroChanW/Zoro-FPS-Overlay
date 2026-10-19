//! Small shared helpers.
use std::os::windows::ffi::OsStrExt;
use std::ffi::{c_void, OsStr};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::*;

pub(crate) fn stat_value(raw: u32) -> Option<u32> {
    if raw == STAT_NONE { None } else { Some(raw) }
}

pub(crate) fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(std::iter::once(0)).collect()
}

pub(crate) fn message_box(text: &str, caption: &str) {
    let t = wide(text);
    let c = wide(caption);
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x0000_0040); }
}

pub(crate) fn current_exe_folder() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub(crate) fn process_name_for_pid(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() { return None; }
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as DWORD;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len);
        let _ = CloseHandle(process);
        if ok == 0 || len == 0 { return None; }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        let leaf = path.rsplit(['\\', '/']).next()?.to_ascii_lowercase();
        Some(leaf)
    }
}

pub(crate) fn is_system_or_shell_process(name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        "explorer.exe", "dwm.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe",
        "searchhost.exe", "searchapp.exe", "textinputhost.exe", "lockapp.exe",
        "applicationframehost.exe", "systemsettings.exe", "taskmgr.exe",
        "gameoverlayui.exe", "steamwebhelper.exe", "radeonsoftware.exe",
        "amdow.exe", "zoro-fps-overlay.exe", "zoro-fps-overlay-debug.exe",
    ];
    EXCLUDED.iter().any(|x| name.eq_ignore_ascii_case(x))
}

pub(crate) fn log_event(path: &PathBuf, message: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{:?} | {}", std::time::SystemTime::now(), message);
    }
}

pub(crate) fn process_id_is_running(process_id: u32) -> bool {
    if process_id == 0 { return false; }
    unsafe {
        let process = OpenProcess(SYNCHRONIZE, 0, process_id);
        if process.is_null() { return false; }
        let wait_result = WaitForSingleObject(process, 0);
        let _ = CloseHandle(process);
        wait_result == WAIT_TIMEOUT_RESULT
    }
}

// Older builds force-killed PresentMon, which could leave its ETW session behind.
// Before truncating the previous run's diagnostic log, stop that stale session if
// its owning overlay process has already exited.

pub(crate) fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}


// True when the calling process's token belongs to the given built-in group
// (the Administrators group only counts while the process is elevated).
fn token_in_builtin_group(rid: DWORD) -> bool {
    unsafe {
        let nt_authority: [u8; 6] = [0, 0, 0, 0, 0, 5];
        let mut sid: *mut c_void = std::ptr::null_mut();
        // S-1-5-32-<rid>: BUILTIN\\<group>
        if AllocateAndInitializeSid(&nt_authority, 2, 32, rid, 0, 0, 0, 0, 0, 0, &mut sid) == 0 {
            return false;
        }
        let mut member: BOOL = 0;
        let ok = CheckTokenMembership(std::ptr::null_mut(), sid, &mut member);
        FreeSid(sid);
        ok != 0 && member != 0
    }
}

// Running as administrator (elevated).
pub(crate) fn is_elevated() -> bool {
    token_in_builtin_group(544)
}

// PresentMon can only start its ETW trace as an administrator or as a member of the
// "Performance Log Users" group.
pub(crate) fn has_trace_access() -> bool {
    is_elevated() || token_in_builtin_group(559)
}

pub(crate) fn find_overlay_window() -> HWND {
    unsafe { FindWindowW(wide(OVERLAY_CLASS).as_ptr(), std::ptr::null()) }
}

// Opens the settings window, or brings the one that is already open to the front.
pub(crate) fn open_settings_window() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't locate this application: {}", e))?;
    Command::new(exe)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map(|_| ()).map_err(|e| format!("Couldn't open the settings window: {}", e))
}

// Only one settings window at a time. Returns false (after focusing the open one) if another exists.
pub(crate) fn claim_settings_instance() -> bool {
    unsafe {
        let _ = CreateMutexW(std::ptr::null(), 0, wide("Local\\ZoroFPSOverlaySettings").as_ptr());
        if GetLastError() != ERROR_ALREADY_EXISTS { return true; }
        let window = FindWindowW(std::ptr::null(), wide(SETTINGS_TITLE).as_ptr());
        if !window.is_null() {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
        false
    }
}

// Starts the overlay again with administrator rights (Windows asks the user to confirm) and
// then closes the old, non-elevated one.
pub(crate) fn relaunch_overlay_as_admin() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't locate this application: {}", e))?;
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(), wide("runas").as_ptr(), wide(&exe.to_string_lossy()).as_ptr(),
            wide("--overlay --replace").as_ptr(), std::ptr::null(), SW_SHOWNORMAL,
        )
    };
    if (result as usize) <= 32 {
        return Err("The administrator prompt was cancelled or could not be shown.".to_string());
    }
    // The new overlay waits until the old one is gone.
    let old = find_overlay_window();
    if !old.is_null() {
        unsafe { PostMessageW(old, WM_QUIT_OVERLAY, 0, 0); }
    }
    Ok(())
}
