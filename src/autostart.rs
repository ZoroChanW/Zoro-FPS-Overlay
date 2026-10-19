//! "Start with Windows": a value in the current user's Run key. No administrator rights needed.
use crate::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "ZoroFPSOverlay";

fn open_run_key() -> Option<HANDLE> {
    let mut key: HANDLE = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, std::ptr::null(), 0,
            KEY_QUERY_VALUE | KEY_SET_VALUE, std::ptr::null(), &mut key, std::ptr::null_mut(),
        )
    };
    if result == 0 && !key.is_null() { Some(key) } else { None }
}

pub(crate) fn autostart_enabled() -> bool {
    let Some(key) = open_run_key() else { return false; };
    let mut size: DWORD = 0;
    let result = unsafe {
        RegQueryValueExW(key, wide(VALUE_NAME).as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
    };
    unsafe { RegCloseKey(key); }
    result == 0
}

pub(crate) fn set_autostart(enabled: bool) -> Result<(), String> {
    let key = open_run_key().ok_or_else(|| "Couldn't open the Windows startup list.".to_string())?;
    let name = wide(VALUE_NAME);
    let result = if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Couldn't locate this application: {}", e))?;
        let command = wide(&format!("\"{}\" --overlay", exe.display()));
        unsafe { RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, command.as_ptr() as *const u8, (command.len() * 2) as DWORD) }
    } else {
        // Deleting a value that is not there is fine.
        let _ = unsafe { RegDeleteValueW(key, name.as_ptr()) };
        0
    };
    unsafe { RegCloseKey(key); }
    if result == 0 { Ok(()) } else { Err("Windows refused to change the startup list.".to_string()) }
}
