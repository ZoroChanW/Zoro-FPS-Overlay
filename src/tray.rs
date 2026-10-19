//! The notification-area (system tray) icon and its menu.
use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use crate::*;

pub(crate) const WM_TRAY: u32 = WM_APP + 1;
pub(crate) const WM_QUIT_OVERLAY: u32 = WM_APP + 2;
const TRAY_ID: UINT = 1;

const CMD_SETTINGS: usize = 1;
const CMD_TOGGLE: usize = 2;
const CMD_AUTOSTART: usize = 3;
const CMD_ADMIN: usize = 4;
const CMD_QUIT: usize = 5;

// Windows sends this message to every window when Explorer restarts and the tray is rebuilt.
pub(crate) static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static ICON: OnceLock<usize> = OnceLock::new();

pub(crate) fn register_taskbar_message() {
    let id = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
    TASKBAR_CREATED.store(id, Ordering::Relaxed);
}

// A small dark rounded square with a light "Z", drawn pixel by pixel.
fn icon_pixels(size: i32) -> Vec<u32> {
    let radius = size as f32 * 0.22;
    let mut pixels = vec![0u32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded square.
            let cx = fx.clamp(radius, size as f32 - radius);
            let cy = fy.clamp(radius, size as f32 - radius);
            let inside = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() <= radius;
            if !inside { continue; }
            // The Z: top bar, bottom bar and a diagonal.
            let u = fx / size as f32;
            let v = fy / size as f32;
            let top = (0.26..0.38).contains(&v) && (0.26..0.74).contains(&u);
            let bottom = (0.62..0.74).contains(&v) && (0.26..0.74).contains(&u);
            let diagonal_x = 0.74 - (v - 0.34) / (0.66 - 0.34) * 0.48;
            let diagonal = (0.34..0.66).contains(&v) && (u - diagonal_x).abs() < 0.075;
            pixels[(y * size + x) as usize] = if top || bottom || diagonal { 0xFFE8_E4D9 } else { 0xFF18_1817 };
        }
    }
    pixels
}

fn tray_icon() -> HICON {
    *ICON.get_or_init(|| unsafe {
        let size = 32;
        let info = BITMAPINFO {
            header: BITMAPINFOHEADER {
                bi_size: size_of::<BITMAPINFOHEADER>() as u32, bi_width: size, bi_height: -size,
                bi_planes: 1, bi_bit_count: 32, bi_compression: 0, bi_size_image: 0,
                bi_x_pels: 0, bi_y_pels: 0, bi_clr_used: 0, bi_clr_important: 0,
            },
            colors: [0],
        };
        let screen = GetDC(std::ptr::null_mut());
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        ReleaseDC(std::ptr::null_mut(), screen);
        if color.is_null() || bits.is_null() { return 0; }
        let pixels = icon_pixels(size);
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
        let mask_bits = [0u8; 32 * 4];
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr() as *const std::ffi::c_void);
        let icon_info = ICONINFO { is_icon: 1, hot_x: 0, hot_y: 0, mask, color };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color);
        if !mask.is_null() { DeleteObject(mask); }
        icon as usize
    }) as HICON
}

fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
    data.size = size_of::<NOTIFYICONDATAW>() as DWORD;
    data.hwnd = hwnd;
    data.id = TRAY_ID;
    data.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.callback_message = WM_TRAY;
    data.icon = tray_icon();
    let tip = if is_elevated() { "Zoro FPS Overlay (administrator)" } else { "Zoro FPS Overlay" };
    for (slot, unit) in data.tip.iter_mut().zip(tip.encode_utf16().take(127)) { *slot = unit; }
    data
}

pub(crate) fn add_tray_icon(hwnd: HWND) {
    let mut data = notify_data(hwnd);
    unsafe { Shell_NotifyIconW(NIM_ADD, &mut data); }
}

pub(crate) fn remove_tray_icon(hwnd: HWND) {
    let mut data = notify_data(hwnd);
    unsafe { Shell_NotifyIconW(NIM_DELETE, &mut data); }
}

fn add_item(menu: HANDLE, id: usize, text: &str, checked: bool) {
    let flags = MF_STRING | if checked { MF_CHECKED } else { 0 };
    unsafe { AppendMenuW(menu, flags, id, wide(text).as_ptr()); }
}

// Shows the right-click menu at the cursor and carries out the chosen command.
pub(crate) fn show_tray_menu(hwnd: HWND) {
    let toggle_key = APP.get().and_then(|m| m.lock().ok()).map(|s| s.settings.hotkey_toggle.clone()).unwrap_or_default();
    let quit_key = APP.get().and_then(|m| m.lock().ok()).map(|s| s.settings.hotkey_quit.clone()).unwrap_or_default();
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() { return; }
        add_item(menu, CMD_SETTINGS, "Settings...", false);
        add_item(menu, CMD_TOGGLE, &format!("Show / hide overlay\t{}", toggle_key), false);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add_item(menu, CMD_AUTOSTART, "Start with Windows", autostart_enabled());
        if !is_elevated() { add_item(menu, CMD_ADMIN, "Restart as administrator", false); }
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add_item(menu, CMD_QUIT, &format!("Quit\t{}", quit_key), false);

        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        // The menu only closes when clicking elsewhere if our window is in the foreground.
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, cursor.x, cursor.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match command as usize {
            CMD_SETTINGS => { let _ = open_settings_window(); }
            CMD_TOGGLE => toggle_overlay_visibility(hwnd),
            CMD_AUTOSTART => { let _ = set_autostart(!autostart_enabled()); }
            CMD_ADMIN => { let _ = relaunch_overlay_as_admin(); }
            CMD_QUIT => { PostQuitMessage(0); }
            _ => {}
        }
    }
}
