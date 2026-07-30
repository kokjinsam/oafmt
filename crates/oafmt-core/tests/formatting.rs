//! Formatter behavior, preservation, idempotence, and safe-rejection tests.
#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test setup and assertions intentionally fail fast on broken fixtures"
)]

use oafmt_core::{FormatError, InputFormat, format};

struct SuccessCase {
    name: &'static str,
    input: &'static str,
    expected: &'static str,
    format: InputFormat,
}

const SUCCESS_CASES: &[SuccessCase] = &[
    SuccessCase {
        name: "YAML root and Operation order",
        input: include_str!("fixtures/basic.input.yaml"),
        expected: include_str!("fixtures/basic.expected.yaml"),
        format: InputFormat::Yaml,
    },
    SuccessCase {
        name: "strict JSON with positional separators",
        input: include_str!("fixtures/basic.input.json"),
        expected: include_str!("fixtures/basic.expected.json"),
        format: InputFormat::Json,
    },
    SuccessCase {
        name: "OpenAPI 3.2 query Operation",
        input: include_str!("fixtures/query.input.yaml"),
        expected: include_str!("fixtures/query.expected.yaml"),
        format: InputFormat::Yaml,
    },
    SuccessCase {
        name: "comments, blank lines, and repeated entry-like comment text",
        input: include_str!("fixtures/comments.input.yaml"),
        expected: include_str!("fixtures/comments.expected.yaml"),
        format: InputFormat::Yaml,
    },
    SuccessCase {
        name: "keys outside the paths Operation location stay byte exact",
        input: include_str!("fixtures/locations.input.yaml"),
        expected: include_str!("fixtures/locations.expected.yaml"),
        format: InputFormat::Yaml,
    },
    SuccessCase {
        name: "Phase 3 classification does not expand formatting eligibility",
        input: include_str!("fixtures/phase3-output-lock.input.yaml"),
        expected: include_str!("fixtures/phase3-output-lock.expected.yaml"),
        format: InputFormat::Yaml,
    },
    SuccessCase {
        name: "already ordered bytes including scalar lexemes stay identical",
        input: include_str!("fixtures/noop.yaml"),
        expected: include_str!("fixtures/noop.yaml"),
        format: InputFormat::Yaml,
    },
    SuccessCase {
        name: "standard YAML string-tagged OpenAPI version",
        input: "paths: {}\nopenapi: !!str 3.1.0\ninfo: {title: Pets, version: v}\n",
        expected: "openapi: !!str 3.1.0\ninfo: {title: Pets, version: v}\npaths: {}\n",
        format: InputFormat::Yaml,
    },
];

#[test]
fn successful_fixtures_are_exact_deterministic_idempotent_and_lossless() {
    for case in SUCCESS_CASES {
        let first = format(case.input, case.format)
            .unwrap_or_else(|error| panic!("{} unexpectedly failed: {error}", case.name));
        let repeat = format(case.input, case.format)
            .unwrap_or_else(|error| panic!("{} repeat failed: {error}", case.name));
        let second = format(&first.output, case.format)
            .unwrap_or_else(|error| panic!("{} second pass failed: {error}", case.name));

        assert_eq!(first.output, case.expected, "{} exact output", case.name);
        assert_eq!(
            first.changed,
            case.input != case.expected,
            "{} changed flag",
            case.name
        );
        assert_eq!(repeat, first, "{} determinism", case.name);
        assert_eq!(second.output, case.expected, "{} idempotence", case.name);
        assert!(!second.changed, "{} second-pass changed flag", case.name);
        assert_eq!(
            case.input.len(),
            case.expected.len(),
            "{} silently lost or synthesized bytes",
            case.name
        );
    }
}

#[test]
fn malformed_input_is_rejected_without_output() {
    assert!(matches!(
        format("openapi: [\n", InputFormat::Yaml),
        Err(FormatError::Input(_))
    ));
}

#[test]
fn unsafe_or_unsupported_inputs_fail_closed() {
    let cases = [
        ("multiple documents", "openapi: 3.1.0\n---\nother: doc\n"),
        ("non-map root", "- openapi: 3.1.0\n"),
        ("missing openapi", "info: {}\n"),
        ("non-string openapi", "openapi: [3, 1, 0]\n"),
        ("incomplete openapi", "openapi: '3.1'\n"),
        ("unsupported openapi", "openapi: 3.3.0\n"),
        ("non-string tagged openapi", "openapi: !!float 3.1.0\n"),
        ("custom-tagged openapi", "openapi: !version 3.1.0\n"),
        (
            "duplicate root key",
            "openapi: 3.1.0\nopenapi: 3.1.1\ninfo: {}\n",
        ),
        (
            "duplicate Operation key",
            "openapi: 3.1.0\npaths:\n  /x:\n    get:\n      summary: one\n      summary: two\n",
        ),
        ("flow YAML root", "{openapi: 3.1.0, info: {}, paths: {}}\n"),
        (
            "flow YAML Operation",
            "openapi: 3.1.0\npaths:\n  /x:\n    get: {responses: {}, summary: x}\n",
        ),
        ("explicit root key", "? openapi\n: 3.1.0\ninfo: {}\n"),
        (
            "anchor movement",
            "info: &info {title: x, version: v}\nopenapi: 3.1.0\npaths: {}\n",
        ),
    ];

    for (name, input) in cases {
        assert!(
            matches!(format(input, InputFormat::Yaml), Err(FormatError::Input(_))),
            "{name}"
        );
    }

    for (name, input) in [
        ("JSON comment", "{\"openapi\":\"3.1.0\" // no\n}"),
        ("JSON trailing comma", "{\"openapi\":\"3.1.0\",}"),
        ("JSON scalar root", "\"3.1.0\""),
    ] {
        assert!(
            matches!(format(input, InputFormat::Json), Err(FormatError::Input(_))),
            "{name}"
        );
    }
}

#[test]
fn bom_crlf_unicode_and_no_final_newline_are_preserved() {
    let input =
        "\u{feff}info:\r\n  title: Café ☕\r\n  version: 00000000000000000001\r\nopenapi: 3.0.3";
    let expected =
        "\u{feff}openapi: 3.0.3\r\ninfo:\r\n  title: Café ☕\r\n  version: 00000000000000000001";
    let result = format(input, InputFormat::Yaml).expect("lexical fixture should format");

    assert_eq!(result.output, expected);
    assert!(result.changed);
    assert!(!result.output.ends_with(['\r', '\n']));
    assert_eq!(result.output.len(), input.len());
    assert_eq!(
        format(&result.output, InputFormat::Yaml).unwrap().output,
        expected
    );
}

#[test]
fn anchors_and_non_string_keys_may_pass_unchanged_outside_an_actual_reorder() {
    let input = "openapi: 3.1.0\ninfo: &info\n  title: x\n  version: v\npaths: {}\ncomponents:\n  schemas:\n    Lookup:\n      properties:\n        1: one\nx-copy: *info\n";
    let result = format(input, InputFormat::Yaml).expect("unchanged syntax should pass");

    assert_eq!(result.output, input);
    assert!(!result.changed);
}
