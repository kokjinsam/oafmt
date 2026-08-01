# Fuzzing

The fuzz workspace is independent from the published workspace. It has its own
`Cargo.lock`, uses `libfuzzer-sys` 0.4.13, and does not change the root Rust 1.85
dependency graph.

Install the pinned tools:

```sh
rustup toolchain install nightly-2026-07-30 --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
```

Build every target:

```sh
cargo +nightly-2026-07-30 fuzz build
cargo deny --manifest-path fuzz/Cargo.toml \
  --config fuzz/deny.toml --locked check
```

The four raw-byte targets are:

- `format_yaml`: formatting invariants with explicit YAML syntax
- `format_json`: formatting invariants with explicit JSON syntax
- `classify_oas`: deterministic YAML and JSON semantic classification
- `reorder_edits`: inspected edits plus overlapping and malformed edit plans

Generated writable corpora live under ignored
`fuzz/corpus/<target>`. Keep that directory first and the committed read-only
seed directories after it:

```sh
mkdir -p fuzz/corpus/format_yaml
cargo +nightly-2026-07-30 fuzz run format_yaml \
  fuzz/corpus/format_yaml fuzz/seeds/yaml -- \
  -seed=424242 -max_len=65536 -timeout=5 -rss_limit_mb=2048

mkdir -p fuzz/corpus/classify_oas
cargo +nightly-2026-07-30 fuzz run classify_oas \
  fuzz/corpus/classify_oas fuzz/seeds/yaml fuzz/seeds/json -- \
  -seed=424242 -max_len=65536 -timeout=5 -rss_limit_mb=2048
```

Use `fuzz/seeds/json` for `format_json`; use both committed seed directories for
`classify_oas` and `reorder_edits`. To replay one saved failure exactly:

```sh
cargo +nightly-2026-07-30 fuzz run reorder_edits \
  fuzz/artifacts/reorder_edits/crash-<hash> -- -runs=1
```

Minimize a failure with:

```sh
cargo +nightly-2026-07-30 fuzz tmin reorder_edits \
  fuzz/artifacts/reorder_edits/crash-<hash>
```

Every minimized failure must become a deterministic regression at the nearest
production test seam before a fix is accepted. Add the minimized input to the
appropriate committed seed directory only when it remains useful as permanent
fuzz coverage. Do not commit generated `fuzz/corpus`, `fuzz/artifacts`,
`fuzz/coverage`, or `fuzz/target` contents.
