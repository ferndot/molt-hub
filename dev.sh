#!/usr/bin/env bash
set -euo pipefail

# Molt Hub — single-command dev launcher
# Starts the Axum backend and Vite frontend with color-coded output.
# Pass --desktop to also launch the Tauri desktop shell.

ROOT="$(cd "$(dirname "$0")" && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found in PATH (install Rust via https://rustup.rs)" >&2
  exit 1
fi
CARGO="cargo"
DESKTOP=false

for arg in "$@"; do
  case "$arg" in
    --desktop) DESKTOP=true ;;
  esac
done

cleanup() {
  echo ""
  echo "Shutting down..."
  kill 0 2>/dev/null
  wait 2>/dev/null
}
trap cleanup EXIT INT TERM

# Color prefixes
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

# Backend — never auto-open; we always open the browser to the Vite dev server below
(cd "$ROOT" && $CARGO run --bin molt-hub -- serve --no-open 2>&1 \
  | sed "s/^/$(printf "${RED}[backend]${NC} ")/") &

# Frontend
(cd "$ROOT" && npm run dev 2>&1 \
  | sed "s/^/$(printf "${GREEN}[frontend]${NC} ")/") &

# Desktop (optional)
if [ "$DESKTOP" = true ]; then
  (cd "$ROOT/crates/tauri" && $CARGO tauri dev 2>&1 \
    | sed "s/^/$(printf "${BLUE}[desktop]${NC} ")/") &
fi

echo "================================================"
echo " Molt Hub dev servers starting..."
echo "   Backend:  http://127.0.0.1:13401"
echo "   Frontend: http://127.0.0.1:5173"
if [ "$DESKTOP" = true ]; then
  echo "   Desktop:  Tauri window"
fi
echo "   Press Ctrl+C to stop all"
echo "================================================"

# Open browser to Vite dev server (not the backend static build) so
# both browser and desktop see the same live UI.
if [ "$DESKTOP" = false ]; then
  (sleep 2 && open "http://127.0.0.1:5173") &
fi

wait
