//! Classified preservation matrix through the production formatter.
#![expect(
    clippy::panic,
    reason = "test assertions intentionally fail fast with case context"
)]

use oafmt_core::{FormatError, InputFormat, format};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    Exact,
    Transformed,
    Normalized,
    Rejected,
}

struct SuccessCase {
    name: &'static str,
    input: String,
    expected: String,
    classification: Classification,
}

struct RejectionCase {
    name: &'static str,
    input: &'static str,
    classification: Classification,
}

impl SuccessCase {
    fn exact(name: &'static str, source: impl Into<String>) -> Self {
        let source = source.into();
        Self {
            name,
            expected: source.clone(),
            input: source,
            classification: Classification::Exact,
        }
    }

    fn transformed(
        name: &'static str,
        input: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            name,
            input: input.into(),
            expected: expected.into(),
            classification: Classification::Transformed,
        }
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the table keeps exact preservation inputs beside their expected output"
)]
fn successful_preservation_matrix_is_exact_deterministic_idempotent_and_semantic() {
    let cases = [
        SuccessCase::transformed(
            "comments, blank lines, scalar styles, indicators, numeric lexemes, tags, and flow values",
            r#"paths:
  /pets:
    get:
      responses: {} # inline response

      # extension comment stays with the extension
      x-preservation:
        plain: 001234567890123456789012345678901234567890
        decimal: 12345678901234567890.12345678901234567890
        numeric-string: '0012345678901234567890'
        single: 'kept ''quoted'''
        double: "kept\nquoted"
        literal: |2-
            literal
            text
        folded: >+2
            folded
            text

        local: !local value
        standard: !!str 123
        flow: [one, {two: 2}]
      summary: "List pets" # inline summary
openapi: 3.1.0
info: {title: Pets, version: v}
"#,
            r#"openapi: 3.1.0
info: {title: Pets, version: v}
paths:
  /pets:
    get:
      summary: "List pets" # inline summary

      # extension comment stays with the extension
      x-preservation:
        plain: 001234567890123456789012345678901234567890
        decimal: 12345678901234567890.12345678901234567890
        numeric-string: '0012345678901234567890'
        single: 'kept ''quoted'''
        double: "kept\nquoted"
        literal: |2-
            literal
            text
        folded: >+2
            folded
            text

        local: !local value
        standard: !!str 123
        flow: [one, {two: 2}]
      responses: {} # inline response
"#,
        ),
        SuccessCase::exact(
            "unchanged flow collections and scalar lexemes",
            r#"openapi: 3.0.3
info:
  title: "Café ☕"
  version: '00000000000000000001'
paths: {}
x-values: {plain: 000123, decimal: 1.2300, string: "1.2300", items: [one, {two: 2}]}
"#,
        ),
        SuccessCase::exact(
            "anchors, repeated aliases, merge keys, and recursive aliases without movement",
            r"openapi: 3.1.0
info: &info
  title: Pets
  version: v
paths: {}
x-first: *info
x-second: *info
x-defaults: &defaults
  enabled: true
x-derived:
  <<: *defaults
x-cycle: &cycle
  self: *cycle
",
        ),
        SuccessCase::transformed(
            "UTF-8 BOM, CRLF, Unicode, and no final newline",
            "\u{feff}info:\r\n  title: Café ☕\r\n  version: 00000000000000000001\r\nopenapi: 3.0.3",
            "\u{feff}openapi: 3.0.3\r\ninfo:\r\n  title: Café ☕\r\n  version: 00000000000000000001",
        ),
        SuccessCase::exact("deep block nesting", deeply_nested_document(128)),
    ];

    for case in &cases {
        let first = format(&case.input, InputFormat::Yaml)
            .unwrap_or_else(|error| panic!("{} unexpectedly failed: {error}", case.name));
        let repeat = format(&case.input, InputFormat::Yaml)
            .unwrap_or_else(|error| panic!("{} repeat failed: {error}", case.name));
        let second = format(&first.output, InputFormat::Yaml)
            .unwrap_or_else(|error| panic!("{} second pass failed: {error}", case.name));

        assert_eq!(first.output, case.expected, "{} exact output", case.name);
        assert_eq!(
            first.changed,
            case.classification == Classification::Transformed,
            "{} changed status",
            case.name
        );
        assert_eq!(repeat, first, "{} determinism", case.name);
        assert_eq!(second.output, case.expected, "{} idempotence", case.name);
        assert!(!second.changed, "{} second-pass changed status", case.name);
        assert_eq!(
            case.input.len(),
            case.expected.len(),
            "{} silently lost or synthesized bytes",
            case.name
        );
    }

    assert!(
        !cases
            .iter()
            .any(|case| case.classification == Classification::Normalized),
        "normalization remains an explicit empty classification because formatting only reorders source slices"
    );
}

#[test]
fn unsafe_movement_and_unsupported_alias_syntax_are_classified_as_rejected() {
    let cases = [
        RejectionCase {
            name: "root movement with anchor",
            input: "info: &info {title: Pets, version: v}\nopenapi: 3.1.0\npaths: {}\nx-copy: *info\n",
            classification: Classification::Rejected,
        },
        RejectionCase {
            name: "Operation movement with repeated aliases",
            input: "openapi: 3.1.0\ninfo: &info {title: Pets, version: v}\npaths:\n  /pets:\n    get:\n      responses: {}\n      x-first: *info\n      x-second: *info\n      summary: list\n",
            classification: Classification::Rejected,
        },
        RejectionCase {
            name: "root movement with recursive alias",
            input: "info: {title: Pets, version: v}\nx-cycle: &cycle\n  self: *cycle\npaths: {}\nopenapi: 3.1.0\n",
            classification: Classification::Rejected,
        },
        RejectionCase {
            name: "root movement with merge key",
            input: "info: {title: Pets, version: v}\nx-derived:\n  <<: {enabled: true}\npaths: {}\nopenapi: 3.1.0\n",
            classification: Classification::Rejected,
        },
    ];

    for case in &cases {
        let result = format(case.input, InputFormat::Yaml);
        match case.classification {
            Classification::Rejected => assert!(
                matches!(
                    result,
                    Err(FormatError::Input(ref message))
                        if message
                            == "reordering is rejected when anchors, aliases, or merge keys are present"
                ),
                "{}: {result:?}",
                case.name
            ),
            classification => panic!(
                "{}: rejection case has unexpected classification {classification:?}",
                case.name
            ),
        }
    }
}

fn deeply_nested_document(depth: usize) -> String {
    let mut source =
        String::from("openapi: 3.1.0\ninfo: {title: Pets, version: v}\npaths: {}\nx-deep:\n");
    for level in 0..depth {
        source.push_str(&"  ".repeat(level + 1));
        source.push_str("level:\n");
    }
    source.push_str(&"  ".repeat(depth + 1));
    source.push_str("value: kept\n");
    source
}
