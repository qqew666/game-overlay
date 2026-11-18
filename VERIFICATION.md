# 1.5.4 verification

Windows x64 release build passed. Eight tests cover layout, motion, package headers and numeric patch version comparisons. MSI installation, upgrade from 1.5.3, shortcuts, autostart and uninstallation passed in Wine.

Actual standalone update activation was tested under Wine 10. The downloaded EXE was copied to the permanent application path. Autostart was migrated with correct quoting, a native Shell Link was saved, and the temporary download was deleted. ShellExecute of the Start Menu shortcut then launched the application again. Repeated start/close worked. A legacy 1.5.3 parent-process handoff was also checked.

The settings rows were tested through both their icon area and switch. Keyboard Tab/Space focus and overlay visibility passed. The glass appearance was inspected from an actual application screenshot.

Windows-native composition, real audio output and live HTTPS update downloads still require a Windows check. Source-only GitHub releases intentionally have no downloadable application asset until the owner attaches one.

# 1.5.3 verification

Windows x64 release cross-build and six layout/movement tests passed.
A native Win32 helper and X11 UI input were used with the actual EXE in Wine 10.

- Simultaneous startup created one resident process, panel and overlay.
- Repeated launch preserved both HWNDs and process IDs for the panel and overlay.
- Launch from tray restored the same resident instance.
- White and dark themes repainted and persisted their registry values.
- Clicking Mimi created one artwork window in the same process and reused it on subsequent clicks.
- Closing the artwork kept the panel alive; reopening worked.
- Main WM_CLOSE removed the panel, overlay and owned artwork window.
- A fresh launch after shutdown successfully released/reacquired the instance guard.

Mica rendering, theme title bars and UAC elevation still require a native Windows check. The original user-provided image is embedded unchanged. Applications and installers are not attached to the GitHub release, as requested.

# 1.5.2 verification

Windows x64 release cross-build passed, including the embedded icon resource (.rsrc).
Six existing layout and movement tests pass. MSI installation, upgrade from 1.5.1 and uninstallation passed under Wine.
The actual EXE was tested under Wine 10. F2 changed the ping target and F3 restored it. This was repeated with both keys reserved by a second application, confirming the registration-failure fallback.
All six scale values were checked through UI clicks and registry queries. All three palettes, glass toggle, audio toggle, persistence and Exit were checked. Click audio is embedded WAV data and uses asynchronous WinMM playback.

Native Windows 11 backdrop appearance and audible output require a real Windows desktop and audio device. The supplied art link opens externally; its destination was not downloaded into the application. The separate unspecified application-switch button could not be reproduced without its name or shortcut.

# 1.5.1 verification

Release cross-build for Windows x64 succeeded with Rust 1.100.0 and MinGW GCC 14.
Three motion regression tests and three layout tests passed.

The actual EXE was run in Wine 10. A separate native Win32 probe moved a foreground window and observed 16 distinct overlay positions before settling at the expected coordinates (520, 320). Switching focus to the panel preserved the position.

Installer checks cover installation of the exact EXE, shortcuts, autostart, upgrade from 1.5.0, and removal. Monitor-bound tests cover negative coordinates and an overlay larger than its work area.

Windows gaming performance, physical multi-monitor setups and UAC still require testing on Windows 10/11. Wine does not validate real game FPS or Windows desktop composition.

Historical 1.5.0 report follows.

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
