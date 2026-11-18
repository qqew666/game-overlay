//! In-process image viewer for the user-provided artwork.
use crate::draw::{self, Surface};
use std::result::Result::Ok;
use std::sync::OnceLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MonitorFromWindow, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
fn pixels() -> Option<&'static (u32, u32, Vec<u8>)> {
    static IMAGE: OnceLock<Option<(u32, u32, Vec<u8>)>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            let image = image::load_from_memory(include_bytes!("../assets/mimi-art.jpeg"))
                .ok()?
                .to_rgba8();
            let (width, height) = image.dimensions();
            let mut bytes = image.into_raw();
            for pixel in bytes.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            Some((width, height, bytes))
        })
        .as_ref()
}
unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            if let Some(surface) = Surface::new(rect.right, rect.bottom) {
                let mut graphics = std::ptr::null_mut();
                if GdipCreateFromHDC(surface.dc(), &mut graphics) == Status(0) {
                    draw::round_fill(
                        graphics,
                        0.,
                        0.,
                        rect.right as f32,
                        rect.bottom as f32,
                        0.,
                        draw::C_BG,
                    );
                    if let Some((width, height, data)) = pixels() {
                        let scale = ((rect.right - 24).max(1) as f32 / *width as f32)
                            .min((rect.bottom - 24).max(1) as f32 / *height as f32);
                        let (w, h) = (*width as f32 * scale, *height as f32 * scale);
                        let mut bitmap = std::ptr::null_mut();
                        if GdipCreateBitmapFromScan0(
                            *width as i32,
                            *height as i32,
                            (*width * 4) as i32,
                            0x26200a,
                            Some(data.as_ptr()),
                            &mut bitmap,
                        ) == Status(0)
                        {
                            let _ = GdipSetInterpolationMode(
                                graphics,
                                InterpolationModeHighQualityBicubic,
                            );
                            let _ = GdipDrawImageRect(
                                graphics,
                                bitmap.cast(),
                                (rect.right as f32 - w) / 2.,
                                (rect.bottom as f32 - h) / 2.,
                                w,
                                h,
                            );
                            let _ = GdipDisposeImage(bitmap.cast());
                        }
                    } else {
                        draw::text(
                            graphics,
                            "Не удалось прочитать изображение",
                            14.,
                            false,
                            draw::C_TEXT,
                            0.,
                            0.,
                            rect.right as f32,
                            rect.bottom as f32,
                            draw::ALIGN_CENTER,
                        );
                    }
                    let _ = GdipDeleteGraphics(graphics);
                }
                surface.blit(dc);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_SIZE => {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_KEYDOWN if wp.0 == 27 => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wp, lp),
    }
}
pub unsafe fn show(parent: HWND) {
    draw::gdiplus_init();
    if let Ok(hwnd) = FindWindowW(w!("GameOverlayArtwork"), None) {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
        return;
    }
    let module = GetModuleHandleW(None).unwrap_or_default();
    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: HINSTANCE(module.0),
        lpszClassName: w!("GameOverlayArtwork"),
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        hIcon: LoadIconW(module, PCWSTR(1usize as *const u16)).unwrap_or_default(),
        ..Default::default()
    };
    RegisterClassW(&class);
    let mut monitor = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(
        MonitorFromWindow(parent, MONITOR_DEFAULTTONEAREST),
        &mut monitor,
    )
    .as_bool()
    {
        return;
    }
    let area = monitor.rcWork;
    let (width, height) = pixels()
        .map(|(w, h, _)| (*w as f32, *h as f32))
        .unwrap_or((600., 700.));
    let scale = ((area.right - area.left - 100).max(100) as f32 / width)
        .min((area.bottom - area.top - 120).max(100) as f32 / height)
        .min(1.0);
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: (width * scale) as i32 + 24,
        bottom: (height * scale) as i32 + 24,
    };
    let style = WS_OVERLAPPEDWINDOW;
    let _ = AdjustWindowRect(&mut rect, style, false);
    if let Ok(hwnd) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("GameOverlayArtwork"),
        w!("Mimi — изображение"),
        style,
        area.left + ((area.right - area.left) - (rect.right - rect.left)) / 2,
        area.top + ((area.bottom - area.top) - (rect.bottom - rect.top)) / 2,
        rect.right - rect.left,
        rect.bottom - rect.top,
        parent,
        None,
        module,
        None,
    ) {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
}
