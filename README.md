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

# 2. Run the API server (serves the UI on http://localhost:13401)
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

## Building for Release

### macOS app bundle (`.app` / `.dmg`)

```bash
# 1. Install frontend dependencies
cd ui && npm install && cd ..

# 2. Build the frontend
cd ui && npm run build && cd ..

# 3. Build the Tauri release bundle
cd crates/tauri && cargo tauri build
```

The `.app` bundle and `.dmg` installer are written to:
```
crates/tauri/target/release/bundle/macos/Molt Hub.app
crates/tauri/target/release/bundle/dmg/Molt Hub_*.dmg
```

### CLI server binary only

```bash
cargo build --release --bin molt-hub
# Binary at: target/release/molt-hub
```

### Prerequisites

- **Rust** toolchain (`rustup` — stable)
- **Node.js** ≥ 18 with npm
- **Tauri CLI** (installed automatically via `cargo tauri`)
- macOS: Xcode Command Line Tools (`xcode-select --install`)

> **OAuth:** Uses HTTPS pages from [`oauth-bridge/redirect-uris.json`](oauth-bridge/redirect-uris.json) (see [`oauth-bridge/README.md`](oauth-bridge/README.md)). **Desktop:** bridge → **`molthub://`** → local API. **Browser dev:** use **Finish in browser (local API)** on the bridge page while `molt-hub serve` is running. **GitHub/Jira OAuth app credentials** are **embedded at compile time** only (`crates/server/build.rs`): set `MOLTHUB_GITHUB_CLIENT_ID`, `MOLTHUB_GITHUB_CLIENT_SECRET`, `MOLTHUB_JIRA_CLIENT_ID`, and `MOLTHUB_JIRA_CLIENT_SECRET` in the environment or in a **`.env`** file when you build (the build walks upward from `crates/server/` and loads each `.env`; first value wins per key). Optional redirect overrides at runtime: `MOLTHUB_*_REDIRECT_URI`.
>
> After you connect Jira/GitHub, **access tokens** are stored in the **OS credential store** (keychain / equivalent). Per-user `.env` under the config dir is still used for other settings (see [`.env.example`](.env.example)). Distributed binaries contain whatever secrets were present at build time — treat release pipelines accordingly.

## License

[Antiracist Ethical Source License (ATR v0.6)](LICENSE)
