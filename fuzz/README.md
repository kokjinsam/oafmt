# Fuzzing

The fuzz workspace is independent from the published workspace. It has its own
`Cargo.lock`, uses `libfuzzer-sys` 0.4.13, and does not change the root Rust 1.85
dependency graph.

Install the pinned fuzz tools through the public setup recipe:

```sh
just setup fuzz
```

Run the bounded smoke campaign for every target:

```sh
just fuzz-smoke
```

The four raw-byte targets are:

- `format_yaml`: formatting invariants with explicit YAML syntax
- `format_json`: formatting invariants with explicit JSON syntax
- `classify_oas`: deterministic YAML and JSON semantic classification
- `reorder_edits`: inspected edits plus overlapping and malformed edit plans

Generated writable corpora live under ignored `fuzz/corpus/<target>`. Run a
long campaign for all targets or one named target with the public recipe:

```sh
just fuzz
just fuzz format_yaml
```

Use the following direct `cargo-fuzz` commands only for specialized crash
analysis that the public recipes cannot express. Replay one saved failure:

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
