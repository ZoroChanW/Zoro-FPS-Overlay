//! The settings window (eframe / egui).
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use eframe::egui;
use crate::*;

// ---------------------------------------------------------------- Overlay launcher

pub(crate) fn launch_overlay_process() -> Result<(), String> {
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

pub(crate) const ANCHOR_LABELS: [&str; 9] = [
    "Top left", "Top center", "Top right",
    "Middle left", "Center", "Middle right",
    "Bottom left", "Bottom center", "Bottom right",
];

pub(crate) const SWATCHES: [&str; 6] = ["E8E4D9", "FFFFFF", "64FF82", "5CC8FF", "FFD166", "FF6B6B"];

// Palette. Every filled accent surface pairs with C_ON_ACCENT (dark) text, so
// light buttons can never end up with light, unreadable labels.
pub(crate) const C_BG: egui::Color32 = egui::Color32::from_rgb(12, 12, 12);
pub(crate) const C_DEEP: egui::Color32 = egui::Color32::from_rgb(8, 8, 8);
pub(crate) const C_SURFACE: egui::Color32 = egui::Color32::from_rgb(20, 20, 19);
pub(crate) const C_RAISED: egui::Color32 = egui::Color32::from_rgb(32, 32, 30);
pub(crate) const C_BORDER: egui::Color32 = egui::Color32::from_rgb(40, 40, 38);
pub(crate) const C_BORDER_HI: egui::Color32 = egui::Color32::from_rgb(82, 81, 76);
pub(crate) const C_TEXT: egui::Color32 = egui::Color32::from_rgb(236, 233, 225);
pub(crate) const C_MUTED: egui::Color32 = egui::Color32::from_rgb(130, 128, 121);
pub(crate) const C_ACCENT: egui::Color32 = egui::Color32::from_rgb(224, 219, 207);
pub(crate) const C_ON_ACCENT: egui::Color32 = egui::Color32::from_rgb(16, 16, 15);
pub(crate) const C_ERROR: egui::Color32 = egui::Color32::from_rgb(232, 122, 112);

pub(crate) fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

pub(crate) fn color_from_hex(value: &str) -> egui::Color32 {
    let normalized = normalize_color(value).unwrap_or_else(|| "E8E4D9".to_string());
    let rgb = u32::from_str_radix(&normalized, 16).unwrap_or(0xE8E4D9);
    egui::Color32::from_rgb(((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8)
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BtnKind { Primary, Secondary, Ghost }

pub(crate) fn action_button(ui: &mut egui::Ui, label: &str, kind: BtnKind, width: f32) -> egui::Response {
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

pub(crate) fn anchor_picker(ui: &mut egui::Ui, horizontal: &mut u8, vertical: &mut u8) {
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

pub(crate) fn color_swatch(ui: &mut egui::Ui, color: egui::Color32, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let ctx = ui.ctx().clone();
    let hov = ctx.animate_bool_with_time(response.id.with("hov"), response.hovered(), 0.12);
    let sel = ctx.animate_bool_with_time(response.id.with("sel"), selected, 0.18);
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 7.5 + hov * 1.5, color);
    painter.circle_stroke(rect.center(), 10.8, egui::Stroke::new(1.5, C_ACCENT.gamma_multiply(sel)));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn key_hint(ui: &mut egui::Ui, key: &str, action: &str) {
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
pub(crate) fn chip(ui: &mut egui::Ui, label: &str, selected: bool, min_width: f32) -> egui::Response {
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
pub(crate) fn block_size<G: std::ops::Deref<Target = egui::Galley>>(galleys: &[G], row: bool, line_gap: f32, item_gap: f32) -> egui::Vec2 {
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

pub(crate) fn toggle_row(ui: &mut egui::Ui, title: &str, subtitle: &str, value: &mut bool) {
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

pub(crate) fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) {
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

pub(crate) fn card_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(egui::RichText::new(title).size(16.0).strong().color(C_TEXT));
    ui.label(egui::RichText::new(subtitle).size(12.0).color(C_MUTED));
}

pub(crate) struct SettingsApp {
    pub(crate) settings: OverlaySettings,
    pub(crate) settings_path: PathBuf,
    pub(crate) color_edit: String,
    pub(crate) status: String,
    pub(crate) status_error: bool,
    pub(crate) saved_at: Option<Instant>,
    pub(crate) recent_apps: Vec<String>,
    pub(crate) recent_checked: Instant,
    pub(crate) app_edit: String,
    // Which hotkey is waiting for a key press: 0 = show/hide, 1 = quit.
    pub(crate) capturing: Option<u8>,
    pub(crate) autostart: bool,
}

impl SettingsApp {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, settings_path: PathBuf) -> Self {
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
            capturing: None,
            autostart: autostart_enabled(),
        }
    }

    pub(crate) fn save(&mut self) -> bool {
        let Some(color) = normalize_color(&self.color_edit) else {
            self.status = "Enter a valid RRGGBB color.".to_string();
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

    pub(crate) fn draw_position_panel(&mut self, ui: &mut egui::Ui) {
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

        ui.add_space(14.0);
        ui.label(egui::RichText::new("MONITOR").size(10.5).strong().color(C_MUTED));
        let monitors = list_monitors();
        ui.horizontal_wrapped(|ui| {
            if chip(ui, "Follow game", self.settings.monitor == 0, 0.0).clicked() { self.settings.monitor = 0; }
            for (index, monitor) in monitors.iter().enumerate() {
                let label = if monitor.primary { format!("{} (main)", index + 1) } else { format!("{}", index + 1) };
                if chip(ui, &label, self.settings.monitor as usize == index + 1, 0.0).clicked() {
                    self.settings.monitor = (index + 1) as u8;
                }
            }
        });
        let hint = if self.settings.monitor == 0 {
            "The overlay moves to the monitor of the app in front."
        } else {
            "The overlay always stays on this monitor (numbered left to right)."
        };
        ui.label(egui::RichText::new(hint).size(11.5).color(C_MUTED));
    }

    // The hotkey the user just pressed while a capture button was waiting.
    fn pressed_hotkey(ctx: &egui::Context) -> Option<Result<String, ()>> {
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } = event {
                if key == egui::Key::Escape { return Some(Err(())); }
                let Some(name) = egui_key_name(key) else { continue; };
                let mut mods = 0;
                if modifiers.ctrl { mods |= MOD_CONTROL; }
                if modifiers.alt { mods |= MOD_ALT; }
                if modifiers.shift { mods |= MOD_SHIFT; }
                return Some(Ok(format_hotkey(mods, name)));
            }
        }
        None
    }

    fn hotkey_row(&mut self, ui: &mut egui::Ui, index: u8, title: &str) {
        let waiting = self.capturing == Some(index);
        let current = if index == 0 { self.settings.hotkey_toggle.clone() } else { self.settings.hotkey_quit.clone() };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).color(C_TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if waiting { "Press a key...".to_string() } else { current.clone() };
                if chip(ui, &label, waiting, 130.0).clicked() {
                    self.capturing = if waiting { None } else { Some(index) };
                }
            });
        });
        if waiting {
            match Self::pressed_hotkey(ui.ctx()) {
                Some(Ok(text)) => {
                    if parse_hotkey(&text).is_some() {
                        if index == 0 { self.settings.hotkey_toggle = text; } else { self.settings.hotkey_quit = text; }
                        self.status = "Hotkey changed. Press Save to apply it.".to_string();
                        self.status_error = false;
                    } else {
                        self.status = "Letters and digits need Ctrl, Alt or Shift too.".to_string();
                        self.status_error = true;
                    }
                    self.capturing = None;
                }
                Some(Err(())) => self.capturing = None,
                None => ui.ctx().request_repaint(),
            }
        }
    }

    pub(crate) fn draw_system_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Hotkeys & startup", "Global keys work inside games too.");
        ui.add_space(12.0);
        self.hotkey_row(ui, 0, "Show / hide overlay");
        ui.add_space(4.0);
        self.hotkey_row(ui, 1, "Quit overlay");
        ui.label(egui::RichText::new("Click a button, then press the keys. Esc cancels.").size(11.5).color(C_MUTED));

        ui.add_space(12.0);
        let before = self.autostart;
        toggle_row(ui, "Start with Windows", "Runs the overlay in the tray when you sign in.", &mut self.autostart);
        if self.autostart != before {
            match set_autostart(self.autostart) {
                Ok(()) => { self.status = if self.autostart { "Will start with Windows." } else { "Won't start with Windows." }.to_string(); self.status_error = false; }
                Err(error) => { self.autostart = before; self.status = error; self.status_error = true; }
            }
        }

        ui.add_space(12.0);
        ui.label(egui::RichText::new("FPS ACCESS").size(10.5).strong().color(C_MUTED));
        if has_trace_access() {
            ui.label(egui::RichText::new("This app can read frame timing.").size(11.5).color(C_MUTED));
        } else {
            ui.label(egui::RichText::new("The overlay shows FPS ADMIN when PresentMon lacks permission. Run it as administrator, or add your account to the Performance Log Users group.").size(11.5).color(C_ERROR));
            ui.add_space(6.0);
            if action_button(ui, "Restart overlay as admin", BtnKind::Secondary, 190.0).clicked() {
                match relaunch_overlay_as_admin() {
                    Ok(()) => { self.status = "Overlay restarting as administrator.".to_string(); self.status_error = false; }
                    Err(error) => { self.status = error; self.status_error = true; }
                }
            }
        }
    }

    pub(crate) fn draw_preview_panel(&mut self, ui: &mut egui::Ui) {
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
            gpu_temp: Some(62),
            vram_used_mb: Some(6349),
            vram_total_mb: Some(12288),
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
        // Room for the graph under the text.
        let text_height = size.y;
        let graph_size = if self.settings.show_graph {
            Some(egui::vec2(self.settings.graph_width as f32 * scale, self.settings.graph_height as f32 * scale))
        } else {
            None
        };
        if let Some(graph) = graph_size {
            size.y += GRAPH_GAP as f32 * scale + graph.y;
            size.x = size.x.max(graph.x);
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
                egui::pos2(cursor_x, y + (text_height - item_h) / 2.0)
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

        // A made-up frame-time trace: mostly even, with a few spikes.
        if let Some(graph) = graph_size {
            let graph_x = match self.settings.horizontal_anchor {
                0 => x,
                1 => x + (size.x - graph.x) / 2.0,
                _ => x + size.x - graph.x,
            };
            let graph_rect = egui::Rect::from_min_size(egui::pos2(graph_x, y + text_height + GRAPH_GAP as f32 * scale), graph);
            painter.rect_filled(graph_rect, egui::CornerRadius::same(2), egui::Color32::from_black_alpha(120));
            let bars = ((graph.x / 2.0) as usize).max(10);
            let bar_w = graph.x / bars as f32;
            for i in 0..bars {
                let wobble = ((i * 7919 + 13) % 17) as f32 / 17.0;
                let spike = i % 23 == 17;
                let level = if spike { 0.9 } else { 0.28 + wobble * 0.12 };
                let bar_h = (graph.y * level).max(1.0);
                let bar_color = if spike { egui::Color32::from_rgb(255, 107, 107) } else { color };
                let bar = egui::Rect::from_min_max(
                    egui::pos2(graph_rect.left() + i as f32 * bar_w, graph_rect.bottom() - bar_h),
                    egui::pos2(graph_rect.left() + (i as f32 + 1.0) * bar_w, graph_rect.bottom()),
                );
                painter.rect_filled(bar, egui::CornerRadius::ZERO, bar_color);
            }
        }
    }

    pub(crate) fn draw_appearance_panel(&mut self, ui: &mut egui::Ui) {
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
        ui.label(egui::RichText::new("RRGGBB, for example E8E4D9.").size(11.5).color(C_MUTED));

        ui.add_space(6.0);
        toggle_row(ui, "Bold text", "Heavier weight reads better over busy scenes", &mut self.settings.bold);
        toggle_row(ui, "Text shadow", "A soft dark shadow behind the text", &mut self.settings.shadow);
    }

    // 0 = automatic, 1 = always show, 2 = never show.
    pub(crate) fn rule_of(&self, name: &str) -> u8 {
        if app_list_contains(&self.settings.always_apps, name) { 1 }
        else if app_list_contains(&self.settings.never_apps, name) { 2 }
        else { 0 }
    }

    pub(crate) fn set_rule(&mut self, name: &str, rule: u8) {
        self.settings.always_apps.retain(|a| !a.eq_ignore_ascii_case(name));
        self.settings.never_apps.retain(|a| !a.eq_ignore_ascii_case(name));
        match rule {
            1 => self.settings.always_apps.push(name.to_string()),
            2 => self.settings.never_apps.push(name.to_string()),
            _ => {}
        }
    }

    pub(crate) fn draw_visibility_panel(&mut self, ui: &mut egui::Ui) {
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

    pub(crate) fn draw_display_panel(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "Overlay items", "Switch each reading on or off.");
        ui.add_space(8.0);
        toggle_row(ui, "FPS", "Average of the last 30 frames", &mut self.settings.show_fps);
        toggle_row(ui, "Frametime", "Milliseconds per frame", &mut self.settings.show_frametime);
        toggle_row(ui, "1% low", "Slowest 1% of the last ~600 frames", &mut self.settings.show_low1);
        toggle_row(ui, "CPU usage", "Total processor load", &mut self.settings.show_cpu);
        toggle_row(ui, "RAM usage", "Physical memory in use", &mut self.settings.show_ram);
        toggle_row(ui, "GPU usage", "Busiest GPU engine, from Windows counters", &mut self.settings.show_gpu);
        toggle_row(ui, "GPU temperature", "Degrees Celsius (NVIDIA, and AMD/Intel where Windows reports it)", &mut self.settings.show_gpu_temp);
        toggle_row(ui, "VRAM usage", "Video memory in use / total", &mut self.settings.show_vram);
        ui.add_space(4.0);
        toggle_row(ui, "Frametime graph", "Bars of recent frame times under the readings", &mut self.settings.show_graph);
        if self.settings.show_graph {
            ui.add_space(6.0);
            ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(120.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Graph width").color(C_TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("{} px", self.settings.graph_width)).strong().color(C_TEXT));
                });
            });
            ui.add(egui::Slider::new(&mut self.settings.graph_width, 120..=480).show_value(false));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Graph height").color(C_TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("{} px", self.settings.graph_height)).strong().color(C_TEXT));
                });
            });
            ui.add(egui::Slider::new(&mut self.settings.graph_height, 30..=120).show_value(false));
        }
        // At least one item must stay on; a toggle that would turn all off snaps back.
        if ALL_METRICS.iter().all(|m| !self.settings.is_enabled(*m)) {
            self.settings.show_fps = true;
        }
    }

    pub(crate) fn draw_layout_panel(&mut self, ui: &mut egui::Ui) {
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

    pub(crate) fn reset_defaults(&mut self) {
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
                    ui.label(egui::RichText::new("v0.8.0").size(11.0).color(C_MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        key_hint(ui, &self.settings.hotkey_quit.clone(), "Quit");
                        ui.add_space(8.0);
                        key_hint(ui, &self.settings.hotkey_toggle.clone(), "Hide / show");
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
                        ui.add_space(8.0);
                        card(ui, |ui| self.draw_system_panel(ui));
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

pub(crate) fn run_settings_gui() {
    if !claim_settings_instance() { return; }
    let settings_path = settings_file_path();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(SETTINGS_TITLE)
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

// Keys the hotkey capture understands, by the name stored in settings.ini.
fn egui_key_name(key: egui::Key) -> Option<&'static str> {
    use egui::Key::*;
    Some(match key {
        F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6",
        F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",
        A => "A", B => "B", C => "C", D => "D", E => "E", F => "F", G => "G", H => "H", I => "I",
        J => "J", K => "K", L => "L", M => "M", N => "N", O => "O", P => "P", Q => "Q", R => "R",
        S => "S", T => "T", U => "U", V => "V", W => "W", X => "X", Y => "Y", Z => "Z",
        Num0 => "0", Num1 => "1", Num2 => "2", Num3 => "3", Num4 => "4",
        Num5 => "5", Num6 => "6", Num7 => "7", Num8 => "8", Num9 => "9",
        Insert => "Insert", Delete => "Delete", Home => "Home", End => "End",
        PageUp => "PageUp", PageDown => "PageDown", Pause => "Pause",
        _ => return None,
    })
}
