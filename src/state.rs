//! Shared state between the control panel, the overlay and the measuring
//! threads. Everything the UI reads lives here; no thread owns the truth.

use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU8, AtomicUsize, Ordering,
};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::ping::Target;

/// Rolling ping samples, newest last.
pub const HISTORY_LEN: usize = 60;

/// Where the FPS number comes from. The panel shows this honestly: the DXGI
/// provider reports the game itself, the DWM counters only the compositor.
pub const SOURCE_NONE: u8 = 0;
pub const SOURCE_ETW: u8 = 1;
pub const SOURCE_DWM: u8 = 2;

/// Состояние проверки обновления, отображаемое на кнопке панели.
pub const UPDATE_IDLE: u8 = 0;
pub const UPDATE_CHECKING: u8 = 1;
pub const UPDATE_NONE: u8 = 2;
pub const UPDATE_NEWER: u8 = 3;
pub const UPDATE_READY: u8 = 4;
pub const UPDATE_FAILED: u8 = 5;

pub struct AppState {
    /// Round trip time in tenths of a millisecond, `-1` when lost.
    pub ping_deci: AtomicI32,
    pub ping_lost: AtomicBool,
    pub fps: AtomicU32,
    pub frametime_centi_ms: AtomicU32,
    pub fps_source: AtomicU8,
    /// Free-form description of the FPS source, shown in the panel footer.
    pub fps_detail: Mutex<String>,
    /// Title of the window the overlay follows, e.g. "Rust".
    pub game_name: Mutex<String>,
    pub visible: AtomicBool,
    pub compact: AtomicBool,
    compact_change: Mutex<()>,
    /// Overlay size in percent of the designed card (100 = as designed).
    pub overlay_scale: AtomicU32,
    pub target_index: AtomicUsize,
    pub follow_window: AtomicBool,
    pub should_quit: AtomicBool,
    pub verbose: AtomicBool,
    pub autostart: AtomicBool,
    /// Rolling ping samples, see [`HISTORY_LEN`].
    pub history: Mutex<Vec<i32>>,
    /// `host:port` of the target currently being probed.
    pub target_name: Mutex<String>,
    pub targets: Mutex<Vec<Target>>,
    /// Short-lived status line ("автозапуск: вкл") and when it was raised.
    pub notice: Mutex<Option<(String, Instant)>>,
    /// Window position chosen by the positioning pass, used when blitting.
    pub pos: Mutex<(i32, i32)>,
    /// Handle of the overlay window, `0` before it exists.
    pub overlay_hwnd: AtomicIsize,
    /// Handle of the panel window, `0` before it exists.
    pub panel_hwnd: AtomicIsize,
    /// Один из `UPDATE_*`: что показывать на кнопке проверки обновлений.
    pub update_status: AtomicU8,
    /// Версия из последнего релиза, как её вернул GitHub.
    pub update_latest: Mutex<String>,
    /// Флаг-запрос: воркер увидит его и пойдёт в сеть.
    pub update_request: AtomicBool,
    /// Путь к скачанному установщику, когда обновление уже на диске.
    pub update_file: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(targets: Vec<Target>) -> Arc<AppState> {
        Arc::new(AppState {
            ping_deci: AtomicI32::new(-1),
            ping_lost: AtomicBool::new(true),
            fps: AtomicU32::new(0),
            frametime_centi_ms: AtomicU32::new(0),
            fps_source: AtomicU8::new(SOURCE_NONE),
            fps_detail: Mutex::new(String::new()),
            game_name: Mutex::new(String::new()),
            visible: AtomicBool::new(true),
            compact_change: Mutex::new(()),
            compact: AtomicBool::new(crate::settings::load_preference("MiniMode", 0) != 0),
            overlay_scale: AtomicU32::new(crate::settings::DEFAULT_SCALE),
            target_index: AtomicUsize::new(0),
            follow_window: AtomicBool::new(true),
            should_quit: AtomicBool::new(false),
            verbose: AtomicBool::new(false),
            autostart: AtomicBool::new(false),
            history: Mutex::new(Vec::new()),
            target_name: Mutex::new(String::new()),
            targets: Mutex::new(targets),
            notice: Mutex::new(None),
            pos: Mutex::new((0, 0)),
            overlay_hwnd: AtomicIsize::new(0),
            panel_hwnd: AtomicIsize::new(0),
            update_status: AtomicU8::new(UPDATE_IDLE),
            update_latest: Mutex::new(String::new()),
            update_request: AtomicBool::new(false),
            update_file: Mutex::new(None),
        })
    }

    pub fn toggle_compact(&self) {
        let Ok(_guard) = self.compact_change.lock() else {
            return;
        };
        let enabled = !self.compact.fetch_xor(true, Ordering::Relaxed);
        crate::settings::save_preference("MiniMode", enabled as u32);
        self.set_notice(if enabled {
            "Мини-режим включён"
        } else {
            "Полный оверлей включён"
        });
    }

    /// Просит фоновый воркер проверить обновление. Повторный вызов во время
    /// уже идущей проверки ничего не ломает: флаг просто останется взведённым.
    pub fn request_update_check(&self) {
        self.update_status.store(UPDATE_CHECKING, Ordering::Relaxed);
        self.update_request.store(true, Ordering::Relaxed);
    }

    pub fn update_latest_string(&self) -> String {
        self.update_latest
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub fn push_ping_sample(&self, deci: i32) {
        if let Ok(mut h) = self.history.lock() {
            h.push(deci);
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

    pub fn target_count(&self) -> usize {
        self.targets.lock().map(|t| t.len()).unwrap_or(0)
    }

    /// Moves to the next (`+1`) or previous (`-1`) ping target.
    pub fn cycle_target(&self, delta: isize) {
        let count = self.target_count();
        if count == 0 {
            return;
        }
        let current = self.target_index.load(Ordering::Relaxed) as isize;
        let next = (current + delta).rem_euclid(count as isize) as usize;
        self.target_index.store(next, Ordering::Relaxed);
        if let Ok(mut h) = self.history.lock() {
            h.clear();
        }
    }

    pub fn game_name_string(&self) -> String {
        self.game_name.lock().map(|g| g.clone()).unwrap_or_default()
    }
}
