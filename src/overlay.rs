//! Transparent click-through overlay window that draws the FPS / ping text.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_NOREPEAT, VK_END, VK_F1, VK_F2, VK_F3, VK_F4,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Shared state between the overlay thread and the measuring threads.
pub struct AppState {
    pub ping_ms: AtomicI32,
    pub ping_lost: AtomicBool,
    pub fps: AtomicU32,
    pub frametime_centi_ms: AtomicU32,
    pub visible: AtomicBool,
    pub target_index: AtomicUsize,
    pub follow_window: AtomicBool,
    pub should_quit: AtomicBool,
    pub verbose: AtomicBool,
}

impl AppState {
    pub fn new() -> Arc<AppState> {
        Arc::new(AppState {
            ping_ms: AtomicI32::new(-1),
            ping_lost: AtomicBool::new(true),
            fps: AtomicU32::new(0),
            frametime_centi_ms: AtomicU32::new(0),
            visible: AtomicBool::new(true),
            target_index: AtomicUsize::new(0),
            follow_window: AtomicBool::new(true),
            should_quit: AtomicBool::new(false),
            verbose: AtomicBool::new(false),
        })
    }
}

const OVERLAY_W: i32 = 210;
const OVERLAY_H: i32 = 62;

/// Creates a null terminated UTF-16 string that lives forever.
fn wide_static(s: &str) -> PCWSTR {
    let mut buf: Vec<u16> = s.encode_utf16().collect();
    buf.push(0);
    let leaked: &'static mut [u16] = Box::leak(buf.into_boxed_slice());
    PCWSTR(leaked.as_ptr())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            unsafe { paint(hwnd) };
            LRESULT(0)
        }
        WM_TIMER => {
            unsafe {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppState;
                if !state.is_null() {
                    let st = &*state;
                    position_overlay(hwnd, st);
                    let _ = InvalidateRect(hwnd, None, false);
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
                                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 235, LWA_ALPHA);
                                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            } else {
                                let _ = ShowWindow(hwnd, SW_HIDE);
                            }
                        }
                        2 => {
                            let n = st.target_index.load(Ordering::Relaxed) + 1;
                            st.target_index.store(n, Ordering::Relaxed);
                        }
                        3 => {
                            let n = st.target_index.load(Ordering::Relaxed);
                            st.target_index.store(n.wrapping_sub(1), Ordering::Relaxed);
                        }
                        4 => {
                            let v = !st.follow_window.load(Ordering::Relaxed);
                            st.follow_window.store(v, Ordering::Relaxed);
                        }
                        5 => {
                            st.should_quit.store(true, Ordering::Relaxed);
                            PostQuitMessage(0);
                        }
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

unsafe fn paint(hwnd: HWND) {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppState;
    if state.is_null() {
        return;
    }
    let st = &*state;

    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    // background
    let brush = CreateSolidBrush(COLORREF(0x00201A17));
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: OVERLAY_W,
        bottom: OVERLAY_H,
    };
    FillRect(hdc, &rect, brush);
    let _ = DeleteObject(brush);

    // accent bar
    let accent = CreateSolidBrush(COLORREF(0x00A0E8FF));
    rect.right = 4;
    FillRect(hdc, &rect, accent);
    let _ = DeleteObject(accent);

    let fps = st.fps.load(Ordering::Relaxed);
    let ft = st.frametime_centi_ms.load(Ordering::Relaxed);
    let ping = st.ping_ms.load(Ordering::Relaxed);
    let lost = st.ping_lost.load(Ordering::Relaxed);

    let ping_text = if lost {
        "PING: -- ms".to_string()
    } else {
        format!("PING: {} ms", ping)
    };
    let fps_text = if fps == 0 {
        "FPS: --".to_string()
    } else {
        format!("FPS: {}  ({:.1} ms)", fps, ft as f32 / 100.0)
    };

    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, COLORREF(0x00F0F0F0));

    let font = CreateFontW(
        -16,
        0,
        0,
        0,
        700,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        0,
        wide_static("Segoe UI"),
    );
    let old = SelectObject(hdc, HGDIOBJ(font.0));

    let mut r = RECT {
        left: 14,
        top: 8,
        right: OVERLAY_W - 8,
        bottom: 32,
    };
    let mut buf: Vec<u16> = fps_text.encode_utf16().collect();
    buf.push(0);
    DrawTextW(hdc, &mut buf, &mut r, DT_LEFT | DT_SINGLELINE | DT_VCENTER);

    let mut r2 = RECT {
        left: 14,
        top: 32,
        right: OVERLAY_W - 8,
        bottom: 58,
    };
    let mut buf2: Vec<u16> = ping_text.encode_utf16().collect();
    buf2.push(0);
    SetTextColor(hdc, COLORREF(0x0090E050));
    DrawTextW(hdc, &mut buf2, &mut r2, DT_LEFT | DT_SINGLELINE | DT_VCENTER);

    SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(font.0));
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn position_overlay(hwnd: HWND, st: &AppState) {
    if !st.visible.load(Ordering::Relaxed) {
        return;
    }
    if st.follow_window.load(Ordering::Relaxed) {
        let fg = GetForegroundWindow();
        if !fg.is_invalid() && fg != hwnd {
            let mut r = RECT::default();
            if GetWindowRect(fg, &mut r).is_ok() {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    r.left + 16,
                    r.top + 16,
                    OVERLAY_W,
                    OVERLAY_H,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                return;
            }
        }
    }
    let x = GetSystemMetrics(SM_CXSCREEN) - OVERLAY_W - 16;
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        x,
        16,
        OVERLAY_W,
        OVERLAY_H,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
}

/// FPS measurement based on the DXGI present counters reported by DWM for the
/// foreground window (`cDXPresentSubmitted` / `cFrameSubmitted`).
pub fn fps_thread(state: Arc<AppState>) {
    use std::time::Instant;

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

        match result {
            Ok(()) => {
                let dx_present = info.cDXPresentSubmitted;
                let frame_submitted = info.cFrameSubmitted;
                let count = if dx_present > 0 {
                    dx_present as u64
                } else {
                    frame_submitted
                };

                if state.verbose.load(Ordering::Relaxed) {
                    eprintln!("[fps] dx_present={} frame_submitted={}", dx_present, frame_submitted);
                }

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
            }
            Err(e) => {
                if state.verbose.load(Ordering::Relaxed) {
                    eprintln!("[fps] DwmGetCompositionTimingInfo failed: {:?}", e);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Creates the overlay window and runs its message loop (blocking).
pub fn run_overlay(state: Arc<AppState>) {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = wide_static("GameOverlayClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE(hinstance.0),
            lpszClassName: class_name,
            hCursor: LoadCursorW(HINSTANCE(std::ptr::null_mut()), IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let ex_style = WS_EX_LAYERED
            | WS_EX_TRANSPARENT
            | WS_EX_TOPMOST
            | WS_EX_TOOLWINDOW
            | WS_EX_NOACTIVATE;

        let hwnd = match CreateWindowExW(
            ex_style,
            class_name,
            wide_static("GameOverlay"),
            WS_POPUP,
            0,
            0,
            OVERLAY_W,
            OVERLAY_H,
            HWND(std::ptr::null_mut()),
            HMENU(std::ptr::null_mut()),
            HINSTANCE(hinstance.0),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 235, LWA_ALPHA);

        let state_ptr = Arc::into_raw(state.clone()) as isize;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr);

        // hotkeys: F1 toggle, F2/F3 switch target, F4 follow window, End = quit
        let hotkeys: [(i32, u32); 5] = [
            (1, VK_F1.0 as u32),
            (2, VK_F2.0 as u32),
            (3, VK_F3.0 as u32),
            (4, VK_F4.0 as u32),
            (5, VK_END.0 as u32),
        ];
        for (id, vk) in hotkeys {
            if let Err(e) = RegisterHotKey(hwnd, id, MOD_NOREPEAT, vk) {
                if state.verbose.load(Ordering::Relaxed) {
                    eprintln!("[hotkey] id {id} (vk {vk:#x}) failed: {e:?}");
                }
            }
        }

        SetTimer(hwnd, 1, 100, None);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        position_overlay(hwnd, &state);

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
