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
