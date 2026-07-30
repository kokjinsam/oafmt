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
oafmt --write FILE...
oafmt --check FILE...
oafmt --diff FILE...
oafmt --check --config path/to/oafmt.toml DIRECTORY...
oafmt --stdin-filepath virtual.yaml < input.yaml
```

Plain stdout mode and `--stdin-filepath` accept one document. Stdin cannot be
combined with file arguments, and `--write` does not accept stdin. Write,
check, and diff modes accept one or more literal files, recursive directory
selectors, or native glob selectors. Quote a glob to prevent the shell from
expanding it:

```sh
oafmt --check .
oafmt --diff 'apis/**/openapi.{yaml,json}' # rejected: braces are unsupported
oafmt --diff 'apis/**/openapi.?ml'
```

Directory discovery recursively selects only `openapi.yaml`, `openapi.yml`, and
`openapi.json` by default. A native glob supplies its own inclusion pattern.
Discovered candidates must end in `.yaml`, `.yml`, or `.json`; a selector that
has no candidates after filtering is an error. Paths expanded by a shell arrive
as ordinary literal files and retain the literal-file rules.

For `--write`, `--check`, and `--diff`, `oafmt` finds the nearest `oafmt.toml`
from the current directory upward. `--config PATH` overrides that lookup.
Configuration is strict and affects discovery only:

```toml
[discovery]
include = ["apis/**/openapi.yaml", "apis/**/openapi.json"]
exclude = ["apis/generated/**"]
respect_gitignore = true
```

`include`, when supplied, must be non-empty and replaces the default directory
basenames. `exclude` is optional and always wins. Patterns are relative to the
configuration file's directory. The supported component-aware glob syntax is
`*`, `?`, character classes, and `**`. Brace expansion and shell syntax are not
supported; literal braces may be matched inside character classes, such as
`file[{].yaml` and `file[}].yaml`. Unknown fields, invalid types, empty supplied
include lists, missing explicit config files, and malformed patterns are errors
before any file is processed or written.

Directory and native-glob discovery respects repository and nested `.gitignore`
files by default, without consulting global or system ignore files. Set
`respect_gitignore = false` to disable that filtering. VCS metadata is always
skipped. Configured excludes apply after Git-ignore rules. Literal files bypass
include, exclude, and Git-ignore filtering.

Inputs are normalized lexically without canonicalizing file symlinks,
deduplicated, sorted by normalized absolute identity, and processed serially.
An explicit file spelling wins over a discovered spelling. Discovered-only
paths use a current-directory-relative spelling when possible. Read-only modes
follow explicitly named file symlinks and may follow an explicitly named
directory-symlink root. Write mode rejects both kinds before replacement.
Discovery never follows nested directory symlinks or selects discovered file
symlinks.

Write mode resolves every selector and preflights every selected file before
making any change, then performs permission-preserving atomic replacement per
changed file. A config, selector, traversal, input, or formatting failure
prevents every replacement. A later replacement failure does not roll back
earlier successful replacements, so a multi-file write is not transactional
across the set. The formatter does not resolve references or process files in
parallel.

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
