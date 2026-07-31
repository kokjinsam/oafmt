//! Lossless YAML and strict JSON syntax inspection and source-slice reordering
//! for `oafmt`.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use yaml_edit::{SyntaxKind, YamlFile, YamlNode, yaml_eq};

fn string_key(node: YamlNode) -> Option<String> {
    let YamlNode::Scalar(scalar) = node else {
        return None;
    };
    let decoded = scalar.as_string();
    yaml_edit::yaml_eq(&scalar, &decoded).then_some(decoded)
}

const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_MAPPING_ENTRIES: usize = 1024;

/// Concrete input syntax selected explicitly by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    /// YAML syntax.
    Yaml,
    /// Strict JSON syntax.
    Json,
}

/// A half-open byte range in the original UTF-8 source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteRange {
    /// Inclusive start byte.
    pub start: usize,
    /// Exclusive end byte.
    pub end: usize,
}

/// One mapping entry discovered through authoritative CST ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// Complete entry range, including its trailing comma when present.
    pub range: ByteRange,
    /// Entry range excluding its trailing comma.
    pub content_range: ByteRange,
    /// Decoded string key, when the key is a string.
    pub key: Option<String>,
    /// Decoded scalar value, when the value is a string scalar.
    pub scalar_value: Option<String>,
    /// Range of a direct mapping value.
    pub value_mapping: Option<ByteRange>,
    /// Range of a direct sequence value.
    pub value_sequence: Option<ByteRange>,
    /// Whether the entry uses explicit-key YAML syntax.
    pub explicit_key: bool,
}

/// A mapping and its direct entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingInfo {
    /// Complete mapping range.
    pub range: ByteRange,
    /// Whether the mapping uses flow syntax.
    pub flow_style: bool,
    /// Direct mapping entries in source order.
    pub entries: Vec<EntryInfo>,
}

/// One sequence item discovered through authoritative CST ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceItemInfo {
    /// Complete item value range.
    pub range: ByteRange,
    /// Range of a direct mapping value.
    pub value_mapping: Option<ByteRange>,
    /// Range of a direct sequence value.
    pub value_sequence: Option<ByteRange>,
}

/// A sequence and its ordered direct items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceInfo {
    /// Complete sequence range.
    pub range: ByteRange,
    /// Direct sequence items in source order.
    pub items: Vec<SequenceItemInfo>,
}

/// Neutral CST facts used by the OpenAPI-aware orchestration layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentInfo {
    /// Root mapping range.
    pub root: ByteRange,
    /// All mappings reachable from the document.
    pub mappings: Vec<MappingInfo>,
    /// All sequences reachable from the document.
    pub sequences: Vec<SequenceInfo>,
    /// Whether reordering could affect anchors, aliases, or merge keys.
    pub anchor_order_risk: bool,
}

/// A production parse/edit failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxError {
    /// Input cannot be handled without weakening syntax or resource contracts.
    InvalidInput(String),
    /// An internal formatter assumption failed.
    InternalInvariant(String),
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) | Self::InternalInvariant(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for SyntaxError {}

/// Parse one accepted document and expose mapping relationships and CST ranges.
///
/// # Errors
///
/// Returns [`SyntaxError::InvalidInput`] when resource limits, syntax, document
/// count, or root shape are unsupported.
pub fn inspect_document(source: &str, format: InputFormat) -> Result<DocumentInfo, SyntaxError> {
    use rowan::ast::AstNode;

    check_production_resource_limits(source)?;
    if format == InputFormat::Json {
        parse_strict_json(source)?;
    }

    let file = YamlFile::from_str(source)
        .map_err(|error| SyntaxError::InvalidInput(format!("invalid input: {error}")))?;
    let documents: Vec<_> = file.documents().collect();
    if documents.len() != 1 {
        return Err(SyntaxError::InvalidInput(format!(
            "expected one document, found {}",
            documents.len()
        )));
    }
    let document = &documents[0];
    let root = document
        .as_mapping()
        .ok_or_else(|| SyntaxError::InvalidInput("document root must be a mapping".into()))?;

    if format == InputFormat::Json && !root.is_flow_style() {
        return Err(SyntaxError::InvalidInput(
            "strict JSON object root required".into(),
        ));
    }

    let mut mappings = Vec::new();
    let mut sequences = Vec::new();
    collect_mapping_info(source, &root, &mut mappings, &mut sequences)?;
    let root_range = mapping_byte_range(source, &root);
    let anchor_order_risk = document.syntax().descendants_with_tokens().any(|element| {
        element.as_token().is_some_and(|token| {
            matches!(
                token.kind(),
                SyntaxKind::ANCHOR | SyntaxKind::REFERENCE | SyntaxKind::MERGE_KEY
            )
        })
    });

    Ok(DocumentInfo {
        root: root_range,
        mappings,
        sequences,
        anchor_order_risk,
    })
}

/// Reorder known entries in each non-overlapping mapping while retaining unknown slots.
///
/// # Errors
///
/// Returns [`SyntaxError`] when a mapping cannot be reordered without loss or
/// when supplied mapping ranges violate internal formatter invariants.
pub fn reorder_mappings(
    source: &str,
    mappings: &[(&MappingInfo, &[&str])],
    format: InputFormat,
) -> Result<String, SyntaxError> {
    let mut replacements = Vec::with_capacity(mappings.len());
    for (mapping, order) in mappings {
        let replacement = render_mapping(source, mapping, order, format)?;
        if replacement != source[mapping.range.start..mapping.range.end] {
            replacements.push((mapping.range, replacement));
        }
    }
    replacements.sort_by_key(|(range, _)| *range);
    if replacements
        .windows(2)
        .any(|pair| pair[0].0.end > pair[1].0.start)
    {
        return Err(SyntaxError::InternalInvariant(
            "formatted mapping ranges overlap".into(),
        ));
    }

    let mut output = source.to_owned();
    for (range, replacement) in replacements.into_iter().rev() {
        if replacement.len() != range.end - range.start {
            return Err(SyntaxError::InternalInvariant(
                "mapping reorder changed byte length".into(),
            ));
        }
        output.replace_range(range.start..range.end, &replacement);
    }
    Ok(output)
}

/// Reparse and compare complete document semantics.
///
/// # Errors
///
/// Returns [`SyntaxError::InvalidInput`] when strict JSON parsing fails and
/// [`SyntaxError::InternalInvariant`] when formatted output changes semantics.
pub fn validate_semantic_preservation(
    before: &str,
    after: &str,
    format: InputFormat,
) -> Result<(), SyntaxError> {
    if before == after {
        return Ok(());
    }
    if format == InputFormat::Json {
        let before = parse_strict_json(before)?;
        let after = parse_strict_json(after)?;
        if before != after {
            return Err(SyntaxError::InternalInvariant(
                "JSON semantics changed during formatting".into(),
            ));
        }
        return Ok(());
    }

    let before = parse_single_yaml_mapping(before)?;
    let after = parse_single_yaml_mapping(after)?;
    if !mappings_semantically_equal(&before, &after) {
        return Err(SyntaxError::InternalInvariant(
            "YAML semantics changed during formatting".into(),
        ));
    }
    Ok(())
}

fn mappings_semantically_equal(before: &yaml_edit::Mapping, after: &yaml_edit::Mapping) -> bool {
    let before_entries: Vec<_> = before.entries().collect();
    let after_entries: Vec<_> = after.entries().collect();
    before_entries.len() == after_entries.len()
        && before_entries.iter().all(|before_entry| {
            let Some(before_key) = before_entry.key_node() else {
                return false;
            };
            let Some(before_value) = before_entry.value_node() else {
                return false;
            };
            after_entries.iter().any(|after_entry| {
                let Some(after_key) = after_entry.key_node() else {
                    return false;
                };
                let Some(after_value) = after_entry.value_node() else {
                    return false;
                };
                yaml_eq(&before_key, &after_key)
                    && yaml_nodes_semantically_equal(&before_value, &after_value)
            })
        })
}

fn yaml_nodes_semantically_equal(before: &YamlNode, after: &YamlNode) -> bool {
    match (before.as_mapping(), after.as_mapping()) {
        (Some(before), Some(after)) => mappings_semantically_equal(before, after),
        (None, None) => yaml_eq(before, after),
        _ => false,
    }
}

fn check_production_resource_limits(source: &str) -> Result<(), SyntaxError> {
    if source.len() > MAX_INPUT_BYTES {
        return Err(SyntaxError::InvalidInput(format!(
            "input exceeds {MAX_INPUT_BYTES} bytes"
        )));
    }
    if source
        .split_inclusive(['\n', '\r'])
        .any(|line| line.len() > MAX_LINE_BYTES)
    {
        return Err(SyntaxError::InvalidInput(format!(
            "a line exceeds {MAX_LINE_BYTES} bytes"
        )));
    }
    Ok(())
}

fn parse_strict_json(source: &str) -> Result<serde_json::Value, SyntaxError> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    serde_json::from_str(source)
        .map_err(|error| SyntaxError::InvalidInput(format!("invalid JSON: {error}")))
}

fn parse_single_yaml_mapping(source: &str) -> Result<yaml_edit::Mapping, SyntaxError> {
    let file = YamlFile::from_str(source).map_err(|error| {
        SyntaxError::InternalInvariant(format!("output did not parse: {error}"))
    })?;
    let documents: Vec<_> = file.documents().collect();
    if documents.len() != 1 {
        return Err(SyntaxError::InternalInvariant(
            "output document count changed".into(),
        ));
    }
    documents[0]
        .as_mapping()
        .ok_or_else(|| SyntaxError::InternalInvariant("output root is not a mapping".into()))
}

fn collect_mapping_info(
    source: &str,
    mapping: &yaml_edit::Mapping,
    mappings: &mut Vec<MappingInfo>,
    sequences: &mut Vec<SequenceInfo>,
) -> Result<(), SyntaxError> {
    use rowan::ast::AstNode;

    let mut entries = Vec::new();
    let flow_style = mapping.is_flow_style();
    for entry in mapping.entries() {
        let key = entry.key_node().and_then(string_key);
        let value = entry
            .value_node()
            .ok_or_else(|| SyntaxError::InvalidInput("mapping entry has no value".into()))?;
        let scalar_value = value.as_scalar().map_or_else(
            || {
                value.as_tagged().and_then(|tagged| {
                    (tagged.tag().as_deref() == Some("!!str"))
                        .then(|| tagged.value())
                        .flatten()
                        .map(|scalar| scalar.as_string())
                })
            },
            |scalar| {
                let decoded = scalar.as_string();
                yaml_eq(scalar, &decoded).then_some(decoded)
            },
        );
        let value_mapping = value
            .as_mapping()
            .map(|child| mapping_byte_range(source, child));
        let value_sequence = value.as_sequence().map(sequence_byte_range);
        let range = to_byte_range(entry.syntax().text_range());
        let comma_start = entry
            .syntax()
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind() == SyntaxKind::COMMA)
            .map(|token| usize::from(token.text_range().start()));
        let content_range = ByteRange {
            start: range.start,
            end: comma_start.unwrap_or(range.end),
        };
        let explicit_key = entry
            .syntax()
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .any(|token| token.kind() == SyntaxKind::QUESTION);

        entries.push(EntryInfo {
            range,
            content_range,
            key,
            scalar_value,
            value_mapping,
            value_sequence,
            explicit_key,
        });

        if let Some(child) = value.as_mapping() {
            collect_mapping_info(source, child, mappings, sequences)?;
        } else if let Some(child) = value.as_sequence() {
            collect_sequence_info(source, child, mappings, sequences)?;
        }
    }
    mappings.push(MappingInfo {
        range: mapping_byte_range(source, mapping),
        flow_style,
        entries,
    });
    Ok(())
}

fn collect_sequence_info(
    source: &str,
    sequence: &yaml_edit::Sequence,
    mappings: &mut Vec<MappingInfo>,
    sequences: &mut Vec<SequenceInfo>,
) -> Result<(), SyntaxError> {
    let mut items = Vec::new();
    for value in sequence.values() {
        let range = yaml_node_byte_range(&value);
        let value_mapping = value
            .as_mapping()
            .map(|child| mapping_byte_range(source, child));
        let value_sequence = value.as_sequence().map(sequence_byte_range);
        items.push(SequenceItemInfo {
            range,
            value_mapping,
            value_sequence,
        });

        if let Some(child) = value.as_mapping() {
            collect_mapping_info(source, child, mappings, sequences)?;
        } else if let Some(child) = value.as_sequence() {
            collect_sequence_info(source, child, mappings, sequences)?;
        }
    }
    sequences.push(SequenceInfo {
        range: sequence_byte_range(sequence),
        items,
    });
    Ok(())
}

fn yaml_node_byte_range(node: &YamlNode) -> ByteRange {
    use rowan::ast::AstNode;

    let range = match node {
        YamlNode::Scalar(node) => node.syntax().text_range(),
        YamlNode::Mapping(node) => node.syntax().text_range(),
        YamlNode::Sequence(node) => node.syntax().text_range(),
        YamlNode::Alias(node) => node.syntax().text_range(),
        YamlNode::TaggedNode(node) => node.syntax().text_range(),
    };
    to_byte_range(range)
}

fn to_byte_range(range: rowan::TextRange) -> ByteRange {
    ByteRange {
        start: range.start().into(),
        end: range.end().into(),
    }
}

fn sequence_byte_range(sequence: &yaml_edit::Sequence) -> ByteRange {
    use rowan::ast::AstNode;

    to_byte_range(sequence.syntax().text_range())
}

fn mapping_byte_range(source: &str, mapping: &yaml_edit::Mapping) -> ByteRange {
    use rowan::ast::AstNode;

    let mut range = to_byte_range(mapping.syntax().text_range());
    if !mapping.is_flow_style() {
        let line_start = source[..range.start]
            .rfind(['\n', '\r'])
            .map_or(0, |index| index + 1);
        if source[line_start..range.start]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'))
        {
            range.start = line_start;
        }
    }
    range
}

fn render_mapping(
    source: &str,
    mapping: &MappingInfo,
    order: &[&str],
    format: InputFormat,
) -> Result<String, SyntaxError> {
    if mapping.entries.is_empty() || mapping.entries.len() > MAX_MAPPING_ENTRIES {
        return Err(SyntaxError::InvalidInput(format!(
            "formatted mapping must contain 1..={MAX_MAPPING_ENTRIES} entries, found {}",
            mapping.entries.len()
        )));
    }
    match format {
        InputFormat::Yaml if mapping.flow_style => {
            return Err(SyntaxError::InvalidInput(
                "flow-style YAML is not supported at a formatted location".into(),
            ));
        }
        InputFormat::Json if !mapping.flow_style => {
            return Err(SyntaxError::InvalidInput(
                "strict JSON object required at a formatted location".into(),
            ));
        }
        _ => {}
    }
    if mapping.entries.iter().any(|entry| entry.explicit_key) {
        return Err(SyntaxError::InvalidInput(
            "explicit YAML keys are not supported at a formatted location".into(),
        ));
    }

    let mut seen = HashSet::with_capacity(mapping.entries.len());
    for entry in &mapping.entries {
        let key = entry.key.as_deref().ok_or_else(|| {
            SyntaxError::InvalidInput("formatted mappings require string keys".into())
        })?;
        if !seen.insert(key) {
            return Err(SyntaxError::InvalidInput(format!(
                "duplicate mapping key at formatted location: {key:?}"
            )));
        }
    }

    let mut desired: Vec<(usize, usize)> = mapping
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            entry.key.as_deref().and_then(|entry_key| {
                order
                    .iter()
                    .position(|ordered_key| *ordered_key == entry_key)
                    .map(|position| (index, position))
            })
        })
        .collect();
    desired.sort_by_key(|(_, position)| *position);

    let mut source_indices: Vec<usize> = (0..mapping.entries.len()).collect();
    let mut desired = desired.into_iter().map(|(index, _)| index);
    for (index, source_index) in source_indices.iter_mut().enumerate() {
        if mapping.entries[index]
            .key
            .as_deref()
            .is_some_and(|key| order.contains(&key))
        {
            *source_index = desired.next().ok_or_else(|| {
                SyntaxError::InternalInvariant(
                    "known mapping positions and entries have different counts".into(),
                )
            })?;
        }
    }
    if source_indices.iter().copied().eq(0..mapping.entries.len()) {
        return Ok(source[mapping.range.start..mapping.range.end].to_owned());
    }

    match format {
        InputFormat::Yaml => render_block_mapping(source, mapping, &source_indices),
        InputFormat::Json => render_json_mapping(source, mapping, &source_indices),
    }
}

fn render_block_mapping(
    source: &str,
    mapping: &MappingInfo,
    source_indices: &[usize],
) -> Result<String, SyntaxError> {
    let (Some(first), Some(last)) = (mapping.entries.first(), mapping.entries.last()) else {
        return Err(SyntaxError::InternalInvariant(
            "cannot render an empty YAML mapping".into(),
        ));
    };
    let suffix = &source[last.range.end..mapping.range.end];
    let mut attached = Vec::with_capacity(mapping.entries.len());
    let mut slot_indents = Vec::with_capacity(mapping.entries.len());
    attached.push("");
    slot_indents.push(&source[mapping.range.start..first.range.start]);
    for pair in mapping.entries.windows(2) {
        let gap = &source[pair[0].range.end..pair[1].range.start];
        if !gap.split_inclusive('\n').all(|line| {
            let content = line.trim_matches([' ', '\t', '\r', '\n']);
            content.is_empty() || content.starts_with('#')
        }) {
            return Err(SyntaxError::InvalidInput(
                "unsupported trivia between formatted YAML entries".into(),
            ));
        }
        let (attached_trivia, indent) = split_final_line_indent(gap);
        slot_indents.push(indent);
        attached.push(attached_trivia);
    }
    let (bodies, endings): (Vec<_>, Vec<_>) = mapping
        .entries
        .iter()
        .map(|entry| split_trailing_line_ending(&source[entry.range.start..entry.range.end]))
        .unzip();

    let mut output = String::with_capacity(mapping.range.end - mapping.range.start);
    for (slot, source_index) in source_indices.iter().copied().enumerate() {
        let trivia = attached[source_index];
        output.push_str(trivia);
        output.push_str(slot_indents[slot]);
        output.push_str(bodies[source_index]);
        output.push_str(endings[slot]);
    }
    output.push_str(suffix);
    if output.len() != mapping.range.end - mapping.range.start {
        return Err(SyntaxError::InternalInvariant(format!(
            "YAML mapping assembly changed byte length from {} to {}",
            mapping.range.end - mapping.range.start,
            output.len()
        )));
    }
    Ok(output)
}

fn split_final_line_indent(gap: &str) -> (&str, &str) {
    gap.rfind('\n').map_or_else(
        || {
            gap.rfind('\r')
                .map_or(("", gap), |last_return| gap.split_at(last_return + 1))
        },
        |last_newline| gap.split_at(last_newline + 1),
    )
}

fn split_trailing_line_ending(block: &str) -> (&str, &str) {
    block.strip_suffix("\r\n").map_or_else(
        || {
            block.strip_suffix('\n').map_or_else(
                || {
                    block
                        .strip_suffix('\r')
                        .map_or((block, ""), |block| (block, "\r"))
                },
                |block| (block, "\n"),
            )
        },
        |block| (block, "\r\n"),
    )
}

fn render_json_mapping(
    source: &str,
    mapping: &MappingInfo,
    source_indices: &[usize],
) -> Result<String, SyntaxError> {
    let (Some(first), Some(last)) = (mapping.entries.first(), mapping.entries.last()) else {
        return Err(SyntaxError::InternalInvariant(
            "cannot render an empty JSON mapping".into(),
        ));
    };
    let prefix = &source[mapping.range.start..first.content_range.start];
    let suffix = &source[last.content_range.end..mapping.range.end];
    let separators: Vec<_> = mapping
        .entries
        .windows(2)
        .map(|pair| &source[pair[0].content_range.end..pair[1].content_range.start])
        .collect();

    let mut output = String::with_capacity(mapping.range.end - mapping.range.start);
    output.push_str(prefix);
    for (slot, source_index) in source_indices.iter().copied().enumerate() {
        let entry = &mapping.entries[source_index];
        output.push_str(&source[entry.content_range.start..entry.content_range.end]);
        if let Some(separator) = separators.get(slot) {
            output.push_str(separator);
        }
    }
    output.push_str(suffix);
    if output.len() != mapping.range.end - mapping.range.start {
        return Err(SyntaxError::InternalInvariant(
            "JSON mapping assembly changed byte length".into(),
        ));
    }
    Ok(output)
}
