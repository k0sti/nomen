# Nomen — build, deploy, and manage

# Build and restart everything
deploy: build-all restart

# Build web UI only
build-web:
    cd web && npm run build

# Build Rust backend only
build-rust:
    cargo build --release

# Build everything
build-all: build-web build-rust

# Restart the nomen service
restart:
    sudo systemctl restart nomen

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
