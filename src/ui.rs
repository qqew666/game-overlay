//! Warm, accessible control panel. Painting and input share layout.rs geometry.
use crate::draw::{self, argb, Surface, ALIGN_CENTER, ALIGN_LEFT};
use crate::layout::{self, Action, Rect, SCALES};
use crate::state::{
    AppState, SOURCE_ETW, UPDATE_CHECKING, UPDATE_FAILED, UPDATE_NEWER, UPDATE_NONE, UPDATE_READY,
};
use std::sync::{atomic::Ordering, Arc};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipDeleteGraphics, GdipScaleWorldTransform, GdipSetSmoothingMode,
    GdipSetTextRenderingHint, GpGraphics, MatrixOrderAppend, SmoothingModeAntiAlias, Status,
    TextRenderingHintAntiAliasGridFit,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
const PANEL_W: i32 = layout::WIDTH;
const PANEL_H: i32 = layout::HEIGHT;
const TRAY_MSG: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_ID: usize = 1;
const CMD_SHOW: usize = 1;
const CMD_TOGGLE_OVERLAY: usize = 2;
const CMD_TOGGLE_AUTOSTART: usize = 3;
const CMD_QUIT: usize = 4;
const CMD_CHECK_UPDATE: usize = 5;
struct PanelCtx {
    state: Arc<AppState>,
    hwnd: HWND,
    hover: Option<Action>,
    pressed: Option<Action>,
    focus: Option<Action>,
    tray: bool,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn short(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    } else {
        s.into()
    }
}
fn mouse_point(hwnd: HWND, p: LPARAM) -> (f32, f32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    let scale = (r.right as f32 / PANEL_W as f32)
        .min(r.bottom as f32 / PANEL_H as f32)
        .max(0.1);
    (
        (p.0 & 0xffff) as i16 as f32 / scale,
        ((p.0 >> 16) & 0xffff) as i16 as f32 / scale,
    )
}
fn text(g: *mut GpGraphics, s: &str, size: f32, bold: bool, color: u32, r: Rect, align: i32) {
    draw::text(g, s, size, bold, color, r.x, r.y, r.w, r.h, align);
}
fn card(g: *mut GpGraphics, r: Rect, bg: u32) {
    draw::round_card(g, r.x, r.y, r.w, r.h, 14., bg, draw::C_BORDER);
}
fn control(g: *mut GpGraphics, ctx: &PanelCtx, a: Action, label: &str, selected: bool) {
    let r = layout::rect(a);
    let bg = if ctx.pressed == Some(a) {
        argb(255, 215, 189, 156)
    } else if selected {
        argb(255, 232, 210, 181)
    } else if ctx.hover == Some(a) {
        argb(255, 239, 225, 204)
    } else {
        draw::C_CARD
    };
    card(g, r, bg);
    draw::line(
        g,
        r.x + 14.,
        r.y + 2.,
        r.x + r.w - 14.,
        r.y + 2.,
        argb(150, 255, 255, 255),
        1.,
    );
    if ctx.focus == Some(a) {
        let p = draw::rounded_rect(r.x + 3., r.y + 3., r.w - 6., r.h - 6., 11.);
        draw::stroke(g, p, draw::C_ACCENT, 2.);
        draw::path_free(p);
    }
    text(g, label, 12., true, draw::C_TEXT, r, ALIGN_CENTER);
}
unsafe fn paint(hwnd: HWND) {
    draw::gdiplus_init();
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let Some(ctx) = ctx_of(hwnd) else {
        let _ = EndPaint(hwnd, &ps);
        return;
    };
    let st = &ctx.state;
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    if let Some(surface) = Surface::new(rc.right, rc.bottom) {
        let mut g = std::ptr::null_mut();
        if GdipCreateFromHDC(surface.dc(), &mut g) == Status(0) && !g.is_null() {
            let _ = GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            let _ = GdipSetTextRenderingHint(g, TextRenderingHintAntiAliasGridFit);
            draw::round_fill(
                g,
                0.,
                0.,
                rc.right as f32,
                rc.bottom as f32,
                0.,
                if crate::appearance::NATIVE_GLASS.load(Ordering::Relaxed) {
                    argb(225, 246, 238, 224)
                } else {
                    draw::C_BG
                },
            );
            let scale = (rc.right as f32 / PANEL_W as f32).min(rc.bottom as f32 / PANEL_H as f32);
            let _ = GdipScaleWorldTransform(g, scale, scale, MatrixOrderAppend);
            card(g, Rect::new(20., 20., 248., 720.), argb(255, 235, 219, 196));
            text(
                g,
                "GAME OVERLAY",
                17.,
                true,
                draw::C_TEXT,
                Rect::new(36., 36., 216., 30.),
                ALIGN_LEFT,
            );
            text(
                g,
                "Твоё спокойное игровое место",
                10.,
                false,
                draw::C_DIM,
                Rect::new(36., 66., 216., 18.),
                ALIGN_LEFT,
            );
            draw::draw_mimi(g, 36., 100., 216.);
            if ctx.hover == Some(Action::Mimi) {
                let p = draw::rounded_rect(36., 100., 216., 216., 8.);
                draw::stroke(g, p, draw::C_ACCENT, 2.);
                draw::path_free(p);
            }
            for (i, label) in ["Беж", "Роза", "Шалфей"].iter().enumerate() {
                control(
                    g,
                    ctx,
                    Action::Theme(i as u32),
                    label,
                    crate::appearance::THEME.load(Ordering::Relaxed) == i as u32,
                );
            }
            control(
                g,
                ctx,
                Action::Glass,
                "Стекло",
                crate::appearance::GLASS.load(Ordering::Relaxed),
            );
            control(
                g,
                ctx,
                Action::Sound,
                "Звук",
                crate::appearance::SOUND.load(Ordering::Relaxed),
            );
            text(
                g,
                "Mimi · art by Typh",
                10.,
                false,
                draw::C_DIM,
                Rect::new(36., 324., 216., 22.),
                ALIGN_CENTER,
            );
            text(
                g,
                "Всё под рукой",
                19.,
                true,
                draw::C_TEXT,
                Rect::new(36., 374., 216., 32.),
                ALIGN_LEFT,
            );
            for (i, line) in [
                "F1   показать / скрыть",
                "F2 / F3   сменить цель",
                "F4   следовать за игрой",
                "F6   автозапуск",
                "Tab + Enter   меню",
                "Esc   свернуть в трей",
            ]
            .iter()
            .enumerate()
            {
                text(
                    g,
                    line,
                    11.,
                    false,
                    draw::C_DIM,
                    Rect::new(36., 422. + i as f32 * 25., 216., 24.),
                    ALIGN_LEFT,
                );
            }
            text(
                g,
                &format!("v{} · portable", crate::APP_VERSION),
                10.,
                true,
                draw::C_DIM,
                Rect::new(36., 692., 216., 24.),
                ALIGN_LEFT,
            );
            text(
                g,
                "Панель управления",
                25.,
                true,
                draw::C_TEXT,
                Rect::new(292., 22., 636., 38.),
                ALIGN_LEFT,
            );
            text(
                g,
                "Показатели игры и настройки оверлея",
                11.,
                false,
                draw::C_DIM,
                Rect::new(292., 62., 636., 20.),
                ALIGN_LEFT,
            );
            card(g, Rect::new(292., 96., 636., 100.), draw::C_CARD);
            let fps = st.fps.load(Ordering::Relaxed);
            let ping = st.ping_deci.load(Ordering::Relaxed);
            text(
                g,
                "FPS",
                10.,
                true,
                draw::C_DIM,
                Rect::new(312., 106., 90., 20.),
                ALIGN_LEFT,
            );
            text(
                g,
                &if fps == 0 {
                    "—".into()
                } else {
                    fps.to_string()
                },
                36.,
                true,
                draw::fps_color(fps),
                Rect::new(310., 124., 220., 54.),
                ALIGN_LEFT,
            );
            draw::line(g, 590., 114., 590., 178., draw::C_BORDER, 1.);
            text(
                g,
                "PING · ms",
                10.,
                true,
                draw::C_DIM,
                Rect::new(614., 106., 200., 20.),
                ALIGN_LEFT,
            );
            text(
                g,
                &if ping < 0 || st.ping_lost.load(Ordering::Relaxed) {
                    "—".into()
                } else {
                    format!("{:.1}", ping as f32 / 10.)
                },
                36.,
                true,
                draw::ping_color(ping / 10, ping < 0),
                Rect::new(612., 124., 290., 54.),
                ALIGN_LEFT,
            );
            for (a, label, hint, on) in [
                (
                    Action::ToggleAutostart,
                    "Запускать с Windows",
                    "Автоматически при входе",
                    st.autostart.load(Ordering::Relaxed),
                ),
                (
                    Action::ToggleFollow,
                    "Следовать за игрой",
                    "Закреплять у активного окна",
                    st.follow_window.load(Ordering::Relaxed),
                ),
                (
                    Action::ToggleOverlay,
                    "Показывать оверлей",
                    "Поверх окна игры · F1",
                    st.visible.load(Ordering::Relaxed),
                ),
            ] {
                control(g, ctx, a, "", false);
                let r = layout::rect(a);
                text(
                    g,
                    label,
                    12.,
                    true,
                    draw::C_TEXT,
                    Rect::new(r.x + 16., r.y + 5., 480., 22.),
                    ALIGN_LEFT,
                );
                text(
                    g,
                    hint,
                    9.,
                    false,
                    draw::C_DIM,
                    Rect::new(r.x + 16., r.y + 27., 480., 16.),
                    ALIGN_LEFT,
                );
                draw::round_fill(
                    g,
                    r.x + r.w - 66.,
                    r.y + 13.,
                    48.,
                    24.,
                    12.,
                    if on { draw::C_ACCENT } else { draw::C_BORDER },
                );
                let knob =
                    draw::ellipse_path(r.x + r.w - if on { 40. } else { 62. }, r.y + 17., 16., 16.);
                draw::fill(g, knob, draw::C_CARD);
                draw::path_free(knob);
            }
            text(
                g,
                "Размер оверлея",
                12.,
                true,
                draw::C_TEXT,
                Rect::new(292., 406., 636., 24.),
                ALIGN_LEFT,
            );
            for pct in SCALES {
                control(
                    g,
                    ctx,
                    Action::Scale(pct),
                    &format!("{pct}%"),
                    pct == st.overlay_scale.load(Ordering::Relaxed),
                );
            }
            text(
                g,
                "Цель пинга",
                12.,
                true,
                draw::C_TEXT,
                Rect::new(292., 494., 636., 24.),
                ALIGN_LEFT,
            );
            control(g, ctx, Action::PreviousTarget, "‹", false);
            control(g, ctx, Action::NextTarget, "›", false);
            let list = st.targets.lock().map(|v| v.clone()).unwrap_or_default();
            let index = st.target_index.load(Ordering::Relaxed) % list.len().max(1);
            let target = list
                .get(index)
                .map(|v| v.label())
                .unwrap_or_else(|| "Нет целей".into());
            text(
                g,
                &format!(
                    "{}   ·   {} / {}",
                    short(&target, 42),
                    if list.is_empty() { 0 } else { index + 1 },
                    list.len()
                ),
                12.,
                true,
                draw::C_TEXT,
                Rect::new(352., 526., 516., 44.),
                ALIGN_CENTER,
            );
            let exact = st.fps_source.load(Ordering::Relaxed) == SOURCE_ETW;
            control(
                g,
                ctx,
                Action::Elevate,
                if exact {
                    "Перезапуск от админа"
                } else {
                    "Точный FPS · админ"
                },
                false,
            );
            control(g, ctx, Action::HideToTray, "Свернуть в трей", false);
            text(
                g,
                if exact {
                    "Точный FPS активен"
                } else {
                    "Базовый FPS"
                },
                10.,
                false,
                draw::C_DIM,
                Rect::new(724., 594., 204., 48.),
                ALIGN_CENTER,
            );
            let label = match st.update_status.load(Ordering::Relaxed) {
                UPDATE_CHECKING => "Проверяю обновления…".into(),
                UPDATE_NEWER => "Скачиваю обновление…".into(),
                UPDATE_READY => format!("Запустить v{}", st.update_latest_string()),
                UPDATE_NONE => "Версия актуальна · проверить снова".into(),
                UPDATE_FAILED => "Ошибка обновления · повторить".into(),
                _ => "Проверить обновления".into(),
            };
            control(g, ctx, Action::CheckUpdate, &label, false);
            control(g, ctx, Action::Quit, "Закрыть приложение", false);
            let notice = st.notice.lock().ok().and_then(|v| {
                v.as_ref()
                    .filter(|(_, t)| t.elapsed().as_secs() < 8)
                    .map(|(s, _)| s.clone())
            });
            let detail = st.fps_detail.lock().map(|v| v.clone()).unwrap_or_default();
            text(
                g,
                &short(&notice.unwrap_or(detail), 85),
                10.,
                false,
                draw::C_DIM,
                Rect::new(292., 714., 636., 26.),
                ALIGN_LEFT,
            );
            let _ = GdipDeleteGraphics(g);
        }
        surface.blit(hdc);
    }
    let _ = EndPaint(hwnd, &ps);
}
fn toggle_autostart(state: &AppState) {
    match crate::autostart::toggle() {
        Ok(on) => {
            state.autostart.store(on, Ordering::Relaxed);
            state.set_notice(if on {
                "автозапуск: вкл"
            } else {
                "автозапуск: выкл"
            });
        }
        Err(e) => {
            crate::vlog!("[autostart] {e}");
            state.set_notice("автозапуск: ошибка");
        }
    }
}

fn toggle_overlay(state: &AppState) {
    let on = !state.visible.load(Ordering::Relaxed);
    state.visible.store(on, Ordering::Relaxed);
    state.set_notice(if on {
        "оверлей: вкл"
    } else {
        "оверлей: выкл"
    });
}

fn elevate(state: &AppState) {
    unsafe {
        let exe = std::env::current_exe().unwrap_or_default();
        let exe_w = wide(&exe.display().to_string());
        let verb = wide("runas");
        let result = windows::Win32::UI::Shell::ShellExecuteW(
            HWND(std::ptr::null_mut()),
            PCWSTR(verb.as_ptr()),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        if result.0 as isize > 32 {
            state.should_quit.store(true, Ordering::Relaxed);
        } else {
            state.set_notice("запуск от админа отменён");
        }
    }
}

fn perform(ctx: &mut PanelCtx, action: Action) {
    crate::appearance::click();
    let st = ctx.state.clone();
    match action {
        Action::Mimi => unsafe {
            let url = wide("https://rule34.gg/post?id=11674366");
            let result = windows::Win32::UI::Shell::ShellExecuteW(
                ctx.hwnd,
                PCWSTR::null(),
                PCWSTR(url.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if result.0 as isize <= 32 {
                ctx.state.set_notice("Не удалось открыть страницу арта");
            }
        },
        Action::Theme(theme) => {
            crate::appearance::THEME.store(theme, Ordering::Relaxed);
            crate::settings::save_preference("Theme", theme);
        }
        Action::Glass => {
            let enabled = !crate::appearance::GLASS.fetch_xor(true, Ordering::Relaxed);
            crate::settings::save_preference("Glass", enabled as u32);
            unsafe {
                crate::appearance::apply_glass(ctx.hwnd);
            }
        }
        Action::Sound => {
            let enabled = !crate::appearance::SOUND.fetch_xor(true, Ordering::Relaxed);
            crate::settings::save_preference("Sound", enabled as u32);
        }
        Action::ToggleAutostart => toggle_autostart(&st),
        Action::ToggleFollow => {
            let v = !st.follow_window.load(Ordering::Relaxed);
            st.follow_window.store(v, Ordering::Relaxed);
            st.set_notice(if v {
                "слежение: вкл"
            } else {
                "слежение: выкл"
            });
        }
        Action::ToggleOverlay => toggle_overlay(&st),
        Action::PreviousTarget => {
            st.cycle_target(-1);
            if st.target_count() < 2 {
                st.set_notice("Доступна одна цель пинга");
            }
        }
        Action::NextTarget => {
            st.cycle_target(1);
            if st.target_count() < 2 {
                st.set_notice("Доступна одна цель пинга");
            }
        }
        Action::Scale(pct) => {
            st.overlay_scale.store(pct, Ordering::Relaxed);
            crate::settings::save_scale(pct);
            st.set_notice(&format!("размер: {pct}%"));
        }
        Action::Elevate => elevate(&st),
        Action::HideToTray => unsafe {
            let _ = ShowWindow(ctx.hwnd, if ctx.tray { SW_HIDE } else { SW_MINIMIZE });
        },
        Action::CheckUpdate => {
            if st.update_status.load(Ordering::Relaxed) == UPDATE_READY {
                let file = st.update_file.lock().ok().and_then(|f| f.clone());
                if let Some(file) = file {
                    if crate::update::launch_installer(&file) {
                        st.should_quit.store(true, Ordering::Relaxed);
                    } else {
                        st.set_notice("Не удалось открыть обновление");
                    }
                }
            } else if !matches!(
                st.update_status.load(Ordering::Relaxed),
                UPDATE_CHECKING | UPDATE_NEWER
            ) {
                st.request_update_check();
            }
        }
        Action::Quit => st.should_quit.store(true, Ordering::Relaxed),
    }
    unsafe {
        let _ = InvalidateRect(ctx.hwnd, None, false);
    }
}

// ---------------------------------------------------------------------------\n// Tray\n// ---------------------------------------------------------------------------\n

unsafe fn tray_add(ctx: &mut PanelCtx) {
    if ctx.tray {
        return;
    }
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = ctx.hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = TRAY_MSG;
    nid.hIcon = LoadIconW(
        GetModuleHandleW(None).unwrap_or_default(),
        PCWSTR(1usize as *const u16),
    )
    .unwrap_or_default();
    let tip = "GameOverlay";
    for (i, ch) in tip.encode_utf16().take(127).enumerate() {
        nid.szTip[i] = ch;
    }
    ctx.tray = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();
}

unsafe fn tray_remove(ctx: &mut PanelCtx) {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = ctx.hwnd;
    nid.uID = 1;
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    ctx.tray = false;
}

unsafe fn tray_menu(ctx: &mut PanelCtx) {
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let overlay_on = ctx.state.visible.load(Ordering::Relaxed);
    let autostart_on = ctx.state.autostart.load(Ordering::Relaxed);

    let show = wide("Показать панель");
    let overlay = wide(if overlay_on {
        "Оверлей: скрыть"
    } else {
        "Оверлей: показать"
    });
    let autostart = wide(if autostart_on {
        "Автозапуск: выключить"
    } else {
        "Автозапуск: включить"
    });
    let quit = wide("Выход");
    let update = wide("Проверить обновления");

    let _ = AppendMenuW(menu, MF_STRING, CMD_SHOW, PCWSTR(show.as_ptr()));
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        CMD_TOGGLE_OVERLAY,
        PCWSTR(overlay.as_ptr()),
    );
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        CMD_TOGGLE_AUTOSTART,
        PCWSTR(autostart.as_ptr()),
    );
    let _ = AppendMenuW(menu, MF_STRING, CMD_CHECK_UPDATE, PCWSTR(update.as_ptr()));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, PCWSTR(quit.as_ptr()));

    let mut pt = windows::Win32::Foundation::POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(ctx.hwnd);
    let chosen = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_RETURNCMD,
        pt.x,
        pt.y,
        0,
        ctx.hwnd,
        None,
    );
    let _ = PostMessageW(ctx.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);

    match chosen.0 as usize {
        CMD_SHOW => unsafe {
            let _ = ShowWindow(ctx.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(ctx.hwnd);
        },
        CMD_TOGGLE_OVERLAY => toggle_overlay(&ctx.state),
        CMD_TOGGLE_AUTOSTART => toggle_autostart(&ctx.state),
        CMD_CHECK_UPDATE => ctx.state.request_update_check(),
        CMD_QUIT => ctx.state.should_quit.store(true, Ordering::Relaxed),
        _ => {}
    }
}

// ---------------------------------------------------------------------------\n// Window procedure\n// ---------------------------------------------------------------------------\n

unsafe fn ctx_of(hwnd: HWND) -> Option<&'static mut PanelCtx> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PanelCtx;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
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
            unsafe { paint(hwnd) };
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            unsafe {
                if let Some(ctx) = ctx_of(hwnd) {
                    if ctx.state.should_quit.load(Ordering::Relaxed) {
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(ctx) = ctx_of(hwnd) {
                let (x, y) = mouse_point(hwnd, lparam);
                ctx.hover = layout::hit_test(x, y);
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if let Some(ctx) = ctx_of(hwnd) {
                ctx.hover = None;
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_SETCURSOR if (lparam.0 as u32 & 0xffff) == HTCLIENT => {
            let active = ctx_of(hwnd).map(|c| c.hover.is_some()).unwrap_or(false);
            let _ = SetCursor(
                LoadCursorW(None, if active { IDC_HAND } else { IDC_ARROW }).unwrap_or_default(),
            );
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            if let Some(ctx) = ctx_of(hwnd) {
                let (x, y) = mouse_point(hwnd, lparam);
                ctx.pressed = layout::hit_test(x, y);
                ctx.focus = ctx.pressed;
                if ctx.pressed.is_some() {
                    SetCapture(hwnd);
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_point(hwnd, lparam);
            let action = if let Some(ctx) = ctx_of(hwnd) {
                let down = ctx.pressed.take();
                down.filter(|a| Some(*a) == layout::hit_test(x, y))
            } else {
                None
            };
            let _ = ReleaseCapture();
            if let (Some(ctx), Some(action)) = (ctx_of(hwnd), action) {
                perform(ctx, action);
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_CAPTURECHANGED | WM_KILLFOCUS => {
            if let Some(ctx) = ctx_of(hwnd) {
                ctx.pressed = None;
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if let Some(ctx) = ctx_of(hwnd) {
                match wparam.0 as u16 {
                    9 => {
                        let backwards = GetKeyState(0x10) < 0;
                        let actions = layout::actions();
                        let current = ctx.focus.and_then(|a| actions.iter().position(|v| *v == a));
                        let next = match current {
                            Some(i) if backwards => (i + actions.len() - 1) % actions.len(),
                            Some(i) => (i + 1) % actions.len(),
                            None if backwards => actions.len() - 1,
                            None => 0,
                        };
                        ctx.focus = Some(actions[next]);
                    }
                    13 | 32 if lparam.0 & (1 << 30) == 0 => {
                        if let Some(action) = ctx.focus {
                            perform(ctx, action);
                        }
                    }
                    27 => perform(ctx, Action::HideToTray),
                    _ => {}
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(ctx) = ctx_of(hwnd) {
                perform(ctx, Action::HideToTray);
            }
            LRESULT(0)
        }
        TRAY_MSG => {
            unsafe {
                if let Some(ctx) = ctx_of(hwnd) {
                    match (lparam.0 as u32) & 0xffff {
                        WM_LBUTTONUP => {
                            let _ = ShowWindow(hwnd, SW_SHOW);
                            let _ = SetForegroundWindow(hwnd);
                        }
                        WM_RBUTTONUP => tray_menu(ctx),
                        _ => {}
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                if let Some(ctx) = ctx_of(hwnd) {
                    tray_remove(ctx);
                }
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ---------------------------------------------------------------------------\n// Entry point\n// ---------------------------------------------------------------------------\n

/// Creates the control panel and runs its message loop (blocking, main thread).
pub fn run_panel(state: Arc<AppState>, start_hidden: bool) {
    // Фоновый воркер обновлений: спит, пока панель не попросит проверку.
    {
        let state = state.clone();
        std::thread::spawn(move || loop {
            if state.should_quit.load(Ordering::Relaxed) {
                break;
            }
            if !state.update_request.swap(false, Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            state
                .update_status
                .store(UPDATE_CHECKING, Ordering::Relaxed);
            match crate::update::check() {
                Ok(release) => {
                    if let Ok(mut v) = state.update_latest.lock() {
                        *v = release.version.clone();
                    }
                    if !crate::update::is_newer(&release.version, crate::APP_VERSION) {
                        state.update_status.store(UPDATE_NONE, Ordering::Relaxed);
                        continue;
                    }
                    state.update_status.store(UPDATE_NEWER, Ordering::Relaxed);
                    // Prefer the standalone EXE. Launch only on the next explicit click.
                    let Some(url) = release.exe_url.or(release.msi_url) else {
                        state.update_status.store(UPDATE_FAILED, Ordering::Relaxed);
                        continue;
                    };
                    match crate::update::download(&url) {
                        Ok(path) => {
                            let shown = path.to_string_lossy().into_owned();
                            if let Ok(mut f) = state.update_file.lock() {
                                *f = Some(shown.clone());
                            }
                            state.update_status.store(UPDATE_READY, Ordering::Relaxed);
                            state.set_notice("Обновление скачано. Нажми кнопку, чтобы запустить.");
                        }
                        Err(error) => {
                            state.set_notice(error);
                            state.update_status.store(UPDATE_FAILED, Ordering::Relaxed);
                        }
                    }
                }
                Err(error) => {
                    state.set_notice(error);
                    state.update_status.store(UPDATE_FAILED, Ordering::Relaxed);
                }
            }
        });
    }

    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = wide("GameOverlayPanel");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE(hinstance.0),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hCursor: LoadCursorW(HINSTANCE(std::ptr::null_mut()), IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        let mut wc = wc;
        wc.hIcon = LoadIconW(hinstance, PCWSTR(1usize as *const u16)).unwrap_or_default();
        RegisterClassW(&wc);

        let style = WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX & !WS_THICKFRAME;
        let mut work_area = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut RECT as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let fit = if work_area.right > work_area.left && work_area.bottom > work_area.top {
            ((work_area.right - work_area.left - 40) as f32 / PANEL_W as f32)
                .min((work_area.bottom - work_area.top - 80) as f32 / PANEL_H as f32)
                .min(1.0)
                .max(0.4)
        } else {
            1.0
        };
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (PANEL_W as f32 * fit) as i32,
            bottom: (PANEL_H as f32 * fit) as i32,
        };
        let _ = AdjustWindowRect(&mut rect, style, false);

        let title = wide("GameOverlay — уютный контроль игры");
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            HWND(std::ptr::null_mut()),
            HMENU(std::ptr::null_mut()),
            HINSTANCE(hinstance.0),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        crate::appearance::apply_glass(hwnd);
        // Title bar and rounded corners.
        let dark: u32 = 0;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(20),
            &dark as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
        let corners: u32 = 2;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33),
            &corners as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        let ctx = Box::into_raw(Box::new(PanelCtx {
            state: state.clone(),
            hwnd,
            hover: None,
            pressed: None,
            focus: None,
            tray: false,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx as isize);
        state.panel_hwnd.store(hwnd.0 as isize, Ordering::Relaxed);

        if let Some(c) = ctx.as_mut() {
            tray_add(c);
        }

        if !start_hidden {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }

        SetTimer(hwnd, TIMER_ID, 100, None);

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
