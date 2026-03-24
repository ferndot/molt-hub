#!/usr/bin/env bash
set -euo pipefail

# Molt Hub — release build (see README.md "Building for Release")

ROOT="$(cd "$(dirname "$0")" && pwd)"
SERVER_ONLY=false
SKIP_INSTALL=false

usage() {
  cat <<'EOF'
Usage: ./release.sh [options]

  Build release artifacts for Molt Hub.

Options:
  --server-only   Build only the molt-hub CLI (skip UI + Tauri).
  --skip-install  Skip npm install in ui/ (use when deps are unchanged).
  -h, --help      Show this help.

Default: npm install, vite build, then cargo tauri build (desktop .app / .dmg).
EOF
}

for arg in "$@"; do
  case "$arg" in
    --server-only) SERVER_ONLY=true ;;
    --skip-install) SKIP_INSTALL=true ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $arg" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [ "$SERVER_ONLY" = true ]; then
  echo "==> Building molt-hub (release binary)"
  (cd "$ROOT" && cargo build --release --bin molt-hub)
  echo ""
  echo "Binary: $ROOT/target/release/molt-hub"
  exit 0
fi

if [ "$SKIP_INSTALL" = false ]; then
  echo "==> Installing UI dependencies"
  (cd "$ROOT/ui" && npm install)
else
  echo "==> Skipping npm install (--skip-install)"
fi

echo "==> Building UI"
(cd "$ROOT/ui" && npm run build)

echo "==> Building Tauri bundle"
(cd "$ROOT/crates/tauri" && cargo tauri build)

echo ""
echo "Artifacts (workspace target dir):"
echo "  App: $ROOT/target/release/bundle/macos/Molt Hub.app"
echo "  DMG: $ROOT/target/release/bundle/dmg/Molt Hub_*.dmg"
