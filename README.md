# oafmt

`oafmt` puts OpenAPI fields in a consistent order. It formats YAML and JSON
files for OpenAPI 3.0, 3.1, and 3.2. It keeps comments, spacing, scalar styles,
and other source details.

Before:

```yaml
paths: {}
openapi: 3.1.0
info: {title: Pets, version: '1'}
```

After:

```yaml
openapi: 3.1.0
info: {title: Pets, version: '1'}
paths: {}
```

## Installation

Install with Cargo:

```sh
cargo install oafmt
```

Install with Homebrew:

```sh
brew install kokjinsam/tap/oafmt
```

Prebuilt files are on
[GitHub Releases](https://github.com/kokjinsam/oafmt/releases).

## Use

Print one formatted file to standard output:

```sh
oafmt openapi.yaml
```

Use the other common modes for one or more files or directories:

```sh
oafmt --write openapi.yaml
oafmt --check .
oafmt --diff openapi.yaml
```

`--write` updates files. `--check` reports files that need changes. `--diff`
shows the changes without updating files.

For directory discovery, add an `oafmt.toml` file when you need custom paths:

```toml
[discovery]
include = ["apis/**/openapi.yaml"]
exclude = ["apis/generated/**"]
```

See [Advanced CLI behavior](ADVANCED_USAGE.md) for discovery, glob,
configuration, ignore, path, symlink, and write-safety details.

## What oafmt changes

`oafmt` sorts known fields at the document root. It also sorts known fields in
standard HTTP operations under `paths`. It does not format schemas, responses,
callbacks, webhooks, extensions, or other nested content. It does not lint,
resolve references, or check whether the API follows all OpenAPI rules.

See [YAML preservation](YAML_PRESERVATION.md) for the exact preservation rules.

## Development

Use the public project commands:

```sh
just setup
just check
just audit
```

See [Fuzzing](fuzz/README.md) for fuzz-test instructions.

This project is primarily coded by AI under human direction and review.

## License

MIT
