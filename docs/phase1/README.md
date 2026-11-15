# Agentkit Studio: phase-one prototype

Agentkit Studio is an independent desktop application and terminal runtime. The desktop shell uses Tauri 2, Svelte 5 and TypeScript. The engine, local client and standalone CLI use Rust; one daemon owns SQLite. Closing the desktop window does not cancel a running task.

The application language is English and its shell is LTR. Chat prose and the composer use automatic text direction for Persian, Arabic and mixed content. File names are isolated from surrounding text; filesystem paths, commands and raw/code output remain LTR. Phase one attaches files; it does not provide a full document editor.

## Build and run

Requirements: Rust stable with Cargo, Node/npm, and the native Tauri prerequisites for the host. macOS requires Xcode command-line tools. Windows requires the Microsoft C++ build tools and WebView2. Linux requires the WebKitGTK/GTK development packages described in the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). Dependencies are locked in Cargo.lock and apps/desktop/package-lock.json.

```sh
npm --prefix apps/desktop ci
cargo build --locked -p agentkit-cli
bash scripts/desktop-dev.sh
```

Build a release app and bundle its independent CLI:

```sh
bash scripts/desktop-build.sh
```

On native Windows, from PowerShell:

```powershell
npm --prefix apps/desktop ci
.\scripts\desktop-build.ps1
```

The build scripts stage the CLI under the desktop's resource directory. The bundled GUI resolves it without requiring a global agentkit executable. A development launch can override `AGENTKIT_ENGINE_BIN` with an absolute compiled CLI path.

Use the standalone Rust CLI directly:

```sh
./target/debug/agentkit run --provider codex --project . --prompt "Explain this project"
./target/debug/agentkit runs
./target/debug/agentkit status RUN_ID
./target/debug/agentkit events RUN_ID --follow
./target/debug/agentkit stop RUN_ID
```

Additional commands: `approve RUN_ID REQUEST_ID --allow|--deny`, `resume RUN_ID`, `handshake`, and `metrics --gui-pid PID`. `--json` is available globally. `--image PATH` and `--file PATH` can be repeated on `run`; `--role NAME` requests an installed role. Native model CLIs must be installed and authenticated separately.

`AGENTKIT_DATA_DIR` selects an isolated engine database and endpoint. GUI and CLI must use the same value to share test state. With no override, both use the same OS-specific per-user data directory. Local IPC uses a Unix socket on macOS/Linux and a named pipe on Windows, with a per-user authentication token. The daemon starts on demand and exits after an idle interval with no active runs. It does not automatically repeat interrupted work after restart.

Locally built macOS arm64 artifacts are under `target/phase1/`: the application ZIP, standalone `agentkit` executable, and an explicitly browser-fixture preview. These ignored build outputs are not part of the source checkout. See [verification results](VERIFICATION.md) for measured resources and remaining acceptance work.

## Provider capabilities and evidence

| Capability | Codex | Claude |
| --- | --- | --- |
| Transport | App Server JSON-RPC | CLI stream-json |
| Text and images | Authenticated native smoke passed | Implemented; fake protocol tested; live model access blocked |
| Attached files | Readable path references; random file code verified live | Readable path references; live processing unverified |
| Installed roles | Native delegation requested; actual role availability must be confirmed | Native `--agent` option |
| Agent identity | Thread/collaboration event IDs | Observed agent IDs and spawning-call mapping; unknown relationships remain unknown |
| Approvals | Command and file-change requests | Unsupported in this prototype; protected actions are denied |
| Stop/resume | Implemented; fake protocol tested | Implemented; fake protocol tested |
| Authenticated model execution | Verified with existing ChatGPT login, 2026-11-12 | Blocked: configured gateway/model access |

Installed native CLIs were also exercised with empty scratch homes: both established protocol sessions, then correctly failed for missing authentication. This checks the real executable/protocol boundary, not successful model execution. See [provider-smoke.json](provider-smoke.json). No credentials were copied during those initial unauthenticated checks and no real agent configuration was modified.

With subsequent user approval, one authenticated Codex run successfully read a random fixture code, identified the left/right colors of an attached image, and returned an exact mixed Persian/English line. Its temporary auth-cache copy and session state were deleted; the original cache was unchanged. Claude Code used the existing local gateway token. The initial default Opus attempt and a separately approved corrected `sonnet` attempt both failed at model access; `sonnet` resolved to `claude-sonnet-5-5`. Both Claude attempts reported zero input/output tokens. A read-only gateway model-list request returned HTTP 404. Claude authenticated model execution therefore remains unverified. See [authenticated-smoke.json](authenticated-smoke.json).

The `mock` provider is explicitly synthetic and never silently substitutes for Codex or Claude. Prompts containing `[approval]`, `[hold]`, `[history]` or `[fail]` exercise approval, stop, 10,000-event history or failure paths without contacting a model.

## Verification boundaries

- macOS arm64: native release bundle builds and starts; engine/CLI behavior is exercised with isolated test state.
- Browser preview on macOS: English shell and mixed RTL content are checked separately from the native WebKit app.
- Native editing, clipboard, file-dialog and drag/drop interaction require further manual validation. The UI inspection tool timed out; macOS accessibility automation was denied, so no native input pass is claimed.
- Windows and Linux: build scripts/source paths are prepared, but native execution is **unverified** on this machine.
- Graceful stop cleans up the tested provider process group/descendant. A hard kill of the engine can leave a POSIX provider descendant alive; restart marks the interrupted run but does not guarantee crash-proof process cleanup.
- Resource measurements must identify which processes are counted. OS-brokered WebKit helpers may not be daemon/GUI descendants. RSS sums can double-count shared pages. Mock activity does not represent a real model process workload.

## Checks

```sh
cargo test --locked --workspace
npm --prefix apps/desktop run check
npm --prefix apps/desktop run build
AGENTKIT_TEST_BIN="$PWD/target/debug/agentkit" python3 -I -m unittest discover -s tests/phase1 -v
```

Socket/process integration tests need a host that permits local IPC and process inspection. They isolate HOME, CODEX_HOME, Claude settings and engine state.
