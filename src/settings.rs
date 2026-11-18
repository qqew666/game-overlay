//! Settings that outlive a run.
//!
//! One DWORD under `HKCU\Software\mimi(typh)\GameOverlay`, so the overlay comes
//! back at the size it was left at and nothing needs administrator rights.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_DWORD,
};

const SUBKEY: PCWSTR = w!("Software\\mimi(typh)\\GameOverlay");
const VALUE_SCALE: PCWSTR = w!("OverlayScale");

pub const DEFAULT_SCALE: u32 = 100;
/// Anything outside this range in the registry is treated as corrupt.
const SCALE_MIN: u32 = 40;
const SCALE_MAX: u32 = 200;

pub fn load_scale() -> u32 {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, SUBKEY, 0, KEY_QUERY_VALUE, &mut key) != ERROR_SUCCESS {
            return DEFAULT_SCALE;
        }
        let mut data: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let mut kind = REG_DWORD;
        let status = RegQueryValueExW(
            key,
            VALUE_SCALE,
            None,
            Some(&mut kind),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if status == ERROR_SUCCESS && (SCALE_MIN..=SCALE_MAX).contains(&data) {
            data
        } else {
            DEFAULT_SCALE
        }
    }
}

pub fn save_scale(scale: u32) {
    let scale = scale.clamp(SCALE_MIN, SCALE_MAX);
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyW(HKEY_CURRENT_USER, SUBKEY, &mut key) != ERROR_SUCCESS {
            return;
        }
        let bytes = scale.to_ne_bytes();
        let _ = RegSetValueExW(key, VALUE_SCALE, 0, REG_DWORD, Some(&bytes));
        let _ = RegCloseKey(key);
    }
}

pub fn load_preference(name: &str, fallback: u32) -> u32 {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, SUBKEY, 0, KEY_QUERY_VALUE, &mut key) != ERROR_SUCCESS {
            return fallback;
        }
        let mut value = 0u32;
        let mut size = 4;
        let mut kind = REG_DWORD;
        let result = RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if result == ERROR_SUCCESS && kind == REG_DWORD && size == 4 {
            value
        } else {
            fallback
        }
    }
}
pub fn save_preference(name: &str, value: u32) {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyW(HKEY_CURRENT_USER, SUBKEY, &mut key) == ERROR_SUCCESS {
            let _ = RegSetValueExW(
                key,
                PCWSTR(name.as_ptr()),
                0,
                REG_DWORD,
                Some(&value.to_le_bytes()),
            );
            let _ = RegCloseKey(key);
        }
    }
}
