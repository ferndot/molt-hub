#!/usr/bin/env bash
set -euo pipefail

# Molt Hub — single-command dev launcher
# Starts the Axum backend and Vite frontend with color-coded output.
# Pass --desktop to also launch the Tauri desktop shell.

CARGO="/Users/fdot/.cargo/bin/cargo"
ROOT="$(cd "$(dirname "$0")" && pwd)"
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

# Backend
(cd "$ROOT" && $CARGO run --bin molt-hub -- serve 2>&1 \
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
echo "   Backend:  http://localhost:3001"
echo "   Frontend: http://localhost:5173"
if [ "$DESKTOP" = true ]; then
  echo "   Desktop:  Tauri window"
fi
echo "   Press Ctrl+C to stop all"
echo "================================================"

wait
