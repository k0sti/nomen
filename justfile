# Nomen — build, deploy, and manage

# Show available commands
default:
    @just --list

# Alias for explicit help
help:
    @just --list

# --- Service ---

# Start the service
service-start:
    systemctl --user enable --now nomen

# Stop the service
service-stop:
    systemctl --user stop nomen

# Restart the service
service-restart:
    systemctl --user restart nomen

# Show service status
service-status:
    systemctl --user status nomen

# Follow service logs
service-logs:
    journalctl --user -u nomen -f

# Show last 50 lines of logs
service-logs-short:
    journalctl --user -u nomen -n 50 --no-pager

# --- Build ---

# Build web UI only
build-web:
    cd web && bun run build

# Build Rust backend / CLI only
build-rust:
    cargo build --release

# Build everything
build-all: build-web build-rust

# Install the Nomen CLI locally
install:
    cargo install --path . --locked

# --- Deploy ---

# Build and restart everything
deploy: build-all service-restart

# Deploy web UI (build + restart)
deploy-web: build-web service-restart

# Deploy backend (build + restart)
deploy-rust: build-rust service-restart

# --- Development ---

# Fast compile check
check:
    cargo check --workspace

# Lint with clippy
lint:
    cargo clippy --workspace -- -D warnings

# Run tests
test:
    cargo test

# Full CI pipeline
ci: check lint test

# Dev mode: watch and rebuild web UI
dev-web:
    cd web && bun run dev

# --- CLI shortcuts ---

# Run CLI search
search query:
    cargo run --release -- search "{{query}}"

# Sync relay events to local DB
sync:
    cargo run --release -- sync
