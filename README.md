# Agentkit Studio

A desktop workspace for chatting with models and coordinating coding agents through your existing Codex and Claude Code installations.

Start a project-free conversation to chat directly with a selected model. Attach a project to let Studio’s internal Superagent propose tasks, select agents and coordinate execution. Each chat saves its permission mode; writing plans wait for approval in the default **Ask for approval** mode.

Studio is an independent application built with Tauri 2, Svelte 5, TypeScript and Rust. The desktop app and standalone CLI share one engine; its local daemon owns the SQLite history database. The separate agentkit project is not required.

## Features

- **Direct model conversations:** choose a provider, model and reasoning effort per project-free chat, with persistent selection and history.
- **Project orchestration:** coordinate installed roles with task dependencies, activity details, results, stop controls and approvals for writing plans.
- **Per-chat permissions:** choose Ask for approval, Approve for me with native provider review, or Full access from the composer. Project-free chats and read-only tasks retain their read-only restrictions.
- **Agent settings:** enable roles, set model and effort defaults, and define additional allowed provider/model/effort combinations. Roles with the same name across providers appear as one agent with multiple definition sources.
- **Automatic model replacement:** disconnecting a provider moves affected agents to an explicitly allowed, same-name or recognized same-tier model on a connected provider. If no replacement exists, Studio retains the saved selection and displays a red agent-card border with guidance. Direct-chat selections remain unchanged.
- **Existing provider accounts:** connect Codex or Claude Code using native CLI sign-in. Claude supports its existing routing configuration or the official account route.
- **Local history:** manage projects and conversations, sort chats and inspect provider activity. Closing the window does not cancel a running task.
- **Attachments and mixed text:** attach files and images; write Persian, Arabic or mixed-direction prose in an English, left-to-right interface.
- **CLI and optional MCP bridge:** access the same engine from the terminal or enable external MCP access in Connections.
- **Development tools:** open Browser, iOS Simulator, Android Emulator and a real Terminal beside the chat, with explicit agent access controls. See [setup, usage and current limitations](docs/devtools.md).

Studio settings do not rewrite installed agent definitions. Installed or advertised catalog metadata does not guarantee that an account can execute a model.

## Installation

These instructions build and install from source. Build on the operating system you want to run. Prebuilt installers are not included in the source checkout.

**Verification status:** native builds and interactive UI checks have been performed on macOS arm64. Ubuntu 24.04 arm64 passed the automated suite, native release builds, Debian package installation and native WebKitGTK startup under Xvfb in Docker. Interactive Ubuntu desktop use, Ubuntu x86_64, other Linux distributions and Windows remain unverified. Distribution signing and notarization are not configured. See [verification details](docs/phase2/VERIFICATION.md).

### Common prerequisites

Install these before following your platform’s steps:

- [Git](https://git-scm.com/downloads).
- [Rust stable and Cargo](https://www.rust-lang.org/tools/install). Windows requires the MSVC toolchain.
- [Node.js](https://nodejs.org/en/download) with npm: Node 22.12 or later, or Node 20.19 or later within the 20.x line, as required by the current Vite dependency.
- [Python 3](https://www.python.org/downloads/), used by build and verification scripts. On macOS/Linux, make it available as `python3`.
- The native [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system.

### macOS

1. Install Xcode Command Line Tools:

   ```sh
   xcode-select --install
   ```

2. After installing the common prerequisites, open a new terminal, clone and build:

   ```sh
   git clone https://github.com/IbrahimHosseini/Agentkit-studio.git
   cd Agentkit-studio
   npm --prefix apps/desktop ci
   bash scripts/desktop-build.sh
   ```

3. Open the output folder and drag **Agentkit Studio.app** into **Applications**:

   ```sh
   open target/release/bundle/macos
   ```

   Launch Agentkit Studio from Applications. The macOS script produces an `.app` bundle, not a DMG. Keep its bundled resources inside the application.

### Linux

This example targets Debian/Ubuntu systems with WebKitGTK 4.1 development packages. For other distributions, use the [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

The source-build instructions below were verified on **Ubuntu 24.04 arm64** in Docker. The generated Debian package installed successfully; both the installed desktop app and the AppImage loaded the native frontend and started their bundled engine under a virtual display. This is headless startup verification, not an interactive desktop acceptance test.

1. Install native dependencies in addition to the common prerequisites:

   ```sh
   sudo apt update
   sudo apt install build-essential curl wget file pkg-config \
     libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
     libayatana-appindicator3-dev librsvg2-dev
   ```

2. Clone and build on Linux:

   ```sh
   git clone https://github.com/IbrahimHosseini/Agentkit-studio.git
   cd Agentkit-studio
   npm --prefix apps/desktop ci
   bash scripts/desktop-build.sh
   ```

3. Find generated packages under `target/release/bundle/`. For a generated Debian package:

   ```sh
   sudo apt install ./target/release/bundle/deb/*.deb
   ```

   Launch Agentkit Studio from your application menu. If an AppImage is generated instead, make that file executable and launch it. In environments without FUSE, the tested alternative is `APPIMAGE_EXTRACT_AND_RUN=1 "./target/release/bundle/appimage/Agentkit Studio_0.1.0_aarch64.AppImage"` (adjust the filename for your version and architecture). FUSE-mounted execution has not been verified. Package formats depend on the build host and Tauri tooling. See [Tauri’s Debian packaging notes](https://v2.tauri.app/distribute/debian/) for distribution compatibility.

### Windows

1. Install the common prerequisites and these native components:
   - **Visual Studio Build Tools** with **Desktop development with C++** and a Windows SDK.
   - **Microsoft Edge WebView2 Runtime**.
   - Rust’s MSVC toolchain matching your machine, such as `stable-x86_64-pc-windows-msvc` on x64 Windows.

   Follow the [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows) for component links. Reopen PowerShell after installation.

2. Clone and build in PowerShell:

   ```powershell
   git clone https://github.com/IbrahimHosseini/Agentkit-studio.git
   Set-Location Agentkit-studio
   npm --prefix apps/desktop ci
   .\scripts\desktop-build.ps1
   ```

3. Open the build output:

   ```powershell
   explorer .\target\release\bundle
   ```

   Run the generated installer from `nsis` (`*-setup.exe`) or `msi` (`*.msi`), then launch Agentkit Studio from the Start menu. Tauri may download installer tooling during the first build; see its [Windows installer guide](https://v2.tauri.app/distribute/windows-installer/).

These output paths assume the default Cargo target directory. If you set `CARGO_TARGET_DIR`, look in that directory instead.

### Connect your providers

Studio bundles its own engine. Provider CLIs must be installed and authenticated separately. Install [Codex CLI](https://github.com/openai/codex) and/or [Claude Code](https://code.claude.com/docs/en/setup) using their official instructions and supported installation method for your platform.

1. Complete sign-in in each provider’s CLI.
2. Open Studio’s **Connections** page and choose **Connect to Studio**. A fresh profile starts disconnected.
3. Open **Agents** to review discovered roles and their model selections. Refresh model catalogs when needed.
4. Start a project-free chat using the composer model selector, or attach a project and configure the coordinator in **Preferences**.

Connections control Studio execution; they do not sign in or sign out of the provider account. Stop active or queued work before disconnecting a provider.

See the [usage guide](docs/phase2/README.md) for agent settings, catalogs, approvals, conversation management, provider routing and MCP setup.

## Development

After installing platform prerequisites and running `npm --prefix apps/desktop ci`, start the native development app on macOS/Linux:

```sh
bash scripts/desktop-dev.sh
```

On Windows, use PowerShell:

```powershell
cargo build --locked -p agentkit-cli
$env:AGENTKIT_ENGINE_BIN = (Resolve-Path .\target\debug\agentkit.exe).Path
npm --prefix apps/desktop run tauri -- dev
```

Adjust the engine path if you use `CARGO_TARGET_DIR`. A browser-only Vite preview does not connect to the local engine; use the Tauri app for working conversations and settings.

## Standalone CLI

Build without packaging the desktop app:

```sh
cargo build --locked --release -p agentkit-cli
```

The executable is `target/release/agentkit` on macOS/Linux or `target\release\agentkit.exe` on Windows. Add its directory to your `PATH` if desired. The executable name is `agentkit`; this is Studio’s own engine and CLI.

After enabling a provider in Studio, examples on macOS/Linux are:

```sh
./target/release/agentkit run --provider codex --project . --prompt "Explain this project"
./target/release/agentkit runs
./target/release/agentkit status RUN_ID
./target/release/agentkit stop RUN_ID
./target/release/agentkit --json rpc studio/settings
```

On Windows, use `.\target\release\agentkit.exe` instead. GUI and CLI use the same daemon and local history. `AGENTKIT_DATA_DIR` selects an alternative engine data directory; use the same value for both clients when sharing that state.

## Verification

Run the isolated check suite from the repository root:

```sh
python3 -I scripts/verify-isolated.py
```

On Windows, use `py -3 -I scripts/verify-isolated.py`. The script runs Rust tests, Svelte checking, frontend helper tests, frontend and CLI builds, and phase-one and phase-two Python suites. It isolates application homes and engine state and uses fake provider protocols. Dependencies must already be in the local Cargo cache because Cargo commands run offline. Unix-socket acceptance tests are skipped on Windows; a passing suite there would not establish native Windows acceptance.

Native GUI checks, actual account/model execution and each operating system require separate verification. The model-replacement update passed 117 automated tests and native macOS arm64 UI checks using isolated offline fixtures; no real model requests were made for that update.

The Ubuntu 24.04 arm64 follow-up also passed all 117 tests, the documented release build and Debian installation. Native Debian/AppImage startup was checked under Xvfb with isolated homes and fake providers. No real provider inference or interactive Ubuntu desktop check was performed.

## Project structure

| Path | Purpose |
| --- | --- |
| `apps/desktop` | Svelte/TypeScript UI and Tauri desktop shell |
| `crates/agentkit-core` | Shared engine, scheduler, local IPC and SQLite history |
| `crates/agentkit-providers` | Codex and Claude transport adapters |
| `crates/agentkit-protocol` | Shared protocol types |
| `crates/agentkit-cli` | Standalone CLI and daemon entry point |
| `scripts` | Development, packaging and verification helpers |
| `tests` | Isolated integration tests and offline provider fixtures |
| `docs/phase2` | Current usage and verification documentation |
| `docs/phase1` | Historical prototype documentation and evidence |

## License

[MIT](LICENSE).
