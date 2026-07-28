# oafmt

`oafmt` is a deterministic, syntax-preserving formatter for OpenAPI documents.
It does not lint, resolve references, or perform general OpenAPI validation.

Phase 3 accepts one UTF-8 YAML or strict JSON OpenAPI 3.0.x, 3.1.x, or 3.2.x
entry document and classifies reachable OpenAPI objects with version-specific
semantics. User-visible formatting remains limited to fixed fields at the
document root and in fixed-method Operation Objects directly below the entry
document's `paths`, retaining unknown-field slots and all original source
slices. The Phase 1 experiment remains historical evidence in
[`PHASE_1_YAML_PRESERVATION.md`](PHASE_1_YAML_PRESERVATION.md).

```sh
oafmt FILE
oafmt --write FILE
oafmt --check FILE
oafmt --diff FILE
oafmt --stdin-filepath virtual.yaml < input.yaml
```

The formatter has no configuration, file discovery, reference resolution, or
multi-input mode.

## Development

The local and CI gates are:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --all-features
```

## License

MIT
