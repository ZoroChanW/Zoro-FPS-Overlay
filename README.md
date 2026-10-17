[نسخه فارسی README (Persian README)](README-fa.md)

# Zoro FPS Overlay v0.3.2

A lightweight FPS overlay for Windows that uses PresentMon to measure frame rates and a modern **egui/eframe** graphical interface to customize its appearance.

## Features

- Displays FPS in a transparent, click-through overlay window.
- Detects the foreground process and retrieves frame data from PresentMon through stdout.
- Modern settings window with a black, gray, and off-white color theme.
- Choose from 9 screen anchor positions and fine-tune placement using pixel-based X/Y offsets. Positive X always moves the overlay to the right, and positive Y always moves it downward.
- Approximate preview of the overlay's position and appearance.
- Customize text size and color.
- Saves settings to `%APPDATA%\ZoroFPSOverlay\settings.ini`.
- Automatically applies saved configuration changes to the running overlay.
- Close the settings window without closing the overlay.
- `F8` toggles overlay visibility, and `F10` completely exits the overlay. Both shortcuts are also displayed in the GUI.
- The settings GUI uses `eframe`/`egui` with the Glow renderer to avoid adding the default `wgpu` dependencies.

## Building on Windows

**Prerequisites:** Rust and Cargo. On the first build, Cargo must be able to download `eframe` and its dependencies from crates.io.

1. Extract the project ZIP archive.
2. Open PowerShell in the project directory and run:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\download-presentmon.ps1
```

3. Run `build-release.bat`.
4. The executable will be located at:

```text
target\release\zoro-fps-overlay.exe
```

**Important:** Keep `PresentMon.exe` in the same directory as the executable.

## Usage

- Run the executable without arguments to open the settings window.
- Adjust the anchor position, offsets, text size, and color.
- Click `Save changes` to save your settings.
- Click `Save & Start Overlay` to save the settings, start the overlay, and close the GUI.
- Click `Close` to close only the settings window.
- While in-game, press `F8` to show or hide the overlay, or `F10` to exit it completely.

The GUI preview displays a sample value of `144 FPS`. This is a demonstration value, not the live FPS of your game.

## Notes

- The transparent overlay window is implemented using the Win32 API so that egui does not interfere with the continuous rendering loop of the FPS overlay. egui is used only in the settings window.
- The overlay checks the settings file every 500 milliseconds and updates its position and appearance when changes are detected.
- Displaying the overlay in **Fullscreen Exclusive** mode is not guaranteed by Windows or the game. FPS measurement and overlay visibility are separate capabilities.
- This version has not been compiled or tested on Windows in the current development environment. After downloading the project, build it using `build-release.bat` and keep any compilation errors for troubleshooting.

## License

MIT. PresentMon is a separate tool distributed under its own license. Please review the official PresentMon project's license before using it.

## Version 0.3.1 Notes

This version was updated to match the `egui`/`eframe` 0.37 UI API, including the use of `Panel` instead of `TopBottomPanel` and applying styles through `style_of` and `set_style_of`.

## Changes in v0.3.2

- Fixed right-side positioning by enabling Per-Monitor DPI Awareness V2 before creating windows, preventing coordinate mismatches caused by Windows display scaling.
- Made offset behavior predictable: positive X moves right, and positive Y moves down, regardless of the selected anchor.
- Redesigned the UI with a black, gray, and off-white palette, removing the purple accent and decorative colors.
- Made the `F8` and `F10` shortcuts visible in the GUI header.
