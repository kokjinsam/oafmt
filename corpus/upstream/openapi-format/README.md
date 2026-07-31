# `openapi-format` upstream corpus

This corpus records 36 investigated cases from
[`thim81/openapi-format`](https://github.com/thim81/openapi-format) at commit
`bee0bebc84221c5cf25574dd6af74c135d7efe05` (MIT).

Only the `input.yaml` or `input.json` files for 17 useful cases were copied.
Those inputs are verbatim upstream bytes. Fifteen cases are executable; the
other two are retained as non-executable references. Options, custom
configuration, overlays, and upstream outputs were not copied.

Files named `expected.oafmt.yaml` or `expected.oafmt.json` are locally reviewed
expectations owned by `oafmt`. Paths to upstream outputs are provenance only:
upstream output is never an `oafmt` formatting oracle.

The integration test verifies every copied input against the SHA-256 recorded
in `manifest.toml`:

```sh
cargo test --locked -p oafmt-core --test upstream_corpus
```

For an independent local listing, run:

```sh
shasum -a 256 corpus/upstream/openapi-format/cases/*/input.*
```
