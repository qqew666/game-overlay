# GameOverlay 1.5.0 verification

## Build

- Release built successfully for x86_64-pc-windows-gnu with Rust 1.100.0 and MinGW GCC 14.
- PE32+ x64 Windows GUI executable; 1,303,040 bytes.
- Imported DLL inspection: only Windows system DLLs; no non-system runtime files required.
- JPEG portrait is embedded by include_bytes! and decoded once in memory.
- git diff --check passed.

## Automated geometry checks

Three passing tests in src/layout.rs cover control reachability, bounds, pairwise non-overlap, and noninteractive titles/gaps.

## Running the actual EXE under Wine 10

Verified with X11 mouse/key events and Windows registry queries:

- Overlay button shows and hides the native overlay window.
- All four scale buttons save the correct DWORD (80, 100, 125, 150).
- Press-and-drag outside the original button cancels the action.
- Autostart toggle creates/removes the application's HKCU Run value.
- Tab followed by Space toggles Follow with visible switch feedback.
- Fifth ping target can be selected (github.com:443, 5 / 5).
- Update button responds independently from Exit and shows failure/retry status.
- Tray button hides the Win32 panel (IsWindowVisible returns 0); restoring the window works.
- A Win32 client resize to 768 x 608 preserves drawing and pointer mapping (150% selected correctly through scaled coordinates).
- Exit closes the application.

Actual application screenshots: docs/panel.png and docs/panel-small.png.

## Limits

This is a Linux build/test environment, not a Windows gaming PC. Wine reports an HTTPS failure for the GitHub update request, so real update download/launch was not verified here. Windows UAC elevation, actual game FPS/ETW accuracy, ICMP availability, tray behavior in Windows Explorer, and layered-window visual composition still need a Windows 10/11 x64 check. No claim is made that Wine measurements represent real game performance.

The executable is unsigned. Existing docs/overlay.png is retained as a historical pre-1.5 screenshot.
