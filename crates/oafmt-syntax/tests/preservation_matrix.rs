use std::collections::HashSet;
use std::str::FromStr;

use oafmt_syntax::{
    InputFormat, MAX_INPUT_BYTES, MAX_LINE_BYTES, MAX_MAPPING_ENTRIES, MoveError, inspect_document,
    move_root_mapping_entry,
};
use yaml_edit::{Document, YamlFile, yaml_eq};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preservation {
    ExactlyPreserved,
    SemanticallyPreservedButNormalized,
    IntentionallyTransformed,
    Rejected,
}

#[test]
fn neutral_inventory_links_mapping_entries_to_ordered_sequence_items() {
    let source = "root:\n  values:\n    - first:\n        nested: true\n    - second: false\n";
    let document = inspect_document(source, InputFormat::Yaml).unwrap();
    assert_eq!(document.sequences.len(), 1);
    let sequence = &document.sequences[0];
    assert_eq!(sequence.items.len(), 2);
    let first_mapping = sequence.items[0].value_mapping.expect("mapping item");
    assert!(
        document
            .mappings
            .iter()
            .any(|mapping| mapping.range == first_mapping)
    );
    let values_entry = document
        .mappings
        .iter()
        .flat_map(|mapping| &mapping.entries)
        .find(|entry| entry.key.as_deref() == Some("values"))
        .unwrap();
    assert_eq!(values_entry.value_sequence, Some(sequence.range));
}

struct MoveCase {
    name: &'static str,
    input: &'static str,
    expected: &'static str,
    key: &'static str,
    to: usize,
    preservation: Preservation,
    semantic_assertion: bool,
}

const MOVE_CASES: &[MoveCase] = &[
    MoveCase {
        name: "comment attachment and blank lines",
        input: include_str!("fixtures/comments.input.yaml"),
        expected: include_str!("fixtures/comments.expected.yaml"),
        key: "third",
        to: 1,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "plain, quoted, literal, folded, indicators, and large numeric lexemes",
        input: include_str!("fixtures/scalars.input.yaml"),
        expected: include_str!("fixtures/scalars.expected.yaml"),
        key: "scalars",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "flow, block, and empty collections",
        input: include_str!("fixtures/collections.input.yaml"),
        expected: include_str!("fixtures/collections.expected.yaml"),
        key: "collections",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "local and standard tags",
        input: include_str!("fixtures/tags.input.yaml"),
        expected: include_str!("fixtures/tags.expected.yaml"),
        key: "tagged",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "directives and document markers",
        input: include_str!("fixtures/directives.input.yaml"),
        expected: include_str!("fixtures/directives.expected.yaml"),
        key: "first",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "anchors, aliases, and merge keys without reordering",
        input: include_str!("fixtures/anchors.yaml"),
        expected: include_str!("fixtures/anchors.yaml"),
        key: "defaults",
        to: 0,
        preservation: Preservation::ExactlyPreserved,
        semantic_assertion: false,
    },
    MoveCase {
        name: "Unicode",
        input: "尾: 終わり\ncafé: \"☕\"\n",
        expected: "café: \"☕\"\n尾: 終わり\n",
        key: "café",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "CRLF line endings",
        input: "tail: done\r\nfirst: one\r\n",
        expected: "first: one\r\ntail: done\r\n",
        key: "first",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "unterminated final entry",
        input: "first: one\nsecond: two",
        expected: "second: two\nfirst: one",
        key: "second",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
    MoveCase {
        name: "UTF-8 BOM",
        input: "\u{feff}tail: done\nfirst: one\n",
        expected: "\u{feff}first: one\ntail: done\n",
        key: "first",
        to: 0,
        preservation: Preservation::IntentionallyTransformed,
        semantic_assertion: true,
    },
];

#[test]
fn executable_preservation_matrix() {
    let mut observed = HashSet::new();

    for case in MOVE_CASES {
        observed.insert(case.name);
        let rendered = move_root_mapping_entry(case.input, case.key, case.to)
            .unwrap_or_else(|error| panic!("{} unexpectedly rejected: {error}", case.name));

        assert_eq!(rendered, case.expected, "{} output", case.name);
        assert_eq!(
            move_root_mapping_entry(case.input, case.key, case.to).unwrap(),
            rendered,
            "{} determinism",
            case.name
        );
        assert_eq!(
            move_root_mapping_entry(&rendered, case.key, case.to).unwrap(),
            rendered,
            "{} second-pass idempotence",
            case.name
        );
        if case.semantic_assertion {
            assert_semantically_preserved(case.input, &rendered, case.name);
        }
        assert_eq!(
            rendered.len(),
            case.input.len(),
            "{} silently lost or synthesized bytes",
            case.name
        );
        assert!(matches!(
            case.preservation,
            Preservation::ExactlyPreserved | Preservation::IntentionallyTransformed
        ));
    }

    assert_eq!(observed.len(), MOVE_CASES.len());
    assert!(
        !MOVE_CASES
            .iter()
            .any(|case| { case.preservation == Preservation::SemanticallyPreservedButNormalized })
    );
}

struct RejectionCase {
    name: &'static str,
    input: &'static str,
    key: &'static str,
    to: usize,
    expected: fn(&MoveError) -> bool,
    preservation: Preservation,
}

#[test]
fn executable_rejection_matrix() {
    let cases = [
        RejectionCase {
            name: "anchor alias merge movement",
            input: include_str!("fixtures/anchors.yaml"),
            key: "alias",
            to: 0,
            expected: |error| matches!(error, MoveError::AnchorOrderRisk),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "duplicate keys",
            input: include_str!("fixtures/duplicate-keys.yaml"),
            key: "same",
            to: 0,
            expected: |error| matches!(error, MoveError::DuplicateKey(key) if key == "same"),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "non-string keys",
            input: include_str!("fixtures/non-string-key.yaml"),
            key: "plain",
            to: 0,
            expected: |error| matches!(error, MoveError::NonStringKey),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "multi-document input",
            input: include_str!("fixtures/multi-document.yaml"),
            key: "first",
            to: 0,
            expected: |error| matches!(error, MoveError::DocumentCount(2)),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "malformed input",
            input: include_str!("fixtures/malformed.yaml"),
            key: "first",
            to: 0,
            expected: |error| matches!(error, MoveError::InvalidYaml(_)),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "root flow mapping",
            input: "{first: one, second: two}\n",
            key: "second",
            to: 0,
            expected: |error| matches!(error, MoveError::RootBlockMappingRequired),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "entry text repeated in preceding header comment",
            input: "# first: one\nfirst: one\nsecond: two\n",
            key: "second",
            to: 0,
            expected: |error| matches!(error, MoveError::UntrustworthySourceRange),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "root mapping explicit-key syntax",
            input: "? first\n: one\nsecond: two\n",
            key: "second",
            to: 0,
            expected: |error| matches!(error, MoveError::UntrustworthySourceRange),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "single scalar document",
            input: "scalar\n",
            key: "scalar",
            to: 0,
            expected: |error| matches!(error, MoveError::RootBlockMappingRequired),
            preservation: Preservation::Rejected,
        },
        RejectionCase {
            name: "empty input",
            input: "",
            key: "missing",
            to: 0,
            expected: |error| {
                matches!(
                    error,
                    MoveError::DocumentCount(0)
                        | MoveError::RootBlockMappingRequired
                        | MoveError::EntryCount(0)
                )
            },
            preservation: Preservation::Rejected,
        },
    ];

    for case in cases {
        let error = match move_root_mapping_entry(case.input, case.key, case.to) {
            Ok(_) => panic!("{} unexpectedly succeeded", case.name),
            Err(error) => error,
        };
        assert!((case.expected)(&error), "{}: {error:?}", case.name);
        assert_eq!(case.preservation, Preservation::Rejected);
    }
}

#[test]
fn resource_risks_are_bounded_before_or_during_parsing() {
    let oversized = "x".repeat(MAX_INPUT_BYTES + 1);
    assert_eq!(
        move_root_mapping_entry(&oversized, "x", 0),
        Err(MoveError::InputTooLarge)
    );

    let long_line = format!("key: {}\n", "x".repeat(MAX_LINE_BYTES));
    assert_eq!(
        move_root_mapping_entry(&long_line, "key", 0),
        Err(MoveError::LineTooLong)
    );

    let too_many_entries = (0..=MAX_MAPPING_ENTRIES)
        .map(|index| format!("key{index}: value\n"))
        .collect::<String>();
    assert_eq!(
        move_root_mapping_entry(&too_many_entries, "key0", 0),
        Err(MoveError::EntryCount(MAX_MAPPING_ENTRIES + 1))
    );

    let excessive_flow_depth = format!(
        "first: {}value{}\nsecond: value\n",
        "[".repeat(257),
        "]".repeat(257)
    );
    assert!(matches!(
        move_root_mapping_entry(&excessive_flow_depth, "second", 0),
        Err(MoveError::InvalidYaml(_))
    ));
}

fn assert_semantically_preserved(before: &str, after: &str, name: &str) {
    let before = Document::from_str(before).unwrap();
    let after = Document::from_str(after).unwrap();
    let before = before.as_mapping().unwrap();
    let after = after.as_mapping().unwrap();

    assert_eq!(before.len(), after.len(), "{name} entry count");
    for before_entry in before.entries() {
        let before_key = before_entry.key_node().unwrap();
        let before_value = before_entry.value_node().unwrap();
        let after_entry = after
            .entries()
            .find(|entry| {
                entry
                    .key_node()
                    .is_some_and(|after_key| yaml_eq(&before_key, &after_key))
            })
            .unwrap_or_else(|| panic!("{name} lost key {before_key}"));
        let after_value = after_entry.value_node().unwrap();
        assert!(
            yaml_eq(&before_value, &after_value),
            "{name} changed value for {before_key}"
        );
    }

    assert!(YamlFile::from_str(after.to_string().as_str()).is_ok());
}
