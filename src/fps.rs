//! Frame rate of the foreground window.
//!
//! The primary source is the DXGI ETW provider: every `Present` call the
//! focused process makes is counted, so the number is the game's own frame
//! rate, not a compositor side effect. Starting a realtime trace session needs
//! administrator rights (or membership in Performance Log Users), so when the
//! session cannot be started the module falls back to the DWM composition
//! counters — which is what the old build always reported, and why it sat at
//! ~10 on the desktop.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE,
};
use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
    CONTROLTRACE_HANDLE, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP,
    EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

use crate::state::{AppState, SOURCE_DWM, SOURCE_ETW};
use crate::vlog;

/// `Microsoft-Windows-DXGI`.
const DXGI_PROVIDER: GUID = GUID::from_values(
    0xCA11_C036,
    0x0102,
    0x4A2D,
    [0xA6, 0xAD, 0xF0, 0x3C, 0xFE, 0xD5, 0xD3, 0xC9],
);
const SESSION_NAME: &str = "GameOverlayFps";

/// `Present_Start` and `PresentMultiplaneOverlay_Start`; one per presented
/// frame on both the legacy blt and the multiplane overlay path.
const EVENT_PRESENT_START: u16 = 42;
const EVENT_MPO_PRESENT_START: u16 = 178;

const TRACE_LEVEL_INFORMATION: u8 = 4;

/// Presents seen by the consumer callback for [`WATCH_PID`].
static PRESENT_COUNT: AtomicU64 = AtomicU64::new(0);
static WATCH_PID: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Foreground window
// ---------------------------------------------------------------------------

struct Foreground {
    hwnd: isize,
    pid: u32,
    title: String,
}

fn foreground() -> Option<Foreground> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
        Some(Foreground {
            hwnd: hwnd.0 as isize,
            pid,
            title: String::from_utf16_lossy(&buf[..len]),
        })
    }
}

/// Executable name without the extension, e.g. `RustClient`.
fn process_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let file = path.rsplit('\\').next().unwrap_or(&path);
        let stem = file.trim_end_matches(".exe");
        if stem.is_empty() {
            None
        } else {
            Some(stem.to_string())
        }
    }
}

/// Best label for the focused window: the process name, else the title.
fn describe(fg: &Foreground) -> String {
    if let Some(name) = process_name(fg.pid) {
        return name;
    }
    let title = fg.title.trim();
    if title.is_empty() {
        "рабочий стол".to_string()
    } else if title.chars().count() > 28 {
        format!("{}…", title.chars().take(27).collect::<String>())
    } else {
        title.to_string()
    }
}

// ---------------------------------------------------------------------------
// ETW session
// ---------------------------------------------------------------------------

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    if record.is_null() {
        return;
    }
    let r = &*record;
    if r.EventHeader.ProviderId != DXGI_PROVIDER {
        return;
    }
    let id = r.EventHeader.EventDescriptor.Id;
    if id != EVENT_PRESENT_START && id != EVENT_MPO_PRESENT_START {
        return;
    }
    let pid = WATCH_PID.load(Ordering::Relaxed);
    if pid == 0 || r.EventHeader.ProcessId != pid {
        return;
    }
    PRESENT_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Session properties followed by the logger name, in one allocation: ETW
/// stores the name inside the buffer the header points into.
fn session_buffer(name: &[u16]) -> Vec<u8> {
    let base = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    let name_bytes = name.len() * 2;
    let mut buf = vec![0u8; base + name_bytes];
    unsafe {
        let p = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
        (*p).Wnode.BufferSize = buf.len() as u32;
        (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*p).LoggerNameOffset = base as u32;
        (*p).LogFileNameOffset = 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(
            name.as_ptr().cast::<u8>(),
            buf.as_mut_ptr().add(base),
            name_bytes,
        );
    }
    buf
}

fn session_name() -> Vec<u16> {
    SESSION_NAME.encode_utf16().chain(std::iter::once(0)).collect()
}

fn stop_session() -> Result<(), String> {
    let name = session_name();
    let mut buf = session_buffer(&name);
    let status = unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            PCWSTR(name.as_ptr()),
            buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            EVENT_TRACE_CONTROL_STOP,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("ControlTraceW(STOP) -> {}", status.0))
    }
}

/// Starts the realtime session and the consumer thread. Error text is shown in
/// the panel, so it names the privilege that is missing.
fn start_etw() -> Result<(), String> {
    let name = session_name();
    let mut buf = session_buffer(&name);
    let mut session = CONTROLTRACE_HANDLE::default();

    let mut status = unsafe {
        StartTraceW(
            &mut session,
            PCWSTR(name.as_ptr()),
            buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
        )
    };

    // A previous run that was killed outright can leave the logger behind.
    let mut attempt = 0;
    while status == ERROR_ALREADY_EXISTS && attempt < 3 {
        attempt += 1;
        let _ = stop_session();
        std::thread::sleep(Duration::from_millis(150));
        buf = session_buffer(&name);
        status = unsafe {
            StartTraceW(
                &mut session,
                PCWSTR(name.as_ptr()),
                buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            )
        };
    }

    if status != ERROR_SUCCESS {
        return Err(match status.0 {
            5 => "без прав администратора".to_string(),
            183 => "сессия ETW занята другой копией".to_string(),
            code => format!("StartTrace -> {code}"),
        });
    }

    let enabled = unsafe {
        EnableTraceEx2(
            session,
            &DXGI_PROVIDER,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
            TRACE_LEVEL_INFORMATION,
            0,
            0,
            0,
            None,
        )
    };
    if enabled != ERROR_SUCCESS {
        let _ = stop_session();
        return Err(format!("EnableTraceEx2 -> {}", enabled.0));
    }

    // The consumer runs on its own thread; `ProcessTrace` blocks until the
    // session is stopped.
    std::thread::spawn(move || unsafe {
        let mut logfile = EVENT_TRACE_LOGFILEW::default();
        logfile.LoggerName = PWSTR(name.as_ptr() as *mut u16);
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(on_event);

        let handle = OpenTraceW(&mut logfile);
        if handle.Value == u64::MAX {
            vlog!("[fps] OpenTraceW failed");
            return;
        }
        let _ = ProcessTrace(&[handle], None, None);
        let _ = CloseTrace(handle);
        vlog!("[fps] consumer stopped");
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Sampling
// ---------------------------------------------------------------------------

fn publish(state: &AppState, fps: f64) {
    let old = state.fps.load(Ordering::Relaxed) as f64;
    let smoothed = if old <= 0.0 { fps } else { old * 0.5 + fps * 0.5 };
    let value = smoothed.round().clamp(0.0, 9999.0) as u32;
    state.fps.store(value, Ordering::Relaxed);
    let ms = if value > 0 { 1000.0 / value as f64 } else { 0.0 };
    state
        .frametime_centi_ms
        .store((ms * 100.0).round() as u32, Ordering::Relaxed);
}

fn etw_sampler(state: Arc<AppState>) {
    let mut last_hwnd: isize = 0;

    loop {
        if state.should_quit.load(Ordering::Relaxed) {
            break;
        }

        let fg = foreground();
        let pid = fg.as_ref().map(|f| f.pid).unwrap_or(0);
        let hwnd = fg.as_ref().map(|f| f.hwnd).unwrap_or(0);

        if hwnd != last_hwnd {
            last_hwnd = hwnd;
            if let Some(f) = fg.as_ref() {
                let label = describe(f);
                if let Ok(mut name) = state.game_name.lock() {
                    *name = label;
                }
            } else if let Ok(mut name) = state.game_name.lock() {
                name.clear();
            }
        }

        WATCH_PID.store(pid, Ordering::Relaxed);
        PRESENT_COUNT.store(0, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(1000));
        let presents = PRESENT_COUNT.load(Ordering::Relaxed);

        if state.should_quit.load(Ordering::Relaxed) {
            break;
        }

        let fps = presents as f64;
        publish(&state, fps);
        if state.verbose.load(Ordering::Relaxed) {
            vlog!("[fps] pid {pid} -> {presents} presents/s");
        }
    }
}

/// Composition counters of DWM. Kept as the fallback: it is honest about the
/// compositor, not the game, and idles at ~10 Hz when nothing is presenting.
fn dwm_sampler(state: Arc<AppState>) {
    let mut last_count: u64 = 0;
    let mut last_time = Instant::now();

    loop {
        if state.should_quit.load(Ordering::Relaxed) {
            break;
        }

        if let Some(fg) = foreground() {
            let label = describe(&fg);
            if let Ok(mut name) = state.game_name.lock() {
                *name = label;
            }
        }

        let mut info = DWM_TIMING_INFO::default();
        info.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;
        let result = unsafe {
            DwmGetCompositionTimingInfo(windows::Win32::Foundation::HWND(std::ptr::null_mut()), &mut info)
        };

        if result.is_ok() {
            let count = if info.cDXPresentSubmitted > 0 {
                info.cDXPresentSubmitted as u64
            } else {
                info.cFrameSubmitted
            };
            if count > last_count && last_time.elapsed() > Duration::from_millis(400) {
                let fps = (count - last_count) as f64 / last_time.elapsed().as_secs_f64();
                publish(&state, fps);
                last_count = count;
                last_time = Instant::now();
            } else if count < last_count {
                last_count = count;
                last_time = Instant::now();
            }
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Starts measuring. Called once, after `AppState` exists.
pub fn start(state: Arc<AppState>) {
    match start_etw() {
        Ok(()) => {
            state.fps_source.store(SOURCE_ETW, Ordering::Relaxed);
            if let Ok(mut d) = state.fps_detail.lock() {
                *d = "точно · DXGI Present".to_string();
            }
            std::thread::spawn(move || etw_sampler(state));
        }
        Err(err) => {
            vlog!("[fps] ETW unavailable: {err}");
            state.fps_source.store(SOURCE_DWM, Ordering::Relaxed);
            if let Ok(mut d) = state.fps_detail.lock() {
                *d = format!("базовый режим · {err}");
            }
            std::thread::spawn(move || dwm_sampler(state));
        }
    }
}

pub fn shutdown() {
    let _ = stop_session();
}
