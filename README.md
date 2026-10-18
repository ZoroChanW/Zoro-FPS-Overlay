[نسخه فارسی (Persian README)](README-fa.md)

# Zoro FPS Overlay

![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-lightgrey)
![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)

A lightweight, minimal FPS overlay for Windows. It shows FPS, frametime, 1% lows and CPU / RAM / GPU usage on top of your game, and it does that **without injecting anything into the game**.

<!-- Add a screenshot of the overlay and of the settings window here, for example:
![Settings window](docs/settings.png) -->

## Highlights

- **Six readings, each one switchable:** FPS, frametime, 1% low, CPU usage, RAM usage and GPU usage.
- **Column or row layout:** stack the readings vertically or line them up side by side.
- **No injection, no hooking:** frame timing comes from [PresentMon](https://github.com/GameTechDev/PresentMon) and the overlay is a separate, click-through window. Nothing is loaded into the game process.
- **Made to be adjusted:** 9 screen anchors with pixel offsets, 7 fonts, size, color, bold, text shadow, labels and spacing, with a live preview.
- **Games only, by default:** the overlay shows for games, not for your browser or editor, with no flicker. Per-app Show / Hide rules override the automatic choice, and **All apps** mode is one click away.
- **Changes apply instantly:** save in the settings window and the running overlay updates within half a second.
- **Tiny footprint:** the settings window is built with `egui`/`eframe` and is only open when you need it.

## What it shows

| Reading | What it means |
| --- | --- |
| **FPS** | Average over the last 30 frames. |
| **Frametime** | Average time per frame over the last 30 frames, in milliseconds. |
| **1% low** | The slowest 1% of the last ~600 frames, converted to FPS. It appears after about 60 frames have been collected. |
| **CPU usage** | Total processor load across all cores. |
| **RAM usage** | Share of physical memory in use. |
| **GPU usage** | Load of the busiest GPU engine, from the Windows "GPU Engine" performance counters (the same source Task Manager uses). Works with NVIDIA, AMD and Intel GPUs. |

The GPU counters are only queried while the GPU item is switched on.

## Getting started

### Build from source

**Requirements:** Windows 10 or 11, Rust and Cargo. The first build downloads `eframe` and its dependencies from crates.io.

1. Clone or download this repository.
2. Open PowerShell in the project folder and download PresentMon:

   ```powershell
   Set-ExecutionPolicy -Scope Process Bypass
   .\scripts\download-presentmon.ps1
   ```

3. Run `build-release.bat`.
4. The executable is created at:

   ```text
   target\release\zoro-fps-overlay.exe
   ```

> **Important:** `PresentMon.exe` must stay in the same folder as `zoro-fps-overlay.exe`.

### Using it

1. Run `zoro-fps-overlay.exe` without arguments to open the settings window.
2. Choose which readings to show, where they go and how they look. The preview at the top updates as you change things.
3. Press **Save & Start**. This saves your settings, starts the overlay and closes the settings window. **Save** only saves, and **Reset** restores the defaults (they are applied when you save).
4. Start a game. In the default **Games only** mode the overlay appears once the focused app has been rendering frames for a moment, and hides again on the desktop, in system windows and in everyday apps such as browsers.

You can reopen the settings window at any time. Closing it does not stop the overlay.

### Hotkeys

| Key | Action |
| --- | --- |
| `F8` | Show or hide the overlay |
| `F10` | Quit the overlay completely |

## Customization

| Option | Details |
| --- | --- |
| **Position** | 9 anchors (corners, edges, center) plus X/Y offsets in pixels. Positive X moves right, positive Y moves down, whichever anchor you pick. |
| **Layout** | Column (one item per line) or Row (items side by side). Spacing is adjustable for each. |
| **Items** | FPS, frametime, 1% low, CPU, RAM, GPU. At least one stays on. |
| **Labels** | Show `CPU 23%` or just `23%`. |
| **Font** | Segoe UI, Consolas, Arial, Tahoma, Verdana, Bahnschrift or Courier New. The font must be installed. Consolas keeps digits evenly spaced. |
| **Size and color** | Text size of 18 to 64 px, and any `RRGGBB` color. Pure black (`000000`) is not allowed because it is the overlay's transparent color. |
| **Style** | Bold text and an optional soft shadow. |
| **When to show** | **Games only** (default) or **All apps**, plus per-app Auto / Show / Hide rules. See below. |

Settings are stored in `%APPDATA%\ZoroFPSOverlay\settings.ini`. The running overlay checks the file every 500 ms and applies changes by itself.

<details>
<summary>settings.ini reference</summary>

| Key | Values | Default |
| --- | --- | --- |
| `horizontal_anchor` | `0` left, `1` center, `2` right | `0` |
| `vertical_anchor` | `0` top, `1` middle, `2` bottom | `0` |
| `offset_x`, `offset_y` | pixels, -4000 to 4000 | `0` |
| `font_size` | 18 to 64 | `34` |
| `color` | `RRGGBB`, not `000000` | `E8E4D9` |
| `show_fps`, `show_frametime`, `show_low1`, `show_cpu`, `show_ram`, `show_gpu` | `1` or `0` | FPS on, the rest off |
| `layout` | `0` column, `1` row | `0` |
| `line_gap` | pixels between lines (column), 0 to 40 | `2` |
| `item_gap` | pixels between items (row), 0 to 60 | `18` |
| `show_labels` | `1` or `0` | `1` |
| `bold`, `shadow` | `1` or `0` | bold on, shadow off |
| `font_family` | one of the fonts listed above | `Segoe UI` |
| `visibility` | `0` games only, `1` all apps | `0` |
| `always_apps`, `never_apps` | comma-separated process names, e.g. `game.exe,other.exe` | empty |

</details>

## When the overlay shows

- **Games only** (default): the focused app is measured with PresentMon. The overlay appears after it has produced at least 10 FPS for about 1.2 seconds, and it stays visible until frames have been missing for 5 seconds. A loading screen or a low-FPS scene therefore never hides it, and a short alt-tab does not make it flicker. Browsers, media players, chat apps, editors, terminals, office apps and game launchers are skipped.
- **All apps:** the overlay shows on any app that is in the foreground and rendering frames.
- **Per-app rules:** apps the overlay has seen rendering frames are listed in the settings window. Set one to **Show** (always), **Hide** (never) or **Auto**. You can also add a process name by hand, and remove an app from the list with the `×` button. A **Hide** rule wins over a **Show** rule.
- `F8` hides the overlay manually. Showing it again only works while the rules want it visible.

Detection is a heuristic, not a guarantee. If it ever picks wrong, a Show or Hide rule fixes that app for good.

## How it works

- **Frame timing** is read by a hidden [PresentMon](https://github.com/GameTechDev/PresentMon) process that watches the application in the foreground. The overlay parses its CSV output and computes FPS, frametime and 1% low.
- **The overlay** is a transparent, always-on-top, click-through Win32 window. It is sized to fit its text exactly, so right-anchored text can reach the very edge of the monitor.
- **CPU and RAM** come from `GetSystemTimes` and `GlobalMemoryStatusEx`. **GPU** comes from the Windows PDH "GPU Engine" counters. They refresh once per second.
- **The settings window** is the only part that uses `egui`/`eframe` (Glow renderer, so the default `wgpu` stack is not pulled in).

Nothing is injected into the game, and no game memory is read.

## Limitations

- **Windows only**, tested on Windows 10 and 11.
- **Fullscreen Exclusive** games may cover the overlay. Windowed, borderless and most "fullscreen" DirectX 11/12 games (which Windows runs in a borderless-like mode) work. Measuring FPS and drawing on top of a game are two separate problems, and the second one is up to Windows and the game.
- **GPU usage** needs the Windows "GPU Engine" counters (Windows 10 1709 or newer with a WDDM 2.0+ driver). If they are not available, the item shows `GPU --`.
- **Anti-cheat:** the overlay does not touch game processes, but it is a third-party tool. If you play online games with strict anti-cheat, use it at your own discretion.

## Troubleshooting

The overlay writes two log files to your temp folder (`%TEMP%`):

- `zoro-fps-overlay-diagnostic.log` for what the overlay is doing, including why GPU counters might be unavailable.
- `zoro-fps-overlay-presentmon.log` for PresentMon's own error output.

| Problem | What to try |
| --- | --- |
| Overlay never appears | Make sure `PresentMon.exe` is next to the executable, and that the game is the foreground window. Press `F8` in case it was hidden. |
| `FPS ERR` | PresentMon could not start or exited. Check the PresentMon log. |
| `FPS ...` | Waiting for the first frames from the game. |
| `GPU --` | The GPU counters are unavailable or still warming up. Wait a few seconds, then check the diagnostic log. |
| Overlay is hidden in a game | The game is probably in exclusive fullscreen. Switch it to borderless or windowed. |

## Roadmap

Ideas, not promises:

- Detect exclusive fullscreen and offer a fallback.
- Per-game profiles.
- CPU/GPU temperatures and VRAM usage.

## Changelog

**v0.6.1**
- Apps can be removed from the **When to show** list with the `×` button (this also clears their Show / Hide rule once you save).

**v0.6.0**
- Games-only visibility with a dwell time and hysteresis, so there is no flicker and no false hiding in games.
- Per-app Auto / Show / Hide rules and an **All apps** mode.
- A short alt-tab no longer restarts the measurement.
- `F8` can no longer show the overlay on the desktop.

**v0.5.0**
- CPU, RAM and GPU usage readings, each one switchable.
- Column and row layouts with adjustable spacing.
- Font choice, bold, text shadow and label options.
- The preview stays pinned while the settings scroll.

**v0.4.0**
- Multi-line overlay with FPS, frametime and 1% low.
- Fixed the two settings columns sharing one scroll position.

**v0.3.2**
- Fixed positioning on the right side of the screen (Per-Monitor DPI awareness, and a window that fits its text).
- Predictable offsets: positive X moves right, positive Y moves down.
- Redesigned the settings window with a minimal black, gray and off-white palette.
- `F8` and `F10` are shown in the window header.

**v0.3.1**
- Updated to the `egui`/`eframe` 0.37 UI API.

## License

MIT. PresentMon is a separate tool with its own license. Please review the official PresentMon project's license before using or redistributing it.
