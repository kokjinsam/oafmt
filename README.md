# oafmt

`oafmt` is intended to be a format-only tool for OpenAPI documents in v1. It
will not lint, validate, or modify API semantics.

Phase 0 establishes the Rust workspace and the packaging skeleton for the
`oafmt` binary. Phase 1 adds only an executable YAML preservation experiment in
`oafmt-syntax`; its findings and limits are recorded in
[`PHASE_1_YAML_PRESERVATION.md`](PHASE_1_YAML_PRESERVATION.md). The binary still
performs no formatting or other behavior. OpenAPI-aware formatting,
configuration, and file discovery are not implemented.

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
