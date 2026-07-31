//! Pinned upstream-corpus provenance and production-path regression tests.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "corpus validation intentionally fails fast with case-specific diagnostics"
)]

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
};

use oafmt_core::{InputFormat, format};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPOSITORY: &str = "https://github.com/thim81/openapi-format";
const COMMIT: &str = "bee0bebc84221c5cf25574dd6af74c135d7efe05";
const ORIGIN: &str = "thim81/openapi-format@bee0bebc84221c5cf25574dd6af74c135d7efe05";
const CLASSIFICATIONS: &[&str] = &[
    "compatible",
    "intentional-difference",
    "upstream-bug",
    "out-of-scope",
];
const FROZEN_CASES: &[FrozenCase<'static>] = &[
    FrozenCase::new(
        "yaml-default",
        "intentional-difference",
        true,
        "yaml",
        Some("cases/yaml-default/input.yaml"),
        Some("cases/yaml-default/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "json-default",
        "intentional-difference",
        true,
        "json",
        Some("cases/json-default/input.json"),
        Some("cases/json-default/expected.oafmt.json"),
    ),
    FrozenCase::new(
        "yaml-sort-keep-comments",
        "intentional-difference",
        true,
        "yaml",
        Some("cases/yaml-sort-keep-comments/input.yaml"),
        Some("cases/yaml-sort-keep-comments/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "yaml-sort-query",
        "intentional-difference",
        true,
        "yaml",
        Some("cases/yaml-sort-query/input.yaml"),
        Some("cases/yaml-sort-query/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "yaml-default-bug-big-numbers",
        "compatible",
        true,
        "yaml",
        Some("cases/yaml-default-bug-big-numbers/input.yaml"),
        Some("cases/yaml-default-bug-big-numbers/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "json-default-bug-big-numbers",
        "compatible",
        true,
        "json",
        Some("cases/json-default-bug-big-numbers/input.json"),
        Some("cases/json-default-bug-big-numbers/expected.oafmt.json"),
    ),
    FrozenCase::new(
        "yaml-default-bug-numbers-x-tag",
        "intentional-difference",
        true,
        "yaml",
        Some("cases/yaml-default-bug-numbers-x-tag/input.yaml"),
        Some("cases/yaml-default-bug-numbers-x-tag/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "yaml-preserve-example-props",
        "intentional-difference",
        true,
        "yaml",
        Some("cases/yaml-preserve-example-props/input.yaml"),
        Some("cases/yaml-preserve-example-props/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "json-example-schemas",
        "intentional-difference",
        true,
        "json",
        Some("cases/json-example-schemas/input.json"),
        Some("cases/json-example-schemas/expected.oafmt.json"),
    ),
    FrozenCase::new(
        "yaml-default-bug-examples-properties",
        "intentional-difference",
        true,
        "yaml",
        Some("cases/yaml-default-bug-examples-properties/input.yaml"),
        Some("cases/yaml-default-bug-examples-properties/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "yaml-default-bug-nested-properties",
        "intentional-difference",
        true,
        "yaml",
        Some("cases/yaml-default-bug-nested-properties/input.yaml"),
        Some("cases/yaml-default-bug-nested-properties/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "json-example-request",
        "intentional-difference",
        true,
        "json",
        Some("cases/json-example-request/input.json"),
        Some("cases/json-example-request/expected.oafmt.json"),
    ),
    FrozenCase::new(
        "yaml-big-numbers",
        "compatible",
        true,
        "yaml",
        Some("cases/yaml-big-numbers/input.yaml"),
        Some("cases/yaml-big-numbers/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "json-no-sort",
        "intentional-difference",
        true,
        "json",
        Some("cases/json-no-sort/input.json"),
        Some("cases/json-no-sort/expected.oafmt.json"),
    ),
    FrozenCase::new(
        "yaml-default-bug-x-version-decimal",
        "compatible",
        true,
        "yaml",
        Some("cases/yaml-default-bug-x-version-decimal/input.yaml"),
        Some("cases/yaml-default-bug-x-version-decimal/expected.oafmt.yaml"),
    ),
    FrozenCase::new(
        "yaml-default-newline",
        "intentional-difference",
        false,
        "yaml",
        Some("cases/yaml-default-newline/input.yaml"),
        None,
    ),
    FrozenCase::new(
        "yaml-quote-style-detect",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "yaml-path-ref-quotes",
        "intentional-difference",
        false,
        "yaml",
        Some("cases/yaml-path-ref-quotes/input.yaml"),
        None,
    ),
    FrozenCase::new(
        "yaml-no-sort-keep-comments",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "yaml-sort-components",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "json-sort-components",
        "out-of-scope",
        false,
        "json",
        None,
        None,
    ),
    FrozenCase::new(
        "yaml-sort-component-props",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "json-sort-request-params",
        "out-of-scope",
        false,
        "json",
        None,
        None,
    ),
    FrozenCase::new(
        "yaml-sort-paths-alphabet",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "yaml-sort-paths-tags",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "yaml-filter-query-methods",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "yaml-filter-unused-components",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new("yaml-casing", "out-of-scope", false, "yaml", None, None),
    FrozenCase::new(
        "yaml-casing-properties",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new("yaml-rename", "out-of-scope", false, "yaml", None, None),
    FrozenCase::new("json-rename", "out-of-scope", false, "json", None, None),
    FrozenCase::new(
        "yaml-convert-3.0-3.2",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
    FrozenCase::new(
        "json-convert-3.1",
        "out-of-scope",
        false,
        "json",
        None,
        None,
    ),
    FrozenCase::new("overlay-combi", "out-of-scope", false, "yaml", None, None),
    FrozenCase::new("yaml-ref-quotes", "out-of-scope", false, "yaml", None, None),
    FrozenCase::new(
        "_split/{snap.yaml,snap_station.yaml,snap_station_id.yaml}",
        "out-of-scope",
        false,
        "yaml",
        None,
        None,
    ),
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FrozenCase<'a> {
    id: &'a str,
    classification: &'a str,
    executable: bool,
    format: &'a str,
    copied_input: Option<&'a str>,
    expected_path: Option<&'a str>,
}

impl<'a> FrozenCase<'a> {
    const fn new(
        id: &'a str,
        classification: &'a str,
        executable: bool,
        format: &'a str,
        copied_input: Option<&'a str>,
        expected_path: Option<&'a str>,
    ) -> Self {
        Self {
            id,
            classification,
            executable,
            format,
            copied_input,
            expected_path,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    upstream: Upstream,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Upstream {
    repository: String,
    commit: String,
    license: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    origin: String,
    classification: String,
    executable: bool,
    format: String,
    upstream_paths: Vec<String>,
    copied_input: Option<String>,
    input_sha256: Option<String>,
    expected_path: Option<String>,
    expected_owner: Option<String>,
    upstream_output_paths: Vec<String>,
    upstream_output_is_oracle: bool,
    coverage: String,
    notes: String,
}

#[test]
fn pinned_manifest_and_executable_cases_are_complete_and_exact() {
    let root = corpus_root();
    let manifest_text =
        fs::read_to_string(root.join("manifest.toml")).expect("manifest should be readable");
    let manifest: Manifest = toml::from_str(&manifest_text).expect("manifest should parse");

    assert_eq!(manifest.upstream.repository, REPOSITORY);
    assert_eq!(manifest.upstream.commit, COMMIT);
    assert_eq!(manifest.upstream.license, "MIT");
    validate_aggregate_totals(&manifest);

    let mut ids = HashSet::new();
    let mut copied_inputs = HashSet::new();
    let mut expected_paths = HashSet::new();
    for case in &manifest.cases {
        assert!(ids.insert(&case.id), "duplicate case id {}", case.id);
        if let Some(path) = &case.copied_input {
            assert!(
                copied_inputs.insert(path),
                "duplicate copied input path {path}"
            );
        }
        if let Some(path) = &case.expected_path {
            assert!(
                expected_paths.insert(path),
                "duplicate expected path {path}"
            );
        }
        assert_eq!(case.origin, ORIGIN, "{} origin", case.id);
        assert!(
            CLASSIFICATIONS.contains(&case.classification.as_str()),
            "{} classification {}",
            case.id,
            case.classification
        );
        assert!(
            matches!(case.format.as_str(), "yaml" | "json"),
            "{} format",
            case.id
        );
        assert!(
            !case.upstream_paths.is_empty(),
            "{} upstream provenance",
            case.id
        );
        assert!(
            !case.coverage.trim().is_empty() && !case.notes.trim().is_empty(),
            "{} coverage and notes",
            case.id
        );
        assert!(
            !case.upstream_output_is_oracle,
            "{} must not use upstream output as an oracle",
            case.id
        );
        for output in &case.upstream_output_paths {
            assert!(
                !output.trim().is_empty(),
                "{} upstream output path",
                case.id
            );
        }

        validate_copied_input(&root, case);
        if case.classification == "out-of-scope" {
            assert!(
                case.copied_input.is_none()
                    && case.input_sha256.is_none()
                    && case.expected_path.is_none()
                    && case.expected_owner.is_none(),
                "{} metadata-only file fields",
                case.id
            );
        }
        if case.executable {
            run_executable_case(&root, case);
        } else {
            assert!(
                case.expected_path.is_none() && case.expected_owner.is_none(),
                "{} non-executable local expectation",
                case.id
            );
        }
    }

    let expected_cases = FROZEN_CASES.iter().copied().collect::<BTreeSet<_>>();
    let actual_cases = manifest
        .cases
        .iter()
        .map(FrozenCase::from)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_cases, expected_cases, "frozen case contract");
    validate_fixture_inventory(&root);
}

fn validate_aggregate_totals(manifest: &Manifest) {
    assert_eq!(manifest.cases.len(), 36, "classified case total");
    assert_eq!(
        manifest.cases.iter().filter(|case| case.executable).count(),
        15,
        "executable case total"
    );
    assert_eq!(
        manifest
            .cases
            .iter()
            .filter(|case| case.copied_input.is_some())
            .count(),
        17,
        "copied input total"
    );
    for (classification, expected) in [
        ("compatible", 4),
        ("intentional-difference", 13),
        ("upstream-bug", 0),
        ("out-of-scope", 19),
    ] {
        assert_eq!(
            manifest
                .cases
                .iter()
                .filter(|case| case.classification == classification)
                .count(),
            expected,
            "{classification} case total"
        );
    }
}

impl<'a> From<&'a Case> for FrozenCase<'a> {
    fn from(case: &'a Case) -> Self {
        Self::new(
            &case.id,
            &case.classification,
            case.executable,
            &case.format,
            case.copied_input.as_deref(),
            case.expected_path.as_deref(),
        )
    }
}

fn validate_fixture_inventory(root: &Path) {
    let mut expected = FROZEN_CASES
        .iter()
        .flat_map(|case| [case.copied_input, case.expected_path])
        .flatten()
        .map(PathBuf::from)
        .collect::<BTreeSet<_>>();
    expected.extend(["README.md", "LICENSE", "manifest.toml"].map(PathBuf::from));
    assert_eq!(expected.len(), 35, "frozen fixture inventory total");

    let mut actual = BTreeSet::new();
    collect_regular_files(root, root, &mut actual);
    assert_eq!(actual, expected, "exact fixture filesystem inventory");
}

fn collect_regular_files(root: &Path, directory: &Path, files: &mut BTreeSet<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("fixture directory {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "fixture directory entry in {}: {error}",
                directory.display()
            )
        });
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("fixture entry type {}: {error}", path.display()));
        assert!(
            !file_type.is_symlink(),
            "unexpected fixture symlink {}",
            path.display()
        );
        if file_type.is_dir() {
            collect_regular_files(root, &path, files);
        } else {
            assert!(
                file_type.is_file(),
                "unexpected non-regular fixture entry {}",
                path.display()
            );
            files.insert(
                path.strip_prefix(root)
                    .expect("fixture entry should be below corpus root")
                    .to_path_buf(),
            );
        }
    }
}

fn validate_copied_input(root: &Path, case: &Case) {
    match (&case.copied_input, &case.input_sha256) {
        (Some(input_path), Some(expected_hash)) => {
            let input = fs::read(root.join(input_path))
                .unwrap_or_else(|error| panic!("{} copied input: {error}", case.id));
            let actual_hash = format!("{:x}", Sha256::digest(&input));
            assert_eq!(
                actual_hash, *expected_hash,
                "{} copied input SHA-256",
                case.id
            );
        }
        (None, None) => {
            assert!(
                !case.executable,
                "{} executable case requires a copied input",
                case.id
            );
        }
        _ => panic!(
            "{} copied_input and input_sha256 must both be present or absent",
            case.id
        ),
    }
}

fn run_executable_case(root: &Path, case: &Case) {
    let input_path = case
        .copied_input
        .as_ref()
        .expect("executable case should have copied input");
    let expected_path = case
        .expected_path
        .as_ref()
        .expect("executable case should have local expected output");
    assert_eq!(
        case.expected_owner.as_deref(),
        Some("oafmt"),
        "{} expected owner",
        case.id
    );

    let input = fs::read_to_string(root.join(input_path))
        .unwrap_or_else(|error| panic!("{} input UTF-8: {error}", case.id));
    let expected = fs::read_to_string(root.join(expected_path))
        .unwrap_or_else(|error| panic!("{} expected output: {error}", case.id));
    let input_format = match case.format.as_str() {
        "yaml" => InputFormat::Yaml,
        "json" => InputFormat::Json,
        other => panic!("{} unsupported format {other}", case.id),
    };

    let first = format(&input, input_format)
        .unwrap_or_else(|error| panic!("{} production formatting: {error}", case.id));
    let repeat = format(&input, input_format)
        .unwrap_or_else(|error| panic!("{} repeat formatting: {error}", case.id));
    let second = format(&first.output, input_format)
        .unwrap_or_else(|error| panic!("{} idempotence formatting: {error}", case.id));

    assert_eq!(first.output, expected, "{} exact local output", case.id);
    assert_eq!(
        first.changed,
        input != expected,
        "{} changed status",
        case.id
    );
    assert_eq!(repeat, first, "{} determinism", case.id);
    assert_eq!(second.output, expected, "{} idempotence", case.id);
    assert!(!second.changed, "{} second-pass changed status", case.id);
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/upstream/openapi-format")
}
