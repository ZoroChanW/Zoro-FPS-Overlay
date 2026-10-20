//! Drawing the overlay text into a per-pixel-alpha layered window.
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use crate::*;

pub(crate) fn make_overlay_font(settings: &OverlaySettings) -> HFONT {
    let font_name = wide(&settings.font_family);
    let weight = if settings.bold { 700 } else { 400 };
    unsafe { CreateFontW(settings.font_size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, FONT_QUALITY_ANTIALIASED, 0, font_name.as_ptr()) }
}

pub(crate) fn text_extent(hdc: HDC, text: &str) -> SIZE {
    let wide_text = wide(text);
    let mut size = SIZE { cx: 0, cy: 0 };
    unsafe {
        GetTextExtentPoint32W(hdc, wide_text.as_ptr(), (wide_text.len() - 1) as i32, &mut size);
    }
    size
}

// ClearType colour fringes cannot be blended cleanly over a game, so the text is drawn with
// plain greyscale antialiasing (quality 4) and blended with real per-pixel alpha instead.
const FONT_QUALITY_ANTIALIASED: u32 = 4;
const SHADOW_ALPHA: u32 = 190; // out of 255

// Where each overlay item goes inside a window of the given size, in item order.
pub(crate) fn layout_items(
    hdc: HDC, width: i32, height: i32, settings: &OverlaySettings, samples: &[&str],
) -> Vec<RECT> {
    let _ = height;
    let line_h = text_extent(hdc, "Ag").cy.max(settings.font_size);
    let shadow_off = if settings.shadow { settings.shadow_offset() } else { 0 };
    // Row layout: each item gets a fixed cell sized for its widest value,
    // so items do not jump around when a number gains a digit.
    let cell_widths: Vec<i32> = samples.iter().map(|s| text_extent(hdc, s).cx).collect();
    let row_total: i32 = cell_widths.iter().sum::<i32>()
        + settings.item_gap * (cell_widths.len() as i32 - 1).max(0);
    let mut x_cursor = match settings.horizontal_anchor {
        1 => (width - row_total) / 2,
        2 => width - OVERLAY_PAD_X - shadow_off - row_total,
        _ => OVERLAY_PAD_X,
    };
    (0..samples.len()).map(|index| {
        if settings.layout == 1 {
            let cell_w = cell_widths.get(index).copied().unwrap_or(0);
            let cell = RECT { left: x_cursor, top: OVERLAY_PAD_Y, right: x_cursor + cell_w, bottom: OVERLAY_PAD_Y + line_h };
            x_cursor += cell_w + settings.item_gap;
            cell
        } else {
            let top = OVERLAY_PAD_Y + index as i32 * (line_h + settings.line_gap);
            RECT { left: OVERLAY_PAD_X, top, right: width - OVERLAY_PAD_X - shadow_off, bottom: top + line_h }
        }
    }).collect()
}

// A 32-bit top-down bitmap that GDI can draw into. Starts out fully black.
struct Dib { dc: HDC, bitmap: HBITMAP, old: HGDIOBJ, bits: *mut u32, len: usize }

impl Dib {
    unsafe fn new(screen: HDC, width: i32, height: i32) -> Option<Dib> {
        let dc = CreateCompatibleDC(screen);
        if dc.is_null() { return None; }
        let info = BITMAPINFO {
            header: BITMAPINFOHEADER {
                bi_size: size_of::<BITMAPINFOHEADER>() as u32, bi_width: width, bi_height: -height,
                bi_planes: 1, bi_bit_count: 32, bi_compression: 0, bi_size_image: 0,
                bi_x_pels: 0, bi_y_pels: 0, bi_clr_used: 0, bi_clr_important: 0,
            },
            colors: [0],
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        Some(Dib { dc, bitmap, old, bits: bits as *mut u32, len: (width * height) as usize })
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.len) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

// Draws every item in white onto a black bitmap. Afterwards a pixel's brightness is how much
// of it the text covers.
unsafe fn draw_coverage(
    dib: &Dib, font: HFONT, texts: &[String], rects: &[RECT], align: u32, dx: i32, dy: i32,
) {
    let previous = if !font.is_null() { SelectObject(dib.dc, font) } else { std::ptr::null_mut() };
    SetBkMode(dib.dc, 1);
    SetTextColor(dib.dc, 0x00FF_FFFF);
    let flags = align | DT_SINGLELINE | DT_NOPREFIX | DT_NOCLIP;
    for (text, rect) in texts.iter().zip(rects) {
        let wide_text = wide(text);
        let mut rc = RECT { left: rect.left + dx, top: rect.top + dy, right: rect.right + dx, bottom: rect.bottom + dy };
        DrawTextW(dib.dc, wide_text.as_ptr(), wide_text.len().saturating_sub(1) as i32, &mut rc, flags);
    }
    if !previous.is_null() { SelectObject(dib.dc, previous); }
}

// Renders the overlay and hands it to Windows as a per-pixel-alpha layered window.
// Pixels the text does not touch are fully transparent, so nothing is left around the text.
pub(crate) fn paint_overlay(hwnd: HWND) {
    let snapshot = APP.get().and_then(|m| m.lock().ok()).map(|s| {
        let metrics = s.settings.metrics();
        let texts: Vec<String> = metrics.iter()
            .map(|m| format_metric(*m, &s.values, s.fps_note, s.settings.show_labels))
            .collect();
        let samples: Vec<&'static str> = metrics.iter().map(|m| m.sample(s.settings.show_labels)).collect();
        (texts, samples, s.settings.clone(), s.graph_samples.clone())
    });
    let Some((texts, samples, settings, graph_samples)) = snapshot else { return; };

    unsafe {
        let mut window: RECT = zeroed();
        if GetWindowRect(hwnd, &mut window) == 0 { return; }
        let width = window.right - window.left;
        let height = window.bottom - window.top;
        if width <= 0 || height <= 0 { return; }

        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() { return; }
        let (Some(mut text_dib), Some(mut shadow_dib), Some(out_dib)) =
            (Dib::new(screen, width, height), Dib::new(screen, width, height), Dib::new(screen, width, height))
        else {
            ReleaseDC(std::ptr::null_mut(), screen);
            return;
        };

        let font = make_overlay_font(&settings);
        let measure_previous = if !font.is_null() { SelectObject(text_dib.dc, font) } else { std::ptr::null_mut() };
        let rects = layout_items(text_dib.dc, width, height, &settings, &samples);
        if !measure_previous.is_null() { SelectObject(text_dib.dc, measure_previous); }
        let align = match settings.horizontal_anchor { 1 => DT_CENTER, 2 => DT_RIGHT, _ => DT_LEFT };

        draw_coverage(&text_dib, font, &texts, &rects, align, 0, 0);
        if settings.shadow {
            let o = settings.shadow_offset();
            draw_coverage(&shadow_dib, font, &texts, &rects, align, o, o);
        }
        if !font.is_null() { DeleteObject(font); }

        // COLORREF is 0x00BBGGRR. The bitmap wants premultiplied BGRA.
        let color = settings.color_ref();
        let (cr, cg, cb) = (color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF);
        let have_shadow = settings.shadow;
        let mut out_dib = out_dib;
        {
            let text_px = text_dib.pixels();
            let shadow_px = shadow_dib.pixels();
            let out_px = out_dib.pixels();
            for i in 0..out_px.len() {
                // Greyscale coverage: all three channels match, so the green one is enough.
                let t = (text_px[i] >> 8) & 0xFF;
                let s = if have_shadow { ((shadow_px[i] >> 8) & 0xFF) * SHADOW_ALPHA / 255 } else { 0 };
                // Text over shadow, shadow colour is black (adds no colour of its own).
                let alpha = t + s * (255 - t) / 255;
                out_px[i] = (alpha << 24)
                    | ((cr * t / 255) << 16)
                    | ((cg * t / 255) << 8)
                    | (cb * t / 255);
            }
        }

        if settings.show_graph {
            let graph_w = settings.graph_width.min(width - OVERLAY_PAD_X * 2).max(10);
            let shadow_off = if settings.shadow { settings.shadow_offset() } else { 0 };
            let left = match settings.horizontal_anchor {
                1 => (width - graph_w) / 2,
                2 => width - OVERLAY_PAD_X - shadow_off - graph_w,
                _ => OVERLAY_PAD_X,
            };
            let top = rects.iter().map(|r| r.bottom).max().unwrap_or(0) + GRAPH_GAP;
            draw_graph(out_dib.pixels(), width, height, left, top, graph_w, settings.graph_height, &graph_samples, (cr, cg, cb));
        }

        let position = POINT { x: window.left, y: window.top };
        let size = SIZE { cx: width, cy: height };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { blend_op: 0, blend_flags: 0, source_constant_alpha: 255, alpha_format: AC_SRC_ALPHA };
        UpdateLayeredWindow(hwnd, screen, &position, &size, out_dib.dc, &source, 0, &blend, ULW_ALPHA);
        drop(out_dib);
        drop(text_dib);
        drop(shadow_dib);
        ReleaseDC(std::ptr::null_mut(), screen);
    }
}

// A small bar graph of recent frame times, newest on the right. Bars that are much taller
// than the typical frame (stutter) are drawn in red. The scale follows the typical frame time
// so it does not jump around.
pub(crate) fn draw_graph(
    pixels: &mut [u32], width: i32, height: i32, left: i32, top: i32, graph_w: i32, graph_h: i32,
    samples: &[f32], rgb: (u32, u32, u32),
) {
    let index = |x: i32, y: i32| (y * width + x) as usize;
    let bottom = (top + graph_h).min(height);
    let right = (left + graph_w).min(width);
    if left < 0 || top < 0 || bottom <= top || right <= left { return; }
    // Dark, see-through backdrop so the bars stay readable over a bright game.
    let backdrop = 120u32 << 24;
    for y in top..bottom {
        for x in left..right { pixels[index(x, y)] = backdrop; }
    }
    if samples.is_empty() { return; }

    let mut sorted: Vec<f32> = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let typical = sorted[sorted.len() / 2].max(1.0);
    let scale_max = (typical * 3.0).clamp(20.0, 100.0);
    let normal = 0xFF00_0000 | (rgb.0 << 16) | (rgb.1 << 8) | rgb.2;
    let spike = 0xFFFF_6B6Bu32;

    let columns = (right - left).min(samples.len() as i32);
    let first = samples.len() - columns as usize;
    let height_px = (bottom - top) as f32;
    for column in 0..columns {
        let ms = samples[first + column as usize];
        let bar = ((ms / scale_max).min(1.0) * height_px).round().max(1.0) as i32;
        let color = if ms > typical * 1.8 { spike } else { normal };
        let x = right - columns + column;
        for step in 0..bar.min(bottom - top) {
            pixels[index(x, bottom - 1 - step)] = color;
        }
    }
}

// Returns (widest sample, sum of all sample widths, line height).
pub(crate) fn measure_overlay_lines(settings: &OverlaySettings, samples: &[&str]) -> (i32, i32, i32) {
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

