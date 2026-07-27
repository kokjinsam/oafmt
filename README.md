# oafmt

`oafmt` is intended to be a format-only tool for OpenAPI documents in v1. It
will not lint, validate, or modify API semantics.

Phase 0 establishes the Rust workspace and the packaging skeleton for the
`oafmt` binary. The binary currently performs no formatting or other behavior.
YAML preservation, OpenAPI-aware formatting, configuration, and file discovery
are not implemented.

## Development

The local and CI gates are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --all-features
```

## License

MIT
