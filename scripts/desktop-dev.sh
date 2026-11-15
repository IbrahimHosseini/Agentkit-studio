#!/usr/bin/env bash
# Build the independent CLI first so closing the window cannot own its lifetime.
set -eu
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
cargo build --locked -p agentkit-cli
runtime_target="$(cargo metadata --format-version 1 --no-deps | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
export AGENTKIT_ENGINE_BIN="$runtime_target/debug/agentkit"
npm --prefix "$root/apps/desktop" run tauri dev
