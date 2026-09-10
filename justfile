set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default: check

fmt:
    cargo fmt --all --check

lint:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    if command -v cargo-nextest >/dev/null 2>&1; then \
        cargo nextest run --all-features; \
    else \
        cargo test --all-features; \
    fi

deny:
    cargo deny check

check: fmt lint test deny

build:
    cargo build --workspace --all-targets

msrv:
    cargo +1.88 check --workspace --all-targets
