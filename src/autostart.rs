//! Per-user autostart.
//!
//! Uses the `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value, so no
//! administrator rights are needed and the entry is removed with the profile.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
/// Value name as the user sees it in Task Manager -> Startup.
const VALUE_NAME: PCWSTR = w!("GameOverlay");
const MAX_VALUE_BYTES: u32 = 32768;

/// Opens the Run key with write access for `set`/`clear`.
fn open_run_key(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, access, &mut key) };
    if status == ERROR_SUCCESS {
        Some(key)
    } else {
        None
    }
}

/// Path of the running executable, quoted, as written into the Run value.
fn command_line() -> Option<String> {
    let path = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --tray-quiet", path.display()))
}

/// Registers the current executable for the current user.
pub fn enable() -> Result<(), String> {
    let cmd = command_line().ok_or("cannot resolve the executable path")?;
    let key = open_run_key(KEY_SET_VALUE).ok_or("cannot open HKCU Run key for writing")?;

    // REG_SZ must include the terminating NUL in the byte count.
    let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe {
        std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * std::mem::size_of::<u16>())
    };

    let status = unsafe { RegSetValueExW(key, VALUE_NAME, 0, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(key);
    }

    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("RegSetValueExW failed with {status:?}"))
    }
}

/// Removes the Run value. Succeeds when the value was not present.
pub fn disable() -> Result<(), String> {
    let Some(key) = open_run_key(KEY_SET_VALUE) else {
        return Ok(());
    };
    let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    match status {
        s if s == ERROR_SUCCESS || s == ERROR_FILE_NOT_FOUND => Ok(()),
        s => Err(format!("RegDeleteValueW failed with {s:?}")),
    }
}

/// True when a Run value for the overlay exists.
pub fn is_enabled() -> bool {
    let Some(key) = open_run_key(KEY_QUERY_VALUE) else {
        return false;
    };
    let mut size: u32 = 0;
    let status = unsafe { RegQueryValueExW(key, VALUE_NAME, None, None, None, Some(&mut size)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    status == ERROR_SUCCESS && size > 0 && size <= MAX_VALUE_BYTES
}

/// Flips the current state and reports the new one.
pub fn toggle() -> Result<bool, String> {
    if is_enabled() {
        disable()?;
        Ok(false)
    } else {
        enable()?;
        Ok(true)
    }
}
