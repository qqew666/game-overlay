# 1.5.2

- Embedded beige application icon, also used by the panel and tray.
- Three persisted color palettes and native Windows system backdrop with fallback.
- Asynchronous click audio with a saved mute toggle.
- Added 40% and 60% overlay scales with matching persistence bounds.
- Default ping target selection has three endpoints; explicit CLI targets remain supported.
- F2/F3 work through fallback polling when global registration fails.
- Discard an old ping measurement after changing the target.
- Clickable Mimi portrait opens the user-specified art page in the system browser.
- Compact palette and scale controls share pointer/keyboard layout definitions.

# 1.5.1

- Follow movement updates every 16 ms with time-based smoothing; metrics continue to redraw every 100 ms.
- Opening the control panel preserves the overlay position.
- Monitor work-area bounds support negative coordinates and keep the card above the taskbar.
- Avoid redundant window moves and release timers, hotkeys and the overlay state on shutdown.
- Autostart recreates a missing HKCU Run key instead of failing after an uninstall.
- Tests cover movement convergence, refresh-rate independence and monitor bounds.

# 1.5.0

- Rebuilt the control panel with a beige and brown palette shared by the overlay.
- Embedded a signed Mimi portrait (character by Typh); no external image files needed at runtime.
- Separated all buttons and section headings. A shared geometry module drives drawing, hit testing and tab navigation.
- Added mouse capture, cancellation when releasing outside the original button, hover and keyboard focus feedback, Tab/Shift+Tab/Enter/Space navigation.
- Fit the panel to the available desktop work area, with matching scaled hit testing.
- Added previous/next ping target buttons for any number of configured targets.
- Restored the update action in the tray menu and exposed update failures in the panel.
- Download standalone EXE updates preferentially; launch only after an explicit second click.
- Build script now produces dist/GameOverlay.exe directly, with no WiX/ZIP requirement.
- Use Windows system TLS for GitHub HTTPS requests.

Validation: release cross-build for Windows x64; independent layout regression tests.
See the accompanying verification report for runtime checks and limitations.
