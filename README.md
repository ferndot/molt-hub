# Molt Hub

Mission Control for AI coding agents.

Molt Hub is a desktop dashboard for supervising concurrent AI coding agents.
It provides a triage inbox, kanban board, real-time agent output streaming,
and approval workflows so you stay in control of what your agents ship.

## Architecture

| Layer     | Tech               | Purpose                        |
|-----------|--------------------|--------------------------------|
| Backend   | Rust / Axum        | API server, event store, agent harness |
| Frontend  | SolidJS / Vite     | Reactive UI with WebSocket updates     |
| Desktop   | Tauri              | Native window shell                    |
| Storage   | SQLite             | Event-sourced persistence              |

## Quick Start

```bash
# 1. Install frontend dependencies
cd ui && npm install && cd ..

# 2. Run the API server (serves the UI on http://localhost:3000)
cargo run --bin molt-hub -- serve

# 3. Or run as a native desktop app
cargo run --bin molt-hub-desktop
```

## Development

Start everything with a single command:

```bash
./dev.sh            # Backend + Frontend
./dev.sh --desktop  # Backend + Frontend + Tauri desktop shell
```

Or run services individually in separate terminals:

```bash
# Frontend dev server (localhost:5173)
cd ui && npm run dev

# Backend API server
cargo run --bin molt-hub -- serve

# Desktop shell (optional, wraps the Vite dev server)
cd crates/tauri && cargo tauri dev
```

## Tests

```bash
# Backend (Rust)
cargo test --workspace

# Frontend (Vitest)
cd ui && npx vitest run

# E2E (Playwright — requires browsers installed)
cd ui && npx playwright install --with-deps chromium
cd ui && npm run e2e
```

## Project Structure

```
crates/
  core/       — Domain model, events, config
  server/     — Axum API server + WebSocket layer
  harness/    — Agent adapters + supervisor
  tauri/      — Desktop shell
ui/           — SolidJS frontend
  e2e/        — Playwright end-to-end tests
  src/        — Components, views, stores
```

## License

[Antiracist Ethical Source License (ATR v0.6)](LICENSE)
