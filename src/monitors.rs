//! Finding the monitors and choosing the one the overlay belongs on.
use std::mem::{size_of, zeroed};
use crate::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct MonitorInfo {
    pub(crate) rect: RECT,
    pub(crate) primary: bool,
}

unsafe extern "system" fn collect_monitor(monitor: HANDLE, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
    let list = &mut *(data as *mut Vec<MonitorInfo>);
    if let Some(info) = monitor_info(monitor) { list.push(info); }
    1
}

fn monitor_info(monitor: HANDLE) -> Option<MonitorInfo> {
    unsafe {
        let mut info: MONITORINFO = zeroed();
        info.size = size_of::<MONITORINFO>() as DWORD;
        if GetMonitorInfoW(monitor, &mut info) == 0 { return None; }
        Some(MonitorInfo { rect: info.monitor, primary: info.flags & MONITORINFOF_PRIMARY != 0 })
    }
}

// Every connected monitor, left to right and then top to bottom. The numbers shown in the
// settings window ("Monitor 2") are positions in this list, so they match what you see.
pub(crate) fn list_monitors() -> Vec<MonitorInfo> {
    let mut list: Vec<MonitorInfo> = Vec::new();
    unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), collect_monitor, &mut list as *mut Vec<MonitorInfo> as LPARAM);
    }
    sort_monitors(&mut list);
    list
}

pub(crate) fn sort_monitors(list: &mut [MonitorInfo]) {
    list.sort_by_key(|m| (m.rect.left, m.rect.top));
}

fn fallback_rect() -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(640),
        bottom: unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(480),
    }
}

pub(crate) fn primary_monitor_rect() -> RECT {
    list_monitors().iter().find(|m| m.primary).map(|m| m.rect).unwrap_or_else(fallback_rect)
}

// The monitor that holds most of the given window.
pub(crate) fn monitor_rect_of_window(hwnd: HWND) -> Option<RECT> {
    if hwnd.is_null() { return None; }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() { return None; }
    monitor_info(monitor).map(|m| m.rect)
}

// Which monitor the overlay should be on. "Follow the game" (0) uses the monitor of the
// window in front and keeps the last one while nothing suitable is in front, so the overlay
// does not jump around.
pub(crate) fn pick_screen(monitor_setting: u8, front_window: Option<HWND>, last: RECT) -> RECT {
    if monitor_setting == 0 {
        return front_window.and_then(monitor_rect_of_window).unwrap_or(last);
    }
    let monitors = list_monitors();
    monitors.get(monitor_setting as usize - 1)
        .or_else(|| monitors.iter().find(|m| m.primary))
        .map(|m| m.rect)
        .unwrap_or_else(fallback_rect)
}
