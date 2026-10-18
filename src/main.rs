#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case, dead_code)]

#[cfg(not(windows))]
compile_error!("Zoro FPS Overlay currently supports Windows only.");

use std::collections::{HashMap, VecDeque};
use std::ffi::{c_void, OsStr};
use std::fs::{self, OpenOptions};
use std::mem::{size_of, zeroed};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
// PDH (performance counters) used for the GPU usage reading.
const PDH_FMT_DOUBLE: u32 = 0x0000_0200;
const PDH_MORE_DATA: u32 = 0x8000_07D2;
// Marks "no reading available" in the shared system stats.
const STAT_NONE: u32 = u32::MAX;

// How long PresentMon keeps measuring an app after it loses focus, so a brief alt-tab or a
// notification does not restart the measurement.
const CAPTURE_GRACE: Duration = Duration::from_secs(8);
// A newly focused app must stay in front this long before PresentMon is started for it.
const FOREGROUND_DWELL: Duration = Duration::from_millis(700);
// "Games only": frames must arrive at this rate, for this long, before the overlay appears...
const AUTO_MIN_FPS: u32 = 10;
const AUTO_SHOW_AFTER: Duration = Duration::from_millis(1200);
// ...and once it is showing, it stays until frames have been missing this long.
const AUTO_HIDE_AFTER: Duration = Duration::from_secs(5);
const MAX_APP_RULES: usize = 64;
const RECENT_APPS_MAX: usize = 12;

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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FILETIME { low: u32, high: u32 }
#[repr(C)]
struct MEMORYSTATUSEX {
    dwLength: u32,
    dwMemoryLoad: u32,
    ullTotalPhys: u64,
    ullAvailPhys: u64,
    ullTotalPageFile: u64,
    ullAvailPageFile: u64,
    ullTotalVirtual: u64,
    ullAvailVirtual: u64,
    ullAvailExtendedVirtual: u64,
}
// PDH_FMT_COUNTERVALUE_ITEM_W with the double member of the value union.
#[repr(C)]
struct PdhCounterValueItem { name: *const u16, status: DWORD, double_value: f64 }

#[link(name = "pdh")]
extern "system" {
    fn PdhOpenQueryW(data_source: LPCWSTR, user_data: usize, query: *mut HANDLE) -> i32;
    fn PdhAddEnglishCounterW(query: HANDLE, counter_path: LPCWSTR, user_data: usize, counter: *mut HANDLE) -> i32;
    fn PdhCollectQueryData(query: HANDLE) -> i32;
    fn PdhGetFormattedCounterArrayW(counter: HANDLE, format: DWORD, buffer_size: *mut DWORD, item_count: *mut DWORD, items: *mut c_void) -> i32;
    fn PdhCloseQuery(query: HANDLE) -> i32;
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
    fn GetSystemTimes(idle: *mut FILETIME, kernel: *mut FILETIME, user: *mut FILETIME) -> BOOL;
    fn GlobalMemoryStatusEx(buffer: *mut MEMORYSTATUSEX) -> BOOL;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric { Fps, Frametime, Low1, Cpu, Ram, Gpu }

// Order in which enabled items appear on the overlay.
const ALL_METRICS: [Metric; 6] = [Metric::Fps, Metric::Frametime, Metric::Low1, Metric::Cpu, Metric::Ram, Metric::Gpu];
const FONT_FAMILIES: [&str; 7] = ["Segoe UI", "Consolas", "Arial", "Tahoma", "Verdana", "Bahnschrift", "Courier New"];
// Slightly lighter than pure black so it never matches the transparency color key.
const SHADOW_COLORREF: u32 = 0x0012_1212;

impl Metric {
    // Widest text this item can show; the overlay window and row cells are sized from it.
    fn sample(self, labels: bool) -> &'static str {
        match (self, labels) {
            (Metric::Fps, true) => "1000 FPS",
            (Metric::Fps, false) => "1000",
            (Metric::Frametime, _) => "1000.0 ms",
            (Metric::Low1, true) => "1% low 1000",
            (Metric::Low1, false) => "1000",
            (Metric::Cpu, true) => "CPU 100%",
            (Metric::Ram, true) => "RAM 100%",
            (Metric::Gpu, true) => "GPU 100%",
            (_, false) => "100%",
        }
    }
}

// Latest readings shown on the overlay; None means "no reading yet".
#[derive(Clone, Copy, Default)]
struct MetricValues {
    fps: Option<u32>,
    frametime_ms: Option<f64>,
    low1: Option<u32>,
    cpu: Option<u32>,
    ram: Option<u32>,
    gpu: Option<u32>,
}

fn percent_text(label: &str, value: Option<u32>, labels: bool) -> String {
    match (value, labels) {
        (Some(v), true) => format!("{} {}%", label, v),
        (Some(v), false) => format!("{}%", v),
        (None, true) => format!("{} --", label),
        (None, false) => "--".to_string(),
    }
}

// `note` is a short capture status ("ERR", "...") shown instead of a missing FPS value.
fn format_metric(metric: Metric, values: &MetricValues, note: &str, labels: bool) -> String {
    match metric {
        Metric::Fps => match values.fps {
            Some(v) if labels => format!("{} FPS", v),
            Some(v) => v.to_string(),
            None if !note.is_empty() => format!("FPS {}", note),
            None if labels => "-- FPS".to_string(),
            None => "--".to_string(),
        },
        Metric::Frametime => match values.frametime_ms {
            Some(v) => format!("{:.1} ms", v),
            None => "-- ms".to_string(),
        },
        Metric::Low1 => match values.low1 {
            Some(v) if labels => format!("1% low {}", v),
            Some(v) => v.to_string(),
            None if labels => "1% low --".to_string(),
            None => "--".to_string(),
        },
        Metric::Cpu => percent_text("CPU", values.cpu, labels),
        Metric::Ram => percent_text("RAM", values.ram, labels),
        Metric::Gpu => percent_text("GPU", values.gpu, labels),
    }
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
    // Which items the overlay shows.
    show_fps: bool,
    show_frametime: bool,
    show_low1: bool,
    show_cpu: bool,
    show_ram: bool,
    show_gpu: bool,
    // 0 = items stacked in a column, 1 = items side by side in a row.
    layout: u8,
    // Gap between stacked lines (column) and between items (row), in pixels.
    line_gap: i32,
    item_gap: i32,
    // "CPU 23%" instead of "23%".
    show_labels: bool,
    bold: bool,
    shadow: bool,
    font_family: String,
    // 0 = only apps that render frames like a game, 1 = every app in front.
    visibility: u8,
    // Process names (lowercase, e.g. "game.exe") that always / never get the overlay.
    always_apps: Vec<String>,
    never_apps: Vec<String>,
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
            show_cpu: false,
            show_ram: false,
            show_gpu: false,
            layout: 0,
            line_gap: 2,
            item_gap: 18,
            show_labels: true,
            bold: true,
            shadow: false,
            font_family: "Segoe UI".to_string(),
            visibility: 0,
            always_apps: Vec::new(),
            never_apps: Vec::new(),
        }
    }
}

fn make_overlay_font(settings: &OverlaySettings) -> HFONT {
    let font_name = wide(&settings.font_family);
    let weight = if settings.bold { 700 } else { 400 };
    unsafe { CreateFontW(settings.font_size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, font_name.as_ptr()) }
}

fn text_extent(hdc: HDC, text: &str) -> SIZE {
    let wide_text = wide(text);
    let mut size = SIZE { cx: 0, cy: 0 };
    unsafe {
        GetTextExtentPoint32W(hdc, wide_text.as_ptr(), (wide_text.len() - 1) as i32, &mut size);
    }
    size
}

// Draws one overlay item, with an optional soft shadow underneath.
fn draw_overlay_text(hdc: HDC, text: &str, rc: &mut RECT, align: u32, settings: &OverlaySettings) {
    let wide_text = wide(text);
    let count = wide_text.len().saturating_sub(1) as i32;
    let flags = align | DT_SINGLELINE | DT_NOPREFIX | DT_NOCLIP;
    unsafe {
        if settings.shadow {
            let o = settings.shadow_offset();
            let mut shadow_rc = RECT { left: rc.left + o, top: rc.top + o, right: rc.right + o, bottom: rc.bottom + o };
            SetTextColor(hdc, SHADOW_COLORREF);
            DrawTextW(hdc, wide_text.as_ptr(), count, &mut shadow_rc, flags);
        }
        SetTextColor(hdc, settings.color_ref());
        DrawTextW(hdc, wide_text.as_ptr(), count, rc, flags);
    }
}

impl OverlaySettings {
    fn is_enabled(&self, metric: Metric) -> bool {
        match metric {
            Metric::Fps => self.show_fps,
            Metric::Frametime => self.show_frametime,
            Metric::Low1 => self.show_low1,
            Metric::Cpu => self.show_cpu,
            Metric::Ram => self.show_ram,
            Metric::Gpu => self.show_gpu,
        }
    }

    // Enabled items in display order; the overlay always shows at least FPS.
    fn metrics(&self) -> Vec<Metric> {
        let enabled: Vec<Metric> = ALL_METRICS.iter().copied().filter(|m| self.is_enabled(*m)).collect();
        if enabled.is_empty() { vec![Metric::Fps] } else { enabled }
    }

    fn shadow_offset(&self) -> i32 {
        (self.font_size / 16).max(1)
    }

    fn normalize(&mut self) {
        self.horizontal_anchor = self.horizontal_anchor.min(2);
        self.vertical_anchor = self.vertical_anchor.min(2);
        self.offset_x = self.offset_x.clamp(-4000, 4000);
        self.offset_y = self.offset_y.clamp(-4000, 4000);
        self.font_size = self.font_size.clamp(18, 64);
        self.layout = self.layout.min(1);
        self.line_gap = self.line_gap.clamp(0, 40);
        self.item_gap = self.item_gap.clamp(0, 60);
        // Only fonts from the known list are accepted (the value ends up in CreateFontW).
        self.font_family = FONT_FAMILIES
            .iter()
            .find(|f| f.eq_ignore_ascii_case(self.font_family.trim()))
            .unwrap_or(&FONT_FAMILIES[0])
            .to_string();
        self.visibility = self.visibility.min(1);
        self.always_apps = clean_app_list(&self.always_apps);
        self.never_apps = clean_app_list(&self.never_apps);
        // A Hide rule wins if a name ended up in both lists.
        let never = self.never_apps.clone();
        self.always_apps.retain(|a| !never.contains(a));
        // The overlay always shows at least one item.
        if ALL_METRICS.iter().all(|m| !self.is_enabled(*m)) {
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

    // The window is exactly as large as the widest possible text of the enabled items.
    // A window wider than its text leaves a transparent strip that cannot go past the
    // screen edge, which would keep right-anchored text away from the monitor edge.
    fn window_size(&self) -> (i32, i32) {
        let samples: Vec<&str> = self.metrics().iter().map(|m| m.sample(self.show_labels)).collect();
        let (max_w, sum_w, line_h) = measure_overlay_lines(self, &samples);
        let count = samples.len() as i32;
        let (text_w, text_h) = if self.layout == 1 {
            (sum_w + self.item_gap * (count - 1), line_h)
        } else {
            (max_w, line_h * count + self.line_gap * (count - 1))
        };
        let shadow = if self.shadow { self.shadow_offset() } else { 0 };
        (text_w + OVERLAY_PAD_X * 2 + 2 + shadow, text_h + OVERLAY_PAD_Y * 2 + shadow)
    }
}

// Returns (widest sample, sum of all sample widths, line height).
fn measure_overlay_lines(settings: &OverlaySettings, samples: &[&str]) -> (i32, i32, i32) {
    let fallback = (
        settings.font_size * 5,
        settings.font_size * 5 * samples.len() as i32,
        settings.font_size,
    );
    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        if hdc.is_null() { return fallback; }
        let font = make_overlay_font(settings);
        let previous = if !font.is_null() { SelectObject(hdc, font) } else { std::ptr::null_mut() };
        let mut max_w = 0;
        let mut sum_w = 0;
        for sample in samples {
            let width = text_extent(hdc, sample).cx;
            max_w = max_w.max(width);
            sum_w += width;
        }
        let line_h = text_extent(hdc, "Ag").cy.max(settings.font_size);
        if !previous.is_null() { SelectObject(hdc, previous); }
        if !font.is_null() { DeleteObject(font); }
        ReleaseDC(std::ptr::null_mut(), hdc);
        if max_w > 0 { (max_w, sum_w, line_h) } else { fallback }
    }
}

// Lowercase process name such as "game.exe". Accepts a pasted path and adds ".exe" when
// the name has no extension. Rejects characters that would break the settings file.
fn normalize_app_name(value: &str) -> Option<String> {
    let leaf = value.trim().trim_matches('"').rsplit(['\\', '/']).next()?.trim().to_ascii_lowercase();
    if leaf.is_empty() || leaf.len() > 64 { return None; }
    if leaf.chars().any(|c| c.is_control() || matches!(c, ',' | '=' | '"' | '<' | '>' | '|' | '*' | '?' | ':')) {
        return None;
    }
    if leaf.contains('.') { Some(leaf) } else { Some(format!("{}.exe", leaf)) }
}

fn clean_app_list(list: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = Vec::new();
    for item in list {
        if let Some(name) = normalize_app_name(item) {
            if !cleaned.contains(&name) && cleaned.len() < MAX_APP_RULES {
                cleaned.push(name);
            }
        }
    }
    cleaned
}

fn app_list_contains(list: &[String], name: &str) -> bool {
    list.iter().any(|a| a.eq_ignore_ascii_case(name))
}

// Everyday programs that draw frames but are not games: browsers, media players, chat apps,
// editors, terminals, office tools and game launchers. "Games only" mode skips them, and a
// Show rule in the settings overrides this list.
fn is_known_non_game(name: &str) -> bool {
    const NON_GAMES: &[&str] = &[
        "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe",
        "iexplore.exe", "arc.exe", "msedgewebview2.exe",
        "vlc.exe", "mpv.exe", "mpc-hc.exe", "mpc-hc64.exe", "mpc-be.exe", "mpc-be64.exe",
        "potplayer.exe", "potplayermini.exe", "potplayermini64.exe", "wmplayer.exe", "spotify.exe",
        "discord.exe", "telegram.exe", "slack.exe", "teams.exe", "ms-teams.exe", "whatsapp.exe",
        "zoom.exe", "skype.exe",
        "code.exe", "cursor.exe", "devenv.exe", "idea64.exe", "pycharm64.exe", "rider64.exe",
        "clion64.exe", "webstorm64.exe", "notepad++.exe", "sublime_text.exe",
        "windowsterminal.exe", "wt.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "conhost.exe",
        "notepad.exe", "mspaint.exe", "winword.exe", "excel.exe", "powerpnt.exe", "outlook.exe",
        "onenote.exe", "acrobat.exe", "acrord32.exe", "obs64.exe",
        "steam.exe", "epicgameslauncher.exe", "galaxyclient.exe", "battle.net.exe", "origin.exe",
        "eadesktop.exe", "ubisoftconnect.exe", "upc.exe",
    ];
    NON_GAMES.iter().any(|x| name.eq_ignore_ascii_case(x))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppClass {
    // Never gets the overlay.
    Never,
    // Always gets the overlay while in front.
    Always,
    // Gets it according to the visibility mode.
    Candidate,
}

fn classify_app(settings: &OverlaySettings, name: &str) -> AppClass {
    if app_list_contains(&settings.never_apps, name) { return AppClass::Never; }
    if app_list_contains(&settings.always_apps, name) { return AppClass::Always; }
    if settings.visibility == 0 && is_known_non_game(name) { return AppClass::Never; }
    AppClass::Candidate
}

// Apps the overlay has seen rendering frames, newest first. The settings window reads this
// file to offer them as a list, so nobody has to type process names.
fn recent_apps_path() -> PathBuf {
    settings_file_path().with_file_name("recent_apps.txt")
}

fn save_recent_apps(path: &PathBuf, apps: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, apps.join("\n"));
}

fn load_recent_apps(path: &PathBuf) -> Vec<String> {
    let Ok(contents) = fs::read_to_string(path) else { return Vec::new(); };
    let lines: Vec<String> = contents.lines().map(str::to_string).collect();
    let mut apps = clean_app_list(&lines);
    apps.truncate(RECENT_APPS_MAX);
    apps
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
            "show_cpu" => result.show_cpu = value == "1",
            "show_ram" => result.show_ram = value == "1",
            "show_gpu" => result.show_gpu = value == "1",
            "layout" => if let Ok(v) = value.parse::<u8>() { result.layout = v; },
            "line_gap" => if let Ok(v) = value.parse::<i32>() { result.line_gap = v; },
            "item_gap" => if let Ok(v) = value.parse::<i32>() { result.item_gap = v; },
            "show_labels" => result.show_labels = value == "1",
            "bold" => result.bold = value == "1",
            "shadow" => result.shadow = value == "1",
            "font_family" => result.font_family = value.to_string(),
            "visibility" => if let Ok(v) = value.parse::<u8>() { result.visibility = v; },
            "always_apps" => result.always_apps = value.split(',').map(str::to_string).collect(),
            "never_apps" => result.never_apps = value.split(',').map(str::to_string).collect(),
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
        "horizontal_anchor={}\nvertical_anchor={}\noffset_x={}\noffset_y={}\nfont_size={}\ncolor={}\n\
         show_fps={}\nshow_frametime={}\nshow_low1={}\nshow_cpu={}\nshow_ram={}\nshow_gpu={}\n\
         layout={}\nline_gap={}\nitem_gap={}\nshow_labels={}\nbold={}\nshadow={}\nfont_family={}\n\
         visibility={}\nalways_apps={}\nnever_apps={}\n",
        settings.horizontal_anchor, settings.vertical_anchor, settings.offset_x,
        settings.offset_y, settings.font_size, settings.color_hex,
        settings.show_fps as u8, settings.show_frametime as u8, settings.show_low1 as u8,
        settings.show_cpu as u8, settings.show_ram as u8, settings.show_gpu as u8,
        settings.layout, settings.line_gap, settings.item_gap,
        settings.show_labels as u8, settings.bold as u8, settings.shadow as u8,
        settings.font_family, settings.visibility,
        settings.always_apps.join(","), settings.never_apps.join(",")
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
    values: MetricValues,
    // Short capture status ("ERR", "...") shown in place of a missing FPS value.
    fps_note: &'static str,
    visible: bool,
    target_name: Option<String>,
    target_pid: Option<u32>,
    user_hidden: bool,
    // Whether the rules want the overlay visible right now (before the F8 override).
    wanted: bool,
    // The app that has been in front, with its pid and since when.
    foreground_seen: Option<(String, u32, Instant)>,
    capture_last_focus: Instant,
    // "Games only" tracking for the measured app.
    rate_since: Option<Instant>,
    confirmed: bool,
    last_flowing: Option<Instant>,
    recorded_current: bool,
    recent_apps: Vec<String>,
    recent_path: PathBuf,
}

static APP: OnceLock<Mutex<AppState>> = OnceLock::new();

impl AppState {
    // Puts an app at the top of the recently seen list that the settings window shows.
    fn remember_recent_app(&mut self, name: &str) {
        let Some(name) = normalize_app_name(name) else { return; };
        // Re-read the file first so apps removed from the settings window stay removed.
        self.recent_apps = load_recent_apps(&self.recent_path);
        self.recent_apps.retain(|a| *a != name);
        self.recent_apps.insert(0, name);
        self.recent_apps.truncate(RECENT_APPS_MAX);
        save_recent_apps(&self.recent_path, &self.recent_apps);
    }

    // Forgets the PresentMon readings (system readings are kept).
    fn clear_capture_values(&mut self, note: &'static str) {
        self.values.fps = None;
        self.values.frametime_ms = None;
        self.values.low1 = None;
        self.fps_note = note;
    }
}

// CPU / RAM / GPU readings, refreshed once a second by a background thread.
struct SysStats {
    cpu: AtomicU32,
    ram: AtomicU32,
    gpu: AtomicU32,
    // The GPU counters are only queried while the GPU item is enabled.
    want_gpu: AtomicBool,
}

impl SysStats {
    fn new() -> Self {
        Self {
            cpu: AtomicU32::new(STAT_NONE),
            ram: AtomicU32::new(STAT_NONE),
            gpu: AtomicU32::new(STAT_NONE),
            want_gpu: AtomicBool::new(false),
        }
    }
}

static SYS: OnceLock<Arc<SysStats>> = OnceLock::new();

fn stat_value(raw: u32) -> Option<u32> {
    if raw == STAT_NONE { None } else { Some(raw) }
}

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
    state.clear_capture_values("");
}

fn start_capture(state: &mut AppState, process_name: &str, process_id: u32) {
    stop_capture(state);
    state.target_name = Some(process_name.to_string());
    state.target_pid = Some(process_id);
    state.rate_since = None;
    state.confirmed = false;
    state.last_flowing = None;
    state.recorded_current = false;
    state.capture_last_focus = Instant::now();

    log_event(&state.diagnostic_path, &format!(
        "Starting capture: process={} pid={} presentmon={}",
        process_name, process_id, state.presentmon_path.display()
    ));
    if !state.presentmon_path.exists() {
        log_event(&state.diagnostic_path, "ERROR: PresentMon.exe not found next to overlay executable");
        state.clear_capture_values("ERR");
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
            state.clear_capture_values("ERR");
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
            state.clear_capture_values("...");
            log_event(&state.diagnostic_path, "PresentMon child process spawned; reading CSV from stdout");
        }
        Err(e) => {
            state.clear_capture_values("ERR");
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

fn filetime_u64(ft: &FILETIME) -> u64 {
    ((ft.high as u64) << 32) | ft.low as u64
}

fn wide_ptr_to_string(ptr: *const u16) -> String {
    let mut units: Vec<u16> = Vec::new();
    if ptr.is_null() { return String::new(); }
    unsafe {
        let mut i = 0usize;
        while i < 512 {
            let unit = *ptr.add(i);
            if unit == 0 { break; }
            units.push(unit);
            i += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

// Reads GPU utilization from the Windows "GPU Engine" performance counters (the same
// source Task Manager uses). It works for NVIDIA, AMD and Intel GPUs on Windows 10+.
struct GpuSampler {
    query: HANDLE,
    counter: HANDLE,
}

impl GpuSampler {
    fn new() -> Result<Self, i32> {
        unsafe {
            let mut query: HANDLE = std::ptr::null_mut();
            let status = PdhOpenQueryW(std::ptr::null(), 0, &mut query);
            if status != 0 { return Err(status); }
            let path = wide("\\GPU Engine(*)\\Utilization Percentage");
            let mut counter: HANDLE = std::ptr::null_mut();
            let status = PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter);
            if status != 0 {
                PdhCloseQuery(query);
                return Err(status);
            }
            // The first collection only establishes a baseline.
            let _ = PdhCollectQueryData(query);
            Ok(Self { query, counter })
        }
    }

    // Busiest engine (3D, compute, copy, video...) of the busiest GPU, in percent.
    fn sample(&mut self) -> Option<u32> {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 { return None; }
            let mut size: DWORD = 0;
            let mut count: DWORD = 0;
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut());
            if status as u32 != PDH_MORE_DATA || size == 0 { return None; }
            // u64 elements keep the buffer 8-byte aligned for PdhCounterValueItem.
            let mut buffer: Vec<u64> = vec![0; (size as usize + 7) / 8 + 1];
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, buffer.as_mut_ptr() as *mut c_void);
            if status != 0 { return None; }

            let items = buffer.as_ptr() as *const PdhCounterValueItem;
            let mut totals: HashMap<(String, String), f64> = HashMap::new();
            for i in 0..count as usize {
                let item = &*items.add(i);
                // PDH_CSTATUS_VALID_DATA = 0, PDH_CSTATUS_NEW_DATA = 1
                if item.status != 0 && item.status != 1 { continue; }
                let name = wide_ptr_to_string(item.name);
                let luid = name.split("luid_").nth(1)
                    .and_then(|rest| rest.split("_phys").next())
                    .unwrap_or("")
                    .to_string();
                let engine = name.rsplit("engtype_").next().unwrap_or("").to_string();
                *totals.entry((luid, engine)).or_insert(0.0) += item.double_value;
            }
            let busiest = totals.values().copied().fold(0.0_f64, f64::max);
            Some(busiest.round().clamp(0.0, 100.0) as u32)
        }
    }
}

impl Drop for GpuSampler {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query); }
    }
}

fn spawn_system_stats_thread(stats: Arc<SysStats>, log_path: PathBuf) {
    std::thread::spawn(move || {
        let mut previous_cpu: Option<(u64, u64)> = None; // (idle, total) system times
        let mut gpu_sampler: Option<GpuSampler> = None;
        let mut gpu_error_logged = false;
        let mut gpu_samples_taken = 0u32;
        let mut gpu_misses = 0u32;

        loop {
            unsafe {
                let mut idle = FILETIME::default();
                let mut kernel = FILETIME::default();
                let mut user = FILETIME::default();
                if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
                    let idle_now = filetime_u64(&idle);
                    // Kernel time already includes idle time.
                    let total_now = filetime_u64(&kernel) + filetime_u64(&user);
                    if let Some((idle_before, total_before)) = previous_cpu {
                        let total_delta = total_now.saturating_sub(total_before);
                        let idle_delta = idle_now.saturating_sub(idle_before).min(total_delta);
                        if total_delta > 0 {
                            let busy = (total_delta - idle_delta) as f64 / total_delta as f64 * 100.0;
                            stats.cpu.store(busy.round().clamp(0.0, 100.0) as u32, Ordering::Relaxed);
                        }
                    }
                    previous_cpu = Some((idle_now, total_now));
                }

                let mut memory: MEMORYSTATUSEX = zeroed();
                memory.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
                if GlobalMemoryStatusEx(&mut memory) != 0 {
                    stats.ram.store(memory.dwMemoryLoad.min(100), Ordering::Relaxed);
                }
            }

            if stats.want_gpu.load(Ordering::Relaxed) {
                let mut just_created = false;
                if gpu_sampler.is_none() {
                    match GpuSampler::new() {
                        Ok(sampler) => {
                            gpu_sampler = Some(sampler);
                            gpu_samples_taken = 0;
                            just_created = true;
                        }
                        Err(status) => {
                            if !gpu_error_logged {
                                log_event(&log_path, &format!("GPU usage counters unavailable (PDH status 0x{:08X})", status as u32));
                                gpu_error_logged = true;
                            }
                            stats.gpu.store(STAT_NONE, Ordering::Relaxed);
                        }
                    }
                }
                // A sample needs a baseline from an earlier collection, so a new query waits one tick.
                if !just_created {
                    if let Some(sampler) = gpu_sampler.as_mut() {
                        match sampler.sample() {
                            Some(value) => {
                                stats.gpu.store(value, Ordering::Relaxed);
                                gpu_misses = 0;
                            }
                            None => {
                                gpu_misses += 1;
                                if gpu_misses >= 4 { stats.gpu.store(STAT_NONE, Ordering::Relaxed); }
                            }
                        }
                        gpu_samples_taken += 1;
                    }
                }
                // Wildcard counters are expanded when they are added, so rebuild the query now
                // and then to pick up the GPU engines of programs started in the meantime.
                if gpu_samples_taken >= 30 { gpu_sampler = None; }
            } else if gpu_sampler.is_some() {
                gpu_sampler = None;
                stats.gpu.store(STAT_NONE, Ordering::Relaxed);
            }

            std::thread::sleep(Duration::from_millis(1000));
        }
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
    if let Some(sys) = SYS.get() {
        sys.want_gpu.store(state.settings.show_gpu, Ordering::Relaxed);
        state.values.cpu = stat_value(sys.cpu.load(Ordering::Relaxed));
        state.values.ram = stat_value(sys.ram.load(Ordering::Relaxed));
        state.values.gpu = stat_value(sys.gpu.load(Ordering::Relaxed));
    }
    let now = Instant::now();
    // The app in front, unless it is the desktop, a system window or this program.
    let foreground = foreground_name.as_deref().filter(|n| !is_system_or_shell_process(n));
    let class = foreground.map(|n| classify_app(&state.settings, n));
    let measurable = matches!(class, Some(AppClass::Always) | Some(AppClass::Candidate));

    // A new app must stay in front briefly before it is measured, so alt-tabbing through
    // windows or short-lived helper windows do not restart PresentMon over and over.
    let mut settled = false;
    match foreground {
        Some(name) if measurable => {
            let same = matches!(&state.foreground_seen, Some((n, p, _)) if n == name && *p == pid);
            if !same { state.foreground_seen = Some((name.to_string(), pid, now)); }
            let since = state.foreground_seen.as_ref().map(|(_, _, t)| *t).unwrap_or(now);
            let already_target = state.target_name.as_deref() == Some(name) && state.target_pid == Some(pid);
            settled = already_target || now.duration_since(since) >= FOREGROUND_DWELL;
            if settled {
                if !already_target { start_capture(&mut state, name, pid); }
                state.capture_last_focus = now;
            }
        }
        _ => { state.foreground_seen = None; }
    }

    // Keep measuring for a while after focus moves elsewhere, then stop.
    let capture_exists = state.capture.is_some() || state.target_name.is_some();
    if capture_exists && !settled && now.duration_since(state.capture_last_focus) > CAPTURE_GRACE {
        stop_capture(&mut state);
    }

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
        state.clear_capture_values("ERR");
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
            let now_ms = epoch_millis();
            if fps > 0 && last_sample > 0 && now_ms.saturating_sub(last_sample) < 5000 {
                state.values.fps = Some(fps);
                state.values.frametime_ms = Some(frametime_x100 as f64 / 100.0);
                state.values.low1 = if low1 > 0 { Some(low1) } else { None };
                state.fps_note = "";
            } else {
                state.clear_capture_values("...");
            }
        }
    }

    // "Games only": is the app in front rendering frames steadily, like a game? Once that has
    // held for a moment it is confirmed, and it stays confirmed until frames stop for a while,
    // so a loading screen or a low-FPS scene never hides the overlay.
    if settled {
        let flow = state.capture.as_ref().map(|capture| (
            capture.stats.fps.load(Ordering::Relaxed),
            epoch_millis().saturating_sub(capture.stats.last_sample_ms.load(Ordering::Relaxed)),
            capture.stats.last_sample_ms.load(Ordering::Relaxed),
        ));
        let (fps, age_ms, last_sample) = flow.unwrap_or((0, u64::MAX, 0));
        let flowing = fps > 0 && last_sample > 0 && age_ms < 4000;
        let game_rate = flowing && fps >= AUTO_MIN_FPS && age_ms < 1500;
        if flowing { state.last_flowing = Some(now); }
        if state.confirmed {
            let quiet = state.last_flowing.map_or(true, |t| now.duration_since(t) > AUTO_HIDE_AFTER);
            if quiet {
                state.confirmed = false;
                state.rate_since = None;
            }
        } else if game_rate {
            let start = *state.rate_since.get_or_insert(now);
            if now.duration_since(start) >= AUTO_SHOW_AFTER { state.confirmed = true; }
        } else {
            state.rate_since = None;
        }
        if state.confirmed && !state.recorded_current {
            state.recorded_current = true;
            if let Some(name) = foreground { state.remember_recent_app(name); }
        }
    }

    let wanted = match class {
        None | Some(AppClass::Never) => false,
        Some(AppClass::Always) => settled,
        Some(AppClass::Candidate) => settled && (state.settings.visibility == 1 || state.confirmed),
    };
    state.wanted = wanted;
    let show = wanted && !state.user_hidden;
    if show && !state.visible {
        unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE); }
        state.visible = true;
    } else if !show && state.visible {
        unsafe { ShowWindow(hwnd, SW_HIDE); }
        state.visible = false;
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
                let (texts, samples, settings) = APP.get().and_then(|m| m.lock().ok())
                    .map(|s| {
                        let metrics = s.settings.metrics();
                        let texts: Vec<String> = metrics.iter()
                            .map(|m| format_metric(*m, &s.values, s.fps_note, s.settings.show_labels))
                            .collect();
                        let samples: Vec<&'static str> = metrics.iter()
                            .map(|m| m.sample(s.settings.show_labels))
                            .collect();
                        (texts, samples, s.settings.clone())
                    })
                    .unwrap_or_else(|| (vec!["-- FPS".to_string()], vec!["1000 FPS"], OverlaySettings::default()));
                let font = make_overlay_font(&settings);
                let previous = if !font.is_null() { SelectObject(hdc, font) } else { std::ptr::null_mut() };
                SetBkMode(hdc, 1); // transparent text background

                // Text is aligned by the chosen anchor, so a right-anchored overlay
                // hugs the right edge of the monitor.
                let mut client: RECT = zeroed();
                GetClientRect(hwnd, &mut client);
                let align = match settings.horizontal_anchor {
                    1 => DT_CENTER,
                    2 => DT_RIGHT,
                    _ => DT_LEFT,
                };
                // Same measuring rules as window_size, so the lines always fit the window.
                let line_h = text_extent(hdc, "Ag").cy.max(settings.font_size);
                let shadow_off = if settings.shadow { settings.shadow_offset() } else { 0 };
                // Row layout: each item gets a fixed cell sized for its widest value,
                // so items do not jump around when a number gains a digit.
                let cell_widths: Vec<i32> = samples.iter().map(|s| text_extent(hdc, s).cx).collect();
                let row_total: i32 = cell_widths.iter().sum::<i32>()
                    + settings.item_gap * (cell_widths.len() as i32 - 1).max(0);
                let mut x_cursor = match settings.horizontal_anchor {
                    1 => client.left + ((client.right - client.left) - row_total) / 2,
                    2 => client.right - OVERLAY_PAD_X - shadow_off - row_total,
                    _ => client.left + OVERLAY_PAD_X,
                };
                for (index, line) in texts.iter().enumerate() {
                    let mut rc = if settings.layout == 1 {
                        let cell_w = cell_widths.get(index).copied().unwrap_or(0);
                        let cell = RECT {
                            left: x_cursor,
                            top: OVERLAY_PAD_Y,
                            right: x_cursor + cell_w,
                            bottom: OVERLAY_PAD_Y + line_h,
                        };
                        x_cursor += cell_w + settings.item_gap;
                        cell
                    } else {
                        let top = OVERLAY_PAD_Y + index as i32 * (line_h + settings.line_gap);
                        RECT {
                            left: client.left + OVERLAY_PAD_X,
                            top,
                            right: client.right - OVERLAY_PAD_X - shadow_off,
                            bottom: top + line_h,
                        }
                    };
                    draw_overlay_text(hdc, line, &mut rc, align, &settings);
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
                        // Hiding always works; showing only when the rules want the overlay now.
                        state.visible = state.wanted && !state.user_hidden;
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

// A selectable pill, used for single-choice options such as layout and font.
fn chip(ui: &mut egui::Ui, label: &str, selected: bool, min_width: f32) -> egui::Response {
    let font = egui::FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), C_TEXT);
    let width = (galley.size().x + 26.0).max(min_width);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let ctx = ui.ctx().clone();
    let hov = ctx.animate_bool_with_time(response.id.with("hov"), response.hovered(), 0.12);
    let sel = ctx.animate_bool_with_time(response.id.with("sel"), selected, 0.18);
    // Selected chips are light, so their label switches to the dark on-accent color.
    let fill = mix(mix(C_SURFACE, C_RAISED, hov), C_ACCENT, sel);
    let border = mix(mix(C_BORDER, C_BORDER_HI, hov), C_ACCENT, sel);
    let text = mix(mix(C_MUTED, C_TEXT, hov), C_ON_ACCENT, sel);

    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(9), fill);
    painter.rect_stroke(rect, egui::CornerRadius::same(9), egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, font, text);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// Size of a block of galleys laid out as a column or as a row.
fn block_size<G: std::ops::Deref<Target = egui::Galley>>(galleys: &[G], row: bool, line_gap: f32, item_gap: f32) -> egui::Vec2 {
    let gaps = galleys.len().saturating_sub(1) as f32;
    if row {
        egui::vec2(
            galleys.iter().map(|g| g.size().x).sum::<f32>() + item_gap * gaps,
            galleys.iter().map(|g| g.size().y).fold(0.0_f32, f32::max),
        )
    } else {
        egui::vec2(
            galleys.iter().map(|g| g.size().x).fold(0.0_f32, f32::max),
            galleys.iter().map(|g| g.size().y).sum::<f32>() + line_gap * gaps,
        )
    }
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
    recent_apps: Vec<String>,
    recent_checked: Instant,
    app_edit: String,
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
            recent_apps: load_recent_apps(&recent_apps_path()),
            recent_checked: Instant::now(),
            app_edit: String::new(),
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
        card_title(ui, "Preview", "Illustrative values. The real font is applied on the overlay.");
        ui.add_space(12.0);

        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(260.0), 150.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        // A mid-dark backdrop, so both the text and its shadow are visible.
        painter.rect_filled(rect, egui::CornerRadius::same(10), egui::Color32::from_rgb(30, 32, 36));
        painter.rect_stroke(rect, egui::CornerRadius::same(10), egui::Stroke::new(1.0, C_BORDER), egui::StrokeKind::Inside);

        let scale = 0.48_f32;
        let color = color_from_hex(&self.settings.color_hex);
        let shadow_color = egui::Color32::from_rgb(10, 10, 10);
        let sample = MetricValues {
            fps: Some(144),
            frametime_ms: Some(6.9),
            low1: Some(98),
            cpu: Some(23),
            ram: Some(54),
            gpu: Some(41),
        };
        let texts: Vec<String> = self.settings.metrics().iter()
            .map(|m| format_metric(*m, &sample, "", self.settings.show_labels))
            .collect();
        let row = self.settings.layout == 1;
        let line_gap = self.settings.line_gap as f32 * scale;
        let item_gap = self.settings.item_gap as f32 * scale;

        let layout = |px: f32, tint: egui::Color32| {
            texts.iter()
                .map(|t| painter.layout_no_wrap(t.clone(), egui::FontId::proportional(px), tint))
                .collect::<Vec<_>>()
        };
        let mut px = (self.settings.font_size as f32 * scale).clamp(11.0, 30.0);
        let mut galleys = layout(px, color);
        let mut size = block_size(&galleys, row, line_gap, item_gap);
        // Long rows shrink to fit the preview instead of running off its edge.
        let available = rect.width() - 24.0;
        if size.x > available && size.x > 0.0 {
            px = (px * available / size.x).max(8.0);
            galleys = layout(px, color);
            size = block_size(&galleys, row, line_gap, item_gap);
        }
        let shadow_galleys = if self.settings.shadow { layout(px, shadow_color) } else { Vec::new() };
        let shadow_px = (self.settings.shadow_offset() as f32 * scale * 1.6).max(1.0);

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

        let mut cursor_x = x;
        let mut cursor_y = y;
        for (index, galley) in galleys.into_iter().enumerate() {
            let item_w = galley.size().x;
            let item_h = galley.size().y;
            let pos = if row {
                egui::pos2(cursor_x, y + (size.y - item_h) / 2.0)
            } else {
                let line_x = match self.settings.horizontal_anchor {
                    0 => 0.0,
                    1 => (size.x - item_w) / 2.0,
                    _ => size.x - item_w,
                };
                egui::pos2(x + line_x, cursor_y)
            };
            if let Some(shadow) = shadow_galleys.get(index) {
                painter.galley(pos + egui::vec2(shadow_px, shadow_px), shadow.clone(), shadow_color);
            }
            painter.galley(pos, galley, color);
            cursor_x += item_w + item_gap;
            cursor_y += item_h + line_gap;
        }
    }

    fn draw_appearance_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Appearance", "Font, size and color of the overlay text.");
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
        ui.label(egui::RichText::new("FONT").size(10.5).strong().color(C_MUTED));
        ui.horizontal_wrapped(|ui| {
            for family in FONT_FAMILIES {
                if chip(ui, family, self.settings.font_family == family, 0.0).clicked() {
                    self.settings.font_family = family.to_string();
                }
            }
        });
        ui.label(egui::RichText::new("Must be installed on this PC. Consolas keeps digits evenly spaced.").size(11.5).color(C_MUTED));

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

        ui.add_space(6.0);
        toggle_row(ui, "Bold text", "Heavier weight reads better over busy scenes", &mut self.settings.bold);
        toggle_row(ui, "Text shadow", "A soft dark shadow behind the text", &mut self.settings.shadow);
    }

    // 0 = automatic, 1 = always show, 2 = never show.
    fn rule_of(&self, name: &str) -> u8 {
        if app_list_contains(&self.settings.always_apps, name) { 1 }
        else if app_list_contains(&self.settings.never_apps, name) { 2 }
        else { 0 }
    }

    fn set_rule(&mut self, name: &str, rule: u8) {
        self.settings.always_apps.retain(|a| !a.eq_ignore_ascii_case(name));
        self.settings.never_apps.retain(|a| !a.eq_ignore_ascii_case(name));
        match rule {
            1 => self.settings.always_apps.push(name.to_string()),
            2 => self.settings.never_apps.push(name.to_string()),
            _ => {}
        }
    }

    fn draw_visibility_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "When to show", "Decide which apps get the overlay.");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if chip(ui, "Games only", self.settings.visibility == 0, 120.0).clicked() { self.settings.visibility = 0; }
            if chip(ui, "All apps", self.settings.visibility == 1, 120.0).clicked() { self.settings.visibility = 1; }
        });
        let hint = if self.settings.visibility == 0 {
            "Appears once the focused app has been rendering frames like a game for a moment. Browsers, media players, chat apps, editors and launchers are skipped."
        } else {
            "Appears on every app that renders frames."
        };
        ui.label(egui::RichText::new(hint).size(11.5).color(C_MUTED));

        // The overlay keeps a list of apps it has seen; refresh it while this window is open.
        if self.recent_checked.elapsed() >= Duration::from_millis(1500) {
            self.recent_checked = Instant::now();
            self.recent_apps = load_recent_apps(&recent_apps_path());
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1500));

        let mut names: Vec<String> = self.recent_apps.clone();
        for name in self.settings.always_apps.iter().chain(self.settings.never_apps.iter()) {
            if !names.contains(name) { names.push(name.clone()); }
        }

        ui.add_space(12.0);
        ui.label(egui::RichText::new("APPS").size(10.5).strong().color(C_MUTED));
        if names.is_empty() {
            ui.label(egui::RichText::new("Apps show up here once the overlay has seen them rendering frames.").size(11.5).color(C_MUTED));
        }
        let mut change: Option<(String, u8)> = None;
        let mut remove: Option<String> = None;
        for name in &names {
            let rule = self.rule_of(name);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name.as_str()).color(C_TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if chip(ui, "\u{d7}", false, 32.0).on_hover_text("Remove from list").clicked() { remove = Some(name.clone()); }
                    if chip(ui, "Hide", rule == 2, 0.0).clicked() { change = Some((name.clone(), 2)); }
                    if chip(ui, "Show", rule == 1, 0.0).clicked() { change = Some((name.clone(), 1)); }
                    if chip(ui, "Auto", rule == 0, 0.0).clicked() { change = Some((name.clone(), 0)); }
                });
            });
        }
        if let Some((name, rule)) = change { self.set_rule(&name, rule); }
        if let Some(name) = remove {
            // Forget the app: drop its rules and take it off the recently seen list.
            self.set_rule(&name, 0);
            self.recent_apps.retain(|a| *a != name);
            save_recent_apps(&recent_apps_path(), &self.recent_apps);
            self.recent_checked = Instant::now();
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.app_edit)
                    .hint_text("game.exe")
                    .desired_width(150.0),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if action_button(ui, "Add & show", BtnKind::Secondary, 110.0).clicked() || enter {
                if let Some(name) = normalize_app_name(&self.app_edit) {
                    let total = self.settings.always_apps.len() + self.settings.never_apps.len();
                    if total < MAX_APP_RULES { self.set_rule(&name, 1); }
                    self.app_edit.clear();
                }
            }
        });
        ui.label(egui::RichText::new("Show: always. Hide: never. Auto: let the overlay decide. \u{d7}: remove from the list (save to apply rule changes).").size(11.5).color(C_MUTED));
    }

    fn draw_display_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Overlay items", "Switch each reading on or off.");
        ui.add_space(8.0);
        toggle_row(ui, "FPS", "Average of the last 30 frames", &mut self.settings.show_fps);
        toggle_row(ui, "Frametime", "Milliseconds per frame", &mut self.settings.show_frametime);
        toggle_row(ui, "1% low", "Slowest 1% of the last ~600 frames", &mut self.settings.show_low1);
        toggle_row(ui, "CPU usage", "Total processor load", &mut self.settings.show_cpu);
        toggle_row(ui, "RAM usage", "Physical memory in use", &mut self.settings.show_ram);
        toggle_row(ui, "GPU usage", "Busiest GPU engine, from Windows counters", &mut self.settings.show_gpu);
        // At least one item must stay on; a toggle that would turn all off snaps back.
        if ALL_METRICS.iter().all(|m| !self.settings.is_enabled(*m)) {
            self.settings.show_fps = true;
        }
    }

    fn draw_layout_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Layout", "Stack items in a column or line them up in a row.");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if chip(ui, "Column", self.settings.layout == 0, 104.0).clicked() { self.settings.layout = 0; }
            if chip(ui, "Row", self.settings.layout == 1, 104.0).clicked() { self.settings.layout = 1; }
        });

        ui.add_space(6.0);
        let row = self.settings.layout == 1;
        let (label, spacing, max) = if row {
            ("Item spacing", &mut self.settings.item_gap, 60_i32)
        } else {
            ("Line spacing", &mut self.settings.line_gap, 40_i32)
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(C_TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{} px", *spacing)).strong().color(C_TEXT));
            });
        });
        ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(120.0);
        ui.add(egui::Slider::new(spacing, 0..=max).show_value(false));

        ui.add_space(6.0);
        toggle_row(ui, "Show labels", "\"CPU 23%\" instead of just \"23%\"", &mut self.settings.show_labels);
    }

    fn reset_defaults(&mut self) {
        // Per-app rules are kept: they are choices about games, not about looks.
        let always_apps = self.settings.always_apps.clone();
        let never_apps = self.settings.never_apps.clone();
        self.settings = OverlaySettings::default();
        self.settings.always_apps = always_apps;
        self.settings.never_apps = never_apps;
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
                    ui.label(egui::RichText::new("v0.6.1").size(11.0).color(C_MUTED));
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
                    // The preview stays pinned while the settings below it scroll.
                    card(&mut columns[1], |ui| self.draw_preview_panel(ui));
                    columns[1].add_space(8.0);
                    egui::ScrollArea::vertical().id_salt("scroll-right").auto_shrink([false, false]).show(&mut columns[1], |ui| {
                        card(ui, |ui| self.draw_visibility_panel(ui));
                        ui.add_space(8.0);
                        card(ui, |ui| self.draw_layout_panel(ui));
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
    let sys_log_path = diagnostic_path.clone();
    let recent_path = recent_apps_path();
    let _ = APP.set(Mutex::new(AppState {
        own_pid,
        presentmon_path,
        diagnostic_path,
        presentmon_output_path,
        settings_path,
        settings: settings.clone(),
        last_settings_check: Instant::now(),
        capture: None,
        values: MetricValues::default(),
        fps_note: "",
        visible: false,
        target_name: None,
        target_pid: None,
        user_hidden: false,
        wanted: false,
        foreground_seen: None,
        capture_last_focus: Instant::now(),
        rate_since: None,
        confirmed: false,
        last_flowing: None,
        recorded_current: false,
        recent_apps: load_recent_apps(&recent_path),
        recent_path,
    }));

    let sys_stats = Arc::new(SysStats::new());
    sys_stats.want_gpu.store(settings.show_gpu, Ordering::Relaxed);
    let _ = SYS.set(Arc::clone(&sys_stats));
    spawn_system_stats_thread(sys_stats, sys_log_path);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_are_normalized() {
        assert_eq!(normalize_app_name("C:\\Games\\Foo.EXE"), Some("foo.exe".to_string()));
        assert_eq!(normalize_app_name("  \"Game\"  "), Some("game.exe".to_string()));
        assert_eq!(normalize_app_name("a,b.exe"), None);
        assert_eq!(normalize_app_name("   "), None);
    }

    #[test]
    fn app_lists_drop_duplicates_and_junk() {
        let list = vec!["A.exe".to_string(), "a.exe".to_string(), "b=c".to_string(), "".to_string()];
        assert_eq!(clean_app_list(&list), vec!["a.exe".to_string()]);
    }

    #[test]
    fn classification_follows_rules_then_mode() {
        let mut s = OverlaySettings::default();
        assert_eq!(classify_app(&s, "chrome.exe"), AppClass::Never);
        assert_eq!(classify_app(&s, "somegame.exe"), AppClass::Candidate);
        s.visibility = 1;
        assert_eq!(classify_app(&s, "chrome.exe"), AppClass::Candidate);
        s.visibility = 0;
        s.always_apps = vec!["chrome.exe".to_string()];
        assert_eq!(classify_app(&s, "Chrome.exe"), AppClass::Always);
        s.never_apps = vec!["chrome.exe".to_string()];
        assert_eq!(classify_app(&s, "chrome.exe"), AppClass::Never);
    }

    #[test]
    fn metrics_format_with_and_without_labels() {
        let values = MetricValues { fps: Some(144), cpu: Some(23), ..Default::default() };
        assert_eq!(format_metric(Metric::Fps, &values, "", true), "144 FPS");
        assert_eq!(format_metric(Metric::Fps, &values, "", false), "144");
        assert_eq!(format_metric(Metric::Cpu, &values, "", true), "CPU 23%");
        assert_eq!(format_metric(Metric::Ram, &values, "", true), "RAM --");
        assert_eq!(format_metric(Metric::Fps, &MetricValues::default(), "ERR", true), "FPS ERR");
    }

    #[test]
    fn settings_survive_a_save_and_load() {
        let mut s = OverlaySettings::default();
        s.layout = 1;
        s.show_cpu = true;
        s.visibility = 1;
        s.always_apps = vec!["game.exe".to_string()];
        s.never_apps = vec!["tool.exe".to_string()];
        s.normalize();
        let path = std::env::temp_dir().join(format!("zoro-settings-test-{}.ini", std::process::id()));
        save_settings(&path, &s).unwrap();
        let loaded = load_settings(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(loaded, s);
    }

    #[test]
    fn recent_apps_file_round_trips_and_removal_sticks() {
        let path = std::env::temp_dir().join(format!("zoro-recent-test-{}.txt", std::process::id()));
        save_recent_apps(&path, &["a.exe".to_string(), "b.exe".to_string()]);
        let mut apps = load_recent_apps(&path);
        assert_eq!(apps, vec!["a.exe".to_string(), "b.exe".to_string()]);
        apps.retain(|a| a != "a.exe");
        save_recent_apps(&path, &apps);
        assert_eq!(load_recent_apps(&path), vec!["b.exe".to_string()]);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn settings_never_end_up_with_nothing_to_show() {
        let mut s = OverlaySettings::default();
        s.show_fps = false;
        s.normalize();
        assert!(s.show_fps);
    }
}
