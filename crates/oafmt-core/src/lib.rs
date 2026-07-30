//! Pure formatting orchestration for `oafmt`.
//!
//! The core accepts explicit syntax and source text, applies version-aware
//! ordering, and verifies preservation without filesystem or process effects.
//! Formatting is deterministic and idempotent; classification is not OpenAPI
//! validation.

use std::fmt;

use oafmt_oas::{Edge, ObjectKind, SemanticKind, Version};
use oafmt_syntax::{
    ByteRange, DocumentInfo, MappingInfo, SequenceInfo, SyntaxError, inspect_document,
    reorder_mappings, validate_semantic_preservation,
};

/// Explicit input syntax. No filename or content sniffing occurs in the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    /// YAML syntax.
    Yaml,
    /// Strict JSON syntax.
    Json,
}

/// A successful deterministic formatting result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatResult {
    /// Complete formatted document bytes as UTF-8 text.
    pub output: String,
    /// Whether `output` differs from the input.
    pub changed: bool,
}

/// One provenance step from the entry-document root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteEdge {
    /// A fixed field declared by an OpenAPI Object.
    FixedField(String),
    /// A dynamic key inside an object or map.
    DynamicMapValue(String),
    /// A zero-based sequence position.
    SequenceItem(usize),
}

/// A syntax range with its OpenAPI semantic expectation and complete route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedRange {
    /// Syntax range occupied by the classified value.
    pub range: ByteRange,
    /// OpenAPI semantic expectation at the range.
    pub kind: SemanticKind,
    /// Complete route from the entry-document root.
    pub route: Vec<RouteEdge>,
}

/// The semantic inventory for one supported OpenAPI entry document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationResult {
    /// Supported OpenAPI version family declared by the entry document.
    pub version: Version,
    /// Reachable classified mapping and sequence ranges.
    pub ranges: Vec<ClassifiedRange>,
}

/// Caller/input failures and formatter invariant failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// Caller input cannot be safely formatted or classified.
    Input(String),
    /// A formatter preservation or implementation invariant failed.
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
///
/// # Errors
///
/// Returns [`FormatError::Input`] for unsupported input and
/// [`FormatError::InternalInvariant`] when formatting cannot prove that its
/// output reparses with unchanged semantics.
pub fn format(source: &str, format: InputFormat) -> Result<FormatResult, FormatError> {
    let syntax_format = match format {
        InputFormat::Yaml => oafmt_syntax::InputFormat::Yaml,
        InputFormat::Json => oafmt_syntax::InputFormat::Json,
    };
    let original = inspect_document(source, syntax_format).map_err(map_syntax_error)?;
    let root = mapping_by_range(&original, original.root)?;
    let version = detect_version(root)?;
    let inventory = semantic_inventory(&original, version)?;
    let operation_ranges = eligible_operation_ranges(&inventory);
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
        .map_err(|error| internal_reparse_error("operation formatting", &error))?;
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
        .map_err(|error| internal_reparse_error("root formatting", &error))?;
    validate_semantic_preservation(source, &root_formatted, syntax_format)
        .map_err(map_syntax_error)?;

    Ok(FormatResult {
        output: root_formatted,
        changed,
    })
}

/// Classify reachable mappings and sequences without formatting them.
///
/// # Errors
///
/// Returns [`FormatError::Input`] when the syntax, root shape, or declared
/// OpenAPI version is unsupported, and [`FormatError::InternalInvariant`] when
/// an inspected CST range cannot be resolved.
pub fn classify(source: &str, format: InputFormat) -> Result<ClassificationResult, FormatError> {
    let syntax_format = match format {
        InputFormat::Yaml => oafmt_syntax::InputFormat::Yaml,
        InputFormat::Json => oafmt_syntax::InputFormat::Json,
    };
    let document = inspect_document(source, syntax_format).map_err(map_syntax_error)?;
    let root = mapping_by_range(&document, document.root)?;
    let version = detect_version(root)?;
    semantic_inventory(&document, version)
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

fn semantic_inventory(
    document: &DocumentInfo,
    version: Version,
) -> Result<ClassificationResult, FormatError> {
    let root = mapping_by_range(document, document.root)?;
    let mut ranges = Vec::new();
    walk_mapping(
        document,
        version,
        root,
        SemanticKind::Object(ObjectKind::OpenApi),
        &[],
        &mut ranges,
    );
    ranges.sort_by_key(|classified| classified.range);
    Ok(ClassificationResult { version, ranges })
}

fn walk_mapping(
    document: &DocumentInfo,
    version: Version,
    mapping: &MappingInfo,
    kind: SemanticKind,
    route: &[RouteEdge],
    ranges: &mut Vec<ClassifiedRange>,
) {
    ranges.push(ClassifiedRange {
        range: mapping.range,
        kind,
        route: route.to_vec(),
    });
    if kind == SemanticKind::Opaque {
        return;
    }

    for entry in &mapping.entries {
        let Some(key) = entry.key.as_deref() else {
            continue;
        };
        let Some((child, route_edge)) = version
            .transition(kind, Edge::FixedField(key))
            .map(|child| (child, RouteEdge::FixedField(key.to_owned())))
            .or_else(|| {
                version
                    .transition(kind, Edge::DynamicMapValue(key))
                    .map(|child| (child, RouteEdge::DynamicMapValue(key.to_owned())))
            })
        else {
            continue;
        };
        let mut child_route = route.to_vec();
        child_route.push(route_edge);
        walk_value(
            document,
            version,
            ValueRanges {
                mapping: entry.value_mapping,
                sequence: entry.value_sequence,
            },
            child,
            &child_route,
            ranges,
        );
    }
}

fn walk_sequence(
    document: &DocumentInfo,
    version: Version,
    sequence: &SequenceInfo,
    kind: SemanticKind,
    route: &[RouteEdge],
    ranges: &mut Vec<ClassifiedRange>,
) {
    ranges.push(ClassifiedRange {
        range: sequence.range,
        kind,
        route: route.to_vec(),
    });
    if kind == SemanticKind::Opaque {
        return;
    }

    for (index, item) in sequence.items.iter().enumerate() {
        let Some(child) = version.transition(kind, Edge::SequenceItem(index)) else {
            continue;
        };
        let mut child_route = route.to_vec();
        child_route.push(RouteEdge::SequenceItem(index));
        walk_value(
            document,
            version,
            ValueRanges {
                mapping: item.value_mapping,
                sequence: item.value_sequence,
            },
            child,
            &child_route,
            ranges,
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct ValueRanges {
    mapping: Option<ByteRange>,
    sequence: Option<ByteRange>,
}

fn walk_value(
    document: &DocumentInfo,
    version: Version,
    value: ValueRanges,
    kind: SemanticKind,
    route: &[RouteEdge],
    ranges: &mut Vec<ClassifiedRange>,
) {
    let mapping = matches!(
        kind,
        SemanticKind::Object(_)
            | SemanticKind::ObjectOrReference(_)
            | SemanticKind::Map(_)
            | SemanticKind::Opaque
    )
    .then_some(value.mapping)
    .flatten()
    .and_then(|range| find_mapping(document, range));
    if let Some(mapping) = mapping {
        walk_mapping(document, version, mapping, kind, route, ranges);
        return;
    }
    let sequence = matches!(kind, SemanticKind::Sequence(_) | SemanticKind::Opaque)
        .then_some(value.sequence)
        .flatten()
        .and_then(|range| find_sequence(document, range));
    if let Some(sequence) = sequence {
        walk_sequence(document, version, sequence, kind, route, ranges);
    }
}

fn eligible_operation_ranges(inventory: &ClassificationResult) -> Vec<ByteRange> {
    let mut operations: Vec<_> = inventory
        .ranges
        .iter()
        .filter(|classified| classified.kind == SemanticKind::Object(ObjectKind::Operation))
        .filter(|classified| {
            matches!(
                classified.route.as_slice(),
                [
                    RouteEdge::FixedField(paths),
                    RouteEdge::DynamicMapValue(path),
                    RouteEdge::FixedField(method)
                ] if paths == "paths"
                    && path.starts_with('/')
                    && is_fixed_operation_method(method, inventory.version)
            )
        })
        .map(|classified| classified.range)
        .collect();
    operations.sort_unstable();
    operations.dedup();
    operations
}

fn is_fixed_operation_method(method: &str, version: Version) -> bool {
    matches!(
        method,
        "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
    ) || (version == Version::Oas32 && method == "query")
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

fn find_sequence(document: &DocumentInfo, range: ByteRange) -> Option<&SequenceInfo> {
    document
        .sequences
        .iter()
        .find(|sequence| sequence.range == range)
}

fn map_syntax_error(error: SyntaxError) -> FormatError {
    match error {
        SyntaxError::InvalidInput(message) => FormatError::Input(message),
        SyntaxError::InternalInvariant(message) => FormatError::InternalInvariant(message),
    }
}

fn internal_reparse_error(stage: &str, error: &SyntaxError) -> FormatError {
    FormatError::InternalInvariant(format!("{stage} produced invalid output: {error}"))
}
