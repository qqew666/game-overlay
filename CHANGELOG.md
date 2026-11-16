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
