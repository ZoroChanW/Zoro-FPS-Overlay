#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case, dead_code)]


#[cfg(not(windows))]
compile_error!("Zoro FPS Overlay currently supports Windows only.");

mod autostart;
mod capture;
mod config;
mod ffi;
mod gui;
mod monitors;
mod overlay;
mod render;
mod settings;
mod sysstats;
mod tray;
mod util;

pub(crate) use autostart::*;
pub(crate) use capture::*;
pub(crate) use config::*;
pub(crate) use ffi::*;
pub(crate) use gui::*;
pub(crate) use monitors::*;
pub(crate) use overlay::*;
pub(crate) use render::*;
pub(crate) use settings::*;
pub(crate) use sysstats::*;
pub(crate) use tray::*;
pub(crate) use util::*;


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
