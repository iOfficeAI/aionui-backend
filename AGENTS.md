# AGENTS.md

Project-specific rules and conventions for AI assistants and contributors.

## Build & Test

```bash
cargo build                          # Build (debug)
cargo build --release                # Build (release)
cargo test --workspace               # Run all tests
cargo clippy --workspace -- -D warnings  # Lint (warnings = errors)
cargo fmt --all                      # Format
cargo fmt --all -- --check           # Format check (CI enforces this)
```

Binary name: `aionui-backend` (produced by `crates/aionui-app`).

## Architecture

Cargo workspace with 17 crates under `crates/`. Dependencies flow downward:

- `aionui-common` — shared types, enums, error types, crypto utilities
- `aionui-api-types` — API request/response types, shared across crates
- `aionui-db` — SQLite database layer (sqlx), repository traits and implementations
- `aionui-auth` — JWT, CSRF, password hashing, auth middleware
- `aionui-realtime` — WebSocket manager, event broadcasting
- Domain crates (`aionui-conversation`, `aionui-channel`, `aionui-team`, `aionui-cron`, `aionui-file`, `aionui-office`, `aionui-shell`, `aionui-mcp`, `aionui-ai-agent`, `aionui-extension`, `aionui-system`) — each owns its routes, service, and tests
- `aionui-app` — top-level binary, composes all crates into the axum server

Never introduce circular dependencies or upward references.

## Test Organization

| Location | What goes there |
|----------|----------------|
| Inline `#[cfg(test)]` in each `.rs` file | Unit tests for that module's internals |
| `crates/<crate>/tests/` | Integration / E2E tests for that crate |

## Code Style

- Rust 2024 edition, stable toolchain
- `cargo clippy` must pass without warnings
- `cargo fmt` must pass
- Comments in English, commit messages in English
