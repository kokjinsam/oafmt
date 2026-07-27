# Phase 1: YAML preservation spike

## Recommendation

**Proceed, with revision before production use.**

Rust can perform deterministic, byte-preserving movement of complete YAML
mapping entries when it retains the concrete syntax and reorders trustworthy
original source slices. The spike proves that narrow result for one root block
mapping using implicit keys whose complete entry text resolves uniquely in the
source. Every accepted layout now fails closed when that reconciliation is
ambiguous, so the proceed recommendation remains. Phase 2 must not treat the
experimental API as a production parser or general formatter: nested mapping
selection, dependency-aware anchor movement, and a stronger source-range API
remain unresolved.

No OpenAPI behavior is implemented or inferred here.

## Selected approach

The spike uses `yaml-edit` 0.2.3 with default features disabled. It parses into
a Rowan concrete syntax tree that retains comments, whitespace, scalar text,
styles, tags, directives, and document markers. The experiment:

1. rejects input outside a deliberately narrow safety contract;
2. obtains complete root mapping entries from the CST;
3. rejects explicit-key layouts and requires each entry's exact CST text to
   occur uniquely in ordered, non-overlapping source spans;
4. permits only blank lines and standalone comments between reconciled spans,
   attaching that trivia to the following non-first entry;
5. reorders those original slices while keeping entry-separating line endings
   positional, including an unterminated final entry; and
6. reparses the result, requires one root block mapping with the same number of
   unique keys, and compares every original and rendered key/value pair with
   `yaml_eq`.

The operation selects an entry by unique decoded string key and a destination
index. Repeating the operation is a no-op, which gives second-pass idempotence.
Unchanged byte length remains asserted by the matrix but is not treated as
proof of preservation.

The library's high-level `move_before` and `move_after` operations were not
selected: they remove an entry and rebuild it from a supplied value. That is
not evidence of scalar-lexeme or trivia preservation.

## Alternatives actually evaluated

| Candidate | Evidence | Result |
| --- | --- | --- |
| `yaml-edit` 0.2.3 | Lossless Rowan CST, parse errors, comments/trivia and style retention, entry traversal, deterministic CST rendering | Selected, with original-source slice movement instead of its rebuilding move API |
| `granit-parser` 1.0.0-rc.1 | Pure Rust CST parser with comments and styles | Not selected: parsing/traversal exists, but the evaluated release does not provide the editing/rendering path needed by this spike |
| `yaml-rust2` 0.11.0 | YAML 1.2 event parser with markers and an emitter | Not selected: scanner comments are skipped and the emitter reconstructs YAML, so it cannot establish the required lexical/trivia contract |

## Executable preservation matrix

`crates/oafmt-syntax/tests/preservation_matrix.rs` is authoritative. Fixture
inputs and exact expected outputs live under
`crates/oafmt-syntax/tests/fixtures/`.

| Construct | Classification | Observed contract |
| --- | --- | --- |
| Entry order | Intentionally transformed | Only the selected complete entry block changes position |
| Inline/standalone comments and blank lines | Intentionally transformed | Text is byte-exact; leading inter-entry trivia travels with its following entry; file header stays fixed |
| Plain, single/double quoted, literal, and folded scalars | Intentionally transformed | Complete scalar lexemes and styles are byte-exact |
| Chomping and indentation indicators | Intentionally transformed | `|2-` and `>+2` are byte-exact |
| Large numeric lexemes | Intentionally transformed | Lexeme is never converted to a Rust numeric type |
| Flow/block and empty collections as values | Intentionally transformed | Collection text is byte-exact |
| Local and standard tags | Intentionally transformed | Tags move safely with their complete entry |
| Directives and document markers | Intentionally transformed | Prefix/suffix markers remain fixed and exact |
| Unicode, UTF-8 BOM, LF, CRLF, and an unterminated final entry | Intentionally transformed | Original bytes, line-ending convention, and final-newline state are retained |
| Anchors, aliases, and merge keys with no movement | Exactly preserved | Parsing and rendering retain their syntax byte-for-byte |
| Actual movement when anchors, aliases, or merge keys occur | Rejected | Reordering may change definition/reference resolution order |
| Duplicate keys | Rejected | Semantic identity and selection are ambiguous |
| Non-string keys | Rejected | Outside the spike's selection contract |
| Entry text repeated in a preceding header comment | Rejected | The entry substring has more than one possible source range |
| Root mapping explicit-key syntax | Rejected | Explicit-key entry ownership is outside the spike's trusted layout contract |
| Root flow mapping, scalar root, or empty input | Rejected | The spike moves entries only in a non-empty root block mapping |
| Multi-document input | Rejected | Exactly one document is required |
| Malformed input | Rejected | Strict parse errors are never rendered |
| Oversized, overlong-line, over-entry, or excessive-flow-depth input | Rejected | Bounded before parsing where possible; parser depth limit handles nested flow input |
| Semantically preserved but normalized | No cases | Successful rendering performs no normalization |

Every successful matrix case asserts exact expected output, repeat-run
determinism, second-pass idempotence, unchanged byte length, reparsing, and
semantic key/value preservation where the CST comparison is meaningful. The
implementation also performs reparsing and structural key/value checks before
returning success. These backstops do not prove comment attachment; trustworthy
and unambiguous source-range ownership supplies that evidence. Rejected cases
assert a specific safe-rejection class and produce no rendered output.

## Safety limits

- Input: at most 1 MiB.
- Physical line: at most 64 KiB.
- Root mapping: 1 to 1,024 entries.
- Exactly one UTF-8 YAML document.
- Root must be an implicit-key block mapping with unique string keys whose
  complete rendered entry text occurs exactly once in the source.
- Reconciled entry spans must be ordered and non-overlapping, with only blank
  lines and standalone comments as inter-entry trivia.
- Actual movement is rejected if the CST contains an anchor, alias, or merge
  key.
- `yaml-edit` rejects flow nesting deeper than 256 levels.

These are spike limits, not proposed CLI defaults.

## Remaining risks

- Entry source spans are reconciled by unique exact-text search because the
  evaluated public API does not expose a complete `MappingEntry` byte range.
  Ambiguous or unsupported layouts fail closed, but production work should
  expose or own trustworthy CST ranges rather than depend on text
  reconciliation.
- Only the root block mapping is selectable. Nested block mappings need an
  unambiguous CST path and indentation-aware trivia ownership.
- Anchor/alias/merge syntax is losslessly retained, but safe movement requires
  dependency analysis and semantic comparison that understands merge
  resolution.
- Comment attachment is an explicit local contract, not a YAML-standard
  semantic rule. Reparsing and `yaml_eq` comparison cannot prove attachment;
  trustworthy range ownership must do so, and later UX evidence may require
  revising the rule.
- The selected dependency is young. Its parser behavior and maintenance posture
  need continued scrutiny before committing to a production formatter.
