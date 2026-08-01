use std::collections::HashSet;

use oafmt_core::{InputFormat as CoreFormat, classify, format};
use oafmt_oas::SemanticKind;
use oafmt_syntax::{
    ByteRange, InputFormat as SyntaxFormat, MappingInfo, SyntaxError, inspect_document,
    reorder_mappings, validate_semantic_preservation,
};

const MAX_INPUT_BYTES: usize = 1024 * 1024;
const ROOT_ORDER: &[&str] = &[
    "openapi",
    "$self",
    "info",
    "jsonSchemaDialect",
    "servers",
    "paths",
    "webhooks",
    "components",
    "security",
    "tags",
    "externalDocs",
];

pub fn check_format(data: &[u8], input_format: CoreFormat) {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(first) = format(source, input_format) else {
        return;
    };

    let syntax_format = syntax_format(input_format);
    assert!(inspect_document(&first.output, syntax_format).is_ok());
    assert_eq!(first.changed, first.output.as_bytes() != source.as_bytes());
    assert_eq!(first.output.len(), source.len());
    assert!(validate_semantic_preservation(source, &first.output, syntax_format).is_ok());

    let repeated = format(source, input_format).expect("successful input must remain accepted");
    assert_eq!(repeated, first);

    let second =
        format(&first.output, input_format).expect("successful output must remain accepted");
    assert_eq!(second.output, first.output);
    assert!(!second.changed);
}

pub fn check_classification(data: &[u8]) {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    for input_format in [CoreFormat::Yaml, CoreFormat::Json] {
        let Ok(first) = classify(source, input_format) else {
            continue;
        };
        let repeated =
            classify(source, input_format).expect("successful classification must be repeatable");
        assert_eq!(repeated, first);

        assert!(
            first
                .ranges
                .windows(2)
                .all(|pair| pair[0].range <= pair[1].range)
        );
        for classified in &first.ranges {
            assert!(valid_range(classified.range, source));
        }
        for opaque in first
            .ranges
            .iter()
            .filter(|classified| classified.kind == SemanticKind::Opaque)
        {
            assert!(!first.ranges.iter().any(|candidate| {
                candidate.route.len() > opaque.route.len()
                    && candidate.route.starts_with(&opaque.route)
            }));
        }
    }
}

pub fn check_reordering(data: &[u8]) {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    for input_format in [SyntaxFormat::Yaml, SyntaxFormat::Json] {
        let Ok(document) = inspect_document(source, input_format) else {
            continue;
        };
        let Some(root) = document
            .mappings
            .iter()
            .find(|mapping| mapping.range == document.root)
        else {
            panic!("inspected root mapping must be present");
        };

        if let Ok(output) = reorder_mappings(source, &[(root, ROOT_ORDER)], input_format) {
            assert_eq!(output.len(), source.len());
            if validate_semantic_preservation(source, &output, input_format).is_ok() {
                let reparsed =
                    inspect_document(&output, input_format).expect("validated output must reparse");
                let reparsed_root = reparsed
                    .mappings
                    .iter()
                    .find(|mapping| mapping.range == reparsed.root)
                    .expect("reparsed root mapping must be present");
                let second =
                    reorder_mappings(&output, &[(reparsed_root, ROOT_ORDER)], input_format)
                        .expect("validated output must be reorderable");
                assert_eq!(second, output);
            }
        }

        if let Some(order) = forced_replacement_order(root, input_format) {
            let replacement = reorder_mappings(source, &[(root, &order)], input_format)
                .expect("an inspected mapping with reversible keys must be reorderable");
            assert_ne!(replacement, source);
            assert!(matches!(
                reorder_mappings(source, &[(root, &order), (root, &order)], input_format),
                Err(SyntaxError::InternalInvariant(_))
            ));
        }

        let mut malformed = root.clone();
        malformed.range = ByteRange {
            start: source.len().saturating_add(1),
            end: source.len().saturating_add(2),
        };
        assert!(matches!(
            reorder_mappings(source, &[(&malformed, ROOT_ORDER)], input_format),
            Err(SyntaxError::InvalidInput(_))
        ));

        let mut empty_mapping = root.clone();
        empty_mapping.range.end = empty_mapping.range.start;
        assert!(matches!(
            reorder_mappings(source, &[(&empty_mapping, ROOT_ORDER)], input_format),
            Err(SyntaxError::InvalidInput(_))
        ));

        if let Some(first_entry) = root.entries.first() {
            let mut malformed_entry: MappingInfo = root.clone();
            malformed_entry.entries[0].content_range = ByteRange {
                start: first_entry.range.end.saturating_add(1),
                end: first_entry.range.start,
            };
            assert!(matches!(
                reorder_mappings(source, &[(&malformed_entry, ROOT_ORDER)], input_format),
                Err(SyntaxError::InvalidInput(_))
            ));

            let mut empty_entry = root.clone();
            empty_entry.entries[0].range.end = empty_entry.entries[0].range.start;
            assert!(matches!(
                reorder_mappings(source, &[(&empty_entry, ROOT_ORDER)], input_format),
                Err(SyntaxError::InvalidInput(_))
            ));

            let mut empty_content = root.clone();
            empty_content.entries[0].content_range.end =
                empty_content.entries[0].content_range.start;
            assert!(matches!(
                reorder_mappings(source, &[(&empty_content, ROOT_ORDER)], input_format),
                Err(SyntaxError::InvalidInput(_))
            ));

            let mut outside_entry = root.clone();
            outside_entry.entries[0].range.end = source.len().saturating_add(1);
            assert!(matches!(
                reorder_mappings(source, &[(&outside_entry, ROOT_ORDER)], input_format),
                Err(SyntaxError::InvalidInput(_))
            ));
        }

        if let Some((entry_index, boundary)) =
            root.entries.iter().enumerate().find_map(|(index, entry)| {
                non_character_boundary(source, entry.range).map(|boundary| (index, boundary))
            })
        {
            let mut non_boundary_entry = root.clone();
            non_boundary_entry.entries[entry_index].range.start = boundary;
            assert!(matches!(
                reorder_mappings(source, &[(&non_boundary_entry, ROOT_ORDER)], input_format),
                Err(SyntaxError::InvalidInput(_))
            ));
        }

        if let Some((entry_index, boundary)) =
            root.entries.iter().enumerate().find_map(|(index, entry)| {
                non_character_boundary(source, entry.content_range)
                    .map(|boundary| (index, boundary))
            })
        {
            let mut non_boundary_content = root.clone();
            non_boundary_content.entries[entry_index]
                .content_range
                .start = boundary;
            assert!(matches!(
                reorder_mappings(source, &[(&non_boundary_content, ROOT_ORDER)], input_format),
                Err(SyntaxError::InvalidInput(_))
            ));
        }
    }
}

const fn syntax_format(input_format: CoreFormat) -> SyntaxFormat {
    match input_format {
        CoreFormat::Yaml => SyntaxFormat::Yaml,
        CoreFormat::Json => SyntaxFormat::Json,
    }
}

fn valid_range(range: ByteRange, source: &str) -> bool {
    range.start <= range.end
        && range.end <= source.len()
        && source.is_char_boundary(range.start)
        && source.is_char_boundary(range.end)
}

fn non_character_boundary(source: &str, range: ByteRange) -> Option<usize> {
    source[range.start..range.end]
        .char_indices()
        .find(|(_, character)| character.len_utf8() > 1)
        .map(|(offset, _)| range.start + offset + 1)
}

fn forced_replacement_order(
    mapping: &MappingInfo,
    input_format: SyntaxFormat,
) -> Option<Vec<&str>> {
    if mapping.entries.len() < 2 || mapping.entries.iter().any(|entry| entry.explicit_key) {
        return None;
    }
    if mapping.flow_style != (input_format == SyntaxFormat::Json) {
        return None;
    }
    let keys = mapping
        .entries
        .iter()
        .map(|entry| entry.key.as_deref())
        .collect::<Option<Vec<_>>>()?;
    let unique = keys.iter().copied().collect::<HashSet<_>>();
    if unique.len() != keys.len() {
        return None;
    }
    Some(keys.into_iter().rev().collect())
}
