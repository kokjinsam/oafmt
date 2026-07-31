# YAML preservation

`oafmt` parses YAML into a lossless Rowan concrete syntax tree and reorders
original source slices. It does not re-emit scalar values or apply general YAML
normalization.

Production formatting is deliberately limited to:

- fixed fields at the OpenAPI entry-document root; and
- fixed fields in fixed-method Operation Objects directly below the entry
  document's `paths`.

The fixed methods are `get`, `put`, `post`, `delete`, `options`, `head`,
`patch`, and `trace`. OpenAPI 3.2 also includes `query`; earlier versions do
not.

All other mappings remain opaque to formatting, including Info Objects, Path
Item Objects, schemas and `properties`, responses, callbacks, webhooks,
component Path Items, `additionalOperations`, extensions, example data, and
unexpected shapes.

## Preservation contract

For a successful format:

- output is deterministic and a second pass is byte-identical;
- comments, blank lines, scalar styles, tags, numeric lexemes, Unicode, line
  endings, BOM state, and final-newline state are retained;
- unchanged flow collections and opaque values are retained byte-for-byte;
- output reparses and the complete document passes semantic comparison; and
- only eligible mapping-entry order changes.

Unknown fields retain their positional slots while known fields move through
those slots into their declared order. Standalone trivia between YAML entries
travels with the following source entry; entry indentation and line endings
belong to the destination slot.

Anchors, aliases, merge keys, and recursive or repeated aliases may pass
unchanged. If any formatting would change the document while those constructs
are present, the formatter rejects the input because movement could change
resolution order.

## Safe rejection and limits

The formatter rejects unsupported syntax at a location it would format,
including flow-style YAML mappings, explicit keys, duplicate keys, and
non-string keys. Unrelated opaque mappings do not acquire those restrictions.
Malformed input, multiple documents, unsupported roots, and unsupported or
incomplete OpenAPI versions also fail without output.

Production limits are:

- at most 1 MiB of UTF-8 input;
- at most 64 KiB per physical line;
- at most 1,024 entries in a mapping selected for formatting; and
- the parser's flow-nesting limit.

The classified production matrix is executable in
`crates/oafmt-core/tests/preservation.rs`. It covers exact preservation,
intentional source-slice transformation, the intentionally empty normalization
class, and safe rejection through `oafmt_core::format`. Low-level syntax tests
are limited to inspection, CST ranges, resource limits, semantic validation,
and invalid edit handling.
