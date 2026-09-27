#!/usr/bin/env bash
# Compile, teste et empaquette Vision-Pro.app et le .dmg.
#   scripts/build-macos.sh              → architecture du Mac courant
#   scripts/build-macos.sh --universal  → binaire universel Apple silicon + Intel
set -euo pipefail

cd "$(dirname "$0")/.."
[[ "$(uname -s)" == "Darwin" ]] || { echo "À lancer sur un Mac." >&2; exit 1; }

target_args=()
bundle_dir="apps/desktop/src-tauri/target/release/bundle"
if [[ "${1:-}" == "--universal" ]]; then
  target_args=(--target universal-apple-darwin)
  bundle_dir="apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle"
fi

echo "▸ Vérification TypeScript et tests"
npm run typecheck
npm test

echo "▸ Interfaces web (iPad + bureau)"
npm run build:web

echo "▸ Tests de l'agent Rust"
cargo test --manifest-path agent/Cargo.toml --workspace

echo "▸ Application macOS"
# (bash 3.2 de macOS : un tableau vide doit être développé ainsi avec set -u)
npm run tauri -w @vision-pro/desktop -- build ${target_args[@]+"${target_args[@]}"}

echo
echo "✓ Terminé"
echo "  Application : $bundle_dir/macos/Vision-Pro.app"
echo "  Image disque : $bundle_dir/dmg/"
