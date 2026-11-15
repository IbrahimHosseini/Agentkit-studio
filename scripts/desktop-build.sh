#!/usr/bin/env bash
# Stage the native engine as a desktop resource; no shell plugin is needed.
set -eu
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
cargo build --locked --release -p agentkit-cli
runtime_target="$(cargo metadata --format-version 1 --no-deps | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
mkdir -p apps/desktop/src-tauri/binaries
cp "$runtime_target/release/agentkit" apps/desktop/src-tauri/binaries/agentkit
case "$(uname -s)" in
  Darwin) npm --prefix "$root/apps/desktop" run tauri build -- --bundles app ;;
  *) npm --prefix "$root/apps/desktop" run tauri build ;;
esac
