# Mobile development panels

Studio's iOS Simulator and Android Emulator panels use the same `devtools/mobile/*`
engine operations as the CLI/agent bridge. The devtools router owns conversation
and project validation and agent consent. Screen capture requires the same agent
access grant as input. Listing SDK/device metadata does not capture a screen.

## Prerequisites and behavior

- iOS requires macOS, a full Xcode selected by `xcode-select` (or `DEVELOPER_DIR`),
  and an installed iOS Simulator runtime. `simctl` discovers available iOS devices,
  boots/shuts down a selected device, installs/launches a project build, and captures
  PNG frames. tvOS/watchOS and physical devices are excluded.
- Android uses `adb` and `emulator` from `ANDROID_HOME`, `ANDROID_SDK_ROOT`, the
  standard macOS/Linux/Windows SDK locations, or `PATH`. Create AVDs with Android
  Studio first. Studio lists existing AVDs and running `emulator-*` devices, and
  verifies `ro.kernel.qemu=1` before sending input or capturing a device.
- No SDK, runtime, emulator, or third-party keyboard is downloaded automatically.
  Missing tools and inaccessible simulator services produce actionable errors.
- Hiding a pane is not a shutdown. The user explicitly chooses Shutdown. Studio
  never deletes a simulator/AVD or automatically stops a device booted elsewhere.
- Frames are real PNG screenshots, polled by the frontend; this is not a native
  video stream. Captures are limited to 8 MiB. iOS uses a temporary PNG in the
  private engine directory, removed after reading or failure; frames are not saved
  into conversation history.
- User and agent actions affect the same virtual device. Actions are serialized
  per device to prevent keyboard and gesture sequences from interleaving.

## iOS input helper

`ios-input.m` is embedded in the engine binary at compile time. On the first input
operation Studio builds it with the selected Xcode's `clang`, Foundation and
AppKit, inside its private engine data directory. A changed helper source triggers
a rebuild. No additional package or host Accessibility permission is needed.

The helper uses the selected simulator UDID and CoreSimulator/SimulatorKit's
private HID transport: modern DTUHID XPC when available, with legacy Indigo HID
fallback. It never sends host mouse or keyboard events. It checks
frameworks, classes, selectors and symbols before use and reports unsupported
versions. These private APIs can change between Xcode releases; a successful
build or event-delivery callback alone does not prove every target UI responds.

The ABI was researched against the primary implementation in Meta's MIT-licensed
[idb Indigo header](https://github.com/facebook/idb/blob/main/PrivateHeaders/SimulatorApp/Indigo.h)
and [HID implementation](https://github.com/facebook/idb/blob/main/FBSimulatorControl/HID/SimulatorIndigoHID.swift).
The Studio helper is a small independent Objective-C implementation, not a bundled
copy of idb. Touch is single-pointer only. Rotation and recording are not currently
exposed by this adapter.

Text uses actual device HID key events with a US ASCII mapping, up to 256 characters
per action (including newline and tab). Other scripts return an explicit error;
use the device's on-screen keyboard for those inputs. Unicode clipboard paste was
not reliable on the tested iOS 27.2 runtime without an attached hardware keyboard,
so it is not exposed as a successful text operation. Studio does not read or modify
the host clipboard or hardware-keyboard preferences.

Android SDK `input text` supports single-line ASCII. Other scripts and literal
`%s` return a clear error rather than being corrupted. Use the emulator's keyboard
for those inputs. Shell metacharacters are quoted for the Android shell.

## Engine interface

All requests require `conversation_id` and `platform` (`ios` or `android`). The
conversation must have an attached project. Device operations also require
`device_id`: an iOS UDID or Android `emulator-5554` style serial. To boot an existing
Android AVD, use its list identifier `avd:NAME` and retain the returned running
`device_id`.

| Action | Additional fields | Result |
| --- | --- | --- |
| `list`, `status` | none | `available`, optional `reason`, `devices`, `capabilities`, `notes` |
| `boot` | `device_id` | `ok`, running `device_id`, `state` |
| `shutdown` | `device_id` | `ok`, `device_id` |
| `screenshot` | `device_id` | `mime_type`, base64 `data`, pixel `width`, `height` |
| `tap` | normalized `x`, `y` | `ok` |
| `swipe` | normalized `x`, `y`, `x2`, `y2`; optional `duration_ms` | `ok` |
| `type` | `text` | `ok` |
| `key` | `key` | `ok` |
| `install` | `app_path` | `ok` |
| `launch` | `bundle_id` | `ok` |

Coordinates range from 0 to 1 across the current captured display. Swipe duration
is 100–2000 ms. Keys: `home`, `lock`, `volume_up`, `volume_down`, `enter`,
`backspace`, `tab`, `escape`; `back` is Android-only. Install accepts a simulator
`.app` directory or `.apk` file inside the canonical project directory. Symlinks
cannot escape that directory. Builds themselves run through the terminal tools.

## Verification boundaries

Verified on macOS arm64 with the installed Xcode and a newly created disposable
iPhone 18 Pro / iOS 27.2 simulator. Native screenshots showed taps opening General,
swipes scrolling, and Backspace modifying a focused Settings search field. Actual
CLI RPC HID typing visibly entered the exact marker `StudioTest123`. The
actual Studio CLI RPC captured valid 1206 × 2622 PNG frames and removed temporary
capture files. The helper was compiled with warnings enabled.

Nine isolated Rust unit/fake-SDK integration checks cover input validation,
device filtering, screenshot file handling and metadata, command dispatch, stdin
closure, output bounds and project containment. No provider or real account was
used. Android was also exercised through real CLI RPC on a disposable Android 37
AVD: discovery, boot, PNG capture, Home, Settings launch, tap and visible ASCII
search input passed. See the [verification report](devtools-verification.md) for
the complete native acceptance and cleanup results. Linux and Windows have not
been exercised for this adapter.
