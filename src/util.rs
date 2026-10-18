//! Small shared helpers.
use std::os::windows::ffi::OsStrExt;
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;
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

