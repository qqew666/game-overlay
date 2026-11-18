//! Durable activation for downloaded standalone updates at a stable application path.
use std::path::{Path, PathBuf};
use windows::core::w;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink,
    KF_FLAG_DEFAULT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, MessageBoxW, MB_ICONERROR, MB_OK,
};
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
use std::os::windows::ffi::OsStrExt;
fn known_folder(id: &windows::core::GUID) -> Result<PathBuf, String> {
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).map_err(|e| e.to_string())?;
        let text = raw.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(raw.0.cast()));
        text.map(PathBuf::from)
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
fn shortcut(destination: &Path, target: &Path) -> Result<(), String> {
    unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let target_w = wide(target);
        link.SetPath(PCWSTR(target_w.as_ptr()))
            .map_err(|e| e.to_string())?;
        let working = wide(target.parent().ok_or("Нет каталога приложения")?);
        link.SetWorkingDirectory(PCWSTR(working.as_ptr()))
            .map_err(|e| e.to_string())?;
        link.SetIconLocation(PCWSTR(target_w.as_ptr()), 0)
            .map_err(|e| e.to_string())?;
        let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        let path = wide(destination);
        file.Save(PCWSTR(path.as_ptr()), true)
            .map_err(|e| e.to_string())
    }
}
fn wait_for_previous(pid: u32) -> Result<(), String> {
    if pid == 0 || pid == std::process::id() {
        return Ok(());
    }
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let result = WaitForSingleObject(handle, 15000);
            let _ = CloseHandle(handle);
            if result != WAIT_OBJECT_0 {
                return Err(
                    "Предыдущая версия ещё работает. Закройте её и повторите обновление.".into(),
                );
            }
        }
    }
    Ok(())
}
fn activate(pid: u32, args: &[std::ffi::OsString]) -> Result<(), String> {
    wait_for_previous(pid)?;
    let source = std::env::current_exe().map_err(|e| e.to_string())?;
    let local = std::env::var_os("LOCALAPPDATA").ok_or("Не найден LOCALAPPDATA")?;
    let directory = PathBuf::from(local).join("Programs").join("GameOverlay");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let target = directory.join("GameOverlay.exe");
    let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
    if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
        let pending = directory.join(format!("GameOverlay-{}.pending", std::process::id()));
        std::fs::write(&pending, &bytes).map_err(|e| e.to_string())?;
        if let Err(error) = std::fs::rename(&pending, &target) {
            let _ = std::fs::remove_file(pending);
            return Err(format!("Не удалось сохранить обновление: {error}"));
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
    }
    let _apartment = Apartment;
    let programs = known_folder(&FOLDERID_Programs)?.join("GameOverlay");
    std::fs::create_dir_all(&programs).map_err(|e| e.to_string())?;
    shortcut(&programs.join("GameOverlay.lnk"), &target)?;
    if let Ok(desktop) = known_folder(&FOLDERID_Desktop) {
        let path = desktop.join("GameOverlay.lnk");
        if path.exists() {
            shortcut(&path, &target)?;
        }
    }
    if crate::autostart::is_enabled() {
        crate::autostart::enable_for(&target)?;
    }
    std::process::Command::new(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .args(args)
        .spawn()
        .map_err(|e| format!("Запуск обновления: {e}"))?;
    Ok(())
}
/// Runs before normal startup, including legacy updaters that launch a cached EXE directly.
pub fn bootstrap() -> bool {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let explicit = args
        .first()
        .map(|s| s == "--install-update")
        .unwrap_or(false);
    let cached = std::env::current_exe()
        .ok()
        .map(|p| {
            p.ancestors().skip(1).any(|a| {
                a.file_name()
                    .map(|n| {
                        n.to_string_lossy()
                            .eq_ignore_ascii_case("GameOverlay-update")
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if !explicit && !cached {
        return false;
    }
    let mut pid = if explicit {
        args.get(1)
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    } else {
        0
    };
    if !explicit {
        unsafe {
            if let Ok(window) = FindWindowW(w!("GameOverlayPanel"), None) {
                GetWindowThreadProcessId(window, Some(&mut pid));
            }
        }
    }
    let forwarded = if explicit {
        args.get(2..).unwrap_or(&[]).to_vec()
    } else {
        args.into_iter().filter(|a| a != "--replace").collect()
    };
    if let Err(error) = activate(pid, &forwarded) {
        crate::log::init(true);
        crate::vlog!("Update activation failed: {error}");
        let message: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                w!("GameOverlay — обновление"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    true
}
