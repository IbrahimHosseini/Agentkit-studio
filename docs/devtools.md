# Development tools

Attach a project and use the four icons at the right of the conversation header:
**Browser**, **iOS Simulator**, **Android Emulator**, and **Terminal**. Opening a
tool before the first message creates a project conversation. Panels can be
resized, expanded and closed. Closing a panel hides it; use Close tab or Shut down
to stop its underlying session or device.

## Browser

Start your dev server in Terminal, then enter its localhost URL in Browser. The
pane provides tabs, navigation, live screenshot frames, pointer/keyboard input,
and console output. It runs a real headless Chromium instance through Playwright,
with an isolated, nonpersistent browser context per conversation. It does not
reuse your normal browser profile, logins, extensions or cookies. Web content
never receives the desktop application's privileged Tauri IPC interface.

Install Node.js and Chrome, Chromium or Edge first. Studio detects common browser
locations; `AGENTKIT_NODE_BIN` and `AGENTKIT_BROWSER_BIN` override executable paths.
The desktop bundle includes the Playwright helper and library, but not Node.js or
a browser binary. The standalone CLI in a relocated installation also needs
`AGENTKIT_BROWSER_HELPER` pointing at `browser-runtime/browser.mjs` from the bundle.
No browser is downloaded automatically.

Click the page to focus it; normal typing, paste, shortcuts and wheel scrolling
are forwarded to Chromium. Escape returns focus to the address bar; Tab moves
between Studio controls. Page Tab and Page Escape send those keys to the page.
Only HTTP(S) URLs and `about:blank` are accepted. Downloads and modal page dialogs
are not interactive in this version (dialogs are dismissed and logged).

The pane is screenshot based, not an embedded native browser widget or a video
stream. Browser input currently supports clicks, typing and scrolling; native
drag-and-drop, file uploads, element annotation and recording are not included.
Each conversation supports eight browser tabs, with at most eight browser sessions.

## Terminal

Terminal uses the host's shell and a real PTY, displayed with xterm.js. It supports
interactive input, ANSI output, Ctrl+C, multiple tabs and resizing. The working
directory starts at the attached project. Commands have the permissions of the
Studio user; this terminal is not a sandbox. Output is bounded to the latest 1 MiB
per terminal. Closing a terminal ends its shell; deleting a conversation cleans
up its owned terminals and browser. Closing a pane does not stop a dev server.

## Devices

Choose an existing simulator/emulator and Start it. The pane displays actual
screenshots and supports tap, swipe, text input and hardware keys. Expand Install
and launch app to install a project-local `.app`/`.apk` and launch its bundle ID.
The screenshot control saves the displayed frame. Xcode/runtime and Android
SDK/AVDs must already be installed. Physical devices are excluded.

See [mobile implementation and limitations](devtools-mobile.md), including
Xcode private API compatibility and Android text-input limitations. Rotation and
video recording are not implemented. This is functional development-tool support,
not complete feature parity with Claude Desktop.

## Agent access

User interaction works without enabling agent access. Choose **Allow agent access**
in a pane to grant it for that conversation; **Revoke agent access** blocks future
agent requests. Browser and terminal grants cover the conversation's sessions.
iOS/Android grants cover all simulators/emulators of that platform in the conversation,
not just the currently selected device. Screenshots and terminal output can become
model context and are sent through the selected provider. Use test accounts/data.
Revocation cannot retract output already sent or cancel an action already started.

Writable project tasks receive a temporary `studio_devtools` MCP configuration in
their provider process. No installed agents or user settings are rewritten.
Read-only tasks do not receive this bridge. Successful agent control activity opens
the matching pane. Its MCP tool is `devtools`, with `target` (`browser`, `terminal`,
`ios`, `android`), `action`, and an `arguments` object. The server forces the current
conversation and agent identity and exposes no grant/approval tool. Frame/screenshot
results use MCP image content. Provider execution permissions still apply to the
provider's ordinary tools; granting Terminal explicitly authorizes its shell access.

The optional external MCP bridge remains controlled by Connections. Local trusted
CLI users can invoke the same engine methods, for example:

```sh
agentkit --json rpc devtools/terminal/list --params '{"conversation_id":"CHAT_ID"}'
agentkit --json rpc devtools/browser/open --params '{"conversation_id":"CHAT_ID","url":"http://localhost:3000"}'
agentkit --json rpc devtools/mobile/list --params '{"conversation_id":"CHAT_ID","platform":"ios"}'
```

The bridge's grants are not an OS security boundary against a same-user process
already able to execute arbitrary commands or access the authenticated local engine.

## Verification

See the [verification report](devtools-verification.md) for tested platforms,
native acceptance results and remaining limitations.

Run `python3 -I scripts/verify-isolated.py` with the repository toolchain/cache
configured. It isolates HOME, provider configuration and engine state, and uses
offline provider fixtures. The optional real-Chromium local-fixture test is
`STUDIO_TEST_BROWSER=1 node --test apps/desktop/devtools/browser.test.mjs`; run it
with temporary application homes. It makes no provider/model calls.
