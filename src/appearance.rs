//! Shared appearance and asynchronous click audio.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::Win32::UI::Controls::MARGINS;
pub static THEME: AtomicU32 = AtomicU32::new(0);
pub static GLASS: AtomicBool = AtomicBool::new(true);
pub static NATIVE_GLASS: AtomicBool = AtomicBool::new(false);
pub static SOUND: AtomicBool = AtomicBool::new(true);
pub fn init() {
    THEME.store(
        crate::settings::load_preference("Theme", 0).min(2),
        Ordering::Relaxed,
    );
    GLASS.store(
        crate::settings::load_preference("Glass", 1) != 0,
        Ordering::Relaxed,
    );
    SOUND.store(
        crate::settings::load_preference("Sound", 1) != 0,
        Ordering::Relaxed,
    );
}
pub fn tint(color: u32) -> u32 {
    let a = color & 0xff000000;
    let r = (color >> 16) & 255;
    let g = (color >> 8) & 255;
    let b = color & 255;
    if r < g || g < b {
        return color;
    }
    match THEME.load(Ordering::Relaxed) {
        1 => a | (r << 16) | (((g + b) / 2) << 8) | g,
        2 => a | (g << 16) | (r << 8) | b,
        _ => color,
    }
}
pub fn click() {
    if SOUND.load(Ordering::Relaxed) {
        unsafe {
            let _ = PlaySoundW(
                PCWSTR(include_bytes!("../assets/click.wav").as_ptr().cast()),
                None,
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}
pub unsafe fn apply_glass(hwnd: HWND) {
    let enabled = GLASS.load(Ordering::Relaxed);
    let backdrop: u32 = if enabled { 4 } else { 1 };
    let available = DwmSetWindowAttribute(
        hwnd,
        windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(38),
        &backdrop as *const _ as *const _,
        4,
    )
    .is_ok();
    NATIVE_GLASS.store(enabled && available, Ordering::Relaxed);
    let margin = if enabled && available { -1 } else { 0 };
    let _ = DwmExtendFrameIntoClientArea(
        hwnd,
        &MARGINS {
            cxLeftWidth: margin,
            cxRightWidth: margin,
            cyTopHeight: margin,
            cyBottomHeight: margin,
        },
    );
}
