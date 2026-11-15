//! Control panel window (no console). Custom-painted 32-bpp DIB per frame,
//! shared look with the overlay, no widget toolkit.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipDeleteGraphics, GdipSetSmoothingMode, GdipSetTextRenderingHint,
    GpGraphics, RectF, SmoothingModeAntiAlias, Status, TextRenderingHintAntiAliasGridFit,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::draw::{self, argb, Surface, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT};
use crate::state::{
    AppState, SOURCE_ETW, UPDATE_CHECKING, UPDATE_FAILED, UPDATE_NEWER, UPDATE_NONE, UPDATE_READY,
};

// ---------------------------------------------------------------------------\n// Layout\n// ---------------------------------------------------------------------------\n

const PANEL_W: i32 = 640;
const PANEL_H: i32 = 730;

const TRAY_MSG: u32 = WM_APP + 1;
const TIMER_ID: usize = 1;

// Tray menu ids — used with TPM_RETURNCMD so the id comes back directly.
const CMD_SHOW: usize = 1;
const CMD_TOGGLE_OVERLAY: usize = 2;
const CMD_TOGGLE_AUTOSTART: usize = 3;
const CMD_CHECK_UPDATE: usize = 5;
const CMD_QUIT: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    CheckUpdate,

    ToggleAutostart,
    ToggleFollow,
    ToggleOverlay,
    Target(usize),
    Scale(u32),
    Elevate,
    HideToTray,
    Quit,
}

struct PanelCtx {
    state: Arc<AppState>,
    hwnd: HWND,
    hover: Option<Action>,
    tray: bool,
}

// ---------------------------------------------------------------------------\n// Helpers\n// ---------------------------------------------------------------------------\n

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        s.to_string()
    }
}

fn row_rect(i: usize) -> RectF {
    RectF {
        X: 24.0,
        Y: 292.0 + i as f32 * 54.0,
        Width: 592.0,
        Height: 46.0,
    }
}

fn toggle_rect(i: usize) -> RectF {
    let row = row_rect(i);
    RectF {
        X: 24.0 + 592.0 - 60.0,
        Y: row.Y + 12.0,
        Width: 44.0,
        Height: 22.0,
    }
}

const SCALES: [u32; 4] = [80, 100, 125, 150];

fn scale_chip_rect(i: usize) -> RectF {
    RectF {
        X: 22.0 + i as f32 * 136.0,
        Y: 456.0,
        Width: (128 + i * 4) as f32,
        Height: 40.0,
    }
}

fn target_chip_rect(i: usize) -> RectF {
    RectF {
        X: 20.0 + i as f32 * 144.0,
        Y: 530.0,
        Width: (136 + i * 4) as f32,
        Height: 40.0,
    }
}

fn button_rect(i: usize) -> RectF {
    match i {
        0 => RectF { X: 24.0, Y: 620.0, Width: 168.0, Height: 48.0 },
        1 => RectF { X: 204.0, Y: 620.0, Width: 150.0, Height: 48.0 },
        2 => RectF { X: 366.0, Y: 620.0, Width: 100.0, Height: 48.0 },
        _ => RectF { X: 478.0, Y: 620.0, Width: 138.0, Height: 48.0 },
    }
}

fn inside(r: &RectF, x: f32, y: f32) -> bool {
    x >= r.X && x <= r.X + r.Width && y >= r.Y && y <= r.Y + r.Height
}

fn hit_test(state: &AppState, x: f32, y: f32) -> Option<Action> {
    for i in 0..3 {
        if inside(&row_rect(i), x, y) {
            return Some(match i {
                0 => Action::ToggleAutostart,
                1 => Action::ToggleFollow,
                _ => Action::ToggleOverlay,
            });
        }
    }
    for (i, pct) in SCALES.iter().enumerate() {
        if inside(&scale_chip_rect(i), x, y) {
            return Some(Action::Scale(*pct));
        }
    }
    let count = state.target_count().min(4);
    for i in 0..count {
        if inside(&target_chip_rect(i), x, y) {
            return Some(Action::Target(i));
        }
    }
    for (i, action) in [
        (0, Action::Elevate),
        (1, Action::ToggleOverlay),
        (2, Action::HideToTray),
        (3, Action::CheckUpdate),
        (4, Action::Quit),
    ] {
        if inside(&button_rect(i), x, y) {
            return Some(action);
        }
    }
    None
}

// ---------------------------------------------------------------------------\n// Painting\n// ---------------------------------------------------------------------------\n

unsafe fn paint_text(g: *mut GpGraphics, s: &str, size: f32, bold: bool, color: u32, r: RectF, align: i32) {
    draw::text(g, s, size, bold, color, r.X, r.Y, r.Width, r.Height, align);
}

unsafe fn draw_toggle(g: *mut GpGraphics, r: RectF, on: bool, hover: bool) {
    let track = if on {
        if hover { argb(255, 92, 240, 255) } else { draw::C_CYAN }
    } else if hover {
        argb(255, 58, 64, 84)
    } else {
        argb(255, 44, 49, 64)
    };
    draw::round_fill(g, r.X, r.Y, r.Width, r.Height, r.Height / 2.0, track);
    let knob_x = if on { r.X + r.Width - 19.0 } else { r.X + 3.0 };
    let knob = draw::ellipse_path(knob_x, r.Y + 3.0, 16.0, 16.0);
    draw::fill(g, knob, if on { argb(255, 12, 20, 26) } else { argb(255, 200, 208, 224) });
    draw::path_free(knob);
}

unsafe fn draw_button(g: *mut GpGraphics, r: RectF, label: &str, accent: bool, hover: bool) {
    if accent {
        let bg = if hover { argb(255, 54, 214, 244) } else { draw::C_CYAN };
        draw::round_fill(g, r.X, r.Y, r.Width, r.Height, 12.0, bg);
        paint_text(g, label, 12.5, true, argb(255, 8, 14, 20), r, ALIGN_CENTER);
        return;
    }
    if hover {
        draw::round_fill(g, r.X, r.Y, r.Width, r.Height, 12.0, argb(255, 24, 28, 38));
    }
    let fg = if hover { draw::C_TEXT } else { argb(255, 168, 178, 200) };
    paint_text(g, label, 12.5, false, fg, r, ALIGN_CENTER);
}

unsafe fn paint(hwnd: HWND) {
    draw::gdiplus_init();

    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;

    let ctx = match ctx_of(hwnd) {
        Some(c) => c,
        None => {
            let _ = EndPaint(hwnd, &ps);
            return;
        }
    };
    let st = &ctx.state;

    if let Some(surf) = Surface::new(w, h) {
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipCreateFromHDC(surf.dc(), &mut g) == Status(0) && !g.is_null() {
            let _ = GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            let _ = GdipSetTextRenderingHint(g, TextRenderingHintAntiAliasGridFit);

            // ---- Background: hand-painted dark card with soft edges ----------------

            draw::round_fill(g, 0.0, 0.0, w as f32, h as f32, 6.0, argb(255, 10, 11, 16));

            // ---- Header: Boykisser + wordmark, hand-drawn but stable ---------------

            // Boykisser — slight tilt for human feel, no per-frame jitter.
            draw::push_rotation(g, -6.5, 62.0, 62.0);
            draw::draw_boykisser(g, 62.0, 62.0, 72.0);
            draw::reset_transform(g);

            // Highlighter bar — hand-drawn, slightly wobbly but stable.
            draw::round_fill(g, 102.0, 22.0, 220.0, 34.0, 7.0, argb(51, 255, 86, 218));

            // Wordmark — Segoe Print, stable hand-written feel.
            draw::text_in(
                g,
                "Segoe Print",
                "mimi(typh)",
                28.0,
                true,
                draw::C_TEXT,
                108.0,
                16.0,
                368.0,
                44.0,
                ALIGN_LEFT,
            );

            // Label: hand-written feel, stable.
            paint_text(
                g,
                "оверлей FPS и пинга",
                10.5,
                false,
                draw::C_DIM,
                RectF {
                    X: 108.0,
                    Y: 60.0,
                    Width: 304.0,
                    Height: 18.0,
                },
                ALIGN_LEFT,
            );

            // Version: fixed right-aligned, hand-written feel.
            paint_text(
                g,
                "v1.4.0",
                9.5,
                false,
                argb(255, 120, 132, 158),
                RectF {
                    X: 476.0,
                    Y: 30.0,
                    Width: 140.0,
                    Height: 18.0,
                },
                ALIGN_RIGHT,
            );

            // Hairline under header — hand-drawn, stable.
            draw::line(g, 24.0, 100.0, 616.0, 100.0, argb(255, 30, 35, 50), 1.0);
            draw::line(g, 24.0, 100.0, 120.0, 100.0, draw::C_CYAN, 2.0);

            // ---- Live card — hand-cut, stable corners --------------------------------

            draw::round_card(g, 24.0, 118.0, 592.0, 134.0, 18.0, argb(255, 18, 20, 28), argb(255, 38, 44, 60));

            // Rail — hand-cut, stable.
            draw::round_fill(g, 42.0, 134.0, 6.0, 102.0, 2.0, draw::C_CYAN);
            draw::round_fill(g, 42.0, 134.0, 6.0, 46.0, 2.0, draw::C_MAGENTA);

            let fps = st.fps.load(Ordering::Relaxed);
            let ft = st.frametime_centi_ms.load(Ordering::Relaxed);
            let ping_deci = st.ping_deci.load(Ordering::Relaxed);
            let ping = if ping_deci < 0 { -1 } else { (ping_deci + 5) / 10 };
            let lost = st.ping_lost.load(Ordering::Relaxed);

            // FPS label
            paint_text(
                g,
                "FPS",
                9.5,
                true,
                draw::C_DIM,
                RectF {
                    X: 60.0,
                    Y: 128.0,
                    Width: 128.0,
                    Height: 16.0,
                },
                ALIGN_LEFT,
            );

            // FPS value — hand-written size, stable.
            let fps_val = if fps == 0 { "--".to_string() } else { fps.to_string() };
            paint_text(
                g,
                &fps_val,
                44.0,
                true,
                draw::fps_color(fps),
                RectF {
                    X: 58.0,
                    Y: 142.0,
                    Width: 196.0,
                    Height: 58.0,
                },
                ALIGN_LEFT,
            );

            // PING label
            paint_text(
                g,
                "PING",
                9.5,
                true,
                draw::C_DIM,
                RectF {
                    X: 346.0,
                    Y: 128.0,
                    Width: 128.0,
                    Height: 16.0,
                },
                ALIGN_LEFT,
            );

            // Ping value
            let ping_val = if lost || ping_deci < 0 {
                "--".to_string()
            } else if ping_deci < 100 {
                format!("{:.1}", ping_deci as f32 / 10.0)
            } else {
                format!("{}", ping)
            };
            paint_text(
                g,
                &ping_val,
                44.0,
                true,
                draw::ping_color(ping, lost),
                RectF {
                    X: 344.0,
                    Y: 140.0,
                    Width: 176.0,
                    Height: 58.0,
                },
                ALIGN_RIGHT,
            );

            // ms suffix
            paint_text(
                g,
                "ms",
                10.5,
                false,
                draw::ping_color(ping, lost),
                RectF {
                    X: 518.0,
                    Y: 168.0,
                    Width: 36.0,
                    Height: 14.0,
                },
                ALIGN_LEFT,
            );

            // Game frame text
            let game = st.game_name_string();
            let frame = if fps == 0 {
                "ожидание кадра".to_string()
            } else {
                format!("{:.1} ms кадр", ft as f32 / 100.0)
            };
            let ft_text = if game.is_empty() {
                frame
            } else {
                format!("{} · {frame}", short(&game, 30))
            };
            paint_text(
                g,
                &ft_text,
                9.5,
                false,
                draw::C_DIM,
                RectF {
                    X: 60.0,
                    Y: 208.0,
                    Width: 524.0,
                    Height: 18.0,
                },
                ALIGN_LEFT,
            );

            // ---- Settings section -----------------------------------------------------

            // Settings title
            paint_text(
                g,
                "Настройки",
                12.5,
                true,
                draw::C_TEXT,
                RectF {
                    X: 22.0,
                    Y: 260.0,
                    Width: 304.0,
                    Height: 20.0,
                },
                ALIGN_LEFT,
            );

            // Settings rows — stable, hand-written list.
            let rows = [
                (
                    "Запускать с Windows",
                    "автозапуск при входе в систему",
                    st.autostart.load(Ordering::Relaxed),
                ),
                (
                    "Следовать за окном игры",
                    "карточка держится у активного окна",
                    st.follow_window.load(Ordering::Relaxed),
                ),
                (
                    "Оверлей поверх игры",
                    "или клавиша F1",
                    st.visible.load(Ordering::Relaxed),
                ),
            ];
            for (i, (title, hint, on)) in rows.iter().enumerate() {
                let r = row_rect(i);
                let hovered = ctx.hover == Some(match i {
                    0 => Action::ToggleAutostart,
                    1 => Action::ToggleFollow,
                    _ => Action::ToggleOverlay,
                });

                if hovered {
                    draw::round_fill(g, r.X + 1.0, r.Y, r.Width - 2.0, r.Height, 7.0, argb(255, 20, 24, 34));
                }
                paint_text(
                    g,
                    title,
                    12.0,
                    true,
                    draw::C_TEXT,
                    RectF {
                        X: r.X + 10.0,
                        Y: r.Y + 4.0,
                        Width: 382.0,
                        Height: 20.0,
                    },
                    ALIGN_LEFT,
                );
                paint_text(
                    g,
                    hint,
                    9.0,
                    false,
                    draw::C_DIM,
                    RectF {
                        X: r.X + 10.0,
                        Y: r.Y + 22.0,
                        Width: 382.0,
                        Height: 16.0,
                    },
                    ALIGN_LEFT,
                );
                draw_toggle(g, toggle_rect(i), *on, hovered);

                if i < rows.len() - 1 {
                    draw::line(g, r.X + 12.0, r.Y + r.Height - 3.0, r.X + r.Width - 12.0, r.Y + r.Height - 3.0, argb(255, 26, 30, 42), 1.0);
                }
            }

            // ---- Size section ---------------------------------------------------------

            paint_text(
                g,
                "Размер оверлея",
                12.5,
                true,
                draw::C_TEXT,
                RectF {
                    X: 22.0,
                    Y: 452.0,
                    Width: 304.0,
                    Height: 20.0,
                },
                ALIGN_LEFT,
            );
            paint_text(
                g,
                "применяется сразу и запоминается",
                9.0,
                false,
                draw::C_DIM,
                RectF {
                    X: 140.0,
                    Y: 454.0,
                    Width: 310.0,
                    Height: 16.0,
                },
                ALIGN_LEFT,
            );

            let scale_now = st.overlay_scale.load(Ordering::Relaxed);
            for (i, pct) in SCALES.iter().enumerate() {
                let r = scale_chip_rect(i);
                let active = scale_now == *pct;
                let hovered = ctx.hover == Some(Action::Scale(*pct));
                let chip_bg = if active {
                    argb(255, 22, 38, 50)
                } else if hovered {
                    argb(255, 24, 30, 42)
                } else {
                    argb(255, 16, 19, 27)
                };
                draw::round_fill(g, r.X, r.Y, r.Width, r.Height, 9.0, chip_bg);
                if active || hovered {
                    let p = draw::rounded_rect(r.X, r.Y, r.Width, r.Height, 8.0);
                    draw::stroke(g, p, if active { draw::C_CYAN } else { argb(255, 40, 50, 70) }, 1.0);
                    draw::path_free(p);
                }
                paint_text(
                    g,
                    &format!("{pct}%"),
                    11.0,
                    active,
                    if active { draw::C_CYAN } else { argb(255, 160, 170, 190) },
                    r,
                    ALIGN_CENTER,
                );
            }

            // ---- Targets section ------------------------------------------------------

            paint_text(
                g,
                "Цели пинга",
                12.5,
                true,
                draw::C_TEXT,
                RectF {
                    X: 22.0,
                    Y: 532.0,
                    Width: 304.0,
                    Height: 20.0,
                },
                ALIGN_LEFT,
            );
            paint_text(
                g,
                "F2 / F3 переключают",
                9.0,
                false,
                draw::C_DIM,
                RectF {
                    X: 124.0,
                    Y: 534.0,
                    Width: 268.0,
                    Height: 16.0,
                },
                ALIGN_LEFT,
            );

            let targets: Vec<String> = {
                let list = st.targets.lock().map(|t| t.clone()).unwrap_or_default();
                list.iter().map(|t| t.label()).collect()
            };
            let active = st.target_index.load(Ordering::Relaxed);
            for (i, label) in targets.iter().take(4).enumerate() {
                let r = target_chip_rect(i);
                let is_active = i == active % targets.len().max(1);
                let hovered = ctx.hover == Some(Action::Target(i));
                let chip_bg = if is_active {
                    argb(255, 22, 38, 50)
                } else if hovered {
                    argb(255, 24, 30, 42)
                } else {
                    argb(255, 16, 19, 27)
                };
                draw::round_fill(g, r.X, r.Y, r.Width, r.Height, 8.0, chip_bg);
                if is_active || hovered {
                    let p = draw::rounded_rect(r.X, r.Y, r.Width, r.Height, 7.0);
                    draw::stroke(g, p, if is_active { draw::C_CYAN } else { argb(255, 40, 50, 70) }, 1.0);
                    draw::path_free(p);
                }
                paint_text(
                    g,
                    &short(label, 16),
                    10.0,
                    is_active,
                    if is_active { draw::C_CYAN } else { draw::C_DIM },
                    RectF {
                        X: r.X + 5.0,
                        Y: r.Y + 4.0,
                        Width: r.Width - 10.0,
                        Height: r.Height - 8.0,
                    },
                    ALIGN_CENTER,
                );
            }
            if targets.len() > 4 {
                paint_text(
                    g,
                    &format!("+{}", targets.len() - 4),
                    10.0,
                    true,
                    draw::C_DIM,
                    target_chip_rect(4),
                    ALIGN_CENTER,
                );
            }

                        // ---- Buttons ----------------------------------------------------------------

            // Check-update button — visible as soon as the app starts.
            let latest = st.update_latest_string();
            let check_label = match st.update_status.load(Ordering::Relaxed) {
                UPDATE_CHECKING => "Проверка…".to_string(),
                UPDATE_NONE => "Обновлений нет".to_string(),
                UPDATE_NEWER => format!("Качаю {latest}"),
                UPDATE_READY => format!("Обновление {latest} готово"),
                UPDATE_FAILED => "Ошибка проверки".to_string(),
                _ => "Проверить обновление".to_string(),
            };
            draw_button(g, button_rect(3), &check_label, true, ctx.hover == Some(Action::CheckUpdate));


            // Buttons — fixed sizes, hand-painted look.
            let exact = st.fps_source.load(Ordering::Relaxed) == SOURCE_ETW;
            let elevate_label = if exact {
                "Точный FPS включён"
            } else {
                "Точный FPS (админ)"
            };
            draw_button(g, button_rect(0), elevate_label, !exact, ctx.hover == Some(Action::Elevate));

            let overlay_label = if st.visible.load(Ordering::Relaxed) {
                "Убрать оверлей"
            } else {
                "Вернуть оверлей"
            };
            draw_button(g, button_rect(1), overlay_label, false, ctx.hover == Some(Action::ToggleOverlay));
            draw_button(g, button_rect(2), "В трей", false, ctx.hover == Some(Action::HideToTray));
            draw_button(g, button_rect(4), "Выход", false, ctx.hover == Some(Action::Quit));

            // ---- Footer ----------------------------------------------------------------

            let detail = st.fps_detail.lock().map(|d| d.clone()).unwrap_or_default();
            paint_text(
                g,
                &format!("FPS: {}", short(&detail, 62)),
                9.0,
                false,
                draw::C_DIM,
                RectF {
                    X: 20.0,
                    Y: 680.0,
                    Width: 464.0,
                    Height: 16.0,
                },
                ALIGN_LEFT,
            );

            paint_text(
                g,
                "mimi(typh)",
                9.0,
                true,
                argb(170, 138, 218, 242),
                RectF {
                    X: 476.0,
                    Y: 678.0,
                    Width: 140.0,
                    Height: 16.0,
                },
                ALIGN_RIGHT,
            );

            paint_text(
                g,
                "F1 оверлей · F2/F3 цель · F4 слежение · F6 автозапуск · End выход",
                9.0,
                false,
                argb(120, 136, 148, 170),
                RectF {
                    X: 18.0,
                    Y: 696.0,
                    Width: 604.0,
                    Height: 16.0,
                },
                ALIGN_LEFT,
            );

            let _ = GdipDeleteGraphics(g);
        }
        surf.blit(hdc);
    }

    let _ = EndPaint(hwnd, &ps);
}

// ---------------------------------------------------------------------------\n// Actions\n// ---------------------------------------------------------------------------\n

fn toggle_autostart(state: &AppState) {
    match crate::autostart::toggle() {
        Ok(on) => {
            state.autostart.store(on, Ordering::Relaxed);
            state.set_notice(if on { "автозапуск: вкл" } else { "автозапуск: выкл" });
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
    state.set_notice(if on { "оверлей: вкл" } else { "оверлей: выкл" });
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
    let st = ctx.state.clone();
    match action {
        Action::ToggleAutostart => toggle_autostart(&st),
        Action::ToggleFollow => {
            let v = !st.follow_window.load(Ordering::Relaxed);
            st.follow_window.store(v, Ordering::Relaxed);
            st.set_notice(if v { "слежение: вкл" } else { "слежение: выкл" });
        }
        Action::ToggleOverlay => toggle_overlay(&st),
        Action::Target(i) => {
            st.target_index.store(i, Ordering::Relaxed);
            if let Ok(mut h) = st.history.lock() {
                h.clear();
            }
        }
        Action::Scale(pct) => {
            st.overlay_scale.store(pct, Ordering::Relaxed);
            crate::settings::save_scale(pct);
            st.set_notice(&format!("размер: {pct}%"));
        }
        Action::Elevate => elevate(&st),
        Action::HideToTray => unsafe {
            let _ = ShowWindow(ctx.hwnd, SW_HIDE);
        },
        Action::CheckUpdate => ctx.state.request_update_check(),
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
    nid.hIcon = LoadIconW(HINSTANCE(std::ptr::null_mut()), IDI_APPLICATION).unwrap_or_default();
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
    let overlay = wide(if overlay_on { "Оверлей: скрыть" } else { "Оверлей: показать" });
    let autostart = wide(if autostart_on { "Автозапуск: выключить" } else { "Автозапуск: включить" });
    let quit = wide("Выход");

    let _ = AppendMenuW(menu, MF_STRING, CMD_SHOW, PCWSTR(show.as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE_OVERLAY, PCWSTR(overlay.as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE_AUTOSTART, PCWSTR(autostart.as_ptr()));
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

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
            unsafe {
                let x = (lparam.0 & 0xffff) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
                if let Some(ctx) = ctx_of(hwnd) {
                    let hovered = hit_test(&ctx.state, x, y);
                    if hovered != ctx.hover {
                        ctx.hover = hovered;
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let x = (lparam.0 & 0xffff) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
                if let Some(ctx) = ctx_of(hwnd) {
                    if let Some(action) = hit_test(&ctx.state, x, y) {
                        perform(ctx, action);
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
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
            state.update_status.store(UPDATE_CHECKING, Ordering::Relaxed);
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
                    // Обновление качается само: сначала MSI, иначе portable ZIP.
                    let Some(url) = release.msi_url.or(release.zip_url) else {
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
                            let _ = crate::update::launch_installer(&shown);
                        }
                        Err(_) => state.update_status.store(UPDATE_FAILED, Ordering::Relaxed),
                    }
                }
                Err(_) => state.update_status.store(UPDATE_FAILED, Ordering::Relaxed),
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
        RegisterClassW(&wc);

        let style = WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX & !WS_THICKFRAME;
        let mut rect = RECT { left: 0, top: 0, right: PANEL_W, bottom: PANEL_H };
        let _ = AdjustWindowRect(&mut rect, style, false);

        let title = wide("mimi(typh) — оверлей FPS и пинга");
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

        // Dark title bar + rounded corners.
        let dark: u32 = 1;
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
            tray: false,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx as isize);
                    state.panel_hwnd.store(hwnd.0 as isize, Ordering::Relaxed);

        if let Some(c) = ctx.as_mut() {
            tray_add(c);
            c.state.request_update_check();
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