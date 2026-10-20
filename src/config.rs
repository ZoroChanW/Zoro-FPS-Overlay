//! Tunable constants.
use std::time::Duration;

// Overlay window padding around the text (physical pixels).
pub(crate) const OVERLAY_PAD_X: i32 = 8;
pub(crate) const OVERLAY_PAD_Y: i32 = 4;
// Distance from the screen edge when an anchor is used with offset 0.
pub(crate) const OVERLAY_EDGE_MARGIN: i32 = 24;

// Marks "no reading available" in the shared system stats.
pub(crate) const STAT_NONE: u32 = u32::MAX;

// How long PresentMon keeps measuring an app after it loses focus, so a brief alt-tab or a
// notification does not restart the measurement.
pub(crate) const CAPTURE_GRACE: Duration = Duration::from_secs(8);
// A newly focused app must stay in front this long before PresentMon is started for it.
pub(crate) const FOREGROUND_DWELL: Duration = Duration::from_millis(700);
// "Games only": frames must arrive at this rate, for this long, before the overlay appears...
pub(crate) const AUTO_MIN_FPS: u32 = 10;
pub(crate) const AUTO_SHOW_AFTER: Duration = Duration::from_millis(1200);
// ...and once it is showing, it stays until frames have been missing this long.
pub(crate) const AUTO_HIDE_AFTER: Duration = Duration::from_secs(5);
pub(crate) const OVERLAY_CLASS: &str = "ZoroFpsOverlayWindowV020";
pub(crate) const SETTINGS_TITLE: &str = "Zoro FPS Overlay — Settings";
// Gap between the text and the frametime graph, and how many recent frames are kept for it.
pub(crate) const GRAPH_GAP: i32 = 6;
pub(crate) const GRAPH_HISTORY: usize = 480;
pub(crate) const MAX_APP_RULES: usize = 64;
pub(crate) const RECENT_APPS_MAX: usize = 12;
