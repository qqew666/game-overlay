//! Transparent click-through overlay window.
//!
//! The card is drawn with GDI+ into a 32-bpp DIB and blitted with
//! `UpdateLayeredWindow`, so the corners are truly transparent instead of a
//! square window with a constant alpha.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MonitorFromWindow, AC_SRC_ALPHA, BLENDFUNCTION, HDC,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipDeleteGraphics, GdipScaleWorldTransform, GdipSetSmoothingMode,
    GdipSetTextRenderingHint, GpGraphics, MatrixOrderAppend, SmoothingModeAntiAlias, Status,
    TextRenderingHintAntiAliasGridFit,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT, VK_END, VK_F1, VK_F2, VK_F3,
    VK_F4, VK_F6,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::draw::{self, argb, Surface};
use crate::state::{AppState, SOURCE_ETW};

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

static FALLBACK_KEYS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static PRESSED_KEYS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
unsafe fn poll_target_keys(state: &AppState) {
    let mask = FALLBACK_KEYS.load(Ordering::Relaxed);
    if mask == 0 {
        return;
    }
    let mut pressed = 0;
    for (bit, key, delta) in [(1, VK_F2, 1), (2, VK_F3, -1)] {
        if mask & bit != 0 && GetAsyncKeyState(key.0 as i32) < 0 {
            pressed |= bit;
            if PRESSED_KEYS.load(Ordering::Relaxed) & bit == 0 {
                state.cycle_target(delta);
                crate::appearance::click();
            }
        }
    }
    PRESSED_KEYS.store(pressed, Ordering::Relaxed);
}

const CARD_W: f32 = 400.0;
const CARD_H: f32 = 190.0;
const SHADOW: f32 = 10.0;
const WIN_W: i32 = 420;
const WIN_H: i32 = 210;
const RADIUS: f32 = 22.0;
const NOTICE_SECS: f32 = 2.5;
/// Range the user can pick in the panel; the registry is clamped to it too.
const SCALE_MIN: f32 = 0.4;
const SCALE_MAX: f32 = 2.0;

fn current_scale(st: &AppState) -> f32 {
    (st.overlay_scale.load(Ordering::Relaxed) as f32 / 100.0).clamp(SCALE_MIN, SCALE_MAX)
}

/// Window size for the current scale.
fn window_size(st: &AppState) -> (i32, i32) {
    let s = current_scale(st);
    (
        (WIN_W as f32 * s).round() as i32,
        (WIN_H as f32 * s).round() as i32,
    )
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn short(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    } else {
        s.to_string()
    }
}

unsafe fn render(hwnd: HWND, st: &AppState) {
    draw::gdiplus_init();

    let (win_w, win_h) = window_size(st);
    let Some(surface) = Surface::new(win_w, win_h) else {
        crate::vlog!("[overlay] Surface::new({win_w}x{win_h}) failed");
        return;
    };

    let mut g: *mut GpGraphics = std::ptr::null_mut();
    if unsafe { GdipCreateFromHDC(surface.dc(), &mut g) } != Status(0) || g.is_null() {
        crate::vlog!("[overlay] GdipCreateFromHDC failed");
        return;
    }
    let _ = unsafe { GdipSetSmoothingMode(g, SmoothingModeAntiAlias) };
    let _ = unsafe { GdipSetTextRenderingHint(g, TextRenderingHintAntiAliasGridFit) };
    // Everything below is laid out in the designed 420x210 space; scaling the
    // world transform grows the card and its type together.
    let _ = unsafe {
        GdipScaleWorldTransform(g, current_scale(st), current_scale(st), MatrixOrderAppend)
    };

    let ox = SHADOW;
    let oy = SHADOW;

    // Drop shadow: two stacked rounded rects, wider and softer at the back.
    draw::round_fill(
        g,
        ox - 1.0,
        oy + 5.0,
        CARD_W + 2.0,
        CARD_H,
        RADIUS + 2.0,
        argb(46, 0, 0, 0),
    );
    draw::round_fill(g, ox, oy + 2.0, CARD_W, CARD_H, RADIUS, argb(72, 0, 0, 0));

    if crate::appearance::GLASS.load(Ordering::Relaxed) {
        draw::glass_card(g, ox, oy, CARD_W, CARD_H, RADIUS, draw::C_CARD);
    } else {
        draw::round_card(
            g,
            ox,
            oy,
            CARD_W,
            CARD_H,
            RADIUS,
            draw::C_BG,
            draw::C_BORDER,
        );
    }

    // Accent rail on the left edge: cyan over magenta.
    // The two halves overlap by 4 px so the joint reads as one bar.
    draw::round_fill(g, ox + 10.0, oy + 20.0, 5.0, 70.0, 2.5, draw::C_ACCENT);
    draw::round_fill(g, ox + 10.0, oy + 86.0, 5.0, 70.0, 2.5, draw::C_SOFT);

    let fps = st.fps.load(Ordering::Relaxed);
    let ft = st.frametime_centi_ms.load(Ordering::Relaxed);
    let ping_deci = st.ping_deci.load(Ordering::Relaxed);
    let ping = if ping_deci < 0 {
        -1
    } else {
        (ping_deci + 5) / 10
    };
    let lost = st.ping_lost.load(Ordering::Relaxed);
    let exact = st.fps_source.load(Ordering::Relaxed) == SOURCE_ETW;

    // Header: boykisser on the left, game name, FPS source underneath.
    draw::draw_mimi(g, ox + 25.0, oy + 12.0, 48.0);

    let game = st.game_name_string();
    let title = if game.is_empty() {
        "GameOverlay".to_string()
    } else {
        short(&game, 24)
    };
    draw::text(
        g,
        &title,
        14.0,
        true,
        draw::C_TEXT,
        ox + 84.0,
        oy + 16.0,
        220.0,
        20.0,
        draw::ALIGN_LEFT,
    );
    let source = if exact {
        "точный FPS".to_string()
    } else {
        "базовый FPS".to_string()
    };
    draw::text(
        g,
        &source,
        9.5,
        false,
        draw::C_DIM,
        ox + 84.0,
        oy + 38.0,
        220.0,
        14.0,
        draw::ALIGN_LEFT,
    );

    // Separator under the header.
    draw::line(
        g,
        ox + 24.0,
        oy + 70.0,
        ox + CARD_W - 24.0,
        oy + 70.0,
        argb(40, 167, 131, 98),
        1.0,
    );

    // FPS column.
    draw::text(
        g,
        "FPS",
        9.0,
        true,
        draw::C_DIM,
        ox + 30.0,
        oy + 78.0,
        90.0,
        12.0,
        draw::ALIGN_LEFT,
    );
    let fps_value = if fps == 0 {
        "--".to_string()
    } else {
        fps.to_string()
    };
    draw::text(
        g,
        &fps_value,
        44.0,
        true,
        draw::fps_color(fps),
        ox + 28.0,
        oy + 90.0,
        150.0,
        52.0,
        draw::ALIGN_LEFT,
    );
    let ft_text = if fps == 0 {
        "ожидание кадра".to_string()
    } else {
        format!("{:.1} ms кадр", ft as f32 / 100.0)
    };
    draw::text(
        g,
        &ft_text,
        10.0,
        false,
        draw::C_DIM,
        ox + 30.0,
        oy + 146.0,
        150.0,
        14.0,
        draw::ALIGN_LEFT,
    );

    // Divider between the two readings.
    draw::line(
        g,
        ox + 196.0,
        oy + 78.0,
        ox + 196.0,
        oy + 160.0,
        argb(34, 167, 131, 98),
        1.0,
    );

    // Ping column.
    draw::text(
        g,
        "PING",
        9.0,
        true,
        draw::C_DIM,
        ox + 216.0,
        oy + 78.0,
        90.0,
        12.0,
        draw::ALIGN_LEFT,
    );
    // Sub-10 ms values keep one decimal: a single digit is what a good route
    // to a game server looks like, and rounding it to "0" hides the difference.
    let ping_value = if lost || ping_deci < 0 {
        "--".to_string()
    } else if ping_deci < 100 {
        format!("{:.1}", ping_deci as f32 / 10.0)
    } else {
        format!("{ping}")
    };
    let ping_tone = draw::ping_color(ping, lost);
    draw::text(
        g,
        &ping_value,
        38.0,
        true,
        ping_tone,
        ox + 216.0,
        oy + 90.0,
        118.0,
        50.0,
        draw::ALIGN_RIGHT,
    );
    draw::text(
        g,
        "ms",
        11.0,
        false,
        ping_tone,
        ox + 340.0,
        oy + 108.0,
        30.0,
        14.0,
        draw::ALIGN_LEFT,
    );

    let target = st.target_name.lock().map(|t| t.clone()).unwrap_or_default();
    draw::text(
        g,
        &short(&target, 18),
        9.5,
        false,
        draw::C_DIM,
        ox + 216.0,
        oy + 146.0,
        160.0,
        14.0,
        draw::ALIGN_LEFT,
    );

    // Ping graph across the bottom.
    let samples: Vec<i32> = st.history.lock().map(|h| h.clone()).unwrap_or_default();
    let graph_x = ox + 30.0;
    let graph_w = CARD_W - 60.0;
    let graph_y = oy + 166.0;
    let graph_h = 14.0;

    draw::round_fill(
        g,
        graph_x,
        graph_y + graph_h - 1.0,
        graph_w,
        1.0,
        0.5,
        draw::C_TRACK,
    );

    if samples.len() >= 2 {
        let valid: Vec<f32> = samples
            .iter()
            .filter(|v| **v >= 0)
            .map(|v| *v as f32 / 10.0)
            .collect();
        if valid.len() >= 2 {
            let max = valid.iter().cloned().fold(1.0f32, f32::max).max(30.0);
            let step = graph_w / (samples.len().saturating_sub(1)).max(1) as f32;
            let mut points: Vec<(f32, f32)> = Vec::with_capacity(samples.len() + 2);
            for (i, v) in samples.iter().enumerate() {
                let x = graph_x + step * i as f32;
                let value = if *v < 0 { max } else { *v as f32 / 10.0 };
                let y = graph_y + graph_h - (value / max).clamp(0.0, 1.0) * graph_h;
                points.push((x, y));
            }

            // Soft area under the line, then the line itself.
            let mut area = points.clone();
            area.push((graph_x + graph_w, graph_y + graph_h));
            area.push((graph_x, graph_y + graph_h));
            let poly = draw::polygon(&area);
            draw::fill(g, poly, argb(38, 167, 131, 98));
            draw::path_free(poly);

            let line = draw::polygon(&points);
            draw::stroke(g, line, ping_tone, 1.8);
            draw::path_free(line);

            // Marker on the newest sample.
            if let Some((x, y)) = points.last() {
                let dot = draw::ellipse_path(x - 2.6, *y - 2.6, 5.2, 5.2);
                draw::fill(g, dot, ping_tone);
                draw::path_free(dot);
            }
        }
    }

    // Transient status badge.
    if let Ok(n) = st.notice.lock() {
        if let Some((msg, at)) = n.as_ref() {
            let age = at.elapsed().as_secs_f32();
            if age < NOTICE_SECS {
                let alpha = (255.0 * (1.0 - age / NOTICE_SECS)).clamp(0.0, 255.0) as u8;
                draw::round_fill(
                    g,
                    ox + CARD_W - 184.0,
                    oy + 14.0,
                    168.0,
                    24.0,
                    12.0,
                    argb(alpha.saturating_sub(40), 230, 210, 184),
                );
                draw::text(
                    g,
                    msg,
                    10.0,
                    true,
                    argb(alpha, 65, 47, 37),
                    ox + CARD_W - 184.0,
                    oy + 15.0,
                    168.0,
                    22.0,
                    draw::ALIGN_CENTER,
                );
            }
        }
    }

    let _ = unsafe { GdipDeleteGraphics(g) };

    unsafe {
        surface.premultiply();

        let (x, y) = st.pos.lock().map(|p| *p).unwrap_or((0, 0));
        let dst = POINT { x, y };
        let size = SIZE {
            cx: win_w,
            cy: win_h,
        };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        let placed = UpdateLayeredWindow(
            hwnd,
            HDC::default(),
            Some(&dst),
            Some(&size),
            surface.dc(),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        if placed.is_err() {
            crate::vlog!(
                "[overlay] UpdateLayeredWindow at {x},{y} failed: {:?}",
                placed
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Window plumbing
// ---------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Borrows the `Arc<AppState>` parked in the window's user data. The window
/// owns that reference for its lifetime, so a borrow is enough.
unsafe fn state_of(hwnd: HWND) -> Option<&'static AppState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const AppState;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
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
                if let Some(state) = state_of(hwnd) {
                    render(hwnd, state);
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            unsafe {
                if let Some(st) = state_of(hwnd) {
                    if st.should_quit.load(Ordering::Relaxed) {
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    if wparam.0 == 1 {
                        poll_target_keys(st);
                    }
                    let want = st.visible.load(Ordering::Relaxed);
                    let shown = IsWindowVisible(hwnd).as_bool();
                    if want && !shown {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    } else if !want && shown {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    if want {
                        if wparam.0 == 1 {
                            position_overlay(hwnd, st, false);
                        } else {
                            render(hwnd, st);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            unsafe {
                if let Some(st) = state_of(hwnd) {
                    match wparam.0 as u32 {
                        1 => {
                            let v = !st.visible.load(Ordering::Relaxed);
                            st.visible.store(v, Ordering::Relaxed);
                            st.set_notice(if v {
                                "оверлей: вкл"
                            } else {
                                "оверлей: выкл"
                            });
                        }
                        2 => {
                            crate::appearance::click();
                            st.cycle_target(1);
                        }
                        3 => {
                            crate::appearance::click();
                            st.cycle_target(-1);
                        }
                        4 => {
                            let v = !st.follow_window.load(Ordering::Relaxed);
                            st.follow_window.store(v, Ordering::Relaxed);
                            st.set_notice(if v {
                                "слежение: вкл"
                            } else {
                                "слежение: выкл"
                            });
                        }
                        5 => st.should_quit.store(true, Ordering::Relaxed),
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
            let _ = KillTimer(hwnd, 1);
            let _ = KillTimer(hwnd, 2);
            for id in 1..=6 {
                let _ = UnregisterHotKey(hwnd, id);
            }
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppState;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !ptr.is_null() {
                let state = Arc::from_raw(ptr);
                state.overlay_hwnd.store(0, Ordering::Relaxed);
            }
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

thread_local! {
    static MOTION: std::cell::RefCell<(crate::movement::Motion, std::time::Instant)> =
        std::cell::RefCell::new((crate::movement::Motion::default(), std::time::Instant::now()));
}

unsafe fn position_overlay(hwnd: HWND, st: &AppState, snap: bool) {
    let size = window_size(st);
    let foreground = GetForegroundWindow();
    let panel = HWND(st.panel_hwnd.load(Ordering::Relaxed) as *mut _);
    let following = st.follow_window.load(Ordering::Relaxed);
    let mut foreground_process = 0;
    GetWindowThreadProcessId(foreground, Some(&mut foreground_process));
    let own_window = foreground == panel
        || foreground == hwnd
        || foreground_process == windows::Win32::System::Threading::GetCurrentProcessId();
    let anchor = if following
        && !foreground.is_invalid()
        && !own_window
        && !IsIconic(foreground).as_bool()
    {
        foreground
    } else {
        hwnd
    };
    let mut monitor = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(
        MonitorFromWindow(anchor, MONITOR_DEFAULTTONEAREST),
        &mut monitor,
    )
    .as_bool()
    {
        return;
    }
    let work = monitor.rcWork;
    let area = (work.left, work.top, work.right, work.bottom);
    let mut target = (work.right - size.0 - 20, work.top + 20);
    if following {
        if anchor == foreground && !own_window {
            let mut rect = windows::Win32::Foundation::RECT::default();
            if GetWindowRect(foreground, &mut rect).is_ok() {
                target = (rect.left + 20, rect.top + 20);
            }
        } else if let Ok(position) = st.pos.lock() {
            target = *position;
        }
    }
    target = crate::movement::constrain(target, size, area);
    let position = MOTION.with(|motion| {
        let mut motion = motion.borrow_mut();
        let now = std::time::Instant::now();
        let seconds = now.duration_since(motion.1).as_secs_f64();
        motion.1 = now;
        crate::movement::constrain(motion.0.update(target, seconds, snap), size, area)
    });
    if let Ok(mut current) = st.pos.lock() {
        *current = position;
    }
    let mut current = windows::Win32::Foundation::RECT::default();
    if GetWindowRect(hwnd, &mut current).is_ok()
        && (
            current.left,
            current.top,
            current.right - current.left,
            current.bottom - current.top,
        ) == (position.0, position.1, size.0, size.1)
    {
        return;
    }
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        position.0,
        position.1,
        size.0,
        size.1,
        SWP_NOACTIVATE,
    );
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

        let ex_style =
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;

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
        crate::vlog!("[overlay] window created");
        state.overlay_hwnd.store(hwnd.0 as isize, Ordering::Relaxed);

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
                if id == 2 {
                    FALLBACK_KEYS.fetch_or(1, Ordering::Relaxed);
                }
                if id == 3 {
                    FALLBACK_KEYS.fetch_or(2, Ordering::Relaxed);
                }
                crate::vlog!("[hotkey] id {id} (vk {vk:#x}) failed: {e:?}");
            }
        }

        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex_style.0 | WS_EX_LAYERED.0) as isize);

        position_overlay(hwnd, &state, true);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        render(hwnd, &state);
        SetTimer(hwnd, 1, 16, None);
        SetTimer(hwnd, 2, 100, None);

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
