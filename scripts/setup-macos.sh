#!/usr/bin/env bash
# Prépare un Mac pour compiler Vision-Pro (à lancer une fois, depuis la racine du projet).
set -euo pipefail

cd "$(dirname "$0")/.."

say() { printf '\n\033[1m%s\033[0m\n' "$1"; }
fail() { printf '\n\033[31m✗ %s\033[0m\n' "$1" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || fail "Vision-Pro pour macOS se compile uniquement sur un Mac."

version=$(sw_vers -productVersion)
major=${version%%.*}
(( major >= 13 )) || fail "macOS 13 Ventura ou plus récent est requis (version actuelle : $version)."
say "macOS $version ($(uname -m))"

say "Outils de ligne de commande Xcode (clang, SDK macOS)"
if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  fail "Terminez l'installation des outils Xcode dans la fenêtre qui vient de s'ouvrir, puis relancez ce script."
fi
xcrun --sdk macosx --show-sdk-version >/dev/null || fail "SDK macOS introuvable (xcode-select -p)."
echo "✓ $(xcode-select -p)"

say "Rust"
if ! command -v rustup >/dev/null 2>&1; then
  echo "Installation de rustup (https://rustup.rs)…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
# rust-toolchain.toml fixe le canal stable et les deux cibles Apple (Apple silicon + Intel).
rustup show active-toolchain >/dev/null
rustup target add aarch64-apple-darwin x86_64-apple-darwin
echo "✓ $(rustc --version)"

say "Node.js (22.12 ou plus récent)"
command -v node >/dev/null 2>&1 || fail "Node.js est introuvable. Installez-le depuis https://nodejs.org ou avec « brew install node »."
node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit(a>22||(a===22&&b>=12)?0:1)' \
  || fail "Node.js $(node -v) est trop ancien : 22.12 ou plus récent est requis."
echo "✓ Node $(node -v)"

say "Dépendances JavaScript"
npm ci --no-audit --no-fund

say "Prêt."
echo "Compiler l'application :          npm run build:mac"
echo "Application universelle (M + Intel) : npm run build:mac:universal"
echo "Développement avec rechargement :  npm run dev:mac"
