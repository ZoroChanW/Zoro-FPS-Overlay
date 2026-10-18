//! Settings, metrics, per-app rules and the settings file.
use std::fs;
use std::io::BufRead;
use std::path::PathBuf;
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Metric { Fps, Frametime, Low1, Cpu, Ram, Gpu }

// Order in which enabled items appear on the overlay.
pub(crate) const ALL_METRICS: [Metric; 6] = [Metric::Fps, Metric::Frametime, Metric::Low1, Metric::Cpu, Metric::Ram, Metric::Gpu];
pub(crate) const FONT_FAMILIES: [&str; 7] = ["Segoe UI", "Consolas", "Arial", "Tahoma", "Verdana", "Bahnschrift", "Courier New"];

impl Metric {
    // Widest text this item can show; the overlay window and row cells are sized from it.
    pub(crate) fn sample(self, labels: bool) -> &'static str {
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
pub(crate) struct MetricValues {
    pub(crate) fps: Option<u32>,
    pub(crate) frametime_ms: Option<f64>,
    pub(crate) low1: Option<u32>,
    pub(crate) cpu: Option<u32>,
    pub(crate) ram: Option<u32>,
    pub(crate) gpu: Option<u32>,
}

pub(crate) fn percent_text(label: &str, value: Option<u32>, labels: bool) -> String {
    match (value, labels) {
        (Some(v), true) => format!("{} {}%", label, v),
        (Some(v), false) => format!("{}%", v),
        (None, true) => format!("{} --", label),
        (None, false) => "--".to_string(),
    }
}

// `note` is a short capture status ("ERR", "...") shown instead of a missing FPS value.
pub(crate) fn format_metric(metric: Metric, values: &MetricValues, note: &str, labels: bool) -> String {
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
pub(crate) struct OverlaySettings {
    // Anchors: 0 = left/top, 1 = center/middle, 2 = right/bottom.
    pub(crate) horizontal_anchor: u8,
    pub(crate) vertical_anchor: u8,
    pub(crate) offset_x: i32,
    pub(crate) offset_y: i32,
    pub(crate) font_size: i32,
    // User-facing RGB hex, e.g. "E8E4D9". Win32 COLORREF is converted on use.
    pub(crate) color_hex: String,
    // Which items the overlay shows.
    pub(crate) show_fps: bool,
    pub(crate) show_frametime: bool,
    pub(crate) show_low1: bool,
    pub(crate) show_cpu: bool,
    pub(crate) show_ram: bool,
    pub(crate) show_gpu: bool,
    // 0 = items stacked in a column, 1 = items side by side in a row.
    pub(crate) layout: u8,
    // Gap between stacked lines (column) and between items (row), in pixels.
    pub(crate) line_gap: i32,
    pub(crate) item_gap: i32,
    // "CPU 23%" instead of "23%".
    pub(crate) show_labels: bool,
    pub(crate) bold: bool,
    pub(crate) shadow: bool,
    pub(crate) font_family: String,
    // 0 = only apps that render frames like a game, 1 = every app in front.
    pub(crate) visibility: u8,
    // Process names (lowercase, e.g. "game.exe") that always / never get the overlay.
    pub(crate) always_apps: Vec<String>,
    pub(crate) never_apps: Vec<String>,
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


impl OverlaySettings {
    pub(crate) fn is_enabled(&self, metric: Metric) -> bool {
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
    pub(crate) fn metrics(&self) -> Vec<Metric> {
        let enabled: Vec<Metric> = ALL_METRICS.iter().copied().filter(|m| self.is_enabled(*m)).collect();
        if enabled.is_empty() { vec![Metric::Fps] } else { enabled }
    }

    pub(crate) fn shadow_offset(&self) -> i32 {
        (self.font_size / 16).max(1)
    }

    pub(crate) fn normalize(&mut self) {
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

    pub(crate) fn color_ref(&self) -> u32 {
        let rgb = u32::from_str_radix(&self.color_hex, 16).unwrap_or(0xE8E4D9);
        let r = rgb & 0xFF0000;
        let g = rgb & 0x00FF00;
        let b = rgb & 0x0000FF;
        (r >> 16) | g | (b << 16)
    }

    // The window is exactly as large as the widest possible text of the enabled items.
    // A window wider than its text leaves a transparent strip that cannot go past the
    // screen edge, which would keep right-anchored text away from the monitor edge.
    pub(crate) fn window_size(&self) -> (i32, i32) {
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


// Lowercase process name such as "game.exe". Accepts a pasted path and adds ".exe" when
// the name has no extension. Rejects characters that would break the settings file.
pub(crate) fn normalize_app_name(value: &str) -> Option<String> {
    let leaf = value.trim().trim_matches('"').rsplit(['\\', '/']).next()?.trim().to_ascii_lowercase();
    if leaf.is_empty() || leaf.len() > 64 { return None; }
    if leaf.chars().any(|c| c.is_control() || matches!(c, ',' | '=' | '"' | '<' | '>' | '|' | '*' | '?' | ':')) {
        return None;
    }
    if leaf.contains('.') { Some(leaf) } else { Some(format!("{}.exe", leaf)) }
}

pub(crate) fn clean_app_list(list: &[String]) -> Vec<String> {
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

pub(crate) fn app_list_contains(list: &[String], name: &str) -> bool {
    list.iter().any(|a| a.eq_ignore_ascii_case(name))
}

// Everyday programs that draw frames but are not games: browsers, media players, chat apps,
// editors, terminals, office tools and game launchers. "Games only" mode skips them, and a
// Show rule in the settings overrides this list.
pub(crate) fn is_known_non_game(name: &str) -> bool {
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
pub(crate) enum AppClass {
    // Never gets the overlay.
    Never,
    // Always gets the overlay while in front.
    Always,
    // Gets it according to the visibility mode.
    Candidate,
}

pub(crate) fn classify_app(settings: &OverlaySettings, name: &str) -> AppClass {
    if app_list_contains(&settings.never_apps, name) { return AppClass::Never; }
    if app_list_contains(&settings.always_apps, name) { return AppClass::Always; }
    if settings.visibility == 0 && is_known_non_game(name) { return AppClass::Never; }
    AppClass::Candidate
}

// Apps the overlay has seen rendering frames, newest first. The settings window reads this
// file to offer them as a list, so nobody has to type process names.
pub(crate) fn recent_apps_path() -> PathBuf {
    settings_file_path().with_file_name("recent_apps.txt")
}

pub(crate) fn save_recent_apps(path: &PathBuf, apps: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, apps.join("\n"));
}

pub(crate) fn load_recent_apps(path: &PathBuf) -> Vec<String> {
    let Ok(contents) = fs::read_to_string(path) else { return Vec::new(); };
    let lines: Vec<String> = contents.lines().map(str::to_string).collect();
    let mut apps = clean_app_list(&lines);
    apps.truncate(RECENT_APPS_MAX);
    apps
}

pub(crate) fn normalize_color(value: &str) -> Option<String> {
    let clean = value.trim().trim_start_matches('#');
    if clean.len() != 6 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(clean.to_ascii_uppercase())
}

pub(crate) fn settings_file_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("ZoroFPSOverlay").join("settings.ini")
    } else {
        current_exe_folder().join("settings.ini")
    }
}

pub(crate) fn load_settings(path: &PathBuf) -> OverlaySettings {
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

pub(crate) fn save_settings(path: &PathBuf, settings: &OverlaySettings) -> Result<(), String> {
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

