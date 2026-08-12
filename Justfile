set shell := ["bash", "-euo", "pipefail", "-c"]

default: help

# List the public project commands.
help:
    @just --list

# Provision the exact repository tools. Groups: all, check, or fuzz.
setup group="all":
    scripts/tools setup {{ quote(group) }}

# Run the current-Rust workspace verification suite.
check:
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(scripts/tools env current)"
    scripts/tools check current
    cargo fmt --all -- --check
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    cargo test --locked --workspace --all-features
    cargo build --locked --workspace --all-features
    RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --all-features --no-deps

# Check the workspace with its minimum supported Rust version.
msrv:
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(scripts/tools env msrv)"
    scripts/tools check msrv
    cargo check --locked --workspace --all-features

# Audit root and fuzz-workspace dependencies.
audit:
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(scripts/tools env current)"
    scripts/tools check audit
    cargo-deny --locked check
    cargo-deny --manifest-path fuzz/Cargo.toml --locked check --config fuzz/deny.toml

# Replay committed seeds and run the deterministic bounded fuzz smoke campaign.
fuzz-smoke target="all":
    scripts/fuzz smoke {{ quote(target) }}

# Run the longer scheduled or manual fuzz campaign.
fuzz target="all":
    scripts/fuzz full {{ quote(target) }}

# Validate generated release configuration without releasing.
release-check:
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(scripts/tools env current)"
    scripts/tools check release
    dist generate --check
    dist plan
    git diff --check
