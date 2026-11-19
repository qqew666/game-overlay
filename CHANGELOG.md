# 1.5.5

- Add a compact 280 x 82 logical-pixel overlay window with live FPS and ping.
- Toggle Mini Mode through the panel or F7, with saved preference and hotkey-conflict fallback.
- Reuse themes, glass, scale controls and smooth following in both modes.
- Include the WiX Burn bundle source for the offline Setup.exe installer.

# 1.5.4

- Prefer MSI updates for durable installation.
- Activate standalone updates at a stable per-user application path after the previous process exits.
- Create/update Start Menu and existing desktop shortcuts and preserve enabled autostart at the permanent EXE.
- Support cached EXEs launched by the 1.5.3 updater.
- Isolate downloads into unique staging directories and publish completed package files after header validation.
- Add shaped glass gradients, refractive edge highlights and soft shadows.
- Rework the three settings rows with icons, larger switches and keyboard-only focus outlines.
- Clarify update status for source-only releases.

# 1.5.3

- Keep one resident instance and restore its existing panel on repeated launches.
- Main window Close now exits the app and its overlay; the explicit tray button keeps the session alive.
- Preserve explicit elevation and updater handoff paths.
- Display the user-provided artwork in a reusable, resizable in-process viewer with independent close and Escape handling.
- Ignore owned application windows while following the active application.
- Add white and dark themes, persistent selection and theme-aware title bar.
- Change the native glass backdrop to Mica with a softer tint and improve palette button labels.

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
