//! Production syntax inspection, range, resource-limit, and edit tests.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test setup and assertions intentionally fail fast"
)]

use std::fmt::Write as _;

use oafmt_syntax::{
    ByteRange, InputFormat, SyntaxError, inspect_document, reorder_mappings,
    validate_semantic_preservation,
};

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

#[test]
fn inspected_ranges_are_authoritative_source_slices() {
    let source =
        "openapi: 3.1.0\npaths:\n  /pets:\n    get:\n      responses: {}\n      summary: list\n";
    let document = inspect_document(source, InputFormat::Yaml).unwrap();
    assert_eq!(&source[document.root.start..document.root.end], source);

    for mapping in &document.mappings {
        assert!(mapping.range.start < mapping.range.end);
        assert!(mapping.range.end <= source.len());
        for entry in &mapping.entries {
            assert!(entry.range.start >= mapping.range.start);
            assert!(entry.range.end <= mapping.range.end);
            assert!(entry.content_range.start >= entry.range.start);
            assert!(entry.content_range.end <= entry.range.end);
        }
    }
}

#[test]
fn production_resource_limits_are_enforced() {
    let oversized = "x".repeat(1024 * 1024 + 1);
    assert!(matches!(
        inspect_document(&oversized, InputFormat::Yaml),
        Err(SyntaxError::InvalidInput(message))
            if message == "input exceeds 1048576 bytes"
    ));

    let long_line = format!("openapi: 3.1.0\nx: {}\n", "x".repeat(64 * 1024));
    assert!(matches!(
        inspect_document(&long_line, InputFormat::Yaml),
        Err(SyntaxError::InvalidInput(message))
            if message == "a line exceeds 65536 bytes"
    ));

    let mut too_many_entries = String::new();
    for index in 0..=1024 {
        writeln!(&mut too_many_entries, "key{index}: value").unwrap();
    }
    let document = inspect_document(&too_many_entries, InputFormat::Yaml).unwrap();
    let root = document
        .mappings
        .iter()
        .find(|mapping| mapping.range == document.root)
        .unwrap();
    assert!(matches!(
        reorder_mappings(
            &too_many_entries,
            &[(root, &["key0"])],
            InputFormat::Yaml
        ),
        Err(SyntaxError::InvalidInput(message))
            if message == "formatted mapping must contain 1..=1024 entries, found 1025"
    ));

    let excessive_flow_depth = format!(
        "openapi: 3.1.0\nvalue: {}item{}\n",
        "[".repeat(257),
        "]".repeat(257)
    );
    assert!(matches!(
        inspect_document(&excessive_flow_depth, InputFormat::Yaml),
        Err(SyntaxError::InvalidInput(_))
    ));
}

#[test]
fn invalid_or_overlapping_edits_fail_closed() {
    let source = "info: {}\nopenapi: 3.1.0\npaths: {}\n";
    let document = inspect_document(source, InputFormat::Yaml).unwrap();
    let root = document
        .mappings
        .iter()
        .find(|mapping| mapping.range == document.root)
        .unwrap();

    assert!(matches!(
        reorder_mappings(
            source,
            &[
                (root, &["openapi", "info", "paths"]),
                (root, &["openapi", "info", "paths"]),
            ],
            InputFormat::Yaml,
        ),
        Err(SyntaxError::InternalInvariant(message))
            if message == "formatted mapping ranges overlap"
    ));

    let flow = "{openapi: 3.1.0, info: {}, paths: {}}\n";
    let flow_document = inspect_document(flow, InputFormat::Yaml).unwrap();
    let flow_root = flow_document
        .mappings
        .iter()
        .find(|mapping| mapping.range == flow_document.root)
        .unwrap();
    assert!(matches!(
        reorder_mappings(
            flow,
            &[(flow_root, &["openapi", "info", "paths"])],
            InputFormat::Yaml,
        ),
        Err(SyntaxError::InvalidInput(message))
            if message == "flow-style YAML is not supported at a formatted location"
    ));
}

#[test]
fn semantic_validation_accepts_reordering_and_rejects_value_changes() {
    let before = "info: {title: Pets, version: v}\nopenapi: 3.1.0\npaths: {}\n";
    let reordered = "openapi: 3.1.0\ninfo: {title: Pets, version: v}\npaths: {}\n";
    validate_semantic_preservation(before, reordered, InputFormat::Yaml).unwrap();

    let changed = "openapi: 3.1.0\ninfo: {title: Other, version: v}\npaths: {}\n";
    assert!(matches!(
        validate_semantic_preservation(before, changed, InputFormat::Yaml),
        Err(SyntaxError::InternalInvariant(message))
            if message == "YAML semantics changed during formatting"
    ));
}

#[test]
fn json_semantic_validation_compares_numbers_exactly() {
    for (name, before, after) in [
        (
            "long fractional values",
            r#"{"value":0.1234567890123456789012345678901}"#,
            r#"{"value":0.1234567890123456789012345678902}"#,
        ),
        (
            "30-plus-digit integers",
            r#"{"value":1234567890123456789012345678901234567890}"#,
            r#"{"value":1234567890123456789012345678901234567891}"#,
        ),
        (
            "underflowing exponents",
            r#"{"value":1e-400}"#,
            r#"{"value":2e-400}"#,
        ),
    ] {
        assert!(
            matches!(
                validate_semantic_preservation(before, after, InputFormat::Json),
                Err(SyntaxError::InternalInvariant(message))
                    if message == "JSON semantics changed during formatting"
            ),
            "{name}"
        );
    }
}

#[test]
fn byte_range_ordering_is_stable() {
    let mut ranges = [
        ByteRange { start: 8, end: 10 },
        ByteRange { start: 0, end: 4 },
        ByteRange { start: 4, end: 8 },
    ];
    ranges.sort_unstable();
    assert_eq!(
        ranges,
        [
            ByteRange { start: 0, end: 4 },
            ByteRange { start: 4, end: 8 },
            ByteRange { start: 8, end: 10 },
        ]
    );
}
