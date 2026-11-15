//! GDI+ drawing helpers shared by the overlay card and the control panel.
//!
//! Everything is drawn into a 32-bpp top-down DIB so both surfaces antialias
//! the same way and the overlay can hand the DIB straight to
//! `UpdateLayeredWindow`.

// GDI+ exports a `Status` constant named `Ok`; import the real variants so the
// glob import below cannot shadow them.
use std::result::Result::{Err, Ok};
use std::sync::Once;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HANDLE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Graphics::GdiPlus::*;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// `0xAARRGGBB`, the layout GDI+ expects.
pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub const C_TEXT: u32 = argb(255, 233, 237, 246);
pub const C_DIM: u32 = argb(195, 152, 164, 190);
pub const C_TRACK: u32 = argb(48, 120, 140, 180);
pub const C_GREEN: u32 = argb(255, 96, 240, 150);
pub const C_AMBER: u32 = argb(255, 255, 205, 96);
pub const C_ORANGE: u32 = argb(255, 255, 148, 84);
pub const C_RED: u32 = argb(255, 255, 92, 122);
pub const C_CYAN: u32 = argb(255, 62, 226, 255);
pub const C_MAGENTA: u32 = argb(255, 255, 82, 214);

pub const ALIGN_LEFT: i32 = 0;
pub const ALIGN_CENTER: i32 = 1;
pub const ALIGN_RIGHT: i32 = 2;

pub fn fps_color(fps: u32) -> u32 {
    match fps {
        0 => C_DIM,
        120.. => C_CYAN,
        60..=119 => C_GREEN,
        30..=59 => C_AMBER,
        _ => C_RED,
    }
}

pub fn ping_color(ms: i32, lost: bool) -> u32 {
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
// Paths and brushes
// ---------------------------------------------------------------------------

pub fn gdiplus_init() {
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

pub fn path() -> *mut GpPath {
    let mut p: *mut GpPath = std::ptr::null_mut();
    unsafe {
        let _ = GdipCreatePath(FillModeAlternate, &mut p);
    }
    p
}

pub fn path_free(p: *mut GpPath) {
    if !p.is_null() {
        unsafe {
            let _ = GdipDeletePath(p);
        }
    }
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut GpPath {
    let p = path();
    if p.is_null() {
        return p;
    }
    let d = (r * 2.0).min(w.min(h)).max(0.0);
    unsafe {
        let _ = GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
        let _ = GdipAddPathArc(p, x + w - d, y, d, d, 270.0, 90.0);
        let _ = GdipAddPathArc(p, x + w - d, y + h - d, d, d, 0.0, 90.0);
        let _ = GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
        let _ = GdipClosePathFigure(p);
    }
    p
}

pub fn ellipse_path(x: f32, y: f32, w: f32, h: f32) -> *mut GpPath {
    let p = path();
    if !p.is_null() {
        unsafe {
            let _ = GdipAddPathEllipse(p, x, y, w, h);
        }
    }
    p
}

pub fn fill(g: *mut GpGraphics, p: *mut GpPath, color: u32) {
    if p.is_null() || g.is_null() {
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

pub fn stroke(g: *mut GpGraphics, p: *mut GpPath, color: u32, width: f32) {
    if p.is_null() || g.is_null() {
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

pub fn polygon(points: &[(f32, f32)]) -> *mut GpPath {
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

pub fn line(g: *mut GpGraphics, x1: f32, y1: f32, x2: f32, y2: f32, color: u32, width: f32) {
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

/// Fills a rounded rectangle in one call.
pub fn round_fill(g: *mut GpGraphics, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
    let p = rounded_rect(x, y, w, h, r);
    fill(g, p, color);
    path_free(p);
}

/// Fills and outlines a rounded rectangle in one call.
pub fn round_card(
    g: *mut GpGraphics,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    bg: u32,
    border: u32,
) {
    let p = rounded_rect(x, y, w, h, r);
    fill(g, p, bg);
    stroke(g, p, border, 1.0);
    path_free(p);
}

/// Draws one line of text inside `rect`, vertically centered. `align` follows
/// `StringAlignment`: 0 = left, 1 = center, 2 = right.
pub fn text(
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
    text_in(g, "Segoe UI", s, size, bold, color, x, y, w, h, align);
}

/// Like [`text`], but with a font family. Falls back to Segoe UI when the
/// requested family is not installed, so a missing font never blanks a label.
pub fn text_in(
    g: *mut GpGraphics,
    family_name: &str,
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
    if s.is_empty() || g.is_null() {
        return;
    }
    unsafe {
        let wanted: Vec<u16> = format!("{family_name}\0").encode_utf16().collect();
        let mut family: *mut GpFontFamily = std::ptr::null_mut();
        let mut status = GdipCreateFontFamilyFromName(PCWSTR(wanted.as_ptr()), std::ptr::null_mut(), &mut family);
        if status != Status(0) {
            let fallback: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            status = GdipCreateFontFamilyFromName(
                PCWSTR(fallback.as_ptr()),
                std::ptr::null_mut(),
                &mut family,
            );
        }
        if status != Status(0) {
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
            let _ = GdipSetStringFormatTrimming(fmt, StringTrimmingEllipsisCharacter);
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
// Transforms
// ---------------------------------------------------------------------------

/// Rotates the following drawing by `degrees` around `(cx, cy)`.
pub fn push_rotation(g: *mut GpGraphics, degrees: f32, cx: f32, cy: f32) {
    unsafe {
        let _ = GdipTranslateWorldTransform(g, cx, cy, MatrixOrderAppend);
        let _ = GdipRotateWorldTransform(g, degrees, MatrixOrderAppend);
        let _ = GdipTranslateWorldTransform(g, -cx, -cy, MatrixOrderAppend);
    }
}

pub fn reset_transform(g: *mut GpGraphics) {
    unsafe {
        let _ = GdipResetWorldTransform(g);
    }
}

// ---------------------------------------------------------------------------
// Boykisser
// ---------------------------------------------------------------------------

/// Vector boykisser: white cat head, black outline, ":3" mouth, pink blush.
pub fn draw_boykisser(g: *mut GpGraphics, cx: f32, cy: f32, size: f32) {
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
// Off-screen surface
// ---------------------------------------------------------------------------

/// 32-bpp top-down DIB with its own memory DC.
///
/// The overlay hands [`Surface::dc`] and the bits to `UpdateLayeredWindow`
/// after [`Surface::premultiply`]; the panel blits it to the window DC, which
/// keeps the custom painting flicker free.
pub struct Surface {
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    pub bits: *mut u32,
    pub w: i32,
    pub h: i32,
}

impl Surface {
    pub unsafe fn new(w: i32, h: i32) -> Option<Surface> {
        if w <= 0 || h <= 0 {
            return None;
        }
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

    pub fn dc(&self) -> HDC {
        self.mem
    }

    /// `UpdateLayeredWindow` wants premultiplied alpha; GDI+ writes straight
    /// alpha, so the RGB channels get scaled by A/255 in place.
    pub unsafe fn premultiply(&self) {
        let count = (self.w * self.h) as usize;
        for i in 0..count {
            let px = *self.bits.add(i);
            let a = (px >> 24) & 0xff;
            if a == 255 || a == 0 {
                continue;
            }
            let scale = |c: u32| -> u32 { (c * a + 127) / 255 };
            let p = (a << 24)
                | (scale((px >> 16) & 0xff) << 16)
                | (scale((px >> 8) & 0xff) << 8)
                | scale(px & 0xff);
            *self.bits.add(i) = p;
        }
    }

    /// Copies the whole surface onto `dst`.
    pub unsafe fn blit(&self, dst: HDC) {
        use windows::Win32::Graphics::Gdi::{BitBlt, SRCCOPY};
        let _ = BitBlt(dst, 0, 0, self.w, self.h, self.mem, 0, 0, SRCCOPY);
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
