# Development tools verification

Verified on **macOS arm64, 2026-11-16**. Linux and Windows were not tested.

## Automated checks

`python3 -I scripts/verify-isolated.py` passed with isolated application homes,
provider configuration and engine state:

- Rust workspace: 46 tests passed, including mobile SDK fixtures and provider transport tests.
- Svelte check: no errors; one existing accessibility warning in `MessageBody.svelte`.
- JavaScript: 43 passed, one optional Chromium test skipped in this default run.
- Desktop frontend and CLI builds passed.
- Phase 1: 10 tests passed. Phase 2: 69 tests passed.

The optional real Chromium test was run separately: all four tests passed against
a local HTTP fixture, including Persian text input, page actions, screenshots,
console output, tabs, conversation isolation and URL restrictions.

## Native verification

- Native Tauri GUI: Browser loaded and controlled a local fixture; Terminal ran a
  real interactive shell and displayed the expected output marker. Missing Android
  SDK feedback was checked using an isolated home.
- iOS CLI/engine RPC: a disposable iPhone 18 Pro / iOS 27.2 simulator returned real
  1206 × 2622 screenshots. Tap, swipe, Backspace and exact ASCII text entry were
  verified from device screenshots. The temporary simulator was shut down and
  deleted; screenshot temporary files and the test daemon were cleaned up.
- Android CLI/engine RPC: a new disposable Android 37 ARM64 AVD, using an already
  installed system image and a private adb server, passed list, boot, 320 × 640
  PNG capture, Home/Back, deterministic Settings launch, normalized tap, ASCII
  typing and swipe. Screenshots confirmed `network` in Settings search and the
  launcher app drawer after swiping. Shutdown passed; the owned emulator exited,
  its private adb server and daemon stopped, and its temporary AVD/home were
  removed. Existing AVDs were not used or modified. Evidence images remain under
  the ignored `target/devtools-verification/android-*.png` paths.
- Installed application GUI follow-up: the exact `/Applications/Agentkit Studio.app`
  ran with isolated homes/configuration/engine state. Browser accepted Persian paste,
  displayed the button result, navigated a local link/back and created/closed a tab. The Console control
  displayed the fixture marker `log: GUI_BUTTON_OK`; remote scrolling displayed
  the fixture footer `SCROLL_OK`.
  Terminal confirmed project cwd, Unicode output, Ctrl+C, panel resize and real local
  HTTP-server startup. No model requests were needed.
- iOS installed GUI: selected and booted a disposable simulator; launched Settings,
  opened General by tapping, returned Home, swiped into Spotlight and entered exactly
  `StudioGUI123` using the panel's text control. The native Save dialog wrote valid
  1206 × 2622 PNGs. Shutdown was requested through the panel and confirmed before deleting the
  owned test simulator.
- Android installed GUI: booted the disposable AVD, launched Settings, tapped search
  and entered `network`, used Home/Back, swiped the Settings list and tapped System
  near the bottom of the screen. Native Save dialogs wrote actual 480 × 960 frames.
  Resizing and expanding the corrected pane preserved the full image and touch
  alignment; shutdown through the panel left the private adb device list empty.
- GUI checks found and fixed two issues: explicit null engine results incorrectly
  disconnected fresh profiles; a shrinking flex child cropped device screenshots
  and mismatched touch coordinates. Public-CLI and decoder regressions cover null
  handling; native full-frame and bottom-touch checks verify the layout correction.
  The corrected installed build started with providers disconnected. Supervisor
  independently inspected both fixes and reran the full isolated suite successfully.
  The owned simulator, temporary AVD/home, test engine, private adb server and
  local HTTP fixture were cleaned up. The corrected installed application was
  reopened with the regular user profile. GUI-saved device evidence is retained under ignored
  `target/devtools-verification/installed-gui/`.

This GUI pass covers the operations above; installing a project-built `.app`/`.apk`,
agent-driven control with a live provider, and every browser interaction are not
established by this acceptance pass.

Provider integrations were tested with offline fixtures only. No paid model,
real provider run, user login, physical phone or production app was used.

## Build and remaining limitations

A debug macOS `.app` was built successfully. Its bundled CLI matches the verified
CLI binary, and its resources contain the browser helper and Playwright library.
The corrected bundle was installed into Applications. Node.js, a supported Chromium browser,
Xcode runtimes and Android SDK/AVDs remain host prerequisites.

This implementation does not claim complete Claude feature parity. Browser and
device displays use screenshot polling. Recording, rotation and annotation are
not implemented. Device text injection supports ASCII only; browser and terminal
support Unicode. iOS input depends on private Xcode APIs and was verified only
against the installed runtime. See [development tools](devtools.md) and
[mobile details](devtools-mobile.md).
