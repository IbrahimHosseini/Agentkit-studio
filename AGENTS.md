# agentkitStudio

Standalone Tauri 2 / Svelte / TypeScript desktop app, shared Rust engine and CLI, SQLite history.

- Read README.md and docs/phase1/README.md before making changes.
- Keep the interface English and LTR. Prose/input must support automatic RTL and mixed text; isolate filenames and keep paths, code and commands LTR.
- Keep Cargo.lock and apps/desktop/package-lock.json in sync with intentional dependency changes.
- Keep GUI and CLI on the same engine and protocol; only the daemon owns SQLite.
- Tests must isolate HOME, CODEX_HOME, Claude configuration and engine state. Never use real credentials or contact paid model services without explicit authorization.
- Never print or commit credentials. Do not change installed agents or user settings without authorization.
- Do not modify the separate agentkit project as part of Studio work.
- Do not commit or push unless asked.
- Do not use a supervisor agent for this project unless the user explicitly requests it. The Studio superagent is a product feature, not permission to invoke a development supervisor.

## Checks

```sh
cargo test --locked --workspace
npm --prefix apps/desktop run check
npm --prefix apps/desktop run build
cargo build --locked -p agentkit-cli
AGENTKIT_TEST_BIN="$PWD/target/debug/agentkit" python3 -I -m unittest discover -s tests/phase1 -v
```

Report tested operating systems explicitly. Preserve distinctions between real provider runs, offline fixtures, browser previews and native GUI verification. See docs/phase1/VERIFICATION.md for known limitations.
