# Nomen — build, deploy, and manage

# Show available commands
default:
    @just --list

# Alias for explicit help
help:
    @just --list

# Install the Nomen CLI locally
install:
    cargo install --path . --locked

# Build web UI only
build-web:
    cd web && npm run build

# Build Rust backend / CLI only
build-rust:
    cargo build --release

# Build everything
build-all: build-web build-rust

# Build and restart everything
deploy: build-all restart

# Restart the nomen service
restart:
    sudo systemctl restart nomen
    @echo "✅ Nomen restarted"
    @sleep 2
    @systemctl status nomen --no-pager | head -15

# Deploy web UI (build + restart)
deploy-web: build-web restart

# Deploy backend (build + restart)
deploy-rust: build-rust restart

# View service logs
logs:
    journalctl -u nomen -f --no-hostname

# View last 50 lines of logs
logs-short:
    journalctl -u nomen -n 50 --no-hostname --no-pager

# Service status
status:
    systemctl status nomen --no-pager

# Run CLI search
search query:
    cargo run --release -- search "{{query}}"

# Sync relay events to local DB
sync:
    cargo run --release -- sync

# Dev mode: watch and rebuild web UI
dev-web:
    cd web && npm run dev

# Run tests
test:
    cargo test
