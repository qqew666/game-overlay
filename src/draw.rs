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

pub const C_TEXT: u32 = argb(255, 65, 47, 37);
pub const C_DIM: u32 = argb(255, 117, 95, 77);
pub const C_TRACK: u32 = argb(70, 167, 131, 98);
pub const C_GREEN: u32 = argb(255, 100, 110, 65);
pub const C_AMBER: u32 = argb(255, 161, 109, 40);
pub const C_ORANGE: u32 = argb(255, 174, 93, 49);
pub const C_RED: u32 = argb(255, 166, 65, 60);
pub const C_ACCENT: u32 = argb(255, 137, 91, 60);
pub const C_SOFT: u32 = argb(255, 198, 157, 118);
pub const C_BG: u32 = argb(255, 246, 238, 224);
pub const C_CARD: u32 = argb(255, 255, 250, 241);
pub const C_BORDER: u32 = argb(255, 222, 205, 182);

pub const ALIGN_LEFT: i32 = 0;
pub const ALIGN_CENTER: i32 = 1;
pub const ALIGN_RIGHT: i32 = 2;

pub fn fps_color(fps: u32) -> u32 {
    match fps {
        0 => C_DIM,
        120.. => C_ACCENT,
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

/// Mimi artwork is compiled into the portable executable.
pub fn draw_mimi(g: *mut GpGraphics, x: f32, y: f32, size: f32) {
    static PIXELS: std::sync::OnceLock<(u32, u32, Vec<u8>)> = std::sync::OnceLock::new();
    let (w, h, pixels) = PIXELS.get_or_init(|| {
        let image = image::load_from_memory(include_bytes!("../assets/mimi-typh.jpg"))
            .expect("embedded Mimi artwork must be a valid JPEG")
            .to_rgba8();
        let (w, h) = image.dimensions();
        let mut bytes = image.into_raw();
        for pixel in bytes.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        (w, h, bytes)
    });
    unsafe {
        let mut bitmap = std::ptr::null_mut();
        if GdipCreateBitmapFromScan0(
            *w as i32,
            *h as i32,
            (*w * 4) as i32,
            0x0026200A,
            Some(pixels.as_ptr()),
            &mut bitmap,
        ) == Status(0)
        {
            let _ = GdipDrawImageRect(g, bitmap.cast(), x, y, size, size);
            let _ = GdipDisposeImage(bitmap.cast());
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
    if d == 0.0 {
        unsafe {
            let _ = GdipAddPathRectangle(p, x, y, w, h);
        }
        return p;
    }
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
        if GdipCreateSolidFill(crate::appearance::tint(color), &mut brush) == Status(0)
            && !brush.is_null()
        {
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
        if GdipCreatePen1(crate::appearance::tint(color), width, Unit(2), &mut pen) == Status(0)
            && !pen.is_null()
        {
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
    let pts: Vec<PointF> = points
        .iter()
        .map(|(x, y)| PointF { X: *x, Y: *y })
        .collect();
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
        let mut status = GdipCreateFontFamilyFromName(
            PCWSTR(wanted.as_ptr()),
            std::ptr::null_mut(),
            &mut family,
        );
        if status != Status(0) {
            let fallback: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            status = GdipCreateFontFamilyFromName(
                PCWSTR(fallback.as_ptr()),
                std::ptr::null_mut(),
                &mut family,
            );
        }
        if status != Status(0) {
            status = GdipGetGenericFontFamilySansSerif(&mut family);
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
        let _ = GdipCreateSolidFill(crate::appearance::tint(color), &mut brush);

        let mut fmt: *mut GpStringFormat = std::ptr::null_mut();
        let _ = GdipCreateStringFormat(0, 0, &mut fmt);
        if !fmt.is_null() {
            let _ = GdipSetStringFormatAlign(fmt, StringAlignment(align));
            let _ = GdipSetStringFormatLineAlign(fmt, StringAlignmentCenter);
            let _ = GdipSetStringFormatTrimming(fmt, StringTrimmingEllipsisCharacter);
        }

        let layout = RectF {
            X: x,
            Y: y,
            Width: w,
            Height: h,
        };
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
        let bmp = match CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0)
        {
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

/// A shaped vertical blend, reused for the glass body and its narrow edge reflections.
fn glass_gradient(
    g: *mut GpGraphics,
    shape: *mut GpPath,
    rect: RectF,
    colors: &[u32],
    positions: &[f32],
) {
    unsafe {
        let mut brush = std::ptr::null_mut();
        if GdipCreateLineBrushFromRect(
            &rect,
            colors[0],
            *colors.last().unwrap(),
            LinearGradientModeVertical,
            WrapModeTileFlipXY,
            &mut brush,
        ) == Status(0)
        {
            let _ = GdipSetLinePresetBlend(
                brush,
                colors.as_ptr(),
                positions.as_ptr(),
                colors.len() as i32,
            );
            let _ = GdipFillPath(g, brush.cast(), shape);
            let _ = GdipDeleteBrush(brush.cast());
        }
    }
}

/// Convex glass with a tinted body, soft elevation and paired refractive rims.
pub fn glass_card(g: *mut GpGraphics, x: f32, y: f32, w: f32, h: f32, r: f32, base: u32) {
    let r = r.min(w.min(h) / 2.);
    let dark = crate::appearance::THEME.load(std::sync::atomic::Ordering::Relaxed) == 4;
    for (spread, alpha) in [(5., 9), (3., 14), (1., 22)] {
        round_fill(
            g,
            x - spread,
            y + 3.,
            w + spread * 2.,
            h + spread,
            r + spread,
            argb(alpha, 30, 18, 8),
        );
    }
    let shape = rounded_rect(x, y, w, h, r);
    let color = crate::appearance::tint(base) & 0xffffff;
    let tint = |alpha: u32| (alpha << 24) | color;
    let colors = [
        argb(if dark { 54 } else { 180 }, 255, 255, 255),
        tint(210),
        tint(158),
        tint(188),
        argb(if dark { 38 } else { 155 }, 255, 255, 255),
    ];
    glass_gradient(
        g,
        shape,
        RectF {
            X: x,
            Y: y,
            Width: w,
            Height: h,
        },
        &colors,
        &[0., 0.12, 0.48, 0.76, 1.],
    );
    stroke(
        g,
        shape,
        argb(if dark { 115 } else { 205 }, 255, 255, 255),
        1.4,
    );
    path_free(shape);
    let inner = rounded_rect(x + 2., y + 2., w - 4., h - 4., (r - 2.).max(1.));
    stroke(
        g,
        inner,
        argb(if dark { 34 } else { 72 }, 255, 255, 255),
        1.,
    );
    path_free(inner);
    // Two curved open strokes emphasize the upper lip and lower refraction.
    let lip = path();
    unsafe {
        let radius = (r - 2.).max(1.);
        let diameter = radius * 2.;
        let _ = GdipAddPathArc(lip, x + 2., y + 2., diameter, diameter, 185., 85.);
        let _ = GdipAddPathLine(lip, x + r, y + 2., x + w - r, y + 2.);
        let _ = GdipAddPathArc(
            lip,
            x + w - diameter - 2.,
            y + 2.,
            diameter,
            diameter,
            270.,
            85.,
        );
    }
    stroke(
        g,
        lip,
        argb(if dark { 150 } else { 235 }, 255, 255, 255),
        1.2,
    );
    path_free(lip);
}

pub fn glass_background(g: *mut GpGraphics, w: f32, h: f32) {
    let shape = rounded_rect(0., 0., w, h, 0.);
    let background = crate::appearance::tint(C_BG);
    let accent = crate::appearance::tint(C_SOFT);
    // A broad, subdued light field remains visible through translucent controls.
    glass_gradient(
        g,
        shape,
        RectF {
            X: 0.,
            Y: 0.,
            Width: w,
            Height: h,
        },
        &[background, accent, background],
        &[0., 0.53, 1.],
    );
    path_free(shape);
}
