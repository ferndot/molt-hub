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

# Backend (skip auto-opening the system browser when using the Tauri shell)
if [ "$DESKTOP" = true ]; then
  BACKEND_SERVE=(run --bin molt-hub -- serve --no-open)
else
  BACKEND_SERVE=(run --bin molt-hub -- serve)
fi
(cd "$ROOT" && $CARGO "${BACKEND_SERVE[@]}" 2>&1 \
  | sed "s/^/$(printf "${RED}[backend]${NC} ")/") &

# Frontend
(cd "$ROOT/ui" && npm run dev 2>&1 \
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
  echo "   Desktop:  Tauri window (browser will not auto-open)"
fi
echo "   Press Ctrl+C to stop all"
echo "================================================"

wait
