//! The overlay window: state, per-tick update, window procedure.
use std::fs;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use crate::*;

pub(crate) fn overlay_geometry(settings: &OverlaySettings) -> (i32, i32, i32, i32) {
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

pub(crate) struct AppState {
    pub(crate) own_pid: u32,
    pub(crate) presentmon_path: PathBuf,
    pub(crate) diagnostic_path: PathBuf,
    pub(crate) presentmon_output_path: PathBuf,
    pub(crate) settings_path: PathBuf,
    pub(crate) settings: OverlaySettings,
    pub(crate) last_settings_check: Instant,
    pub(crate) capture: Option<Capture>,
    pub(crate) values: MetricValues,
    // Short capture status ("ERR", "...") shown in place of a missing FPS value.
    pub(crate) fps_note: &'static str,
    pub(crate) visible: bool,
    pub(crate) target_name: Option<String>,
    pub(crate) target_pid: Option<u32>,
    pub(crate) user_hidden: bool,
    // Whether the rules want the overlay visible right now (before the F8 override).
    pub(crate) wanted: bool,
    // The app that has been in front, with its pid and since when.
    pub(crate) foreground_seen: Option<(String, u32, Instant)>,
    pub(crate) capture_last_focus: Instant,
    // "Games only" tracking for the measured app.
    pub(crate) rate_since: Option<Instant>,
    pub(crate) confirmed: bool,
    pub(crate) last_flowing: Option<Instant>,
    pub(crate) recorded_current: bool,
    pub(crate) recent_apps: Vec<String>,
    pub(crate) recent_path: PathBuf,
}

pub(crate) static APP: OnceLock<Mutex<AppState>> = OnceLock::new();

impl AppState {
    // Puts an app at the top of the recently seen list that the settings window shows.
    pub(crate) fn remember_recent_app(&mut self, name: &str) {
        let Some(name) = normalize_app_name(name) else { return; };
        // Re-read the file first so apps removed from the settings window stay removed.
        self.recent_apps = load_recent_apps(&self.recent_path);
        self.recent_apps.retain(|a| *a != name);
        self.recent_apps.insert(0, name);
        self.recent_apps.truncate(RECENT_APPS_MAX);
        save_recent_apps(&self.recent_path, &self.recent_apps);
    }

    // Forgets the PresentMon readings (system readings are kept).
    pub(crate) fn clear_capture_values(&mut self, note: &'static str) {
        self.values.fps = None;
        self.values.frametime_ms = None;
        self.values.low1 = None;
        self.fps_note = note;
    }
}

pub(crate) fn update_overlay(hwnd: HWND) {
    let fg = unsafe { GetForegroundWindow() };
    let mut pid: DWORD = 0;
    if !fg.is_null() { unsafe { GetWindowThreadProcessId(fg, &mut pid); } }
    let own_pid = unsafe { GetCurrentProcessId() };
    let foreground_name = if pid != 0 && pid != own_pid { process_name_for_pid(pid) } else { None };

    let mut state = match APP.get().and_then(|m| m.lock().ok()) { Some(s) => s, None => return };
    // The settings window writes a tiny INI file. Poll it twice a second so an
    // already-running overlay immediately picks up saved position/style changes.
    let mut settings_changed = false;
    if state.last_settings_check.elapsed() >= Duration::from_millis(500) {
        state.last_settings_check = Instant::now();
        let new_settings = load_settings(&state.settings_path);
        if new_settings != state.settings {
            state.settings = new_settings;
            settings_changed = true;
            let (x, y, width, height) = overlay_geometry(&state.settings);
            unsafe {
                SetWindowPos(hwnd, (-1isize) as HWND, x, y, width, height, SWP_NOACTIVATE);
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
    let visible_now = state.visible;
    drop(state);
    // Redraw while visible (values change), and after any settings change.
    if visible_now || settings_changed { paint_overlay(hwnd); }
}

pub(crate) unsafe extern "system" fn window_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            // The overlay is a per-pixel-alpha layered window; paint_overlay updates it.
            // Painting here only tells Windows that the region is up to date.
            let mut ps: PAINTSTRUCT = zeroed();
            BeginPaint(hwnd, &mut ps);
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


pub(crate) fn run_overlay() {
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
            hbrBackground: GetStockObject(4) as HBRUSH, // BLACK_BRUSH; unused, the window is drawn with per-pixel alpha
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
        SetWindowPos(hwnd, (-1isize) as HWND, overlay_x, overlay_y, overlay_width, overlay_height, SWP_NOACTIVATE);
        paint_overlay(hwnd);
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
