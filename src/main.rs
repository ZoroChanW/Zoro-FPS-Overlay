#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case, dead_code)]

#[cfg(not(windows))]
compile_error!("Zoro FPS Overlay currently supports Windows only.");

use std::collections::VecDeque;
use std::ffi::{c_void, OsStr};
use std::fs::{self, OpenOptions};
use std::mem::{size_of, zeroed};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use eframe::egui;

// Win32 constants
const WS_POPUP: u32 = 0x8000_0000;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const LWA_COLORKEY: u32 = 0x0000_0001;
const WM_PAINT: u32 = 0x000F;
const WM_DESTROY: u32 = 0x0002;
const WM_TIMER: u32 = 0x0113;
const WM_HOTKEY: u32 = 0x0312;
const WM_NCHITTEST: u32 = 0x0084;
const HTTRANSPARENT: isize = -1;
const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;
const SWP_NOACTIVATE: u32 = 0x0010;
const MOD_NOREPEAT: u32 = 0x4000;
const VK_F8: u32 = 0x77;
const VK_F10: u32 = 0x79;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const WAIT_TIMEOUT_RESULT: u32 = 258;
const COLORKEY_BLACK: u32 = 0x0000_0000;
const TIMER_ID: usize = 1;
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;

// DrawText flags
const DT_LEFT: u32 = 0x0000;
const DT_CENTER: u32 = 0x0001;
const DT_RIGHT: u32 = 0x0002;
const DT_SINGLELINE: u32 = 0x0020;
const DT_NOCLIP: u32 = 0x0100;
const DT_NOPREFIX: u32 = 0x0800;

// Overlay window padding around the text (physical pixels).
const OVERLAY_PAD_X: i32 = 8;
const OVERLAY_PAD_Y: i32 = 4;
// Distance from the screen edge when an anchor is used with offset 0.
const OVERLAY_EDGE_MARGIN: i32 = 24;
// Vertical gap between overlay lines (physical pixels).
const OVERLAY_LINE_GAP: i32 = 2;

// Win32 aliases and structures. Using raw FFI keeps this project dependency-free.
type BOOL = i32;
type BYTE = u8;
type WORD = u16;
type DWORD = u32;
type UINT = u32;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;
type HANDLE = *mut c_void;
type HWND = HANDLE;
type HINSTANCE = HANDLE;
type HICON = HANDLE;
type HCURSOR = HANDLE;
type HBRUSH = HANDLE;
type HGDIOBJ = HANDLE;
type HDC = HANDLE;
type HFONT = HANDLE;
type LPCWSTR = *const u16;
type LPWSTR = *mut u16;
type WNDPROC = Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>;

#[repr(C)]
struct POINT { x: i32, y: i32 }
#[repr(C)]
struct RECT { left: i32, top: i32, right: i32, bottom: i32 }
#[repr(C)]
struct SIZE { cx: i32, cy: i32 }
#[repr(C)]
struct MSG { hwnd: HWND, message: UINT, wParam: WPARAM, lParam: LPARAM, time: DWORD, pt: POINT, lPrivate: DWORD }
#[repr(C)]
struct PAINTSTRUCT {
    hdc: HDC,
    fErase: BOOL,
    rcPaint: RECT,
    fRestore: BOOL,
    fIncUpdate: BOOL,
    rgbReserved: [BYTE; 32],
}
#[repr(C)]
struct WNDCLASSEXW {
    cbSize: UINT,
    style: UINT,
    lpfnWndProc: WNDPROC,
    cbClsExtra: i32,
    cbWndExtra: i32,
    hInstance: HINSTANCE,
    hIcon: HICON,
    hCursor: HCURSOR,
    hbrBackground: HBRUSH,
    lpszMenuName: LPCWSTR,
    lpszClassName: LPCWSTR,
    hIconSm: HICON,
}

#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(value: HANDLE) -> BOOL;
    fn RegisterClassExW(wnd_class: *const WNDCLASSEXW) -> WORD;
    fn FindWindowW(class_name: LPCWSTR, window_name: LPCWSTR) -> HWND;
    fn GetSystemMetrics(index: i32) -> i32;
    fn CreateWindowExW(ex_style: DWORD, class_name: LPCWSTR, window_name: LPCWSTR,
        style: DWORD, x: i32, y: i32, width: i32, height: i32,
        parent: HWND, menu: HANDLE, instance: HINSTANCE, param: *mut c_void) -> HWND;
    fn DefWindowProcW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
    fn SetTimer(hwnd: HWND, id: usize, interval_ms: UINT, callback: Option<unsafe extern "system" fn(HWND, UINT, usize, DWORD)>) -> usize;
    fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: UINT, max: UINT) -> BOOL;
    fn TranslateMessage(msg: *const MSG) -> BOOL;
    fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    fn PostQuitMessage(exit_code: i32);
    fn GetForegroundWindow() -> HWND;
    fn GetWindowThreadProcessId(hwnd: HWND, process_id: *mut DWORD) -> DWORD;
    fn RegisterHotKey(hwnd: HWND, id: i32, modifiers: UINT, key: UINT) -> BOOL;
    fn UnregisterHotKey(hwnd: HWND, id: i32) -> BOOL;
    fn SetLayeredWindowAttributes(hwnd: HWND, color_key: DWORD, alpha: BYTE, flags: DWORD) -> BOOL;
    fn SetWindowPos(hwnd: HWND, insert_after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: UINT) -> BOOL;
    fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> BOOL;
    fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    fn LoadCursorW(instance: HINSTANCE, cursor_name: LPCWSTR) -> HCURSOR;
    fn MessageBoxW(hwnd: HWND, text: LPCWSTR, caption: LPCWSTR, kind: UINT) -> i32;
    fn GetDC(hwnd: HWND) -> HDC;
    fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
    fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    fn DrawTextW(hdc: HDC, text: LPCWSTR, count: i32, rect: *mut RECT, format: UINT) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(module_name: LPCWSTR) -> HINSTANCE;
    fn GetCurrentProcessId() -> DWORD;
    fn OpenProcess(access: DWORD, inherit: BOOL, process_id: DWORD) -> HANDLE;
    fn QueryFullProcessImageNameW(process: HANDLE, flags: DWORD, image_name: LPWSTR, size: *mut DWORD) -> BOOL;
    fn WaitForSingleObject(handle: HANDLE, milliseconds: DWORD) -> DWORD;
    fn CloseHandle(handle: HANDLE) -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    fn SetTextColor(hdc: HDC, color: DWORD) -> DWORD;
    fn CreateFontW(height: i32, width: i32, escapement: i32, orientation: i32,
        weight: i32, italic: DWORD, underline: DWORD, strike_out: DWORD,
        charset: DWORD, output_precision: DWORD, clip_precision: DWORD,
        quality: DWORD, pitch_and_family: DWORD, face_name: LPCWSTR) -> HFONT;
    fn SelectObject(hdc: HDC, object: HGDIOBJ) -> HGDIOBJ;
    fn DeleteObject(object: HGDIOBJ) -> BOOL;
    fn GetStockObject(object: i32) -> HGDIOBJ;
    fn GetTextExtentPoint32W(hdc: HDC, text: LPCWSTR, count: i32, size: *mut SIZE) -> BOOL;
}

// Values produced by the PresentMon reader thread and read by the overlay timer.
#[derive(Default)]
struct SharedStats {
    fps: AtomicU32,
    // Average frametime in hundredths of a millisecond (6.94 ms -> 694).
    frametime_x100: AtomicU32,
    low1_fps: AtomicU32,
    last_sample_ms: AtomicU64,
}

struct Capture {
    child: Child,
    process_name: String,
    process_id: u32,
    session_name: String,
    stats: Arc<SharedStats>,
}

#[derive(Clone, Debug, PartialEq)]
struct OverlaySettings {
    // Anchors: 0 = left/top, 1 = center/middle, 2 = right/bottom.
    horizontal_anchor: u8,
    vertical_anchor: u8,
    offset_x: i32,
    offset_y: i32,
    font_size: i32,
    // User-facing RGB hex, e.g. "E8E4D9". Win32 COLORREF is converted on use.
    color_hex: String,
    // Which lines the overlay shows, top to bottom in this order.
    show_fps: bool,
    show_frametime: bool,
    show_low1: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            horizontal_anchor: 0,
            vertical_anchor: 0,
            offset_x: 0,
            offset_y: 0,
            font_size: 34,
            color_hex: "E8E4D9".to_string(),
            show_fps: true,
            show_frametime: false,
            show_low1: false,
        }
    }
}

fn make_overlay_font(font_size: i32) -> HFONT {
    let font_name = wide("Segoe UI");
    unsafe { CreateFontW(font_size, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_name.as_ptr()) }
}

// Measures the widest text the overlay can show, so the window can be exactly as
// wide as its text. A window that is much wider than the text leaves an invisible
// (transparent) strip that cannot go past the screen edge, which made the visible
// text stop short of the right side of the monitor.
fn measure_overlay_text_width(font_size: i32) -> i32 {
    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        if hdc.is_null() { return font_size * 5; }
        let font = make_overlay_font(font_size);
        let previous = if !font.is_null() { SelectObject(hdc, font) } else { std::ptr::null_mut() };
        let sample = wide("1000 FPS");
        let mut size = SIZE { cx: 0, cy: 0 };
        let ok = GetTextExtentPoint32W(hdc, sample.as_ptr(), (sample.len() - 1) as i32, &mut size);
        if !previous.is_null() { SelectObject(hdc, previous); }
        if !font.is_null() { DeleteObject(font); }
        ReleaseDC(std::ptr::null_mut(), hdc);
        if ok != 0 && size.cx > 0 { size.cx } else { font_size * 5 }
    }
}

impl OverlaySettings {
    fn normalize(&mut self) {
        self.horizontal_anchor = self.horizontal_anchor.min(2);
        self.vertical_anchor = self.vertical_anchor.min(2);
        self.offset_x = self.offset_x.clamp(-4000, 4000);
        self.offset_y = self.offset_y.clamp(-4000, 4000);
        self.font_size = self.font_size.clamp(18, 64);
        // The overlay always shows at least one line.
        if !self.show_fps && !self.show_frametime && !self.show_low1 {
            self.show_fps = true;
        }
        self.color_hex = normalize_color(&self.color_hex).unwrap_or_else(|| "E8E4D9".to_string());
    }

    fn color_ref(&self) -> u32 {
        let rgb = u32::from_str_radix(&self.color_hex, 16).unwrap_or(0xE8E4D9);
        let r = rgb & 0xFF0000;
        let g = rgb & 0x00FF00;
        let b = rgb & 0x0000FF;
        (r >> 16) | g | (b << 16)
    }

    // Widest possible text of each enabled line, used to size the window.
    fn line_samples(&self) -> Vec<&'static str> {
        let mut samples = Vec::new();
        if self.show_fps { samples.push("1000 FPS"); }
        if self.show_frametime { samples.push("1000.0 ms"); }
        if self.show_low1 { samples.push("1% low 1000"); }
        if samples.is_empty() { samples.push("1000 FPS"); }
        samples
    }

    fn window_size(&self) -> (i32, i32) {
        let samples = self.line_samples();
        let (text_w, line_h) = measure_overlay_lines(self.font_size, &samples);
        let lines = samples.len() as i32;
        (
            text_w + OVERLAY_PAD_X * 2 + 2,
            line_h * lines + OVERLAY_LINE_GAP * (lines - 1) + OVERLAY_PAD_Y * 2,
        )
    }
}

// Returns (widest line width, line height) for the given sample strings.
fn measure_overlay_lines(font_size: i32, samples: &[&str]) -> (i32, i32) {
    unsafe {
        let fallback = (font_size * 5, font_size);
        let hdc = GetDC(std::ptr::null_mut());
        if hdc.is_null() { return fallback; }
        let font = make_overlay_font(font_size);
        let previous = if !font.is_null() { SelectObject(hdc, font) } else { std::ptr::null_mut() };
        let mut max_w = 0;
        let mut max_h = 0;
        for sample in samples {
            let text = wide(sample);
            let mut size = SIZE { cx: 0, cy: 0 };
            if GetTextExtentPoint32W(hdc, text.as_ptr(), (text.len() - 1) as i32, &mut size) != 0 {
                max_w = max_w.max(size.cx);
                max_h = max_h.max(size.cy);
            }
        }
        if !previous.is_null() { SelectObject(hdc, previous); }
        if !font.is_null() { DeleteObject(font); }
        ReleaseDC(std::ptr::null_mut(), hdc);
        if max_w > 0 { (max_w, max_h.max(font_size)) } else { fallback }
    }
}

fn overlay_lines(settings: &OverlaySettings, fps: &str, frametime: &str, low1: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if settings.show_fps { lines.push(fps.to_string()); }
    if settings.show_frametime { lines.push(frametime.to_string()); }
    if settings.show_low1 { lines.push(low1.to_string()); }
    if lines.is_empty() { lines.push(fps.to_string()); }
    lines
}

fn normalize_color(value: &str) -> Option<String> {
    let clean = value.trim().trim_start_matches('#');
    if clean.len() != 6 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let normalized = clean.to_ascii_uppercase();
    // Pure black is the color-key transparency color of the overlay window.
    if normalized == "000000" { return None; }
    Some(normalized)
}

fn settings_file_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("ZoroFPSOverlay").join("settings.ini")
    } else {
        current_exe_folder().join("settings.ini")
    }
}

fn load_settings(path: &PathBuf) -> OverlaySettings {
    let mut result = OverlaySettings::default();
    let Ok(contents) = fs::read_to_string(path) else { return result; };
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else { continue; };
        let key = key.trim();
        let value = value.trim();
        match key {
            "horizontal_anchor" => if let Ok(v) = value.parse::<u8>() { result.horizontal_anchor = v; },
            "vertical_anchor" => if let Ok(v) = value.parse::<u8>() { result.vertical_anchor = v; },
            "offset_x" => if let Ok(v) = value.parse::<i32>() { result.offset_x = v; },
            "offset_y" => if let Ok(v) = value.parse::<i32>() { result.offset_y = v; },
            "font_size" => if let Ok(v) = value.parse::<i32>() { result.font_size = v; },
            "color" => if let Some(v) = normalize_color(value) { result.color_hex = v; },
            "show_fps" => result.show_fps = value == "1",
            "show_frametime" => result.show_frametime = value == "1",
            "show_low1" => result.show_low1 = value == "1",
            _ => {},
        }
    }
    result.normalize();
    result
}

fn save_settings(path: &PathBuf, settings: &OverlaySettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Couldn't create settings folder: {}", e))?;
    }
    let content = format!(
        "horizontal_anchor={}\nvertical_anchor={}\noffset_x={}\noffset_y={}\nfont_size={}\ncolor={}\nshow_fps={}\nshow_frametime={}\nshow_low1={}\n",
        settings.horizontal_anchor, settings.vertical_anchor, settings.offset_x,
        settings.offset_y, settings.font_size, settings.color_hex,
        settings.show_fps as u8, settings.show_frametime as u8, settings.show_low1 as u8
    );
    fs::write(path, content).map_err(|e| format!("Couldn't save settings: {}", e))
}

fn overlay_geometry(settings: &OverlaySettings) -> (i32, i32, i32, i32) {
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(640);
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(480);
    let (width, height) = settings.window_size();
    // Process DPI awareness is enabled before creating any windows, so these
    // metrics and SetWindowPos coordinates use the same physical-pixel space.
    // Anchor selects the starting point; offsets always follow the intuitive
    // direction: positive X = right, negative X = left; positive Y = down.
    let base_x = match settings.horizontal_anchor {
        1 => (screen_w - width) / 2,
        2 => screen_w - width - OVERLAY_EDGE_MARGIN,
        _ => OVERLAY_EDGE_MARGIN,
    };
    let base_y = match settings.vertical_anchor {
        1 => (screen_h - height) / 2,
        2 => screen_h - height - OVERLAY_EDGE_MARGIN,
        _ => OVERLAY_EDGE_MARGIN,
    };
    let x = (base_x + settings.offset_x).clamp(0, (screen_w - width).max(0));
    let y = (base_y + settings.offset_y).clamp(0, (screen_h - height).max(0));
    (x, y, width, height)
}

struct AppState {
    own_pid: u32,
    presentmon_path: PathBuf,
    diagnostic_path: PathBuf,
    presentmon_output_path: PathBuf,
    settings_path: PathBuf,
    settings: OverlaySettings,
    last_settings_check: Instant,
    capture: Option<Capture>,
    fps_text: String,
    frametime_text: String,
    low1_text: String,
    visible: bool,
    target_name: Option<String>,
    target_pid: Option<u32>,
    user_hidden: bool,
}

static APP: OnceLock<Mutex<AppState>> = OnceLock::new();

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(std::iter::once(0)).collect()
}

fn message_box(text: &str, caption: &str) {
    let t = wide(text);
    let c = wide(caption);
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x0000_0040); }
}

fn current_exe_folder() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn process_name_for_pid(pid: u32) -> Option<String> {
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

fn is_system_or_shell_process(name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        "explorer.exe", "dwm.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe",
        "searchhost.exe", "searchapp.exe", "textinputhost.exe", "lockapp.exe",
        "applicationframehost.exe", "systemsettings.exe", "taskmgr.exe",
        "gameoverlayui.exe", "steamwebhelper.exe", "radeonsoftware.exe",
        "amdow.exe", "zoro-fps-overlay.exe", "zoro-fps-overlay-debug.exe",
    ];
    EXCLUDED.iter().any(|x| name.eq_ignore_ascii_case(x))
}

fn log_event(path: &PathBuf, message: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{:?} | {}", std::time::SystemTime::now(), message);
    }
}

fn process_id_is_running(process_id: u32) -> bool {
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
fn cleanup_stale_session_from_previous_run(presentmon_path: &PathBuf, diagnostic_path: &PathBuf, current_pid: u32) {
    if !presentmon_path.exists() { return; }
    let Ok(previous_log) = fs::read_to_string(diagnostic_path) else { return; };
    let Some(session_name) = previous_log.lines().rev().find_map(|line| {
        line.split_once("Launching PresentMon with session=")
            .map(|(_, value)| value.trim().to_string())
    }) else { return; };
    let Some(owner_pid) = session_name.strip_prefix("ZoroFPS").and_then(|s| s.parse::<u32>().ok()) else { return; };
    if owner_pid == current_pid || process_id_is_running(owner_pid) { return; }

    let result = Command::new(presentmon_path)
        .arg("--terminate_existing_session")
        .arg("--session_name")
        .arg(&session_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    if result.is_err() {
        // Startup diagnostics are reset below; this is intentionally best-effort.
        // The new capture also has --stop_existing_session as a recovery path.
    }
}

fn stop_capture(state: &mut AppState) {
    if let Some(mut capture) = state.capture.take() {
        log_event(
            &state.diagnostic_path,
            &format!("Stopping PresentMon: target={} pid={} session={}", capture.process_name, capture.process_id, capture.session_name),
        );

        // Do not kill PresentMon first: abruptly terminating it can leave its ETW
        // trace session registered. Ask the official CLI to stop that session,
        // then give the collector a brief chance to exit normally.
        if state.presentmon_path.exists() {
            match Command::new(&state.presentmon_path)
                .arg("--terminate_existing_session")
                .arg("--session_name")
                .arg(&capture.session_name)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status()
            {
                Ok(status) => log_event(
                    &state.diagnostic_path,
                    &format!("PresentMon session-stop helper exited with {}", status),
                ),
                Err(e) => log_event(
                    &state.diagnostic_path,
                    &format!("WARNING: could not run PresentMon session-stop helper: {}", e),
                ),
            }
        }

        let deadline = Instant::now() + Duration::from_millis(350);
        loop {
            match capture.child.try_wait() {
                Ok(Some(status)) => {
                    log_event(&state.diagnostic_path, &format!("PresentMon collector exited normally with {}", status));
                    break;
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    log_event(&state.diagnostic_path, "WARNING: PresentMon collector did not exit after session stop; terminating it");
                    let _ = capture.child.kill();
                    let _ = capture.child.wait();
                    break;
                }
                Err(e) => {
                    log_event(&state.diagnostic_path, &format!("WARNING: failed waiting for PresentMon collector: {}", e));
                    let _ = capture.child.kill();
                    let _ = capture.child.wait();
                    break;
                }
            }
        }
    }
    state.target_name = None;
    state.target_pid = None;
    state.fps_text = "-- FPS".to_string();
    state.frametime_text = "-- ms".to_string();
    state.low1_text = "1% low --".to_string();
}

fn start_capture(state: &mut AppState, process_name: &str, process_id: u32) {
    stop_capture(state);
    state.target_name = Some(process_name.to_string());
    state.target_pid = Some(process_id);

    log_event(&state.diagnostic_path, &format!(
        "Starting capture: process={} pid={} presentmon={}",
        process_name, process_id, state.presentmon_path.display()
    ));
    if !state.presentmon_path.exists() {
        log_event(&state.diagnostic_path, "ERROR: PresentMon.exe not found next to overlay executable");
        state.fps_text = "FPS ERR".to_string();
        return;
    }

    let session_name = format!("ZoroFPS{}", state.own_pid);
    // Keep PresentMon diagnostics on disk, but read CSV from stdout. PresentMon
    // opens --output_file in a way that prevents this process from reading the
    // live CSV on some Windows versions (sharing violation / OS error 32).
    let log_stderr = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.presentmon_output_path)
    {
        Ok(file) => file,
        Err(e) => {
            state.fps_text = "FPS ERR".to_string();
            log_event(&state.diagnostic_path, &format!("ERROR: could not open PresentMon error log: {}", e));
            return;
        }
    };

    let mut command = Command::new(&state.presentmon_path);
    command
        .arg("--process_id").arg(process_id.to_string())
        .arg("--output_stdout")
        .arg("--no_console_stats")
        .arg("--v1_metrics")
        // These options reduce work unrelated to a simple FPS counter.
        .arg("--no_track_gpu")
        .arg("--no_track_input")
        .arg("--no_track_display")
        .arg("--session_name").arg(&session_name)
        .arg("--stop_existing_session")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log_stderr))
        .creation_flags(CREATE_NO_WINDOW);

    log_event(&state.diagnostic_path, &format!(
        "Launching PresentMon with session={}", session_name
    ));
    log_event(&state.diagnostic_path, "Capture mode: parsing PresentMon v1 CSV directly from stdout");

    match command.spawn() {
        Ok(mut child) => {
            let stats = Arc::new(SharedStats::default());
            if let Some(stdout) = child.stdout.take() {
                spawn_csv_reader(
                    stdout,
                    Arc::clone(&stats),
                    state.diagnostic_path.clone(),
                );
            } else {
                log_event(&state.diagnostic_path, "ERROR: PresentMon stdout pipe was not available");
            }

            state.capture = Some(Capture {
                child,
                process_name: process_name.to_string(),
                process_id,
                session_name: session_name.clone(),
                stats,
            });
            state.fps_text = "FPS ...".to_string();
            log_event(&state.diagnostic_path, "PresentMon child process spawned; reading CSV from stdout");
        }
        Err(e) => {
            state.fps_text = "FPS ERR".to_string();
            log_event(&state.diagnostic_path, &format!("ERROR: could not launch PresentMon: {}", e));
        }
    }
}

fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => { field.push('"'); chars.next(); }
            '"' => quoted = !quoted,
            ',' if !quoted => { fields.push(std::mem::take(&mut field)); }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn find_metric_column(headers: &[String]) -> Option<(usize, &'static str)> {
    const METRICS: [&str; 4] = [
        "MsBetweenPresents",
        "MsBetweenDisplayChange",
        "MsBetweenAppStart",
        "MsBetweenSimulationStart",
    ];
    for metric in METRICS {
        if let Some(index) = headers.iter().position(|h| {
            h.trim().trim_start_matches('\u{feff}').eq_ignore_ascii_case(metric)
        }) {
            return Some((index, metric));
        }
    }
    None
}

// PresentMon 2.6 emits valid CSV rows to stdout, but live --output_file reads
// can fail on Windows with sharing violation 32. Parse stdout directly instead.
fn spawn_csv_reader<R: Read + Send + 'static>(
    stream: R,
    stats: Arc<SharedStats>,
    diagnostic_path: PathBuf,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let mut metric_index: Option<usize> = None;
        let mut expected_columns = 0usize;
        let mut intervals: Vec<f64> = Vec::with_capacity(30);
        // Recent frame times for the 1% low: the average of the slowest 1% of
        // the last ~600 frames, converted to FPS.
        let mut history: VecDeque<f64> = VecDeque::with_capacity(601);
        let mut last_low_calc = Instant::now();
        let mut first_fps_logged = false;
        let mut last_logged_fps = 0u32;
        let mut last_log_at = Instant::now();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    log_event(&diagnostic_path, &format!("ERROR reading PresentMon stdout: {}", e));
                    break;
                }
            }

            let line_text = line
                .trim_end_matches(['\r', '\n'])
                .trim_start_matches('\u{feff}');
            if line_text.trim().is_empty() {
                continue;
            }
            let fields = csv_fields(line_text);

            if metric_index.is_none() {
                if let Some((idx, name)) = find_metric_column(&fields) {
                    metric_index = Some(idx);
                    expected_columns = fields.len();
                    log_event(&diagnostic_path, &format!(
                        "PresentMon stdout CSV header recognized: {} at column {} ({} columns)",
                        name, idx, expected_columns
                    ));
                } else {
                    log_event(&diagnostic_path, &format!(
                        "WARNING: unrecognized PresentMon CSV header: {}",
                        line_text.chars().take(240).collect::<String>()
                    ));
                }
                continue;
            }

            if fields.len() != expected_columns {
                continue;
            }
            let Some(index) = metric_index else { continue; };
            let Some(raw_ms) = fields.get(index) else { continue; };
            let Ok(ms) = raw_ms.trim().parse::<f64>() else { continue; };
            if !(0.1..=1000.0).contains(&ms) {
                continue;
            }

            intervals.push(ms);
            if intervals.len() > 30 {
                intervals.remove(0);
            }
            if intervals.len() < 2 {
                continue;
            }
            let average_ms = intervals.iter().sum::<f64>() / intervals.len() as f64;
            if average_ms <= 0.0 {
                continue;
            }
            let fps = (1000.0 / average_ms).round().clamp(1.0, 1000.0) as u32;
            stats.fps.store(fps, Ordering::Relaxed);
            stats.frametime_x100.store((average_ms * 100.0).round().clamp(0.0, 4_000_000.0) as u32, Ordering::Relaxed);
            stats.last_sample_ms.store(epoch_millis(), Ordering::Relaxed);

            history.push_back(ms);
            if history.len() > 600 { history.pop_front(); }
            if history.len() >= 60 && last_low_calc.elapsed() >= Duration::from_millis(250) {
                last_low_calc = Instant::now();
                let mut sorted: Vec<f64> = history.iter().copied().collect();
                sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                let count = (sorted.len() / 100).max(1);
                let worst_avg = sorted[..count].iter().sum::<f64>() / count as f64;
                if worst_avg > 0.0 {
                    stats.low1_fps.store((1000.0 / worst_avg).round().clamp(1.0, 1000.0) as u32, Ordering::Relaxed);
                }
            }

            if !first_fps_logged || fps.abs_diff(last_logged_fps) >= 5 || last_log_at.elapsed() >= Duration::from_secs(5) {
                log_event(&diagnostic_path, &format!(
                    "FPS parser updated: {} FPS from {} frame intervals (latest {:.3} ms)",
                    fps, intervals.len(), ms
                ));
                first_fps_logged = true;
                last_logged_fps = fps;
                last_log_at = Instant::now();
            }
        }
        log_event(&diagnostic_path, "PresentMon stdout CSV stream closed");
    });
}

fn update_overlay(hwnd: HWND) {
    let fg = unsafe { GetForegroundWindow() };
    let mut pid: DWORD = 0;
    if !fg.is_null() { unsafe { GetWindowThreadProcessId(fg, &mut pid); } }
    let own_pid = unsafe { GetCurrentProcessId() };
    let foreground_name = if pid != 0 && pid != own_pid { process_name_for_pid(pid) } else { None };

    let mut state = match APP.get().and_then(|m| m.lock().ok()) { Some(s) => s, None => return };
    // The settings window writes a tiny INI file. Poll it twice a second so an
    // already-running overlay immediately picks up saved position/style changes.
    if state.last_settings_check.elapsed() >= Duration::from_millis(500) {
        state.last_settings_check = Instant::now();
        let new_settings = load_settings(&state.settings_path);
        if new_settings != state.settings {
            state.settings = new_settings;
            let (x, y, width, height) = overlay_geometry(&state.settings);
            unsafe {
                SetWindowPos(hwnd, (-1isize) as HWND, x, y, width, height, SWP_NOACTIVATE);
                InvalidateRect(hwnd, std::ptr::null(), 1);
            }
            log_event(&state.diagnostic_path, "Settings reloaded from settings.ini; overlay geometry/style updated");
        }
    }
    let should_capture = foreground_name.as_deref().filter(|n| !is_system_or_shell_process(n));

    if let Some(name) = should_capture {
        let needs_new_capture = state.target_name.as_deref() != Some(name)
            || state.target_pid != Some(pid);
        if needs_new_capture { start_capture(&mut state, name, pid); }

        let mut exited_message: Option<String> = None;
        if let Some(capture) = state.capture.as_mut() {
            match capture.child.try_wait() {
                Ok(Some(status)) => {
                    exited_message = Some(format!("PresentMon exited with status {}; target={} pid={}", status, capture.process_name, capture.process_id));
                }
                Err(e) => {
                    exited_message = Some(format!("ERROR: failed waiting for PresentMon: {}", e));
                }
                Ok(None) => {}
            }
        }
        if let Some(message) = exited_message {
            log_event(&state.diagnostic_path, &message);
            // Clear the exited child handle but keep the target identity, so we do not
            // repeatedly respawn or flood the diagnostic log every 250 ms.
            state.capture = None;
            state.fps_text = "FPS ERR".to_string();
            state.frametime_text = "-- ms".to_string();
            state.low1_text = "1% low --".to_string();
        } else {
            // Only show a reading while frame samples are recent, so a stopped
            // game or a hung PresentMon process does not leave a stale FPS forever.
            let sample = state.capture.as_ref().map(|capture| (
                capture.stats.fps.load(Ordering::Relaxed),
                capture.stats.frametime_x100.load(Ordering::Relaxed),
                capture.stats.low1_fps.load(Ordering::Relaxed),
                capture.stats.last_sample_ms.load(Ordering::Relaxed),
            ));
            if let Some((fps, frametime_x100, low1, last_sample)) = sample {
                let now = epoch_millis();
                if fps > 0 && last_sample > 0 && now.saturating_sub(last_sample) < 5000 {
                    state.fps_text = format!("{} FPS", fps);
                    state.frametime_text = format!("{:.1} ms", frametime_x100 as f64 / 100.0);
                    state.low1_text = if low1 > 0 { format!("1% low {}", low1) } else { "1% low --".to_string() };
                } else {
                    state.fps_text = "FPS ...".to_string();
                    state.frametime_text = "-- ms".to_string();
                    state.low1_text = "1% low --".to_string();
                }
            }
        }
        if !state.user_hidden && !state.visible {
            unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE); }
            state.visible = true;
        } else if state.user_hidden && state.visible {
            unsafe { ShowWindow(hwnd, SW_HIDE); }
            state.visible = false;
        }
    } else {
        if state.capture.is_some() || state.target_name.is_some() { stop_capture(&mut state); }
        if state.visible {
            unsafe { ShowWindow(hwnd, SW_HIDE); }
            state.visible = false;
        }
    }
    drop(state);
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 1); }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_null() {
                let (lines, settings) = APP.get().and_then(|m| m.lock().ok())
                    .map(|s| (
                        overlay_lines(&s.settings, &s.fps_text, &s.frametime_text, &s.low1_text),
                        s.settings.clone(),
                    ))
                    .unwrap_or_else(|| (vec!["-- FPS".to_string()], OverlaySettings::default()));
                let font = make_overlay_font(settings.font_size);
                let previous = if !font.is_null() { SelectObject(hdc, font) } else { std::ptr::null_mut() };
                SetBkMode(hdc, 1); // transparent text background
                SetTextColor(hdc, settings.color_ref());

                // Align the text inside the window according to the chosen anchor.
                // With a right anchor the text hugs the right edge, so it can reach
                // the very edge of the monitor; "FPS" values of different widths
                // (60 vs 144 vs 1000) also keep their anchored edge steady.
                let mut client: RECT = zeroed();
                GetClientRect(hwnd, &mut client);
                let align = match settings.horizontal_anchor {
                    1 => DT_CENTER,
                    2 => DT_RIGHT,
                    _ => DT_LEFT,
                };
                // Same line height rule as measure_overlay_lines, so the window
                // height computed from it always fits the painted lines.
                let probe = wide("Ag");
                let mut probe_size = SIZE { cx: 0, cy: 0 };
                GetTextExtentPoint32W(hdc, probe.as_ptr(), 2, &mut probe_size);
                let line_h = probe_size.cy.max(settings.font_size);
                for (index, line) in lines.iter().enumerate() {
                    let text_w = wide(line);
                    let top = OVERLAY_PAD_Y + index as i32 * (line_h + OVERLAY_LINE_GAP);
                    let mut rc = RECT {
                        left: client.left + OVERLAY_PAD_X,
                        top,
                        right: client.right - OVERLAY_PAD_X,
                        bottom: top + line_h,
                    };
                    DrawTextW(
                        hdc, text_w.as_ptr(), text_w.len().saturating_sub(1) as i32, &mut rc,
                        align | DT_SINGLELINE | DT_NOPREFIX | DT_NOCLIP,
                    );
                }

                if !previous.is_null() { SelectObject(hdc, previous); }
                if !font.is_null() { DeleteObject(font); }
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_TIMER => { update_overlay(hwnd); 0 }
        WM_HOTKEY => {
            let id = wparam as i32;
            if id == 1 {
                if let Some(m) = APP.get() {
                    if let Ok(mut state) = m.lock() {
                        state.user_hidden = !state.user_hidden;
                        state.visible = !state.user_hidden;
                        let visible = state.visible;
                        drop(state);
                        unsafe { ShowWindow(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }); }
                    }
                }
            } else if id == 2 {
                PostQuitMessage(0);
            }
            0
        }
        WM_NCHITTEST => HTTRANSPARENT,
        WM_DESTROY => {
            UnregisterHotKey(hwnd, 1);
            UnregisterHotKey(hwnd, 2);
            if let Some(m) = APP.get() {
                if let Ok(mut state) = m.lock() { stop_capture(&mut state); }
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------------------------------------------------------------- Overlay launcher

fn launch_overlay_process() -> Result<(), String> {
    // The pre-settings v0.1.x overlay used this class name and cannot hot-reload
    // settings.ini. Ask the user to close that one rather than silently attaching
    // the settings UI to an incompatible process.
    let legacy_class = wide("ZoroFpsOverlayWindow");
    if !unsafe { FindWindowW(legacy_class.as_ptr(), std::ptr::null()) }.is_null() {
        return Err("An older Zoro FPS Overlay is still running. Press F10 to close it, then try again.".to_string());
    }
    // The active v0.2+ overlay already hot-reloads settings.ini. Don't spawn a
    // second collector when the user presses Save & Start from the settings UI.
    let current_class = wide("ZoroFpsOverlayWindowV020");
    if !unsafe { FindWindowW(current_class.as_ptr(), std::ptr::null()) }.is_null() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't locate this application: {}", e))?;
    let pm_path = exe.parent().unwrap_or_else(|| std::path::Path::new(".")).join("PresentMon.exe");
    if !pm_path.exists() {
        return Err("PresentMon.exe is missing next to the app. Run scripts\\download-presentmon.ps1 first.".to_string());
    }
    Command::new(exe).arg("--overlay")
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map(|_| ()).map_err(|e| format!("Couldn't start the overlay process: {}", e))
}

// ---------------------------------------------------------------- Settings GUI (eframe + egui)

const ANCHOR_LABELS: [&str; 9] = [
    "Top left", "Top center", "Top right",
    "Middle left", "Center", "Middle right",
    "Bottom left", "Bottom center", "Bottom right",
];

const SWATCHES: [&str; 6] = ["E8E4D9", "FFFFFF", "64FF82", "5CC8FF", "FFD166", "FF6B6B"];

// Palette. Every filled accent surface pairs with C_ON_ACCENT (dark) text, so
// light buttons can never end up with light, unreadable labels.
const C_BG: egui::Color32 = egui::Color32::from_rgb(12, 12, 12);
const C_DEEP: egui::Color32 = egui::Color32::from_rgb(8, 8, 8);
const C_SURFACE: egui::Color32 = egui::Color32::from_rgb(20, 20, 19);
const C_RAISED: egui::Color32 = egui::Color32::from_rgb(32, 32, 30);
const C_BORDER: egui::Color32 = egui::Color32::from_rgb(40, 40, 38);
const C_BORDER_HI: egui::Color32 = egui::Color32::from_rgb(82, 81, 76);
const C_TEXT: egui::Color32 = egui::Color32::from_rgb(236, 233, 225);
const C_MUTED: egui::Color32 = egui::Color32::from_rgb(130, 128, 121);
const C_ACCENT: egui::Color32 = egui::Color32::from_rgb(224, 219, 207);
const C_ON_ACCENT: egui::Color32 = egui::Color32::from_rgb(16, 16, 15);
const C_ERROR: egui::Color32 = egui::Color32::from_rgb(232, 122, 112);

fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

fn color_from_hex(value: &str) -> egui::Color32 {
    let normalized = normalize_color(value).unwrap_or_else(|| "E8E4D9".to_string());
    let rgb = u32::from_str_radix(&normalized, 16).unwrap_or(0xE8E4D9);
    egui::Color32::from_rgb(((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8)
}

#[derive(Clone, Copy, PartialEq)]
enum BtnKind { Primary, Secondary, Ghost }

fn action_button(ui: &mut egui::Ui, label: &str, kind: BtnKind, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::click());
    let ctx = ui.ctx().clone();
    let hover = ctx.animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.14);
    let press = ctx.animate_bool_with_time(response.id.with("press"), response.is_pointer_button_down_on(), 0.07);
    let body = rect.shrink(press * 1.5);

    let (fill, border, text) = match kind {
        BtnKind::Primary => {
            let c = mix(C_ACCENT, egui::Color32::WHITE, hover * 0.4);
            (c, c, C_ON_ACCENT)
        }
        BtnKind::Secondary => (
            mix(C_SURFACE, C_RAISED, hover),
            mix(C_BORDER, C_BORDER_HI, hover),
            C_TEXT,
        ),
        BtnKind::Ghost => (
            mix(C_BG, C_RAISED, hover),
            mix(C_BG, C_BORDER, hover),
            mix(C_MUTED, C_TEXT, hover),
        ),
    };

    let painter = ui.painter();
    painter.rect_filled(body, egui::CornerRadius::same(9), fill);
    painter.rect_stroke(body, egui::CornerRadius::same(9), egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.text(body.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(13.0), text);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn anchor_picker(ui: &mut egui::Ui, horizontal: &mut u8, vertical: &mut u8) {
    let cell = egui::vec2(60.0, 42.0);
    let gap = 6.0;
    let total = egui::vec2(cell.x * 3.0 + gap * 2.0, cell.y * 3.0 + gap * 2.0);
    let (area, _) = ui.allocate_exact_size(total, egui::Sense::hover());
    let ctx = ui.ctx().clone();
    let base_id = ui.id();

    for row in 0..3usize {
        for col in 0..3usize {
            let index = row * 3 + col;
            let min = area.min + egui::vec2(col as f32 * (cell.x + gap), row as f32 * (cell.y + gap));
            let rect = egui::Rect::from_min_size(min, cell);
            let id = base_id.with(("anchor-cell", index));
            let response = ui.interact(rect, id, egui::Sense::click());

            let selected = *horizontal as usize == col && *vertical as usize == row;
            let sel = ctx.animate_bool_with_time(id.with("sel"), selected, 0.18);
            let hov = ctx.animate_bool_with_time(id.with("hov"), response.hovered(), 0.12);

            let fill = mix(mix(C_SURFACE, C_RAISED, hov), C_ACCENT, sel);
            let border = mix(mix(C_BORDER, C_BORDER_HI, hov), C_ACCENT, sel);
            let dot_color = mix(mix(C_BORDER_HI, C_MUTED, hov), C_ON_ACCENT, sel);

            // The dot sits where the overlay will sit inside the cell.
            let dot_x = match col { 0 => rect.left() + 11.0, 1 => rect.center().x, _ => rect.right() - 11.0 };
            let dot_y = match row { 0 => rect.top() + 11.0, 1 => rect.center().y, _ => rect.bottom() - 11.0 };

            let painter = ui.painter();
            painter.rect_filled(rect, egui::CornerRadius::same(9), fill);
            painter.rect_stroke(rect, egui::CornerRadius::same(9), egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
            painter.circle_filled(egui::pos2(dot_x, dot_y), 2.6 + sel * 1.4 + hov * 0.6, dot_color);

            if response.clicked() {
                *horizontal = col as u8;
                *vertical = row as u8;
            }
            let _ = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
    }
}

fn color_swatch(ui: &mut egui::Ui, color: egui::Color32, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let ctx = ui.ctx().clone();
    let hov = ctx.animate_bool_with_time(response.id.with("hov"), response.hovered(), 0.12);
    let sel = ctx.animate_bool_with_time(response.id.with("sel"), selected, 0.18);
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 7.5 + hov * 1.5, color);
    painter.circle_stroke(rect.center(), 10.8, egui::Stroke::new(1.5, C_ACCENT.gamma_multiply(sel)));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn key_hint(ui: &mut egui::Ui, key: &str, action: &str) {
    // Added in right-to-left order: the text first, so the key cap ends up on its left.
    ui.label(egui::RichText::new(action).size(11.5).color(C_MUTED));
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, C_BORDER_HI))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(key).size(11.0).strong().color(C_TEXT));
        });
}

fn toggle_row(ui: &mut egui::Ui, title: &str, subtitle: &str, value: &mut bool) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.0), egui::Sense::click());
    if response.clicked() { *value = !*value; }
    let ctx = ui.ctx().clone();
    let on = ctx.animate_bool_with_time(response.id.with("on"), *value, 0.18);
    let hover = ctx.animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);

    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(10), mix(C_SURFACE, C_RAISED, hover));
    painter.text(rect.left_top() + egui::vec2(12.0, 9.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(13.5), C_TEXT);
    painter.text(rect.left_top() + egui::vec2(12.0, 28.0), egui::Align2::LEFT_TOP, subtitle, egui::FontId::proportional(11.0), C_MUTED);

    let track = egui::Rect::from_center_size(egui::pos2(rect.right() - 12.0 - 17.0, rect.center().y), egui::vec2(34.0, 20.0));
    painter.rect_filled(track, egui::CornerRadius::same(10), mix(C_BORDER, C_ACCENT, on));
    let knob_x = track.left() + 10.0 + (track.width() - 20.0) * on;
    painter.circle_filled(egui::pos2(knob_x, track.center().y), 7.0, mix(C_MUTED, C_ON_ACCENT, on));
    let _ = response.on_hover_cursor(egui::CursorIcon::PointingHand);
}

fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) {
    egui::Frame::new()
        .fill(C_SURFACE)
        .stroke(egui::Stroke::new(1.0, C_BORDER))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        });
}

fn card_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(egui::RichText::new(title).size(16.0).strong().color(C_TEXT));
    ui.label(egui::RichText::new(subtitle).size(12.0).color(C_MUTED));
}

struct SettingsApp {
    settings: OverlaySettings,
    settings_path: PathBuf,
    color_edit: String,
    status: String,
    status_error: bool,
    saved_at: Option<Instant>,
}

impl SettingsApp {
    fn new(cc: &eframe::CreationContext<'_>, settings_path: PathBuf) -> Self {
        let settings = load_settings(&settings_path);
        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = C_BG;
        style.visuals.window_fill = C_SURFACE;
        style.visuals.extreme_bg_color = C_DEEP;
        // No global text override: every widget gets an explicit foreground that
        // contrasts with its own background (see the button helpers above).
        style.visuals.override_text_color = None;
        style.visuals.selection.bg_fill = C_ACCENT;
        style.visuals.selection.stroke = egui::Stroke::new(1.0, C_ON_ACCENT);

        style.visuals.widgets.noninteractive.bg_fill = C_SURFACE;
        style.visuals.widgets.noninteractive.weak_bg_fill = C_SURFACE;
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, C_BORDER);
        style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, C_TEXT);

        style.visuals.widgets.inactive.bg_fill = C_RAISED;
        style.visuals.widgets.inactive.weak_bg_fill = C_RAISED;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, C_BORDER);
        style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, C_TEXT);

        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(46, 46, 43);
        style.visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(46, 46, 43);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, C_BORDER_HI);
        style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(64, 63, 58);
        style.visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(64, 63, 58);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, C_ACCENT);
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

        style.spacing.item_spacing = egui::vec2(10.0, 9.0);
        style.spacing.button_padding = egui::vec2(13.0, 8.0);
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);

        Self {
            color_edit: settings.color_hex.clone(),
            settings,
            settings_path,
            status: "Changes apply after you save.".to_string(),
            status_error: false,
            saved_at: None,
        }
    }

    fn save(&mut self) -> bool {
        let Some(color) = normalize_color(&self.color_edit) else {
            self.status = "Enter a valid RRGGBB color. Pure black is transparent.".to_string();
            self.status_error = true;
            return false;
        };
        self.settings.color_hex = color.clone();
        self.color_edit = color;
        self.settings.normalize();
        match save_settings(&self.settings_path, &self.settings) {
            Ok(()) => {
                self.status = "Saved".to_string();
                self.status_error = false;
                self.saved_at = Some(Instant::now());
                true
            }
            Err(error) => {
                self.status = error;
                self.status_error = true;
                false
            }
        }
    }

    fn draw_position_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Position", "Pick a corner, then nudge it with pixel offsets.");
        ui.add_space(14.0);

        anchor_picker(ui, &mut self.settings.horizontal_anchor, &mut self.settings.vertical_anchor);
        let label_index = (self.settings.vertical_anchor.min(2) as usize) * 3 + self.settings.horizontal_anchor.min(2) as usize;
        ui.label(egui::RichText::new(ANCHOR_LABELS[label_index]).size(12.0).color(C_MUTED));

        ui.add_space(14.0);
        ui.label(egui::RichText::new("OFFSET").size(10.5).strong().color(C_MUTED));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("X").color(C_MUTED));
            ui.add(egui::DragValue::new(&mut self.settings.offset_x).range(-4000..=4000).speed(1).suffix(" px"));
            ui.add_space(12.0);
            ui.label(egui::RichText::new("Y").color(C_MUTED));
            ui.add(egui::DragValue::new(&mut self.settings.offset_y).range(-4000..=4000).speed(1).suffix(" px"));
        });
        ui.label(egui::RichText::new("+X moves right, +Y moves down.").size(11.5).color(C_MUTED));
    }

    fn draw_preview_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Preview", "Illustrative — the FPS value is a placeholder.");
        ui.add_space(12.0);

        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(260.0), 160.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, egui::CornerRadius::same(10), C_DEEP);
        painter.rect_stroke(rect, egui::CornerRadius::same(10), egui::Stroke::new(1.0, C_BORDER), egui::StrokeKind::Inside);

        let font = egui::FontId::proportional((self.settings.font_size as f32 * 0.48).clamp(12.0, 30.0));
        let color = color_from_hex(&self.settings.color_hex);
        let mut preview_lines: Vec<&str> = Vec::new();
        if self.settings.show_fps { preview_lines.push("144 FPS"); }
        if self.settings.show_frametime { preview_lines.push("6.9 ms"); }
        if self.settings.show_low1 { preview_lines.push("1% low 98"); }
        if preview_lines.is_empty() { preview_lines.push("144 FPS"); }
        let galleys: Vec<_> = preview_lines
            .iter()
            .map(|text| painter.layout_no_wrap(text.to_string(), font.clone(), color))
            .collect();
        let line_gap = 2.0;
        let block_w = galleys.iter().map(|g| g.size().x).fold(0.0_f32, f32::max);
        let block_h = galleys.iter().map(|g| g.size().y).sum::<f32>()
            + line_gap * galleys.len().saturating_sub(1) as f32;
        let size = egui::vec2(block_w, block_h);

        let margin = 10.0;
        let sx = rect.width() / 1920.0;
        let sy = rect.height() / 1080.0;
        let base_x = match self.settings.horizontal_anchor {
            0 => rect.left() + margin,
            1 => rect.center().x - size.x / 2.0,
            _ => rect.right() - margin - size.x,
        };
        let base_y = match self.settings.vertical_anchor {
            0 => rect.top() + margin,
            1 => rect.center().y - size.y / 2.0,
            _ => rect.bottom() - margin - size.y,
        };
        let min_x = rect.left() + 4.0;
        let max_x = (rect.right() - 4.0 - size.x).max(min_x);
        let min_y = rect.top() + 4.0;
        let max_y = (rect.bottom() - 4.0 - size.y).max(min_y);
        let target_x = (base_x + self.settings.offset_x as f32 * sx).clamp(min_x, max_x);
        let target_y = (base_y + self.settings.offset_y as f32 * sy).clamp(min_y, max_y);

        // Smoothly glide to the new position instead of snapping.
        let ctx = ui.ctx().clone();
        let x = ctx.animate_value_with_time(egui::Id::new("preview-x"), target_x, 0.22);
        let y = ctx.animate_value_with_time(egui::Id::new("preview-y"), target_y, 0.22);
        let mut cursor_y = y;
        for galley in galleys {
            let line_w = galley.size().x;
            let line_h = galley.size().y;
            let line_x = match self.settings.horizontal_anchor {
                0 => 0.0,
                1 => (block_w - line_w) / 2.0,
                _ => block_w - line_w,
            };
            painter.galley(egui::pos2(x + line_x, cursor_y), galley, color);
            cursor_y += line_h + line_gap;
        }
    }

    fn draw_appearance_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Appearance", "Size and color of the FPS counter.");
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Text size").color(C_TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{} px", self.settings.font_size)).strong().color(C_TEXT));
            });
        });
        ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(120.0);
        ui.add(egui::Slider::new(&mut self.settings.font_size, 18..=64).show_value(false));

        ui.add_space(8.0);
        ui.label(egui::RichText::new("COLOR").size(10.5).strong().color(C_MUTED));
        ui.horizontal(|ui| {
            for hex in SWATCHES {
                let selected = self.settings.color_hex == hex;
                if color_swatch(ui, color_from_hex(hex), selected).clicked() {
                    self.settings.color_hex = hex.to_string();
                    self.color_edit = hex.to_string();
                }
            }
        });
        ui.horizontal(|ui| {
            let mut picked = color_from_hex(&self.settings.color_hex);
            let response = egui::color_picker::color_edit_button_srgba(ui, &mut picked, egui::color_picker::Alpha::Opaque);
            if response.changed() {
                let hex = format!("{:02X}{:02X}{:02X}", picked.r(), picked.g(), picked.b());
                if let Some(normalized) = normalize_color(&hex) {
                    self.settings.color_hex = normalized.clone();
                    self.color_edit = normalized;
                }
            }
            ui.label(egui::RichText::new("HEX").size(11.0).strong().color(C_MUTED));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.color_edit)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(90.0)
                    .char_limit(7),
            );
            if edit.changed() {
                if let Some(normalized) = normalize_color(&self.color_edit) {
                    self.settings.color_hex = normalized;
                }
            }
        });
        ui.label(egui::RichText::new("RRGGBB. Pure black is transparent.").size(11.5).color(C_MUTED));
    }

    fn draw_display_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Overlay lines", "Choose what is shown, one item per line.");
        ui.add_space(8.0);
        toggle_row(ui, "FPS", "Average of the last 30 frames", &mut self.settings.show_fps);
        toggle_row(ui, "Frametime", "Milliseconds per frame", &mut self.settings.show_frametime);
        toggle_row(ui, "1% low", "Slowest 1% of the last ~600 frames", &mut self.settings.show_low1);
        // At least one line must stay on; a toggle that would turn all off snaps back.
        if !self.settings.show_fps && !self.settings.show_frametime && !self.settings.show_low1 {
            self.settings.show_fps = true;
        }
    }

    fn reset_defaults(&mut self) {
        self.settings = OverlaySettings::default();
        self.color_edit = self.settings.color_hex.clone();
        self.status = "Defaults loaded. Press Save to keep them.".to_string();
        self.status_error = false;
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::Panel::top("zoro-header")
            .frame(egui::Frame::new().fill(C_BG).inner_margin(egui::Margin::symmetric(24, 18)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Zoro FPS Overlay").size(18.0).strong().color(C_TEXT));
                    ui.label(egui::RichText::new("v0.4.0").size(11.0).color(C_MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        key_hint(ui, "F10", "Quit");
                        ui.add_space(8.0);
                        key_hint(ui, "F8", "Hide / show");
                    });
                });
            });

        let mut save_clicked = false;
        let mut start_clicked = false;
        let mut reset_clicked = false;
        let recently_saved = self.saved_at.map_or(false, |t| t.elapsed() < Duration::from_millis(1400));
        let flash = ctx.animate_bool_with_time(egui::Id::new("status-flash"), recently_saved, 0.35);
        if recently_saved { ctx.request_repaint_after(Duration::from_millis(1500)); }

        egui::Panel::bottom("zoro-actions")
            .frame(egui::Frame::new().fill(C_BG).inner_margin(egui::Margin::symmetric(24, 16)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let dot_color = if self.status_error { C_ERROR } else { mix(C_MUTED, C_ACCENT, flash) };
                    let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 3.5 + flash * 1.0, dot_color);
                    let text_color = if self.status_error { C_ERROR } else { mix(C_MUTED, C_TEXT, flash) };
                    ui.label(egui::RichText::new(&self.status).size(12.0).color(text_color));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if action_button(ui, "Save & Start", BtnKind::Primary, 130.0).clicked() { start_clicked = true; }
                        if action_button(ui, "Save", BtnKind::Secondary, 84.0).clicked() { save_clicked = true; }
                        if action_button(ui, "Reset", BtnKind::Ghost, 76.0).clicked() { reset_clicked = true; }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(C_BG).inner_margin(egui::Margin::symmetric(24, 8)))
            .show(ui, |ui| {
                ui.columns(2, |columns| {
                    egui::ScrollArea::vertical().id_salt("scroll-left").auto_shrink([false, false]).show(&mut columns[0], |ui| {
                        card(ui, |ui| self.draw_position_panel(ui));
                        ui.add_space(8.0);
                        card(ui, |ui| self.draw_display_panel(ui));
                    });
                    egui::ScrollArea::vertical().id_salt("scroll-right").auto_shrink([false, false]).show(&mut columns[1], |ui| {
                        card(ui, |ui| self.draw_preview_panel(ui));
                        ui.add_space(8.0);
                        card(ui, |ui| self.draw_appearance_panel(ui));
                    });
                });
            });

        if reset_clicked { self.reset_defaults(); }
        if save_clicked { self.save(); }
        if start_clicked && self.save() {
            match launch_overlay_process() {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error) => { self.status = error; self.status_error = true; }
            }
        }
    }
}

fn run_settings_gui() {
    let settings_path = settings_file_path();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Zoro FPS Overlay — Settings")
            .with_inner_size([900.0, 760.0])
            .with_min_inner_size([780.0, 640.0])
            .with_resizable(true),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "Zoro FPS Overlay",
        options,
        Box::new(move |cc| Ok(Box::new(SettingsApp::new(cc, settings_path)))),
    ) {
        message_box(&format!("Could not start the settings GUI:\n{}", error), "Zoro FPS Overlay");
    }
}

fn run_overlay() {
    let folder = current_exe_folder();
    let presentmon_path = folder.join("PresentMon.exe");
    if !presentmon_path.exists() {
        message_box(
            "PresentMon.exe was not found. Run scripts\\download-presentmon.ps1 or place the official PresentMon x64 console executable next to ZoroFPSOverlay.exe, named PresentMon.exe.",
            "Zoro FPS Overlay - setup needed",
        );
        return;
    }

    // Opening the settings app while the overlay is already running must not
    // create a second collector. The existing overlay reloads settings.ini.
    let legacy_class = wide("ZoroFpsOverlayWindow");
    if !unsafe { FindWindowW(legacy_class.as_ptr(), std::ptr::null()) }.is_null() {
        message_box("An older Zoro FPS Overlay is still running. Press F10 to close it, then start this version again.", "Zoro FPS Overlay");
        return;
    }
    let class_name = wide("ZoroFpsOverlayWindowV020");
    if !unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) }.is_null() {
        return;
    }

    let own_pid = unsafe { GetCurrentProcessId() };
    let diagnostic_path = std::env::temp_dir().join("zoro-fps-overlay-diagnostic.log");
    let presentmon_output_path = std::env::temp_dir().join("zoro-fps-overlay-presentmon.log");
    let settings_path = settings_file_path();
    let settings = load_settings(&settings_path);
    cleanup_stale_session_from_previous_run(&presentmon_path, &diagnostic_path, own_pid);
    let _ = fs::write(&diagnostic_path, "");
    let _ = fs::write(&presentmon_output_path, "");
    let _ = APP.set(Mutex::new(AppState {
        own_pid,
        presentmon_path,
        diagnostic_path,
        presentmon_output_path,
        settings_path,
        settings: settings.clone(),
        last_settings_check: Instant::now(),
        capture: None,
        fps_text: "-- FPS".to_string(),
        frametime_text: "-- ms".to_string(),
        low1_text: "1% low --".to_string(),
        visible: false,
        target_name: None,
        target_pid: None,
        user_hidden: false,
    }));

    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let cursor_id = 32512usize as LPCWSTR; // IDC_ARROW
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as UINT,
            style: 0x0002 | 0x0001, // CS_HREDRAW | CS_VREDRAW
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), cursor_id),
            hbrBackground: GetStockObject(4) as HBRUSH, // BLACK_BRUSH; black pixels are color-key transparent
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        if RegisterClassExW(&wc) == 0 {
            message_box("Could not register the overlay window.", "Zoro FPS Overlay");
            return;
        }

        let (overlay_x, overlay_y, overlay_width, overlay_height) = overlay_geometry(&settings);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            class_name.as_ptr(), wide("Zoro FPS Overlay").as_ptr(), WS_POPUP,
            overlay_x, overlay_y, overlay_width, overlay_height,
            std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            message_box("Could not create the overlay window.", "Zoro FPS Overlay");
            return;
        }
        SetLayeredWindowAttributes(hwnd, COLORKEY_BLACK, 255, LWA_COLORKEY);
        SetWindowPos(hwnd, (-1isize) as HWND, overlay_x, overlay_y, overlay_width, overlay_height, SWP_NOACTIVATE);
        // Start hidden. The first supported foreground application makes it visible.
        ShowWindow(hwnd, SW_HIDE);
        let _ = SetTimer(hwnd, TIMER_ID, Duration::from_millis(250).as_millis() as UINT, None);
        let _ = RegisterHotKey(hwnd, 1, MOD_NOREPEAT, VK_F8);  // toggle
        let _ = RegisterHotKey(hwnd, 2, MOD_NOREPEAT, VK_F10); // quit

        let mut msg: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if result <= 0 { break; }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // F10 posts WM_QUIT directly, so explicitly stop the helper process on exit too.
    if let Some(m) = APP.get() {
        if let Ok(mut state) = m.lock() { stop_capture(&mut state); }
    }
}

fn main() {
    // Fix asymmetric positioning under Windows display scaling. Must be called
    // before GetSystemMetrics/CreateWindowEx/egui create any windows.
    unsafe {
        let per_monitor_v2 = (-4isize) as HANDLE; // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        let _ = SetProcessDpiAwarenessContext(per_monitor_v2);
    }
    let is_overlay = std::env::args().skip(1).any(|arg| arg == "--overlay");
    if is_overlay {
        run_overlay();
    } else {
        run_settings_gui();
    }
}
