//! Pure formatting orchestration for `oafmt`.

use std::fmt;

use oafmt_oas::{Location, Version};
use oafmt_syntax::{
    ByteRange, DocumentInfo, MappingInfo, SyntaxError, inspect_document, reorder_mappings,
    validate_semantic_preservation,
};

/// Explicit input syntax. No filename or content sniffing occurs in the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Yaml,
    Json,
}

/// A successful deterministic formatting result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatResult {
    pub output: String,
    pub changed: bool,
}

/// Caller/input failures and formatter invariant failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    Input(String),
    InternalInvariant(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(message) | Self::InternalInvariant(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for FormatError {}

/// Format one OpenAPI entry document without filesystem or process effects.
pub fn format(source: &str, format: InputFormat) -> Result<FormatResult, FormatError> {
    let syntax_format = match format {
        InputFormat::Yaml => oafmt_syntax::InputFormat::Yaml,
        InputFormat::Json => oafmt_syntax::InputFormat::Json,
    };
    let original = inspect_document(source, syntax_format).map_err(map_syntax_error)?;
    let root = mapping_by_range(&original, original.root)?;
    let version = detect_version(root)?;
    let operation_ranges = operation_ranges(&original, root, version);
    let operation_mappings: Vec<_> = operation_ranges
        .iter()
        .map(|range| mapping_by_range(&original, *range))
        .collect::<Result<_, _>>()?;
    let operation_edits: Vec<_> = operation_mappings
        .iter()
        .map(|mapping| (*mapping, version.operation_order()))
        .collect();

    let operations_formatted =
        reorder_mappings(source, &operation_edits, syntax_format).map_err(map_syntax_error)?;
    let reparsed = inspect_document(&operations_formatted, syntax_format)
        .map_err(|error| internal_reparse_error("operation formatting", error))?;
    let reparsed_root = mapping_by_range(&reparsed, reparsed.root)?;
    let root_formatted = reorder_mappings(
        &operations_formatted,
        &[(reparsed_root, version.root_order())],
        syntax_format,
    )
    .map_err(map_syntax_error)?;

    let changed = root_formatted != source;
    if changed && original.anchor_order_risk {
        return Err(FormatError::Input(
            "reordering is rejected when anchors, aliases, or merge keys are present".into(),
        ));
    }

    inspect_document(&root_formatted, syntax_format)
        .map_err(|error| internal_reparse_error("root formatting", error))?;
    validate_semantic_preservation(source, &root_formatted, syntax_format)
        .map_err(map_syntax_error)?;

    Ok(FormatResult {
        output: root_formatted,
        changed,
    })
}

fn detect_version(root: &MappingInfo) -> Result<Version, FormatError> {
    let entries: Vec<_> = root
        .entries
        .iter()
        .filter(|entry| entry.key.as_deref() == Some("openapi"))
        .collect();
    if entries.len() != 1 {
        return Err(FormatError::Input(
            "document must contain exactly one openapi field".into(),
        ));
    }
    let value = entries[0]
        .scalar_value
        .as_deref()
        .ok_or_else(|| FormatError::Input("openapi must be a string".into()))?;
    Version::parse(value).ok_or_else(|| {
        FormatError::Input(format!(
            "unsupported or incomplete openapi version: {value:?}"
        ))
    })
}

fn operation_ranges(
    document: &DocumentInfo,
    root: &MappingInfo,
    version: Version,
) -> Vec<ByteRange> {
    let Some(paths_range) = child_mapping_range(root, "paths") else {
        return Vec::new();
    };
    let Some(paths) = find_mapping(document, paths_range) else {
        return Vec::new();
    };
    let mut operations = Vec::new();
    for path_entry in &paths.entries {
        let Some(path_key) = path_entry.key.as_deref() else {
            continue;
        };
        if version.classify_child(Location::Paths, path_key) != Some(Location::PathItem) {
            continue;
        }
        let Some(path_item_range) = path_entry.value_mapping else {
            continue;
        };
        let Some(path_item) = find_mapping(document, path_item_range) else {
            continue;
        };
        for method_entry in &path_item.entries {
            let Some(method_key) = method_entry.key.as_deref() else {
                continue;
            };
            if version.classify_child(Location::PathItem, method_key) == Some(Location::Operation)
                && let Some(operation) = method_entry.value_mapping
            {
                operations.push(operation);
            }
        }
    }
    operations.sort_unstable();
    operations.dedup();
    operations
}

fn child_mapping_range(mapping: &MappingInfo, key: &str) -> Option<ByteRange> {
    mapping
        .entries
        .iter()
        .find(|entry| entry.key.as_deref() == Some(key))
        .and_then(|entry| entry.value_mapping)
}

fn mapping_by_range(
    document: &DocumentInfo,
    range: ByteRange,
) -> Result<&MappingInfo, FormatError> {
    find_mapping(document, range).ok_or_else(|| {
        FormatError::InternalInvariant("CST mapping range could not be resolved".into())
    })
}

fn find_mapping(document: &DocumentInfo, range: ByteRange) -> Option<&MappingInfo> {
    document
        .mappings
        .iter()
        .find(|mapping| mapping.range == range)
}

fn map_syntax_error(error: SyntaxError) -> FormatError {
    match error {
        SyntaxError::InvalidInput(message) => FormatError::Input(message),
        SyntaxError::InternalInvariant(message) => FormatError::InternalInvariant(message),
    }
}

fn internal_reparse_error(stage: &str, error: SyntaxError) -> FormatError {
    FormatError::InternalInvariant(format!("{stage} produced invalid output: {error}"))
}
