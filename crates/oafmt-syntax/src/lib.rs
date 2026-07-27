//! Phase 1 experiment for lossless YAML mapping-entry movement.
//!
//! This is deliberately a narrow spike, not a formatter API. It accepts one
//! YAML document whose root is an implicit-key block mapping with unique string
//! keys and uniquely reconcilable entry text, moves one complete entry by
//! index, and returns source slices reassembled without re-emitting YAML
//! values.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use yaml_edit::{AsYaml, MappingEntry, SyntaxKind, YamlFile, YamlNode, yaml_eq};

/// Maximum accepted input size for the Phase 1 experiment.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// Maximum accepted physical line size for the Phase 1 experiment.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Maximum number of entries in the mapping moved by the experiment.
pub const MAX_MAPPING_ENTRIES: usize = 1024;

/// A safe-rejection or caller error produced by the Phase 1 experiment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveError {
    /// The input exceeds the byte limit.
    InputTooLarge,
    /// A physical line exceeds the byte limit.
    LineTooLong,
    /// The YAML parser rejected the input.
    InvalidYaml(String),
    /// The input is not exactly one YAML document.
    DocumentCount(usize),
    /// The single document does not contain a block mapping at its root.
    RootBlockMappingRequired,
    /// The mapping is empty or exceeds the entry-count limit.
    EntryCount(usize),
    /// The root mapping contains a key that is not a YAML string.
    NonStringKey,
    /// The root mapping contains semantically duplicate keys.
    DuplicateKey(String),
    /// The requested key does not occur in the mapping.
    KeyNotFound(String),
    /// The requested destination index is outside the mapping.
    IndexOutOfBounds {
        /// Requested destination index.
        to: usize,
        /// Number of entries in the mapping.
        len: usize,
    },
    /// Reordering could change anchor/alias/merge resolution order.
    AnchorOrderRisk,
    /// CST entry ownership cannot be reconciled with the original source.
    UntrustworthySourceRange,
}

impl fmt::Display for MoveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => write!(formatter, "input exceeds {MAX_INPUT_BYTES} bytes"),
            Self::LineTooLong => write!(formatter, "a line exceeds {MAX_LINE_BYTES} bytes"),
            Self::InvalidYaml(error) => write!(formatter, "invalid YAML: {error}"),
            Self::DocumentCount(count) => {
                write!(formatter, "expected one YAML document, found {count}")
            }
            Self::RootBlockMappingRequired => {
                write!(formatter, "a root block mapping is required")
            }
            Self::EntryCount(count) => write!(
                formatter,
                "mapping must contain 1..={MAX_MAPPING_ENTRIES} entries, found {count}"
            ),
            Self::NonStringKey => write!(formatter, "mapping keys must be strings"),
            Self::DuplicateKey(key) => write!(formatter, "duplicate mapping key: {key:?}"),
            Self::KeyNotFound(key) => write!(formatter, "mapping key not found: {key:?}"),
            Self::IndexOutOfBounds { to, len } => {
                write!(
                    formatter,
                    "destination index {to} is outside mapping length {len}"
                )
            }
            Self::AnchorOrderRisk => write!(
                formatter,
                "entry movement is rejected when anchors, aliases, or merge keys are present"
            ),
            Self::UntrustworthySourceRange => {
                write!(formatter, "CST entry source ranges cannot be trusted")
            }
        }
    }
}

impl std::error::Error for MoveError {}

/// Move one complete entry in a single root block mapping.
///
/// `key` selects a unique string key and `to` is its zero-based destination
/// index. Standalone comments and blank lines immediately preceding a
/// non-first entry travel with that entry. Directives, document markers,
/// leading file trivia, and trailing file trivia remain fixed. Reapplying the
/// same movement is a no-op, so successful moves are second-pass idempotent.
/// Explicit-key layouts and ambiguous source-text reconciliation are rejected.
/// Output reparsing and semantic comparison are structural backstops; they do
/// not prove comment ownership.
pub fn move_root_mapping_entry(source: &str, key: &str, to: usize) -> Result<String, MoveError> {
    check_resource_limits(source)?;

    let file =
        YamlFile::from_str(source).map_err(|error| MoveError::InvalidYaml(error.to_string()))?;
    let documents: Vec<_> = file.documents().collect();
    if documents.len() != 1 {
        return Err(MoveError::DocumentCount(documents.len()));
    }

    let document = &documents[0];
    let mapping = document
        .as_mapping()
        .filter(|mapping| !mapping.is_flow_style())
        .ok_or(MoveError::RootBlockMappingRequired)?;

    let entries: Vec<_> = mapping.entries().collect();
    if entries.is_empty() || entries.len() > MAX_MAPPING_ENTRIES {
        return Err(MoveError::EntryCount(entries.len()));
    }
    if entries
        .iter()
        .map(ToString::to_string)
        .any(|entry| is_explicit_key_entry(&entry))
    {
        return Err(MoveError::UntrustworthySourceRange);
    }
    if to >= entries.len() {
        return Err(MoveError::IndexOutOfBounds {
            to,
            len: entries.len(),
        });
    }

    let mut seen_keys = HashSet::with_capacity(entries.len());
    let mut from = None;
    for (index, entry) in entries.iter().enumerate() {
        let entry_key = string_key(entry.key_node().ok_or(MoveError::NonStringKey)?)
            .ok_or(MoveError::NonStringKey)?;
        if !seen_keys.insert(entry_key.clone()) {
            return Err(MoveError::DuplicateKey(entry_key));
        }
        if entry_key == key {
            from = Some(index);
        }
    }
    let from = from.ok_or_else(|| MoveError::KeyNotFound(key.to_string()))?;

    if from != to && has_anchor_order_risk(document) {
        return Err(MoveError::AnchorOrderRisk);
    }

    let entry_texts: Vec<String> = entries.iter().map(ToString::to_string).collect();
    let (prefix, blocks, suffix) = split_entry_blocks(source, &entry_texts)?;
    let (mut blocks, line_endings): (Vec<_>, Vec<_>) =
        blocks.into_iter().map(split_trailing_line_ending).unzip();

    if from != to {
        let moved = blocks.remove(from);
        blocks.insert(to, moved);
    }

    let mut output = String::with_capacity(source.len());
    output.push_str(prefix);
    for (block, line_ending) in blocks.into_iter().zip(line_endings) {
        output.push_str(block);
        output.push_str(line_ending);
    }
    output.push_str(suffix);

    if output.len() != source.len() {
        return Err(MoveError::UntrustworthySourceRange);
    }
    validate_rendered_output(&entries, source, &output)?;

    Ok(output)
}

fn check_resource_limits(source: &str) -> Result<(), MoveError> {
    if source.len() > MAX_INPUT_BYTES {
        return Err(MoveError::InputTooLarge);
    }
    if source
        .split_inclusive(['\n', '\r'])
        .any(|line| line.len() > MAX_LINE_BYTES)
    {
        return Err(MoveError::LineTooLong);
    }
    Ok(())
}

fn string_key(node: YamlNode) -> Option<String> {
    let YamlNode::Scalar(scalar) = node else {
        return None;
    };
    let decoded = scalar.as_string();
    yaml_edit::yaml_eq(&scalar, &decoded).then_some(decoded)
}

fn has_anchor_order_risk(document: &yaml_edit::Document) -> bool {
    let Some(node) = document.as_node() else {
        return false;
    };
    node.descendants_with_tokens().any(|element| {
        element.as_token().is_some_and(|token| {
            matches!(
                token.kind(),
                SyntaxKind::ANCHOR | SyntaxKind::REFERENCE | SyntaxKind::MERGE_KEY
            )
        })
    })
}

fn is_explicit_key_entry(entry: &str) -> bool {
    entry
        .strip_prefix('?')
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

fn is_permitted_inter_entry_trivia(trivia: &str) -> bool {
    (trivia.is_empty() || trivia.starts_with('\n') || trivia.starts_with("\r\n"))
        && trivia.split_inclusive('\n').all(|line| {
            let content = line.trim_matches([' ', '\t', '\r', '\n']);
            content.is_empty() || content.starts_with('#')
        })
}

fn split_trailing_line_ending(block: &str) -> (&str, &str) {
    if let Some(block) = block.strip_suffix("\r\n") {
        (block, "\r\n")
    } else if let Some(block) = block.strip_suffix('\n') {
        (block, "\n")
    } else if let Some(block) = block.strip_suffix('\r') {
        (block, "\r")
    } else {
        (block, "")
    }
}

fn split_entry_blocks<'a>(
    source: &'a str,
    entry_texts: &[String],
) -> Result<(&'a str, Vec<&'a str>, &'a str), MoveError> {
    let mut spans = Vec::with_capacity(entry_texts.len());
    let mut cursor = 0;

    for entry in entry_texts {
        let mut matches = source.match_indices(entry);
        let start = matches
            .next()
            .map(|(start, _)| start)
            .ok_or(MoveError::UntrustworthySourceRange)?;
        if matches.next().is_some() || start < cursor {
            return Err(MoveError::UntrustworthySourceRange);
        }
        let end = start + entry.len();
        if !spans.is_empty() && !is_permitted_inter_entry_trivia(&source[cursor..start]) {
            return Err(MoveError::UntrustworthySourceRange);
        }
        spans.push((start, end));
        cursor = end;
    }

    let prefix = &source[..spans[0].0];
    let mut blocks = Vec::with_capacity(spans.len());
    blocks.push(&source[spans[0].0..spans[0].1]);
    for index in 1..spans.len() {
        blocks.push(&source[spans[index - 1].1..spans[index].1]);
    }
    let suffix = &source[spans.last().expect("entries are non-empty").1..];
    Ok((prefix, blocks, suffix))
}

fn has_same_cst_text(left: &YamlNode, right: &YamlNode) -> bool {
    let left = left.to_string();
    let right = right.to_string();
    left == right
}

fn validate_rendered_output(
    original_entries: &[MappingEntry],
    original_source: &str,
    output: &str,
) -> Result<(), MoveError> {
    let rendered = YamlFile::from_str(output).map_err(|_| MoveError::UntrustworthySourceRange)?;
    let documents: Vec<_> = rendered.documents().collect();
    if documents.len() != 1 {
        return Err(MoveError::UntrustworthySourceRange);
    }
    let mapping = documents[0]
        .as_mapping()
        .filter(|mapping| !mapping.is_flow_style())
        .ok_or(MoveError::UntrustworthySourceRange)?;
    let rendered_entries: Vec<_> = mapping.entries().collect();
    if rendered_entries.len() != original_entries.len() {
        return Err(MoveError::UntrustworthySourceRange);
    }

    let mut rendered_keys = HashSet::with_capacity(rendered_entries.len());
    for entry in &rendered_entries {
        let key = string_key(
            entry
                .key_node()
                .ok_or(MoveError::UntrustworthySourceRange)?,
        )
        .ok_or(MoveError::UntrustworthySourceRange)?;
        if !rendered_keys.insert(key) {
            return Err(MoveError::UntrustworthySourceRange);
        }
    }

    for original in original_entries {
        let original_key = original
            .key_node()
            .ok_or(MoveError::UntrustworthySourceRange)?;
        let original_value = original
            .value_node()
            .ok_or(MoveError::UntrustworthySourceRange)?;
        let pair_preserved = rendered_entries.iter().any(|entry| {
            let Some(rendered_key) = entry.key_node() else {
                return false;
            };
            let Some(rendered_value) = entry.value_node() else {
                return false;
            };
            let value_preserved = yaml_eq(&original_value, &rendered_value)
                || (output == original_source
                    && has_same_cst_text(&original_value, &rendered_value));
            yaml_eq(&original_key, &rendered_key) && value_preserved
        });
        if !pair_preserved {
            return Err(MoveError::UntrustworthySourceRange);
        }
    }

    Ok(())
}

/// Concrete input syntax selected explicitly by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Yaml,
    Json,
}

/// A half-open byte range in the original UTF-8 source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

/// One mapping entry discovered through authoritative CST ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    pub range: ByteRange,
    pub content_range: ByteRange,
    pub key: Option<String>,
    pub scalar_value: Option<String>,
    pub value_mapping: Option<ByteRange>,
    pub explicit_key: bool,
}

/// A mapping and its direct entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingInfo {
    pub range: ByteRange,
    pub flow_style: bool,
    pub entries: Vec<EntryInfo>,
}

/// Neutral CST facts used by the OpenAPI-aware orchestration layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentInfo {
    pub root: ByteRange,
    pub mappings: Vec<MappingInfo>,
    pub anchor_order_risk: bool,
}

/// A production parse/edit failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxError {
    InvalidInput(String),
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
    collect_mapping_info(source, &root, &mut mappings)?;
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
        anchor_order_risk,
    })
}

/// Reorder known entries in each non-overlapping mapping while retaining unknown slots.
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
) -> Result<(), SyntaxError> {
    use rowan::ast::AstNode;

    let mut entries = Vec::new();
    let flow_style = mapping.is_flow_style();
    for entry in mapping.entries() {
        let key = entry.key_node().and_then(string_key);
        let value = entry
            .value_node()
            .ok_or_else(|| SyntaxError::InvalidInput("mapping entry has no value".into()))?;
        let scalar_value = if let Some(scalar) = value.as_scalar() {
            let decoded = scalar.as_string();
            yaml_eq(scalar, &decoded).then_some(decoded)
        } else {
            value.as_tagged().and_then(|tagged| {
                (tagged.tag().as_deref() == Some("!!str"))
                    .then(|| tagged.value())
                    .flatten()
                    .map(|scalar| scalar.as_string())
            })
        };
        let value_mapping = value
            .as_mapping()
            .map(|child| mapping_byte_range(source, child));
        let range = to_byte_range(entry.syntax().text_range());
        let comma_start = entry
            .syntax()
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::COMMA)
            .map(|token| usize::from(token.text_range().start()));
        let content_range = ByteRange {
            start: range.start,
            end: comma_start.unwrap_or(range.end),
        };
        let explicit_key = entry
            .syntax()
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .any(|token| token.kind() == SyntaxKind::QUESTION);

        entries.push(EntryInfo {
            range,
            content_range,
            key,
            scalar_value,
            value_mapping,
            explicit_key,
        });

        if let Some(child) = value.as_mapping() {
            collect_mapping_info(source, child, mappings)?;
        }
    }
    mappings.push(MappingInfo {
        range: mapping_byte_range(source, mapping),
        flow_style,
        entries,
    });
    Ok(())
}

fn to_byte_range(range: rowan::TextRange) -> ByteRange {
    ByteRange {
        start: range.start().into(),
        end: range.end().into(),
    }
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
                "duplicate mapping key at formatted location: {:?}",
                key
            )));
        }
    }

    let mut desired: Vec<usize> = mapping
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.key.as_deref().is_some_and(|key| order.contains(&key)))
        .map(|(index, _)| index)
        .collect();
    desired.sort_by_key(|index| {
        order
            .iter()
            .position(|key| Some(*key) == mapping.entries[*index].key.as_deref())
            .expect("known entry has an order")
    });

    let mut source_indices: Vec<usize> = (0..mapping.entries.len()).collect();
    let mut desired = desired.into_iter();
    for (index, source_index) in source_indices.iter_mut().enumerate() {
        if mapping.entries[index]
            .key
            .as_deref()
            .is_some_and(|key| order.contains(&key))
        {
            *source_index = desired
                .next()
                .expect("known positions and entries have equal counts");
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
    let first = mapping.entries.first().expect("mapping is non-empty");
    let last = mapping.entries.last().expect("mapping is non-empty");
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
    if let Some(last_newline) = gap.rfind('\n') {
        gap.split_at(last_newline + 1)
    } else if let Some(last_return) = gap.rfind('\r') {
        gap.split_at(last_return + 1)
    } else {
        ("", gap)
    }
}

fn render_json_mapping(
    source: &str,
    mapping: &MappingInfo,
    source_indices: &[usize],
) -> Result<String, SyntaxError> {
    let first = mapping.entries.first().expect("mapping is non-empty");
    let last = mapping.entries.last().expect("mapping is non-empty");
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
