//! Transparent click-through overlay window.
//!
//! The overlay is drawn into a 32-bpp DIB with GDI+ (antialiased rounded card,
//! vector boykisser, text, ping graph) and blitted to a layered window with
//! `UpdateLayeredWindow`, so the corners are truly transparent instead of a
//! square window with a constant alpha.

// GDI+ exports a `Status` constant named `Ok`; import the real variants so the
// glob import below cannot shadow them.
use std::result::Result::{Err, Ok};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Instant;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    BOOL, COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, EndPaint, DeleteDC, DeleteObject,
    SelectObject, AC_SRC_ALPHA, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, PAINTSTRUCT,
};
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_NOREPEAT, VK_END, VK_F1, VK_F2, VK_F3, VK_F4, VK_F6,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Shared state between the overlay and the measuring threads.
pub struct AppState {
    /// Round trip time in tenths of a millisecond, `-1` when lost.
    pub ping_deci: AtomicI32,
    pub ping_lost: AtomicBool,
    pub fps: AtomicU32,
    pub frametime_centi_ms: AtomicU32,
    pub visible: AtomicBool,
    pub target_index: AtomicUsize,
    pub follow_window: AtomicBool,
    pub should_quit: AtomicBool,
    pub verbose: AtomicBool,
    pub autostart: AtomicBool,
    /// Rolling ping samples, newest last. Capped at `HISTORY_LEN`.
    pub history: Mutex<Vec<i32>>,
    /// `host:port` of the target currently being probed.
    pub target_name: Mutex<String>,
    /// Short-lived status line ("autostart: on") and when it was raised.
    pub notice: Mutex<Option<(String, Instant)>>,
    /// Window position chosen by the positioning pass, used when blitting.
    pub pos: Mutex<(i32, i32)>,
}

const HISTORY_LEN: usize = 60;

impl AppState {
    pub fn new() -> Arc<AppState> {
        Arc::new(AppState {
            ping_deci: AtomicI32::new(-1),
            ping_lost: AtomicBool::new(true),
            fps: AtomicU32::new(0),
            frametime_centi_ms: AtomicU32::new(0),
            visible: AtomicBool::new(true),
            target_index: AtomicUsize::new(0),
            follow_window: AtomicBool::new(true),
            should_quit: AtomicBool::new(false),
            verbose: AtomicBool::new(false),
            autostart: AtomicBool::new(false),
            history: Mutex::new(Vec::new()),
            target_name: Mutex::new(String::new()),
            notice: Mutex::new(None),
            pos: Mutex::new((0, 0)),
        })
    }

    pub fn push_ping_sample(&self, ms: i32) {
        if let Ok(mut h) = self.history.lock() {
            h.push(ms);
            if h.len() > HISTORY_LEN {
                h.remove(0);
            }
        }
    }

    pub fn set_notice(&self, text: impl Into<String>) {
        if let Ok(mut n) = self.notice.lock() {
            *n = Some((text.into(), Instant::now()));
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry / palette
// ---------------------------------------------------------------------------

const CARD_W: f32 = 300.0;
const CARD_H: f32 = 108.0;
const SHADOW: f32 = 8.0;
const WIN_W: i32 = 316;
const WIN_H: i32 = 124;
const RADIUS: f32 = 16.0;
const NOTICE_SECS: f32 = 2.5;

/// `0xAARRGGBB`, the layout GDI+ expects.
const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

const C_DIM: u32 = argb(160, 132, 143, 166);
const C_TRACK: u32 = argb(48, 120, 140, 180);
const C_GREEN: u32 = argb(255, 96, 240, 150);
const C_AMBER: u32 = argb(255, 255, 205, 96);
const C_ORANGE: u32 = argb(255, 255, 148, 84);
const C_RED: u32 = argb(255, 255, 92, 122);
const C_CYAN: u32 = argb(255, 62, 226, 255);
const C_MAGENTA: u32 = argb(255, 255, 82, 214);

fn fps_color(fps: u32) -> u32 {
    match fps {
        0 => C_DIM,
        120.. => C_CYAN,
        60..=119 => C_GREEN,
        30..=59 => C_AMBER,
        _ => C_RED,
    }
}

fn ping_color(ms: i32, lost: bool) -> u32 {
    if lost || ms < 0 {
        C_RED
    } else {
        match ms {
            0..=49 => C_GREEN,
            50..=99 => C_AMBER,
            100..=149 => C_ORANGE,
            _ => C_RED,
        }
    }
}

// ---------------------------------------------------------------------------
// GDI+ helpers
// ---------------------------------------------------------------------------

fn gdiplus_init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        let mut token: usize = 0;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: BOOL(0),
            SuppressExternalCodecs: BOOL(0),
        };
        let _ = GdiplusStartup(&mut token, &input, std::ptr::null_mut());
    });
}

fn path() -> *mut GpPath {
    let mut p: *mut GpPath = std::ptr::null_mut();
    unsafe {
        let _ = GdipCreatePath(FillModeAlternate, &mut p);
    }
    p
}

fn path_free(p: *mut GpPath) {
    if !p.is_null() {
        unsafe {
            let _ = GdipDeletePath(p);
        }
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut GpPath {
    let p = path();
    if p.is_null() {
        return p;
    }
    let d = (r * 2.0).min(w.min(h));
    unsafe {
        let _ = GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
        let _ = GdipAddPathArc(p, x + w - d, y, d, d, 270.0, 90.0);
        let _ = GdipAddPathArc(p, x + w - d, y + h - d, d, d, 0.0, 90.0);
        let _ = GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
        let _ = GdipClosePathFigure(p);
    }
    p
}

fn ellipse_path(x: f32, y: f32, w: f32, h: f32) -> *mut GpPath {
    let p = path();
    if !p.is_null() {
        unsafe {
            let _ = GdipAddPathEllipse(p, x, y, w, h);
        }
    }
    p
}

fn fill(g: *mut GpGraphics, p: *mut GpPath, color: u32) {
    if p.is_null() {
        return;
    }
    unsafe {
        let mut brush: *mut GpSolidFill = std::ptr::null_mut();
        if GdipCreateSolidFill(color, &mut brush) == Status(0) && !brush.is_null() {
            let _ = GdipFillPath(g, brush.cast(), p);
            let _ = GdipDeleteBrush(brush.cast());
        }
    }
}

fn stroke(g: *mut GpGraphics, p: *mut GpPath, color: u32, width: f32) {
    if p.is_null() {
        return;
    }
    unsafe {
        let mut pen: *mut GpPen = std::ptr::null_mut();
        if GdipCreatePen1(color, width, Unit(2), &mut pen) == Status(0) && !pen.is_null() {
            let _ = GdipDrawPath(g, pen, p);
            let _ = GdipDeletePen(pen);
        }
    }
}

fn polygon(points: &[(f32, f32)]) -> *mut GpPath {
    let p = path();
    if p.is_null() || points.is_empty() {
        return p;
    }
    let pts: Vec<PointF> = points.iter().map(|(x, y)| PointF { X: *x, Y: *y }).collect();
    unsafe {
        let _ = GdipAddPathPolygon(p, pts.as_ptr(), pts.len() as i32);
    }
    p
}

fn line(g: *mut GpGraphics, x1: f32, y1: f32, x2: f32, y2: f32, color: u32, width: f32) {
    let p = path();
    if p.is_null() {
        return;
    }
    unsafe {
        let _ = GdipAddPathLine(p, x1, y1, x2, y2);
        stroke(g, p, color, width);
        path_free(p);
    }
}

/// Draws one line of text inside `rect`, vertically centered. `align` follows
/// `StringAlignment`: 0 = left, 1 = center, 2 = right.
fn text(
    g: *mut GpGraphics,
    s: &str,
    size: f32,
    bold: bool,
    color: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    align: i32,
) {
    if s.is_empty() {
        return;
    }
    unsafe {
        let family_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
        let mut family: *mut GpFontFamily = std::ptr::null_mut();
        if GdipCreateFontFamilyFromName(PCWSTR(family_name.as_ptr()), std::ptr::null_mut(), &mut family)
            != Status(0)
        {
            return;
        }

        let style = if bold { 1 } else { 0 };
        let mut font: *mut GpFont = std::ptr::null_mut();
        if GdipCreateFont(family, size, style, Unit(2), &mut font) != Status(0) {
            let _ = GdipDeleteFontFamily(family);
            return;
        }

        let mut brush: *mut GpSolidFill = std::ptr::null_mut();
        let _ = GdipCreateSolidFill(color, &mut brush);

        let mut fmt: *mut GpStringFormat = std::ptr::null_mut();
        let _ = GdipCreateStringFormat(0, 0, &mut fmt);
        if !fmt.is_null() {
            let _ = GdipSetStringFormatAlign(fmt, StringAlignment(align));
            let _ = GdipSetStringFormatLineAlign(fmt, StringAlignmentCenter);
        }

        let layout = RectF { X: x, Y: y, Width: w, Height: h };
        let mut buf: Vec<u16> = s.encode_utf16().collect();

        if !brush.is_null() {
            let _ = GdipDrawString(
                g,
                PCWSTR(buf.as_mut_ptr()),
                buf.len() as i32,
                font,
                &layout,
                fmt,
                brush.cast(),
            );
        }

        if !fmt.is_null() {
            let _ = GdipDeleteStringFormat(fmt);
        }
        if !brush.is_null() {
            let _ = GdipDeleteBrush(brush.cast());
        }
        let _ = GdipDeleteFont(font);
        let _ = GdipDeleteFontFamily(family);
    }
}

// ---------------------------------------------------------------------------
// Boykisser
// ---------------------------------------------------------------------------

/// Vector boykisser: white cat head, black outline, ":3" mouth, pink blush.
fn draw_boykisser(g: *mut GpGraphics, cx: f32, cy: f32, size: f32) {
    let s = size;
    let white = argb(255, 250, 250, 253);
    let ink = argb(255, 26, 28, 36);
    let blush = argb(190, 255, 128, 176);

    let head_w = s * 0.88;
    let head_h = s * 0.68;
    let hx = cx - head_w / 2.0;
    let hy = cy - head_h / 2.0 + s * 0.08;

    // Ears first, so the head outline overlaps their base.
    let left_ear = polygon(&[
        (cx - s * 0.36, hy + s * 0.16),
        (cx - s * 0.31, hy - s * 0.24),
        (cx - s * 0.07, hy + s * 0.02),
    ]);
    let right_ear = polygon(&[
        (cx + s * 0.36, hy + s * 0.16),
        (cx + s * 0.31, hy - s * 0.24),
        (cx + s * 0.07, hy + s * 0.02),
    ]);
    for ear in [left_ear, right_ear] {
        fill(g, ear, white);
        stroke(g, ear, ink, 2.0);
        path_free(ear);
    }

    let head = ellipse_path(hx, hy, head_w, head_h);
    fill(g, head, white);
    stroke(g, head, ink, 2.2);

    // Eyes.
    for dx in [-1.0f32, 1.0] {
        let eye = ellipse_path(cx + dx * s * 0.16 - s * 0.045, cy - s * 0.04, s * 0.09, s * 0.13);
        fill(g, eye, ink);
        path_free(eye);
    }

    // Blush.
    for dx in [-1.0f32, 1.0] {
        let b = ellipse_path(cx + dx * s * 0.26 - s * 0.07, cy + s * 0.06, s * 0.14, s * 0.07);
        fill(g, b, blush);
        path_free(b);
    }

    // The ":3" mouth: two bottom-half arcs side by side.
    let mouth = path();
    if !mouth.is_null() {
        unsafe {
            let _ = GdipAddPathArc(mouth, cx - s * 0.13, cy + s * 0.10, s * 0.13, s * 0.11, 0.0, 180.0);
            let _ = GdipAddPathArc(mouth, cx + s * 0.00, cy + s * 0.10, s * 0.13, s * 0.11, 0.0, 180.0);
        }
        stroke(g, mouth, ink, 1.8);
        path_free(mouth);
    }

    // Whiskers.
    for (ay, by) in [(-0.02f32, -0.10f32), (0.04, 0.02)] {
        line(g, cx - s * 0.44, cy + s * ay, cx - s * 0.56, cy + s * by, ink, 1.4);
        line(g, cx + s * 0.44, cy + s * ay, cx + s * 0.56, cy + s * by, ink, 1.4);
    }

    path_free(head);
}

// ---------------------------------------------------------------------------
// Surface (32 bpp DIB blitted with UpdateLayeredWindow)
// ---------------------------------------------------------------------------

struct Surface {
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
}

impl Surface {
    unsafe fn new(w: i32, h: i32) -> Option<Surface> {
        let mem = CreateCompatibleDC(HDC::default());
        if mem.0.is_null() {
            return None;
        }

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        // Negative height = top-down rows, matching the pixel loop below.
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bmp = match CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0) {
            Ok(b) => b,
            Err(_) => {
                let _ = DeleteDC(mem);
                return None;
            }
        };

        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);
            return None;
        }

        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        Some(Surface {
            mem,
            bmp,
            old,
            bits: bits.cast::<u32>(),
            w,
            h,
        })
    }

    /// `UpdateLayeredWindow` wants premultiplied alpha; GDI+ writes straight
    /// alpha, so the RGB channels get scaled by A/255 in place.
    unsafe fn premultiply(&self) {
        let count = (self.w * self.h) as usize;
        for i in 0..count {
            let px = *self.bits.add(i);
            let a = (px >> 24) & 0xff;
            if a == 255 || a == 0 {
                continue;
            }
            let scale = |c: u32| -> u32 { (c * a + 127) / 255 };
            let p = (a << 24) | (scale((px >> 16) & 0xff) << 16) | (scale((px >> 8) & 0xff) << 8) | scale(px & 0xff);
            *self.bits.add(i) = p;
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bmp.0));
            let _ = DeleteDC(self.mem);
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

unsafe fn render(hwnd: HWND, st: &AppState) {
    gdiplus_init();

    let Some(surface) = Surface::new(WIN_W, WIN_H) else {
        return;
    };

    let mut g: *mut GpGraphics = std::ptr::null_mut();
    if GdipCreateFromHDC(surface.mem, &mut g) != Status(0) || g.is_null() {
        return;
    }
    let _ = GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
    let _ = GdipSetTextRenderingHint(g, TextRenderingHintAntiAliasGridFit);

    let ox = SHADOW;
    let oy = SHADOW;

    // Drop shadow: two stacked rounded rects, wider and softer at the back.
    let s2 = rounded_rect(ox - 1.0, oy + 4.0, CARD_W + 2.0, CARD_H, RADIUS + 2.0);
    fill(g, s2, argb(46, 0, 0, 0));
    path_free(s2);
    let s1 = rounded_rect(ox, oy + 2.0, CARD_W, CARD_H, RADIUS);
    fill(g, s1, argb(70, 0, 0, 0));
    path_free(s1);

    // Card.
    let card = rounded_rect(ox, oy, CARD_W, CARD_H, RADIUS);
    fill(g, card, argb(238, 15, 17, 24));
    stroke(g, card, argb(80, 96, 112, 148), 1.0);

    // Accent rail: cyan on top, magenta below, clipped to the left edge.
    let accent_x = ox + 6.0;
    let accent_h = CARD_H - 22.0;
    let top = rounded_rect(accent_x, oy + 11.0, 4.0, accent_h * 0.58, 2.0);
    fill(g, top, C_CYAN);
    path_free(top);
    let bottom = rounded_rect(accent_x, oy + 11.0 + accent_h * 0.58, 4.0, accent_h * 0.42, 2.0);
    fill(g, bottom, C_MAGENTA);
    path_free(bottom);

    let fps = st.fps.load(Ordering::Relaxed);
    let ft = st.frametime_centi_ms.load(Ordering::Relaxed);
    let ping_deci = st.ping_deci.load(Ordering::Relaxed);
    let ping = if ping_deci < 0 { -1 } else { (ping_deci + 5) / 10 };
    let lost = st.ping_lost.load(Ordering::Relaxed);

    // FPS block.
    text(g, "FPS", 9.0, true, C_DIM, ox + 20.0, oy + 13.0, 80.0, 14.0, 0);
    let fps_value = if fps == 0 { "--".to_string() } else { fps.to_string() };
    text(g, &fps_value, 30.0, true, fps_color(fps), ox + 18.0, oy + 24.0, 90.0, 34.0, 0);
    let ft_text = if fps == 0 {
        "ожидание кадра".to_string()
    } else {
        format!("{:.1} ms кадр", ft as f32 / 100.0)
    };
    text(g, &ft_text, 9.5, false, C_DIM, ox + 20.0, oy + 58.0, 110.0, 12.0, 0);

    // Ping block.
    text(g, "PING", 9.0, true, C_DIM, ox + 146.0, oy + 13.0, 70.0, 14.0, 0);
    // Sub-10 ms values keep one decimal: a single digit is what a good route
    // to a game server looks like, and rounding it to "0" hides the difference.
    let ping_value = if lost || ping_deci < 0 {
        "--".to_string()
    } else if ping_deci < 100 {
        format!("{:.1}", ping_deci as f32 / 10.0)
    } else {
        format!("{ping}")
    };
    let ping_color_value = ping_color(ping, lost);
    // Right aligned so a one digit value and "120" share the same baseline
    // without a floating gap before the unit.
    text(g, &ping_value, 24.0, true, ping_color_value, ox + 130.0, oy + 25.0, 56.0, 28.0, 2);
    text(g, "ms", 10.0, false, ping_color_value, ox + 189.0, oy + 35.0, 24.0, 14.0, 0);

    let target = st.target_name.lock().map(|t| t.clone()).unwrap_or_default();
    let target_short: String = if target.chars().count() > 16 {
        format!("{}…", target.chars().take(15).collect::<String>())
    } else {
        target
    };
    text(g, &target_short, 9.5, false, C_DIM, ox + 146.0, oy + 58.0, 110.0, 12.0, 0);

    // Boykisser, right side.
    draw_boykisser(g, ox + CARD_W - 42.0, oy + 40.0, 56.0);

    // Ping graph across the bottom.
    let samples: Vec<i32> = st.history.lock().map(|h| h.clone()).unwrap_or_default();
    let graph_x = ox + 20.0;
    let graph_w = CARD_W - 40.0;
    let graph_y = oy + CARD_H - 22.0;
    let graph_h = 12.0;

    let track = rounded_rect(graph_x, graph_y + graph_h - 1.0, graph_w, 1.0, 0.5);
    fill(g, track, C_TRACK);
    path_free(track);

    if samples.len() >= 2 {
        let valid: Vec<f32> = samples
            .iter()
            .filter(|v| **v >= 0)
            .map(|v| *v as f32 / 10.0)
            .collect();
        if valid.len() >= 2 {
            let max = valid.iter().cloned().fold(1.0f32, f32::max).max(30.0);
            let step = graph_w / (samples.len().saturating_sub(1)).max(1) as f32;
            let poly = path();
            if !poly.is_null() {
                let mut first = true;
                for (i, v) in samples.iter().enumerate() {
                    let x = graph_x + step * i as f32;
                    let value = if *v < 0 { max } else { *v as f32 / 10.0 };
                    let y = graph_y + graph_h - (value / max).clamp(0.0, 1.0) * graph_h;
                    if first {
                        let _ = GdipAddPathLine(poly, x, y, x, y);
                        first = false;
                    } else {
                        let prev_v = if samples[i - 1] < 0 {
                            max
                        } else {
                            samples[i - 1] as f32 / 10.0
                        };
                        let py = graph_y + graph_h - (prev_v / max).clamp(0.0, 1.0) * graph_h;
                        let px = graph_x + step * (i - 1) as f32;
                        let _ = GdipAddPathLine(poly, px, py, x, y);
                    }
                }
                stroke(g, poly, ping_color(ping, lost), 1.6);
                path_free(poly);
            }

            // Marker on the newest sample.
            let last = *samples.last().unwrap_or(&0);
            let value = if last < 0 { max } else { last as f32 / 10.0 };
            let y = graph_y + graph_h - (value / max).clamp(0.0, 1.0) * graph_h;
            let dot = ellipse_path(graph_x + graph_w - 2.4, y - 2.4, 4.8, 4.8);
            fill(g, dot, ping_color(ping, lost));
            path_free(dot);
        }
    }

    // Transient status badge.
    if let Ok(n) = st.notice.lock() {
        if let Some((msg, at)) = n.as_ref() {
            let age = at.elapsed().as_secs_f32();
            if age < NOTICE_SECS {
                let alpha = (255.0 * (1.0 - age / NOTICE_SECS)).clamp(0.0, 255.0) as u8;
                let pill = rounded_rect(ox + CARD_W - 108.0, oy + 6.0, 100.0, 18.0, 9.0);
                fill(g, pill, argb(alpha.saturating_sub(30), 40, 44, 58));
                path_free(pill);
                text(
                    g,
                    msg,
                    9.0,
                    true,
                    argb(alpha, 236, 240, 248),
                    ox + CARD_W - 108.0,
                    oy + 7.0,
                    100.0,
                    16.0,
                    1,
                );
            }
        }
    }

    let _ = GdipDeleteGraphics(g);

    surface.premultiply();

    let (x, y) = st.pos.lock().map(|p| *p).unwrap_or((0, 0));
    let dst = POINT { x, y };
    let size = SIZE { cx: WIN_W, cy: WIN_H };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: 0,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    let _ = UpdateLayeredWindow(
        hwnd,
        HDC::default(),
        Some(&dst),
        Some(&size),
        surface.mem,
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
}

// ---------------------------------------------------------------------------
// Window plumbing
// ---------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppState;
                if !state.is_null() {
                    render(hwnd, &*state);
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            unsafe {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppState;
                if !state.is_null() {
                    let st = &*state;
                    position_overlay(hwnd, st);
                    render(hwnd, st);
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            unsafe {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppState;
                if !state.is_null() {
                    let st = &*state;
                    match wparam.0 as u32 {
                        1 => {
                            let v = !st.visible.load(Ordering::Relaxed);
                            st.visible.store(v, Ordering::Relaxed);
                            if v {
                                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                                render(hwnd, st);
                            } else {
                                let _ = ShowWindow(hwnd, SW_HIDE);
                            }
                        }
                        2 => {
                            let n = st.target_index.load(Ordering::Relaxed) + 1;
                            st.target_index.store(n, Ordering::Relaxed);
                            if let Ok(mut h) = st.history.lock() {
                                h.clear();
                            }
                        }
                        3 => {
                            let n = st.target_index.load(Ordering::Relaxed);
                            st.target_index.store(n.wrapping_sub(1), Ordering::Relaxed);
                            if let Ok(mut h) = st.history.lock() {
                                h.clear();
                            }
                        }
                        4 => {
                            let v = !st.follow_window.load(Ordering::Relaxed);
                            st.follow_window.store(v, Ordering::Relaxed);
                            st.set_notice(if v { "слежение: вкл" } else { "слежение: выкл" });
                        }
                        5 => {
                            st.should_quit.store(true, Ordering::Relaxed);
                            PostQuitMessage(0);
                        }
                        6 => match crate::autostart::toggle() {
                            Ok(true) => {
                                st.autostart.store(true, Ordering::Relaxed);
                                st.set_notice("автозапуск: вкл");
                            }
                            Ok(false) => {
                                st.autostart.store(false, Ordering::Relaxed);
                                st.set_notice("автозапуск: выкл");
                            }
                            Err(_) => st.set_notice("автозапуск: ошибка"),
                        },
                        _ => {}
                    }
                }
            }
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn position_overlay(hwnd: HWND, st: &AppState) {
    if !st.visible.load(Ordering::Relaxed) {
        return;
    }

    let mut pos = (0i32, 0i32);

    if st.follow_window.load(Ordering::Relaxed) {
        let fg = GetForegroundWindow();
        if !fg.is_invalid() && fg != hwnd {
            let mut r = windows::Win32::Foundation::RECT::default();
            if GetWindowRect(fg, &mut r).is_ok() {
                pos = (r.left + 16, r.top + 16);
            }
        }
    }

    if pos == (0, 0) {
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        pos = (screen_w - WIN_W - 16, 16);
    }

    if let Ok(mut p) = st.pos.lock() {
        *p = pos;
    }

    // The position also lands here so the window keeps the right size and
    // z-order even before the first blit.
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        pos.0,
        pos.1,
        WIN_W,
        WIN_H,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
}

/// FPS measurement based on the DXGI present counters reported by DWM.
pub fn fps_thread(state: Arc<AppState>) {
    let mut last_hwnd = HWND(std::ptr::null_mut());
    let mut last_count: u64 = 0;
    let mut last_time = Instant::now();

    loop {
        if state.should_quit.load(Ordering::Relaxed) {
            break;
        }

        let fg = unsafe { GetForegroundWindow() };
        if fg.is_invalid() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }

        if fg != last_hwnd {
            last_hwnd = fg;
            last_count = 0;
            last_time = Instant::now();
        }

        let mut info = DWM_TIMING_INFO::default();
        info.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;

        // Per-window queries fail with E_INVALIDARG on some builds, so fall
        // back to the global DWM counters (all DXGI presents in the session).
        let mut result = unsafe { DwmGetCompositionTimingInfo(fg, &mut info) };
        if result.is_err() {
            let mut global = DWM_TIMING_INFO::default();
            global.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;
            result = unsafe { DwmGetCompositionTimingInfo(HWND(std::ptr::null_mut()), &mut global) };
            if result.is_ok() {
                info = global;
            }
        }

        if result.is_ok() {
            let dx_present = info.cDXPresentSubmitted;
            let frame_submitted = info.cFrameSubmitted;
            let count = if dx_present > 0 {
                dx_present as u64
            } else {
                frame_submitted
            };

            if count > 0 && last_count > 0 && count > last_count {
                let dt = last_time.elapsed().as_secs_f64();
                if dt > 0.05 {
                    let fps = (count - last_count) as f64 / dt;
                    let ms = 1000.0 / fps;
                    let old = state.fps.load(Ordering::Relaxed) as f64;
                    let smoothed = if old <= 0.0 { fps } else { old * 0.6 + fps * 0.4 };
                    let fps_value = smoothed.round().clamp(0.0, 9999.0) as u32;
                    state.fps.store(fps_value, Ordering::Relaxed);
                    state
                        .frametime_centi_ms
                        .store((ms * 100.0).round() as u32, Ordering::Relaxed);
                    if state.verbose.load(Ordering::Relaxed) {
                        eprintln!("[fps] {} ({:.1} ms)", fps_value, ms);
                    }
                    last_count = count;
                    last_time = Instant::now();
                }
            } else if count > 0 && last_count == 0 {
                last_count = count;
                last_time = Instant::now();
            }
        } else if state.verbose.load(Ordering::Relaxed) {
            eprintln!("[fps] DwmGetCompositionTimingInfo failed: {:?}", result);
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Creates the overlay window and runs its message loop (blocking).
pub fn run_overlay(state: Arc<AppState>) {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = wide("GameOverlayClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE(hinstance.0),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hCursor: LoadCursorW(HINSTANCE(std::ptr::null_mut()), IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let ex_style = WS_EX_LAYERED
            | WS_EX_TRANSPARENT
            | WS_EX_TOPMOST
            | WS_EX_TOOLWINDOW
            | WS_EX_NOACTIVATE;

        let title = wide("GameOverlay");
        let hwnd = match CreateWindowExW(
            ex_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            0,
            0,
            WIN_W,
            WIN_H,
            HWND(std::ptr::null_mut()),
            HMENU(std::ptr::null_mut()),
            HINSTANCE(hinstance.0),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        let state_ptr = Arc::into_raw(state.clone()) as isize;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr);

        // F1 toggle, F2/F3 target, F4 follow window, F6 autostart, End quit.
        let hotkeys: [(i32, u32); 6] = [
            (1, VK_F1.0 as u32),
            (2, VK_F2.0 as u32),
            (3, VK_F3.0 as u32),
            (4, VK_F4.0 as u32),
            (5, VK_END.0 as u32),
            (6, VK_F6.0 as u32),
        ];
        for (id, vk) in hotkeys {
            if let Err(e) = RegisterHotKey(hwnd, id, MOD_NOREPEAT, vk) {
                if state.verbose.load(Ordering::Relaxed) {
                    eprintln!("[hotkey] id {id} (vk {vk:#x}) failed: {e:?}");
                }
            }
        }

        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex_style.0 | WS_EX_LAYERED.0) as isize);

        position_overlay(hwnd, &state);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        render(hwnd, &state);
        SetTimer(hwnd, 1, 100, None);

        let mut msg = MSG::default();
        loop {
            let r = GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0);
            if r.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
