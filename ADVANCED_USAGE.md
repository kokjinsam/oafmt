# Advanced CLI behavior

This guide explains file selection and safe file updates. For the formatting
boundary and source-preservation rules, see
[YAML preservation](YAML_PRESERVATION.md).

## Input modes

`oafmt` accepts UTF-8 YAML and strict JSON entry documents for OpenAPI 3.0.x,
3.1.x, and 3.2.x.

Plain output mode accepts one file and prints the formatted document to standard
output. `--write`, `--check`, and `--diff` accept one or more literal files,
directories, or glob selectors.

Standard input (stdin) needs a virtual file name so that `oafmt` can select YAML
or JSON:

```sh
oafmt --stdin-filepath virtual.yaml < input.yaml
```

Do not combine stdin with file arguments. `--write` does not accept stdin.
`--config` is available only with `--write`, `--check`, or `--diff`.

## Discovery

A directory selector searches recursively. By default, it selects only files
named `openapi.yaml`, `openapi.yml`, or `openapi.json`.

A glob is a path pattern that `oafmt` expands. It supplies its own inclusion
pattern. Quote it so that the shell does not expand it:

```sh
oafmt --check 'apis/**/openapi.?ml'
```

Discovered files must have a `.yaml`, `.yml`, or `.json` extension. A directory
or glob selector that has no matching file is an error. If the shell expands a
glob first, each result is a literal file and uses the literal-file rules.

Supported glob syntax is `*`, `?`, character classes such as `[ab]`, and `**`.
Brace expansion, repetition, escapes, flags, and `$` wildcards are not
supported. A parent component (`..`) is allowed before a wildcard but not after
one. Literal braces can be in a character class, such as `file[{].yaml`.

## Configuration lookup

For `--write`, `--check`, and `--diff`, `oafmt` searches from the current
directory up to the file-system root for the nearest `oafmt.toml`. Use
`--config PATH` to select a different file.

Configuration affects discovery only:

```toml
[discovery]
include = ["apis/**/openapi.yaml", "apis/**/openapi.json"]
exclude = ["apis/generated/**"]
respect_gitignore = true
```

`include` replaces the default file names and must not be empty. `exclude` is
optional and always wins. Patterns are relative to the directory name used for
the configuration file. If that path contains a symlink, patterns do not move
to the symlink target's directory.

An explicit configuration path can be relative or absolute. It can contain
symlinked files or directories. A `..` component is allowed before a symlink,
but it is rejected after a symlink because the path is ambiguous.

The configuration parser rejects unknown fields, invalid values, empty
`include` lists, missing explicit files, and invalid patterns before it
processes an input.

## Ignore rules

Directory and glob discovery respects repository and nested `.gitignore` files
by default. It does not read global or system ignore files. Set
`respect_gitignore = false` to disable this filtering.

Discovery always skips `.git`, `.hg`, and `.svn` metadata. Configured excludes
apply after Git ignore rules. A literal file bypasses include, exclude, and Git
ignore rules.

## Path order and symlinks

`oafmt` removes duplicate inputs, sorts them by a stable absolute file identity,
and processes them one at a time. If a literal file and discovery select the
same file, the literal path spelling wins. A discovered path is relative to the
current directory when possible.

Read-only modes follow an explicitly named file symlink. They can also follow
an explicitly named directory-symlink root or a glob root that contains a
symlink. `--write` rejects these paths before it replaces any file.

Discovery does not follow a nested directory symlink. It does not select a file
symlink that it finds during a search. It keeps the written path separate from
the file-system identity. Thus, a leading `..` selector or a path that has a
symlink before `..` cannot select a same-named file by mistake.

## Checks before updates

In `--write` mode, `oafmt` resolves all selectors and reads and formats every
selected file before it changes a file. A configuration, selector, traversal,
input, or formatting error prevents all replacements.

For each changed file, `oafmt` writes a temporary file in the same directory,
keeps the original permissions, and syncs the content. It then uses one rename
operation to replace the original file. This atomic replacement prevents other
programs from reading a partly written file. `oafmt` removes the temporary file
if replacement fails. If one replacement succeeds and a later replacement
fails, the earlier change stays. The complete multi-file update is not atomic.
