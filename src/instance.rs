//! One resident application, with explicit handoff during elevation or updating.
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

pub struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
fn restore_existing() -> bool {
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("GameOverlayPanel"), None) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
            return true;
        }
    }
    false
}
pub fn enter(handoff: bool) -> Result<Option<Instance>, windows::core::Error> {
    if !handoff && restore_existing() {
        return Ok(None);
    }
    unsafe {
        let handle = CreateMutexW(None, false, w!("Local\\GameOverlay.SingleInstance"))?;
        let guard = Instance(handle);
        if GetLastError() == ERROR_ALREADY_EXISTS && !handoff {
            for _ in 0..40 {
                if restore_existing() {
                    return Ok(None);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            return Ok(None);
        }
        Ok(Some(guard))
    }
}
