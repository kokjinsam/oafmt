//! Deterministic production-path properties for formatting boundaries.
#![expect(
    clippy::panic,
    clippy::too_many_lines,
    reason = "the generated-document renderer and failure report stay together as one test oracle"
)]

use std::fmt::Write as _;

use oafmt_core::{InputFormat, format};
use oafmt_oas::Version;
use proptest::{
    collection::vec,
    prelude::*,
    test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner},
};

const CASES_PER_CONTEXT: u32 = 64;

const YAML_30_SEED: [u8; 32] = [
    0x30, 0x59, 0x41, 0x4d, 0x4c, 0x03, 0x00, 0x04, 0x61, 0x2f, 0x8c, 0xd1, 0x14, 0x77, 0x39, 0xa0,
    0xbb, 0x5e, 0x18, 0x43, 0x92, 0xef, 0x07, 0x6d, 0xc4, 0x21, 0x9a, 0x35, 0x70, 0xde, 0x4b, 0x11,
];
const YAML_31_SEED: [u8; 32] = [
    0x31, 0x59, 0x41, 0x4d, 0x4c, 0x03, 0x01, 0x02, 0x72, 0x1e, 0x9d, 0xc0, 0x25, 0x66, 0x48, 0xb1,
    0xaa, 0x4f, 0x09, 0x52, 0x83, 0xfe, 0x16, 0x7c, 0xd5, 0x30, 0x8b, 0x24, 0x61, 0xcf, 0x5a, 0x20,
];
const YAML_32_SEED: [u8; 32] = [
    0x32, 0x59, 0x41, 0x4d, 0x4c, 0x03, 0x02, 0x00, 0x43, 0x6f, 0xae, 0xf3, 0x16, 0x55, 0x7b, 0x82,
    0x99, 0x7c, 0x3a, 0x61, 0xb0, 0xcd, 0x25, 0x4f, 0xe6, 0x03, 0xb8, 0x17, 0x52, 0xfc, 0x69, 0x33,
];
const JSON_30_SEED: [u8; 32] = [
    0x30, 0x4a, 0x53, 0x4f, 0x4e, 0x03, 0x00, 0x04, 0x94, 0x1d, 0x70, 0xca, 0x3b, 0xe8, 0x56, 0x02,
    0xaf, 0x63, 0x29, 0xd4, 0x85, 0x10, 0xbc, 0x47, 0xf2, 0x6a, 0x05, 0x9e, 0x31, 0x7d, 0xc0, 0x44,
];
const JSON_31_SEED: [u8; 32] = [
    0x31, 0x4a, 0x53, 0x4f, 0x4e, 0x03, 0x01, 0x02, 0x85, 0x0c, 0x61, 0xdb, 0x2a, 0xf9, 0x47, 0x13,
    0xbe, 0x72, 0x38, 0xc5, 0x94, 0x01, 0xad, 0x56, 0xe3, 0x7b, 0x14, 0x8f, 0x20, 0x6c, 0xd1, 0x55,
];
const JSON_32_SEED: [u8; 32] = [
    0x32, 0x4a, 0x53, 0x4f, 0x4e, 0x03, 0x02, 0x00, 0xb6, 0x3f, 0x52, 0xe8, 0x19, 0xca, 0x74, 0x20,
    0x8d, 0x41, 0x0b, 0xf6, 0xa7, 0x32, 0x9e, 0x65, 0xd0, 0x48, 0x27, 0xbc, 0x13, 0x5f, 0xe2, 0x66,
];

#[derive(Clone, Debug)]
struct GeneratedDocument {
    version: Version,
    format: InputFormat,
    input: String,
    expected: String,
    sentinels: Vec<SentinelBlock>,
}

#[derive(Clone, Debug)]
struct SentinelBlock {
    name: String,
    exact: String,
}

#[derive(Clone, Debug)]
struct Entry {
    key: String,
    body: String,
    leading: String,
}

#[derive(Clone, Debug)]
struct Shape {
    path_count: usize,
    eligible_count: usize,
    crlf: bool,
    root_unknown_slot: u8,
    operation_unknown_slot: u8,
    root_ranks: Vec<u8>,
    operation_ranks: Vec<u8>,
    separator_styles: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
struct RenderStyle {
    format: InputFormat,
    line_ending: &'static str,
    separator_style: u8,
}

#[test]
fn yaml_30_properties() {
    run_context(Version::Oas30, InputFormat::Yaml, YAML_30_SEED);
}

#[test]
fn yaml_31_properties() {
    run_context(Version::Oas31, InputFormat::Yaml, YAML_31_SEED);
}

#[test]
fn yaml_32_properties() {
    run_context(Version::Oas32, InputFormat::Yaml, YAML_32_SEED);
}

#[test]
fn json_30_properties() {
    run_context(Version::Oas30, InputFormat::Json, JSON_30_SEED);
}

#[test]
fn json_31_properties() {
    run_context(Version::Oas31, InputFormat::Json, JSON_31_SEED);
}

#[test]
fn json_32_properties() {
    run_context(Version::Oas32, InputFormat::Json, JSON_32_SEED);
}

fn run_context(version: Version, input_format: InputFormat, seed: [u8; 32]) {
    let config = Config {
        cases: CASES_PER_CONTEXT,
        failure_persistence: None,
        ..Config::default()
    };
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &seed);
    let mut runner = TestRunner::new_with_rng(config, rng);
    let result = runner.run(&document_strategy(version, input_format), |document| {
        check_document(&document)
    });

    if let Err(error) = result {
        let seed_hex = seed
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                let _ = write!(output, "{byte:02x}");
                output
            });
        panic!(
            "production-path property failed\nformat: {input_format:?}\nversion: {version:?}\nseed: {seed_hex}\nminimized GeneratedDocument:\n{error:#?}"
        );
    }
}

fn check_document(document: &GeneratedDocument) -> Result<(), TestCaseError> {
    let first = format(&document.input, document.format)
        .map_err(|error| TestCaseError::fail(format!("formatting failed: {error}")))?;
    let repeat = format(&document.input, document.format)
        .map_err(|error| TestCaseError::fail(format!("repeat formatting failed: {error}")))?;
    let second = format(&first.output, document.format)
        .map_err(|error| TestCaseError::fail(format!("second formatting failed: {error}")))?;

    prop_assert_eq!(
        &first.output,
        &document.expected,
        "source-chunk oracle mismatch for {:?} {:?}",
        document.format,
        document.version
    );
    prop_assert_eq!(
        first.changed,
        document.input != document.expected,
        "changed flag"
    );
    prop_assert_eq!(&repeat, &first, "repeated original formatting");
    prop_assert_eq!(&second.output, &document.expected, "idempotent output");
    prop_assert!(!second.changed, "formatted output changed on second pass");
    prop_assert_eq!(
        document.input.len(),
        document.expected.len(),
        "oracle changed byte length"
    );
    prop_assert_eq!(
        document.input.len(),
        first.output.len(),
        "formatter changed byte length"
    );

    for sentinel in &document.sentinels {
        prop_assert!(!sentinel.exact.is_empty(), "{} is empty", sentinel.name);
        prop_assert_eq!(
            document.input.matches(&sentinel.exact).count(),
            1,
            "{} is not unique in input",
            sentinel.name
        );
        prop_assert_eq!(
            document.expected.matches(&sentinel.exact).count(),
            1,
            "{} did not survive the oracle uniquely",
            sentinel.name
        );
        prop_assert_eq!(
            first.output.matches(&sentinel.exact).count(),
            1,
            "{} did not survive production formatting uniquely",
            sentinel.name
        );
    }

    Ok(())
}

fn document_strategy(
    version: Version,
    input_format: InputFormat,
) -> impl Strategy<Value = GeneratedDocument> {
    (
        1_usize..=3,
        1_usize..=2,
        any::<bool>(),
        0_u8..=7,
        0_u8..=7,
        vec(any::<u8>(), 7),
        vec(any::<u8>(), 24),
        vec(0_u8..=5, 10),
    )
        .prop_map(
            move |(
                path_count,
                eligible_count,
                crlf,
                root_unknown_slot,
                operation_unknown_slot,
                root_ranks,
                operation_ranks,
                separator_styles,
            )| {
                build_document(
                    version,
                    input_format,
                    &Shape {
                        path_count,
                        eligible_count,
                        crlf,
                        root_unknown_slot,
                        operation_unknown_slot,
                        root_ranks,
                        operation_ranks,
                        separator_styles,
                    },
                )
            },
        )
}

fn build_document(version: Version, input_format: InputFormat, shape: &Shape) -> GeneratedDocument {
    let line_ending = if shape.crlf { "\r\n" } else { "\n" };
    let root_style = RenderStyle {
        format: input_format,
        line_ending,
        separator_style: shape.separator_styles[0],
    };
    let id = format!("{}_{}", version_name(version), format_name(input_format));
    let mut sentinels = Vec::new();

    let (paths_input, paths_expected) =
        build_paths(version, root_style, shape, &id, &mut sentinels);
    let components = build_components(root_style, &id, &mut sentinels);

    let mut known_entries = vec![
        scalar_entry("openapi", version_value(version), root_style, true),
        value_entry(
            "info",
            yaml_or_json(
                root_style,
                "{title: Generated, version: '1'}",
                r#"{"title":"Generated","version":"1"}"#,
            ),
            root_style,
        ),
        value_entry("paths", &paths_input, root_style),
        value_entry("components", &components, root_style),
        value_entry(
            "tags",
            yaml_or_json(
                root_style,
                "[{name: generated}, {name: property}]",
                r#"[{"name":"generated"},{"name":"property"}]"#,
            ),
            root_style,
        ),
    ];
    if version != Version::Oas30 {
        known_entries.push(value_entry(
            "webhooks",
            &build_webhooks(root_style, &id, &mut sentinels),
            root_style,
        ));
    }
    known_entries.sort_by_key(|entry| policy_position(version.root_order(), &entry.key));
    let mut source_entries = permute_known(known_entries, &shape.root_ranks);

    let root_extension = opaque_entry(
        "x-root-sentinel",
        &format!("ROOT_EXTENSION_{id}"),
        root_style,
        0,
    );
    sentinels.push(SentinelBlock {
        name: "root extension".into(),
        exact: root_extension.body.clone(),
    });
    let unknown_slot = usize::from(shape.root_unknown_slot) % (source_entries.len() + 1);
    source_entries.insert(unknown_slot, root_extension);
    attach_yaml_trivia(&mut source_entries, root_style, 0, "root");

    let mut formatted_entries = source_entries.clone();
    let Some(paths_entry) = formatted_entries
        .iter_mut()
        .find(|entry| entry.key == "paths")
    else {
        panic!("every generated document has paths");
    };
    paths_entry.body = entry_body("paths", &paths_expected, root_style);

    let input_indices: Vec<_> = (0..source_entries.len()).collect();
    let expected_indices = oracle_indices(&source_entries, version.root_order());
    let mut input = render_mapping(&source_entries, &input_indices, root_style, 0);
    let mut expected = render_mapping(&formatted_entries, &expected_indices, root_style, 0);
    if input_format == InputFormat::Yaml {
        input.push_str(line_ending);
        expected.push_str(line_ending);
    }

    GeneratedDocument {
        version,
        format: input_format,
        input,
        expected,
        sentinels,
    }
}

fn build_paths(
    version: Version,
    root_style: RenderStyle,
    shape: &Shape,
    id: &str,
    sentinels: &mut Vec<SentinelBlock>,
) -> (String, String) {
    let style = RenderStyle {
        separator_style: shape.separator_styles[1],
        ..root_style
    };
    let mut input_paths = Vec::with_capacity(shape.path_count);
    let mut expected_paths = Vec::with_capacity(shape.path_count);

    for path_index in 0..shape.path_count {
        let path = format!("/generated-{path_index}");
        let (input_path_item, expected_path_item) = if path_index == 0 {
            build_primary_path_item(version, style, shape, id, sentinels)
        } else {
            let marker = format!("PATH_EXTENSION_{id}_{path_index}");
            let value = yaml_or_json(
                style,
                &format!(
                    "    delete: [responses, summary]{}    x-path-{path_index}: {{marker: {marker}}}",
                    style.line_ending
                ),
                &format!(
                    r#"{{"delete":["responses","summary"],"x-path-{path_index}":{{"marker":"{marker}"}}}}"#
                ),
            )
            .to_owned();
            (value.clone(), value)
        };
        input_paths.push(value_entry(&path, &input_path_item, style));
        expected_paths.push(value_entry(&path, &expected_path_item, style));
    }

    attach_yaml_trivia(&mut input_paths, style, 2, "path");
    copy_leading(&input_paths, &mut expected_paths);
    let indices: Vec<_> = (0..input_paths.len()).collect();
    (
        render_mapping(&input_paths, &indices, style, 2),
        render_mapping(&expected_paths, &indices, style, 2),
    )
}

fn build_primary_path_item(
    version: Version,
    style: RenderStyle,
    shape: &Shape,
    id: &str,
    sentinels: &mut Vec<SentinelBlock>,
) -> (String, String) {
    let mut input_entries = vec![
        scalar_entry(
            "put",
            yaml_or_json(style, "[responses, summary]", r#"["responses","summary"]"#),
            style,
            false,
        ),
        scalar_entry(
            "trace",
            yaml_or_json(
                style,
                "[description, responses]",
                r#"["description","responses"]"#,
            ),
            style,
            false,
        ),
    ];
    let unexpected_start = input_entries[0].body.clone();
    let unexpected_end = input_entries[1].body.clone();

    if version == Version::Oas32 {
        input_entries.push(value_entry(
            "additionalOperations",
            &build_additional_operations(style, id, sentinels),
            style,
        ));
    }

    let (get_input, get_expected) =
        build_operation(version, style, shape, id, "get", 0, true, true, sentinels);
    input_entries.push(value_entry("get", &get_input, style));

    let query_is_eligible = version == Version::Oas32;
    let (query_input, query_expected) = build_operation(
        version,
        style,
        shape,
        id,
        "query",
        1,
        query_is_eligible,
        false,
        sentinels,
    );
    let query_entry = value_entry("query", &query_input, style);
    if !query_is_eligible {
        sentinels.push(SentinelBlock {
            name: "query Operation outside the 3.2 formatting boundary".into(),
            exact: query_entry.body.clone(),
        });
    }
    input_entries.push(query_entry);

    if version != Version::Oas32 && shape.eligible_count == 2 {
        let (post_input, _) =
            build_operation(version, style, shape, id, "post", 2, true, false, sentinels);
        input_entries.push(value_entry("post", &post_input, style));
    }

    attach_yaml_trivia(&mut input_entries, style, 4, "path-item");
    if style.format == InputFormat::Yaml {
        input_entries[1].leading.clear();
    }
    let mut expected_entries = input_entries.clone();
    replace_entry_value(&mut expected_entries, "get", &get_expected, style);
    replace_entry_value(
        &mut expected_entries,
        "query",
        if query_is_eligible {
            &query_expected
        } else {
            &query_input
        },
        style,
    );
    if version != Version::Oas32 && shape.eligible_count == 2 {
        let (_, post_expected) = build_operation(
            version,
            style,
            shape,
            id,
            "post",
            2,
            true,
            false,
            &mut Vec::new(),
        );
        replace_entry_value(&mut expected_entries, "post", &post_expected, style);
    }

    let indices: Vec<_> = (0..input_entries.len()).collect();
    let input = render_mapping(&input_entries, &indices, style, 4);
    let expected = render_mapping(&expected_entries, &indices, style, 4);
    let method_order = match style.format {
        InputFormat::Yaml => format!(
            "{unexpected_start}{}    {unexpected_end}",
            style.line_ending
        ),
        InputFormat::Json => {
            let separator = json_separator(style.separator_style, 0, 4);
            format!("{unexpected_start}{separator}{unexpected_end}")
        }
    };
    sentinels.push(SentinelBlock {
        name: "unexpected direct shapes and Path Item method-key order".into(),
        exact: method_order,
    });

    (input, expected)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the arguments identify one generated Operation and its formatting boundary"
)]
fn build_operation(
    version: Version,
    parent_style: RenderStyle,
    shape: &Shape,
    id: &str,
    method: &str,
    operation_index: usize,
    eligible: bool,
    include_callback: bool,
    sentinels: &mut Vec<SentinelBlock>,
) -> (String, String) {
    let style = RenderStyle {
        separator_style: shape.separator_styles[2 + operation_index],
        ..parent_style
    };
    let operation_id = format!("{id}_{method}");
    let mut known = vec![
        scalar_entry("summary", &format!("summary_{operation_id}"), style, true),
        scalar_entry(
            "description",
            &format!("description_{operation_id}"),
            style,
            true,
        ),
        scalar_entry(
            "operationId",
            &format!("operation_{operation_id}"),
            style,
            true,
        ),
        value_entry("responses", empty_object(style), style),
    ];
    if include_callback {
        known.push(value_entry(
            "callbacks",
            &build_callback(style, &operation_id, sentinels),
            style,
        ));
    }
    known.sort_by_key(|entry| policy_position(version.operation_order(), &entry.key));

    let rank_start = operation_index * 6;
    let rank_end = (rank_start + 6).min(shape.operation_ranks.len());
    let mut source = permute_known(known, &shape.operation_ranks[rank_start..rank_end]);
    let extension = opaque_entry(
        &format!("x-operation-{method}"),
        &format!("OPERATION_EXTENSION_{operation_id}"),
        style,
        6,
    );
    sentinels.push(SentinelBlock {
        name: format!("{method} Operation extension"),
        exact: extension.body.clone(),
    });
    let unknown_slot =
        (usize::from(shape.operation_unknown_slot) + operation_index) % (source.len() + 1);
    source.insert(unknown_slot, extension);
    attach_yaml_trivia(&mut source, style, 6, method);

    let input_indices: Vec<_> = (0..source.len()).collect();
    let expected_indices = if eligible {
        oracle_indices(&source, version.operation_order())
    } else {
        input_indices.clone()
    };
    (
        render_mapping(&source, &input_indices, style, 6),
        render_mapping(&source, &expected_indices, style, 6),
    )
}

fn build_callback(
    style: RenderStyle,
    operation_id: &str,
    sentinels: &mut Vec<SentinelBlock>,
) -> String {
    let marker = format!("CALLBACK_OPERATION_{operation_id}");
    let exact = match style.format {
        InputFormat::Yaml => join_lines(
            &[
                "post:".into(),
                "              responses: {}".into(),
                format!("              summary: {marker}"),
            ],
            style.line_ending,
        ),
        InputFormat::Json => {
            format!(r#""post":{{"responses":{{}},"summary":"{marker}"}}"#)
        }
    };
    sentinels.push(SentinelBlock {
        name: "callback Operation".into(),
        exact: exact.clone(),
    });
    match style.format {
        InputFormat::Yaml => join_lines(
            &[
                "        after:".into(),
                "          '{$request.body#/url}':".into(),
                format!("            {exact}"),
            ],
            style.line_ending,
        ),
        InputFormat::Json => {
            format!(r#"{{"after":{{"{{$request.body#/url}}":{{{exact}}}}}}}"#)
        }
    }
}

fn build_webhooks(style: RenderStyle, id: &str, sentinels: &mut Vec<SentinelBlock>) -> String {
    let marker = format!("WEBHOOK_OPERATION_{id}");
    let exact = match style.format {
        InputFormat::Yaml => join_lines(
            &[
                "post:".into(),
                "      responses: {}".into(),
                format!("      summary: {marker}"),
            ],
            style.line_ending,
        ),
        InputFormat::Json => {
            format!(r#""post":{{"responses":{{}},"summary":"{marker}"}}"#)
        }
    };
    sentinels.push(SentinelBlock {
        name: "webhook Operation".into(),
        exact: exact.clone(),
    });
    match style.format {
        InputFormat::Yaml => {
            format!("  generated_event:{}    {exact}", style.line_ending)
        }
        InputFormat::Json => format!(r#"{{"generated_event":{{{exact}}}}}"#),
    }
}

fn build_additional_operations(
    style: RenderStyle,
    id: &str,
    sentinels: &mut Vec<SentinelBlock>,
) -> String {
    let marker = format!("ADDITIONAL_OPERATION_{id}");
    let exact = match style.format {
        InputFormat::Yaml => join_lines(
            &[
                "      COPY:".into(),
                "        responses: {}".into(),
                format!("        summary: {marker}"),
            ],
            style.line_ending,
        ),
        InputFormat::Json => {
            format!(r#""COPY":{{"responses":{{}},"summary":"{marker}"}}"#)
        }
    };
    sentinels.push(SentinelBlock {
        name: "3.2 additionalOperations Operation".into(),
        exact: exact.clone(),
    });
    match style.format {
        InputFormat::Yaml => exact,
        InputFormat::Json => format!("{{{exact}}}"),
    }
}

fn build_components(style: RenderStyle, id: &str, sentinels: &mut Vec<SentinelBlock>) -> String {
    match style.format {
        InputFormat::Yaml => {
            let schema_properties = join_lines(
                &[
                    "properties:".into(),
                    "        zebra_property:".into(),
                    format!("          description: SCHEMA_PROPERTIES_{id}_Z"),
                    "          type: string".into(),
                    "        alpha_property:".into(),
                    format!("          description: SCHEMA_PROPERTIES_{id}_A"),
                    "          type: integer".into(),
                ],
                style.line_ending,
            );
            let examples = join_lines(
                &[
                    "examples:".into(),
                    "    generated_example:".into(),
                    "      value:".into(),
                    format!("        responses: EXAMPLES_{id}_RESPONSES"),
                    format!("        summary: EXAMPLES_{id}_SUMMARY"),
                ],
                style.line_ending,
            );
            let component_operation = join_lines(
                &[
                    "get:".into(),
                    "        responses: {}".into(),
                    format!("        summary: COMPONENT_OPERATION_{id}"),
                ],
                style.line_ending,
            );
            sentinels.extend([
                SentinelBlock {
                    name: "schema properties".into(),
                    exact: schema_properties.clone(),
                },
                SentinelBlock {
                    name: "examples".into(),
                    exact: examples.clone(),
                },
                SentinelBlock {
                    name: "component Path Item Operation".into(),
                    exact: component_operation.clone(),
                },
            ]);
            join_lines(
                &[
                    "  schemas:".into(),
                    "    GeneratedSchema:".into(),
                    "      type: object".into(),
                    format!("      {schema_properties}"),
                    format!("  {examples}"),
                    "  pathItems:".into(),
                    "    SharedPath:".into(),
                    format!("      {component_operation}"),
                ],
                style.line_ending,
            )
        }
        InputFormat::Json => {
            let schema_properties = format!(
                r#""properties":{{"zebra_property":{{"description":"SCHEMA_PROPERTIES_{id}_Z","type":"string"}},"alpha_property":{{"description":"SCHEMA_PROPERTIES_{id}_A","type":"integer"}}}}"#
            );
            let examples = format!(
                r#""examples":{{"generated_example":{{"value":{{"responses":"EXAMPLES_{id}_RESPONSES","summary":"EXAMPLES_{id}_SUMMARY"}}}}}}"#
            );
            let component_operation =
                format!(r#""get":{{"responses":{{}},"summary":"COMPONENT_OPERATION_{id}"}}"#);
            sentinels.extend([
                SentinelBlock {
                    name: "schema properties".into(),
                    exact: schema_properties.clone(),
                },
                SentinelBlock {
                    name: "examples".into(),
                    exact: examples.clone(),
                },
                SentinelBlock {
                    name: "component Path Item Operation".into(),
                    exact: component_operation.clone(),
                },
            ]);
            format!(
                r#"{{"schemas":{{"GeneratedSchema":{{"type":"object",{schema_properties}}}}},{examples},"pathItems":{{"SharedPath":{{{component_operation}}}}}}}"#
            )
        }
    }
}

fn opaque_entry(key: &str, marker: &str, style: RenderStyle, indent: usize) -> Entry {
    let value = match style.format {
        InputFormat::Yaml => format!(
            "{}marker: {marker}{}{}responses: opaque_{}{}{}summary: opaque_{}",
            " ".repeat(indent + 2),
            style.line_ending,
            " ".repeat(indent + 2),
            marker,
            style.line_ending,
            " ".repeat(indent + 2),
            marker
        ),
        InputFormat::Json => format!(
            r#"{{"marker":"{marker}","responses":"opaque_{marker}","summary":"opaque_{marker}"}}"#
        ),
    };
    value_entry(key, &value, style)
}

fn scalar_entry(key: &str, value: &str, style: RenderStyle, quote_json: bool) -> Entry {
    let body = match style.format {
        InputFormat::Yaml => format!("{key}: {value}"),
        InputFormat::Json => {
            let value = if quote_json {
                format!(r#""{value}""#)
            } else {
                value.to_owned()
            };
            format!(r#""{key}":{value}"#)
        }
    };
    Entry {
        key: key.into(),
        body,
        leading: String::new(),
    }
}

fn value_entry(key: &str, value: &str, style: RenderStyle) -> Entry {
    Entry {
        key: key.into(),
        body: entry_body(key, value, style),
        leading: String::new(),
    }
}

fn entry_body(key: &str, value: &str, style: RenderStyle) -> String {
    match style.format {
        InputFormat::Yaml => {
            if value.starts_with(['{', '[']) || !value.contains(style.line_ending) {
                format!("{key}: {value}")
            } else {
                format!("{key}:{}{value}", style.line_ending)
            }
        }
        InputFormat::Json => format!(r#""{key}":{value}"#),
    }
}

fn render_mapping(
    entries: &[Entry],
    source_indices: &[usize],
    style: RenderStyle,
    indent: usize,
) -> String {
    match style.format {
        InputFormat::Yaml => {
            let mut output = String::new();
            for (slot, source_index) in source_indices.iter().copied().enumerate() {
                if slot != 0 {
                    output.push_str(style.line_ending);
                }
                let entry = &entries[source_index];
                output.push_str(&entry.leading);
                output.push_str(&" ".repeat(indent));
                output.push_str(&entry.body);
            }
            output
        }
        InputFormat::Json => {
            let mut output = String::from("{");
            for (slot, source_index) in source_indices.iter().copied().enumerate() {
                if slot != 0 {
                    output.push_str(&json_separator(style.separator_style, slot - 1, indent));
                }
                output.push_str(&entries[source_index].body);
            }
            output.push('}');
            output
        }
    }
}

fn oracle_indices(entries: &[Entry], policy: &[&str]) -> Vec<usize> {
    let mut known: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            policy
                .iter()
                .position(|key| *key == entry.key)
                .map(|position| (index, position))
        })
        .collect();
    known.sort_by_key(|(_, position)| *position);

    let mut desired = known.into_iter().map(|(index, _)| index);
    let mut indices: Vec<_> = (0..entries.len()).collect();
    for (slot, source_index) in indices.iter_mut().enumerate() {
        if policy.contains(&entries[slot].key.as_str()) {
            let Some(desired_index) = desired.next() else {
                panic!("known entry slots and source entries have equal counts");
            };
            *source_index = desired_index;
        }
    }
    indices
}

fn permute_known(mut entries: Vec<Entry>, ranks: &[u8]) -> Vec<Entry> {
    let mut indices: Vec<_> = (0..entries.len()).collect();
    indices.sort_by_key(|index| (ranks.get(*index).copied().unwrap_or(0), *index));
    if indices.iter().copied().eq(0..entries.len()) {
        indices.rotate_left(1);
    }
    let mut permuted = Vec::with_capacity(entries.len());
    for index in indices {
        permuted.push(entries[index].clone());
    }
    entries.clear();
    permuted
}

fn attach_yaml_trivia(entries: &mut [Entry], style: RenderStyle, indent: usize, context: &str) {
    if style.format != InputFormat::Yaml {
        return;
    }
    for (index, entry) in entries.iter_mut().enumerate().skip(1) {
        entry.leading = format!(
            "{}# generated {context} entry {index}: {}{}",
            " ".repeat(indent),
            entry.key,
            style.line_ending
        );
    }
}

fn copy_leading(source: &[Entry], target: &mut [Entry]) {
    for (source_entry, target_entry) in source.iter().zip(target) {
        target_entry.leading.clone_from(&source_entry.leading);
    }
}

fn replace_entry_value(entries: &mut [Entry], key: &str, value: &str, style: RenderStyle) {
    let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) else {
        panic!("generated Path Item contains the requested method");
    };
    entry.body = entry_body(key, value, style);
}

fn json_separator(style: u8, slot: usize, indent: usize) -> String {
    match style % 3 {
        0 => ",".into(),
        1 if slot % 2 == 0 => ", ".into(),
        1 => format!(",\n{}", " ".repeat(indent)),
        _ if slot % 2 == 0 => format!(",\r\n{}", " ".repeat(indent + 1)),
        _ => ", ".into(),
    }
}

fn policy_position(policy: &[&str], key: &str) -> usize {
    let Some(position) = policy.iter().position(|candidate| *candidate == key) else {
        panic!("generated known entry is present in its public policy table");
    };
    position
}

const fn yaml_or_json<'a>(style: RenderStyle, yaml: &'a str, json: &'a str) -> &'a str {
    match style.format {
        InputFormat::Yaml => yaml,
        InputFormat::Json => json,
    }
}

const fn empty_object(style: RenderStyle) -> &'static str {
    yaml_or_json(style, "{}", "{}")
}

fn join_lines(lines: &[String], line_ending: &str) -> String {
    lines.join(line_ending)
}

const fn version_value(version: Version) -> &'static str {
    match version {
        Version::Oas30 => "3.0.4",
        Version::Oas31 => "3.1.2",
        Version::Oas32 => "3.2.0",
    }
}

const fn version_name(version: Version) -> &'static str {
    match version {
        Version::Oas30 => "oas30",
        Version::Oas31 => "oas31",
        Version::Oas32 => "oas32",
    }
}

const fn format_name(input_format: InputFormat) -> &'static str {
    match input_format {
        InputFormat::Yaml => "yaml",
        InputFormat::Json => "json",
    }
}
