//! End-to-end CLI stream, exit, discovery, preflight, and mutation contracts.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test setup and assertions intentionally fail fast on fixture or subprocess failure"
)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn missing_input_is_an_argument_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_oafmt"))
        .output()
        .expect("oafmt binary should start");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn stdout_check_and_diff_modes_have_exact_stream_and_exit_behavior() {
    let directory = TestDir::new();
    let changed = directory.write("api.yaml", input_yaml());
    let unchanged = directory.write("ordered.yaml", expected_yaml());

    let output = command().arg(&changed).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_yaml().as_bytes());
    assert!(output.stderr.is_empty());

    let output = command().args(["--check"]).arg(&changed).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "oafmt: formatting changes required: {}\n",
            changed.display()
        )
    );

    let output = command()
        .args(["--check"])
        .arg(&unchanged)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let output = command().args(["--diff"]).arg(&changed).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let diff = String::from_utf8(output.stdout).unwrap();
    assert!(diff.starts_with(&format!(
        "--- {}\n+++ {}\n@@ ",
        changed.display(),
        changed.display()
    )));
    assert!(diff.contains("-paths:\n"));
    assert!(diff.contains("+openapi: 3.1.0\n"));

    let output = command().args(["--diff"]).arg(&unchanged).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn check_processes_multiple_files_in_sorted_deduplicated_order() {
    let directory = TestDir::new();
    let first = directory.write("a.yaml", input_yaml());
    let last = directory.write("z.yaml", input_yaml());
    let equivalent_first = directory.path().join(".").join("a.yaml");

    let output = command()
        .arg("--check")
        .arg(&last)
        .arg(&equivalent_first)
        .arg(&first)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "oafmt: formatting changes required: {}\n\
             oafmt: formatting changes required: {}\n",
            equivalent_first.display(),
            last.display()
        )
    );
}

#[test]
fn duplicate_selection_is_independent_of_argument_order() {
    let directory = TestDir::new();
    directory.write("a.yaml", input_yaml());

    let permutations = [
        ["a.yaml", "./a.yaml", "a.yaml/"],
        ["a.yaml", "a.yaml/", "./a.yaml"],
        ["./a.yaml", "a.yaml", "a.yaml/"],
        ["./a.yaml", "a.yaml/", "a.yaml"],
        ["a.yaml/", "a.yaml", "./a.yaml"],
        ["a.yaml/", "./a.yaml", "a.yaml"],
    ];
    let outputs: Vec<_> = permutations
        .iter()
        .map(|paths| {
            let output = command()
                .current_dir(directory.path())
                .arg("--check")
                .args(paths)
                .output()
                .unwrap();
            (output.status.code(), output.stdout, output.stderr)
        })
        .collect();

    for output in &outputs[1..] {
        assert_eq!(output, &outputs[0]);
    }
    assert_eq!(outputs[0].0, Some(1));
    assert!(outputs[0].1.is_empty());
    assert_eq!(
        String::from_utf8(outputs[0].2.clone()).unwrap(),
        "oafmt: formatting changes required: ./a.yaml\n"
    );

    for paths in [["a.yaml", "a.yaml/"], ["a.yaml/", "a.yaml"]] {
        let output = command()
            .current_dir(directory.path())
            .arg("--check")
            .args(paths)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "oafmt: formatting changes required: a.yaml\n"
        );
    }
}

#[test]
fn absolute_input_does_not_require_an_available_current_directory() {
    let directory = TestDir::new();
    let input = directory.write("api.yaml", input_yaml());
    let deleted_current_directory = directory.path().join("deleted-current-directory");
    fs::create_dir(&deleted_current_directory).unwrap();

    let output = Command::new("sh")
        .arg("-c")
        .arg("cd \"$1\" && rmdir \"$1\" && exec \"$2\" --check \"$3\"")
        .arg("sh")
        .arg(&deleted_current_directory)
        .arg(env!("CARGO_BIN_EXE_oafmt"))
        .arg(&input)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("oafmt: formatting changes required: {}\n", input.display())
    );
}

#[test]
fn diff_concatenates_multiple_changed_files_in_sorted_order() {
    let directory = TestDir::new();
    let first = directory.write("a.yaml", input_yaml());
    let last = directory.write("z.json", input_json());

    let output = command()
        .arg("--diff")
        .arg(&last)
        .arg(&first)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let first_header = format!("--- {}\n+++ {}\n", first.display(), first.display());
    let last_header = format!("--- {}\n+++ {}\n", last.display(), last.display());
    assert_eq!(stdout.matches("--- ").count(), 2);
    assert!(stdout.find(&first_header).unwrap() < stdout.find(&last_header).unwrap());
}

#[test]
fn diff_stdout_closed_pipe_returns_exit_two() {
    let directory = TestDir::new();
    let prefix = input_json().trim_end().strip_suffix('}').unwrap();
    let large_items = vec!["  \"x\""; 20_000].join(",\n");
    let source = format!("{prefix},\n\"x-large\": [\n{large_items}\n]\n}}\n");
    let input = directory.write("api.json", &source);
    let mut child = command()
        .arg("--diff")
        .arg(&input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());

    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("cannot write stdout"), "{stderr}");
}

#[test]
fn diff_keeps_stdout_and_stderr_deterministic_with_invalid_inputs() {
    let directory = TestDir::new();
    let changed = directory.write("a-changed.yaml", input_yaml());
    let malformed = directory.write("b-malformed.yaml", "openapi: [\n");
    let missing = directory.path().join("c-missing.yaml");

    let output = command()
        .arg("--diff")
        .arg(&missing)
        .arg(&malformed)
        .arg(&changed)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("--- ").count(), 1);
    assert!(stdout.starts_with(&format!("--- {}\n", changed.display())));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.find(&malformed.display().to_string()).unwrap()
            < stderr.find(&missing.display().to_string()).unwrap()
    );
    assert_eq!(stderr.lines().count(), 2);
}

#[test]
fn write_formats_multiple_files_only_after_complete_preflight() {
    let directory = TestDir::new();
    let first = directory.write("a.yaml", input_yaml());
    let last = directory.write("z.json", input_json());

    let output = command()
        .arg("--write")
        .arg(&last)
        .arg(&first)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(&first).unwrap(), expected_yaml());
    assert_eq!(fs::read_to_string(&last).unwrap(), expected_json());

    fs::write(&first, input_yaml()).unwrap();
    fs::write(&last, input_json()).unwrap();
    let missing = directory.path().join("zz-missing.yaml");
    let output = command()
        .arg("--write")
        .arg(&first)
        .arg(&missing)
        .arg(&last)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains(&missing.display().to_string())
    );
    assert_eq!(fs::read_to_string(&first).unwrap(), input_yaml());
    assert_eq!(fs::read_to_string(&last).unwrap(), input_json());
}

#[test]
fn multiple_stdout_inputs_and_stdin_with_files_are_argument_errors() {
    let directory = TestDir::new();
    let first = directory.write("a.yaml", input_yaml());
    let last = directory.write("z.yaml", input_yaml());

    for args in [
        vec![first.to_str().unwrap(), last.to_str().unwrap()],
        vec![
            "--check",
            "--stdin-filepath",
            "virtual.yaml",
            first.to_str().unwrap(),
        ],
    ] {
        let output = command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn check_aggregates_mixed_results_and_diagnostics_in_path_order() {
    let directory = TestDir::new();
    let changed = directory.write("a-changed.yaml", input_yaml());
    let unchanged = directory.write("b-unchanged.yaml", expected_yaml());
    let malformed = directory.write("c-malformed.yaml", "openapi: [\n");
    let missing = directory.path().join("d-missing.yaml");
    let unsupported = directory.write("e-unsupported.txt", input_yaml());

    let output = command()
        .arg("--check")
        .arg(&unsupported)
        .arg(&missing)
        .arg(&malformed)
        .arg(&unchanged)
        .arg(&changed)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let positions = [
        stderr.find(&changed.display().to_string()).unwrap(),
        stderr.find(&malformed.display().to_string()).unwrap(),
        stderr.find(&missing.display().to_string()).unwrap(),
        stderr.find(&unsupported.display().to_string()).unwrap(),
    ];
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!stderr.contains(&unchanged.display().to_string()));
    assert_eq!(stderr.lines().count(), 4);
}

#[test]
fn directory_discovery_recurses_for_default_openapi_basenames_only() {
    let directory = TestDir::new();
    let first = directory.write("openapi.yaml", input_yaml());
    let nested = directory.write("nested/openapi.json", input_json());
    directory.write("nested/api.yaml", input_yaml());
    directory.write("nested/openapi.txt", input_yaml());
    directory.write(".git/openapi.yaml", input_yaml());

    let output = command()
        .arg("--check")
        .arg(directory.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "oafmt: formatting changes required: {}\n\
             oafmt: formatting changes required: {}\n",
            nested.display(),
            first.display()
        )
    );
}

#[test]
fn nearest_config_replaces_defaults_and_exclude_wins() {
    let directory = TestDir::new();
    directory.write(
        "oafmt.toml",
        "[discovery]\ninclude = [\"**/api.*\"]\nexclude = [\"**/skip/**\"]\n",
    );
    directory.write("project/api.yaml", input_yaml());
    directory.write("project/openapi.yaml", input_yaml());
    directory.write("project/skip/api.json", input_json());

    let output = command()
        .current_dir(directory.path().join("project"))
        .args(["--check", "."])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "oafmt: formatting changes required: api.yaml\n"
    );
}

#[test]
fn explicit_relative_config_patterns_are_anchored_to_its_directory() {
    let directory = TestDir::new();
    directory.write(
        "configuration/oafmt.toml",
        "[discovery]\ninclude = [\"specs/*.yaml\"]\n",
    );
    directory.write("configuration/specs/api.yaml", input_yaml());
    directory.write("configuration/specs/openapi.json", input_json());

    let output = command()
        .current_dir(directory.path())
        .args([
            "--check",
            "--config",
            "configuration/oafmt.toml",
            "configuration/specs",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "oafmt: formatting changes required: configuration/specs/api.yaml\n"
    );
}

#[test]
fn native_globs_deduplicate_overlaps_and_explicit_spelling_wins() {
    let directory = TestDir::new();
    directory.write("a/openapi.yaml", input_yaml());
    directory.write("b/openapi.json", input_json());

    let selectors = [
        vec!["**/openapi.*", ".", "./a/openapi.yaml"],
        vec!["./a/openapi.yaml", ".", "**/openapi.*"],
    ];
    let outputs: Vec<_> = selectors
        .iter()
        .map(|selectors| {
            let output = command()
                .current_dir(directory.path())
                .arg("--check")
                .args(selectors)
                .output()
                .unwrap();
            (output.status.code(), output.stdout, output.stderr)
        })
        .collect();

    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0].0, Some(1));
    assert!(outputs[0].1.is_empty());
    assert_eq!(
        String::from_utf8(outputs[0].2.clone()).unwrap(),
        "oafmt: formatting changes required: ./a/openapi.yaml\n\
         oafmt: formatting changes required: b/openapi.json\n"
    );
}

#[test]
fn native_glob_uses_supported_dialect_and_config_exclude_applies_last() {
    let directory = TestDir::new();
    directory.write("oafmt.toml", "[discovery]\nexclude = [\"a/**\"]\n");
    directory.write("a/api.yml", input_yaml());
    directory.write("b/api.yml", input_yaml());
    directory.write("b/api.json", input_json());

    let output = command()
        .current_dir(directory.path())
        .args(["--check", "[ab]/api.?ml"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "oafmt: formatting changes required: b/api.yml\n"
    );
}

#[test]
fn braces_in_character_classes_work_in_cli_and_config_patterns() {
    let directory = TestDir::new();
    directory.write("file{.yaml", input_yaml());
    directory.write("file}.yaml", input_yaml());

    let cli = command()
        .current_dir(directory.path())
        .args(["--check", "file[{].yaml", "file[}].yaml"])
        .output()
        .unwrap();
    assert_eq!(cli.status.code(), Some(1));
    assert!(cli.stdout.is_empty());
    assert_eq!(
        String::from_utf8(cli.stderr).unwrap(),
        "oafmt: formatting changes required: file{.yaml\n\
         oafmt: formatting changes required: file}.yaml\n"
    );

    directory.write(
        "oafmt.toml",
        "[discovery]\ninclude = [\"file[{].yaml\", \"file[}].yaml\"]\n",
    );
    let config = command()
        .current_dir(directory.path())
        .args(["--check", "."])
        .output()
        .unwrap();
    assert_eq!(config.status.code(), Some(1));
    assert!(config.stdout.is_empty());
    assert_eq!(
        String::from_utf8(config.stderr).unwrap(),
        "oafmt: formatting changes required: file{.yaml\n\
         oafmt: formatting changes required: file}.yaml\n"
    );

    let literal = command()
        .current_dir(directory.path())
        .args(["--check", "file{.yaml"])
        .output()
        .unwrap();
    assert_eq!(literal.status.code(), Some(1));
    assert!(literal.stdout.is_empty());
    assert_eq!(
        String::from_utf8(literal.stderr).unwrap(),
        "oafmt: formatting changes required: file{.yaml\n"
    );
}

#[test]
fn leading_dot_components_are_normalized_in_globs_and_config_patterns() {
    let directory = TestDir::new();
    directory.write(
        "oafmt.toml",
        "[discovery]\nexclude = [\"./generated/**\"]\n",
    );
    directory.write("kept/openapi.yaml", input_yaml());
    directory.write("generated/openapi.yaml", input_yaml());

    let output = command()
        .current_dir(directory.path())
        .args(["--check", "./**/openapi.yaml"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "oafmt: formatting changes required: kept/openapi.yaml\n"
    );
}

#[test]
fn slashes_and_dots_inside_character_classes_are_preserved_for_wax() {
    let directory = TestDir::new();
    directory.write("file..yaml", input_yaml());

    let cli = command()
        .current_dir(directory.path())
        .args(["--check", "file[a/./b].yaml"])
        .output()
        .unwrap();
    assert_eq!(cli.status.code(), Some(1));
    assert!(cli.stdout.is_empty());
    assert_eq!(
        String::from_utf8(cli.stderr).unwrap(),
        "oafmt: formatting changes required: file..yaml\n"
    );

    directory.write(
        "oafmt.toml",
        "[discovery]\ninclude = [\"file[a/./b].yaml\"]\n",
    );
    let config = command()
        .current_dir(directory.path())
        .args(["--check", "."])
        .output()
        .unwrap();
    assert_eq!(config.status.code(), Some(1));
    assert!(config.stdout.is_empty());
    assert_eq!(
        String::from_utf8(config.stderr).unwrap(),
        "oafmt: formatting changes required: file..yaml\n"
    );
}

#[test]
fn equivalent_selector_spellings_preserve_check_and_diff_output() {
    let directory = TestDir::new();
    directory.write("specs/a/api.yaml", input_yaml());
    directory.write("specs/b/api.yaml", input_yaml());
    let absolute = format!("{}/specs/**/*.yaml", directory.path().display());
    let equivalent_absolute = format!("{}//specs/./**//./*.yaml", directory.path().display());
    let selectors = [
        "specs/**/*.yaml".to_owned(),
        "./specs//./**//./*.yaml".to_owned(),
        absolute,
        equivalent_absolute,
    ];

    for mode in ["--check", "--diff"] {
        let outputs = selectors
            .iter()
            .map(|selector| {
                let output = command()
                    .current_dir(directory.path())
                    .arg(mode)
                    .arg(selector)
                    .output()
                    .unwrap();
                (output.status.code(), output.stdout, output.stderr)
            })
            .collect::<Vec<_>>();

        assert_eq!(outputs[0], outputs[1], "{mode} relative");
        assert_eq!(outputs[2], outputs[3], "{mode} absolute");
    }
}

#[test]
fn invariant_parent_prefixes_and_character_classes_use_partitioned_matching() {
    let directory = TestDir::new();
    fs::create_dir_all(directory.path().join("workspace/current")).unwrap();
    directory.write("workspace/shared/filea.yaml", input_yaml());
    directory.write("workspace/shared/fileb.yaml", input_yaml());
    directory.write("workspace/shared/filed.yaml", input_yaml());
    directory.write("workspace/shared/file{.yaml", input_yaml());

    let output = command()
        .current_dir(directory.path().join("workspace/current"))
        .args([
            "--check",
            "../shared/file[a-c].yaml",
            "./../shared//file[a].yaml",
            "../shared/file[{].yaml",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 3);
    assert!(
        stderr
            .lines()
            .any(|line| line.ends_with("/shared/filea.yaml"))
    );
    assert!(
        stderr
            .lines()
            .any(|line| line.ends_with("/shared/fileb.yaml"))
    );
    assert!(
        stderr
            .lines()
            .any(|line| line.ends_with("/shared/file{.yaml"))
    );
}

#[test]
fn equivalent_config_patterns_normalize_separators_and_dot_components() {
    let directory = TestDir::new();
    directory.write("specs/kept/api.yaml", input_yaml());
    directory.write("specs/generated/api.yaml", input_yaml());
    let configurations = [
        "[discovery]\ninclude = [\"specs/**/*.yaml\"]\nexclude = [\"specs/generated/**\"]\n",
        "[discovery]\ninclude = [\"./specs//./**//*.yaml\"]\nexclude = [\"specs//generated/./**\"]\n",
    ];
    let mut outputs = Vec::new();

    for configuration in configurations {
        directory.write("oafmt.toml", configuration);
        let output = command()
            .current_dir(directory.path())
            .args(["--diff", "."])
            .output()
            .unwrap();
        outputs.push((output.status.code(), output.stdout, output.stderr));
    }

    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0].0, Some(0));
    assert!(outputs[0].2.is_empty());
    assert!(
        String::from_utf8(outputs[0].1.clone())
            .unwrap()
            .starts_with("--- specs/kept/api.yaml\n+++ specs/kept/api.yaml\n")
    );
}

#[test]
fn overlapping_selectors_make_diff_output_and_errors_argument_order_independent() {
    let directory = TestDir::new();
    directory.write("a/openapi.yaml", input_yaml());
    directory.write("b/openapi.yaml", "openapi: [\n");
    let permutations = [
        ["**/openapi.yaml", ".", "./a/openapi.yaml"],
        ["./a/openapi.yaml", ".", "**/openapi.yaml"],
        [".", "**/openapi.yaml", "./a/openapi.yaml"],
    ];
    let outputs: Vec<_> = permutations
        .iter()
        .map(|selectors| {
            let output = command()
                .current_dir(directory.path())
                .arg("--diff")
                .args(selectors)
                .output()
                .unwrap();
            (output.status.code(), output.stdout, output.stderr)
        })
        .collect();

    for output in &outputs[1..] {
        assert_eq!(output, &outputs[0]);
    }
    assert_eq!(outputs[0].0, Some(2));
    assert!(
        String::from_utf8(outputs[0].1.clone())
            .unwrap()
            .starts_with("--- ./a/openapi.yaml\n+++ ./a/openapi.yaml\n")
    );
    let stderr = String::from_utf8(outputs[0].2.clone()).unwrap();
    assert!(stderr.starts_with("oafmt: cannot format b/openapi.yaml: "));
    assert_eq!(stderr.lines().count(), 1);
}

#[test]
fn discovery_respects_repository_boundary_and_nested_gitignore_unless_disabled() {
    let directory = TestDir::new();
    directory.write(".gitignore", "parent-ignored/\n");
    fs::create_dir_all(directory.path().join("repository/.git")).unwrap();
    directory.write("repository/.gitignore", "repository-ignored/\n");
    directory.write("repository/selected/.gitignore", "nested-ignored/\n");
    directory.write("repository/selected/.ignore", "visible/\n");
    directory.write(
        "repository/selected/parent-ignored/openapi.yaml",
        input_yaml(),
    );
    directory.write(
        "repository/selected/repository-ignored/openapi.yaml",
        input_yaml(),
    );
    directory.write(
        "repository/selected/nested-ignored/openapi.json",
        input_json(),
    );
    directory.write("repository/selected/visible/openapi.yaml", input_yaml());

    let respected = command()
        .current_dir(directory.path().join("repository"))
        .args(["--check", "selected"])
        .output()
        .unwrap();
    assert_eq!(respected.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(respected.stderr).unwrap(),
        "oafmt: formatting changes required: selected/parent-ignored/openapi.yaml\n\
         oafmt: formatting changes required: selected/visible/openapi.yaml\n"
    );

    directory.write(
        "repository/oafmt.toml",
        "[discovery]\nrespect_gitignore = false\n",
    );
    let disabled = command()
        .current_dir(directory.path().join("repository"))
        .args(["--check", "selected"])
        .output()
        .unwrap();
    assert_eq!(disabled.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(disabled.stderr).unwrap(),
        "oafmt: formatting changes required: selected/nested-ignored/openapi.json\n\
         oafmt: formatting changes required: selected/parent-ignored/openapi.yaml\n\
         oafmt: formatting changes required: selected/repository-ignored/openapi.yaml\n\
         oafmt: formatting changes required: selected/visible/openapi.yaml\n"
    );
}

#[test]
fn patterned_singletons_respect_gitignore_while_literal_files_still_bypass_it() {
    let directory = TestDir::new();
    fs::create_dir(directory.path().join(".git")).unwrap();
    directory.write(
        ".gitignore",
        "ignored-filea.yaml\nignored-dir/\nnested/filea.yaml\nnested/class-dir/\n",
    );
    directory.write("ignored-filea.yaml", input_yaml());
    directory.write("ignored-file[a].yaml", input_yaml());
    directory.write("ignored-dir/openapi.yaml", input_yaml());
    directory.write("nested/filea.yaml", input_yaml());
    directory.write("nested/class-dir/openapi.yaml", input_yaml());

    for selector in [
        "ignored-file[a].yaml",
        "ignored-di[r]/openapi.yaml",
        "ignored-dir/openapi.[y]aml",
        "nested/file[a].yaml",
        "nested/class-di[r]/openapi.yaml",
        "nested/class-dir/openapi.[y]aml",
    ] {
        let output = command()
            .current_dir(directory.path())
            .args(["--check", selector])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{selector}");
        assert!(output.stdout.is_empty(), "{selector}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("selector produced no supported candidates"),
            "{selector}"
        );
    }

    let literal = command()
        .current_dir(directory.path())
        .args(["--check", "ignored-filea.yaml"])
        .output()
        .unwrap();
    assert_eq!(literal.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(literal.stderr).unwrap(),
        "oafmt: formatting changes required: ignored-filea.yaml\n"
    );

    directory.write("oafmt.toml", "[discovery]\nrespect_gitignore = false\n");
    let disabled = command()
        .current_dir(directory.path())
        .args([
            "--check",
            "ignored-file[a].yaml",
            "ignored-di[r]/openapi.yaml",
            "ignored-dir/openapi.[y]aml",
            "nested/file[a].yaml",
            "nested/class-di[r]/openapi.yaml",
            "nested/class-dir/openapi.[y]aml",
        ])
        .output()
        .unwrap();
    assert_eq!(disabled.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(disabled.stderr).unwrap().lines().count(),
        4
    );
}

#[test]
fn patterned_singletons_always_exclude_vcs_metadata() {
    let directory = TestDir::new();
    fs::create_dir(directory.path().join(".git")).unwrap();
    directory.write(".git/openapi.yaml", input_yaml());
    directory.write(".git/private/openapi.yaml", input_yaml());
    directory.write(".hg/openapi.yaml", input_yaml());
    directory.write(".svn/openapi.yaml", input_yaml());
    symlink(
        directory.path().join(".git/private"),
        directory.path().join("alias"),
    )
    .unwrap();

    for selector in [
        ".[g]it/openapi.yaml",
        ".[h]g/openapi.yaml",
        ".[s]vn/openapi.yaml",
        "alias/openapi.[y]aml",
    ] {
        let output = command()
            .current_dir(directory.path())
            .args(["--check", selector])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{selector}");
        assert!(output.stdout.is_empty(), "{selector}");
    }

    let literal = command()
        .current_dir(directory.path())
        .args(["--check", ".git/openapi.yaml"])
        .output()
        .unwrap();
    assert_eq!(literal.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(literal.stderr).unwrap(),
        "oafmt: formatting changes required: .git/openapi.yaml\n"
    );

    directory.write("oafmt.toml", "[discovery]\nrespect_gitignore = false\n");
    let disabled = command()
        .current_dir(directory.path())
        .args(["--check", ".[g]it/openapi.yaml"])
        .output()
        .unwrap();
    assert_eq!(disabled.status.code(), Some(2));
    assert!(disabled.stdout.is_empty());
}

#[test]
fn directory_symlink_aliases_into_vcs_metadata_are_never_discovered() {
    let directory = TestDir::new();

    for metadata in [".git", ".hg", ".svn"] {
        let input = directory.write(&format!("{metadata}/private/openapi.yaml"), input_yaml());
        let alias_name = format!("alias-{}", &metadata[1..]);
        let alias = directory.path().join(&alias_name);
        symlink(directory.path().join(metadata).join("private"), &alias).unwrap();
        let before = fs::read(&input).unwrap();

        let output = command()
            .current_dir(directory.path())
            .args(["--check", &alias_name])
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2), "{metadata}");
        assert!(output.stdout.is_empty(), "{metadata}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("selector produced no supported candidates"),
            "{metadata}"
        );
        assert_eq!(fs::read(&input).unwrap(), before, "{metadata}");
    }
}

#[test]
fn invalid_configuration_fails_before_write_preflight_or_mutation() {
    let directory = TestDir::new();
    let input = directory.write("openapi.yaml", input_yaml());
    let invalid = [
        "[discovery]\nunknown = true\n",
        "[discovery]\ninclude = \"*.yaml\"\n",
        "[discovery]\ninclude = []\n",
        "[discovery]\ninclude = [\"[\"]\n",
        "[discovery]\nexclude = [\"{a,b}.yaml\"]\n",
    ];

    for source in invalid {
        directory.write("oafmt.toml", source);
        let output = command()
            .current_dir(directory.path())
            .args(["--write", "."])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{source}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(fs::read_to_string(&input).unwrap(), input_yaml());
    }

    let output = command()
        .current_dir(directory.path())
        .args(["--write", "--config", "missing.toml", "openapi.yaml"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&input).unwrap(), input_yaml());
}

#[test]
fn literal_files_bypass_discovery_filters_and_plain_modes_are_config_free() {
    let directory = TestDir::new();
    let input = directory.write("api.yaml", input_yaml());
    fs::create_dir(directory.path().join(".git")).unwrap();
    directory.write(".gitignore", "api.yaml\n");
    directory.write(
        "oafmt.toml",
        "[discovery]\ninclude = []\nexclude = [\"**\"]\n",
    );

    let plain = command()
        .current_dir(directory.path())
        .arg("api.yaml")
        .output()
        .unwrap();
    assert_eq!(plain.status.code(), Some(0));
    assert_eq!(plain.stdout, expected_yaml().as_bytes());
    assert!(plain.stderr.is_empty());

    let stdin = run_with_stdin_in(
        directory.path(),
        &["--stdin-filepath", "virtual.yaml"],
        input_yaml(),
    );
    assert_eq!(stdin.status.code(), Some(0));
    assert_eq!(stdin.stdout, expected_yaml().as_bytes());

    directory.write("oafmt.toml", "[discovery]\nexclude = [\"**\"]\n");
    let explicit = command()
        .current_dir(directory.path())
        .args(["--check", "api.yaml"])
        .output()
        .unwrap();
    assert_eq!(explicit.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(explicit.stderr).unwrap(),
        "oafmt: formatting changes required: api.yaml\n"
    );
    assert_eq!(fs::read_to_string(input).unwrap(), input_yaml());

    let rejected = command()
        .current_dir(directory.path())
        .args([
            "--check",
            "--config",
            "oafmt.toml",
            "--stdin-filepath",
            "virtual.yaml",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
}

#[test]
fn unmatched_and_unsupported_native_globs_are_input_errors() {
    let directory = TestDir::new();
    directory.write("notes.txt", input_yaml());

    for selector in ["**/*.yaml", "**/*.txt", "[", "file[{.yaml"] {
        let output = command()
            .current_dir(directory.path())
            .arg("--check")
            .arg(selector)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{selector}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }

    let brace_expansion = command()
        .current_dir(directory.path())
        .args(["--check", "file{a,b}*.yaml"])
        .output()
        .unwrap();
    assert_eq!(brace_expansion.status.code(), Some(2));
    assert!(brace_expansion.stdout.is_empty());
    assert!(
        String::from_utf8(brace_expansion.stderr)
            .unwrap()
            .contains("brace expansion is not supported")
    );
}

#[test]
fn every_wax_only_syntax_form_and_post_wildcard_parent_is_rejected() {
    let directory = TestDir::new();
    directory.write("api.yaml", input_yaml());
    let patterns = [
        "*.yaml$",
        "*{yaml,json}",
        "*<yaml>",
        "*(?i).yaml",
        r"*\?.yaml",
        "**/../*.yaml",
    ];

    for pattern in patterns {
        directory.write("oafmt.toml", "");
        let cli = command()
            .current_dir(directory.path())
            .args(["--check", pattern])
            .output()
            .unwrap();
        assert_eq!(cli.status.code(), Some(2), "{pattern}");
        assert!(cli.stdout.is_empty(), "{pattern}");
        assert!(
            String::from_utf8(cli.stderr)
                .unwrap()
                .contains("invalid glob selector"),
            "{pattern}"
        );

        directory.write(
            "oafmt.toml",
            &format!("[discovery]\ninclude = [{pattern:?}]\n"),
        );
        let config = command()
            .current_dir(directory.path())
            .args(["--check", "."])
            .output()
            .unwrap();
        assert_eq!(config.status.code(), Some(2), "{pattern}");
        assert!(config.stdout.is_empty(), "{pattern}");
        assert!(
            String::from_utf8(config.stderr)
                .unwrap()
                .contains("invalid configuration"),
            "{pattern}"
        );
    }
}

#[test]
fn discovery_skips_nested_symlinks_and_write_rejects_a_symlink_root() {
    let directory = TestDir::new();
    let real = directory.write("tree/openapi.yaml", input_yaml());
    let outside = directory.write("outside/openapi.yaml", input_yaml());
    fs::create_dir_all(directory.path().join("tree/links")).unwrap();
    symlink(&outside, directory.path().join("tree/links/openapi.yaml")).unwrap();
    symlink(
        directory.path().join("outside"),
        directory.path().join("tree/nested-link"),
    )
    .unwrap();
    symlink(
        directory.path().join("tree"),
        directory.path().join("root-link"),
    )
    .unwrap();

    let discovered = command()
        .current_dir(directory.path())
        .args(["--check", "tree"])
        .output()
        .unwrap();
    assert_eq!(discovered.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(discovered.stderr).unwrap(),
        "oafmt: formatting changes required: tree/openapi.yaml\n"
    );

    let followed = command()
        .current_dir(directory.path())
        .args(["--check", "root-link"])
        .output()
        .unwrap();
    assert_eq!(followed.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(followed.stderr).unwrap(),
        "oafmt: formatting changes required: root-link/openapi.yaml\n"
    );

    let rejected = command()
        .current_dir(directory.path())
        .args(["--write", "root-link"])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert_eq!(fs::read_to_string(real).unwrap(), input_yaml());
    assert_eq!(fs::read_to_string(outside).unwrap(), input_yaml());
}

#[test]
fn write_rejects_equivalent_directory_symlink_root_spellings_before_every_write() {
    let directory = TestDir::new();
    let target = directory.write("tree/openapi.yaml", input_yaml());
    let other = directory.write("other.yaml", input_yaml());
    let link = directory.path().join("root-link");
    symlink(directory.path().join("tree"), &link).unwrap();

    for selector in ["root-link", "root-link/", "root-link/.", "./root-link/./"] {
        fs::write(&target, input_yaml()).unwrap();
        fs::write(&other, input_yaml()).unwrap();
        let output = command()
            .current_dir(directory.path())
            .arg("--write")
            .arg(selector)
            .arg("other.yaml")
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2), "{selector}");
        assert!(output.stdout.is_empty(), "{selector}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("oafmt: cannot discover through symlink in write mode: {selector}\n"),
            "{selector}"
        );
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{selector}"
        );
        assert_eq!(
            fs::read(&target).unwrap(),
            input_yaml().as_bytes(),
            "{selector}"
        );
        assert_eq!(
            fs::read(&other).unwrap(),
            input_yaml().as_bytes(),
            "{selector}"
        );
    }

    let read_only = command()
        .current_dir(directory.path())
        .args(["--check", "./root-link/./"])
        .output()
        .unwrap();
    assert_eq!(read_only.status.code(), Some(1));
    assert!(read_only.stdout.is_empty());
    assert_eq!(
        String::from_utf8(read_only.stderr).unwrap(),
        "oafmt: formatting changes required: root-link/openapi.yaml\n"
    );
}

#[test]
fn write_rejects_intermediate_symlinks_for_literal_file_and_directory_selectors() {
    let directory = TestDir::new();
    let file = directory.write("tree/api.yaml", input_yaml());
    let nested = directory.write("tree/specs/openapi.yaml", input_yaml());
    let other = directory.write("other.yaml", input_yaml());
    let link = directory.path().join("outer/link");
    fs::create_dir_all(directory.path().join("outer")).unwrap();
    symlink(directory.path().join("tree"), &link).unwrap();

    for selector in ["outer/link/api.yaml", "outer/link/specs"] {
        fs::write(&file, input_yaml()).unwrap();
        fs::write(&nested, input_yaml()).unwrap();
        fs::write(&other, input_yaml()).unwrap();
        let before = [
            fs::read(&file).unwrap(),
            fs::read(&nested).unwrap(),
            fs::read(&other).unwrap(),
        ];

        let output = command()
            .current_dir(directory.path())
            .arg("--write")
            .arg(selector)
            .arg("other.yaml")
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2), "{selector}");
        assert!(output.stdout.is_empty(), "{selector}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("oafmt: cannot discover through symlink in write mode: {selector}\n"),
            "{selector}"
        );
        assert_eq!(fs::read(&file).unwrap(), before[0], "{selector}");
        assert_eq!(fs::read(&nested).unwrap(), before[1], "{selector}");
        assert_eq!(fs::read(&other).unwrap(), before[2], "{selector}");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{selector}"
        );
    }

    let file_check = command()
        .current_dir(directory.path())
        .args(["--check", "outer/link/api.yaml"])
        .output()
        .unwrap();
    assert_eq!(file_check.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(file_check.stderr).unwrap(),
        "oafmt: formatting changes required: outer/link/api.yaml\n"
    );

    let directory_check = command()
        .current_dir(directory.path())
        .args(["--check", "outer/link/specs"])
        .output()
        .unwrap();
    assert_eq!(directory_check.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(directory_check.stderr).unwrap(),
        "oafmt: formatting changes required: outer/link/specs/openapi.yaml\n"
    );
}

#[test]
fn write_rejects_symlinks_in_native_glob_prefixes_before_every_write() {
    let directory = TestDir::new();
    let direct = directory.write("tree/api.yaml", input_yaml());
    let nested = directory.write("tree/nested/api.yaml", input_yaml());
    let other = directory.write("other.yaml", input_yaml());
    let root_link = directory.path().join("root-link");
    let intermediate_link = directory.path().join("outer/link");
    fs::create_dir_all(directory.path().join("outer")).unwrap();
    symlink(directory.path().join("tree"), &root_link).unwrap();
    symlink(directory.path().join("tree"), &intermediate_link).unwrap();

    for selector in [
        "root-link/*.yaml",
        "root-link/**/*.yaml",
        "./root-link/./**/*.yaml",
        "outer/link/nested/*.yaml",
    ] {
        fs::write(&direct, input_yaml()).unwrap();
        fs::write(&nested, input_yaml()).unwrap();
        fs::write(&other, input_yaml()).unwrap();

        let output = command()
            .current_dir(directory.path())
            .arg("--write")
            .arg(selector)
            .arg("other.yaml")
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2), "{selector}");
        assert!(output.stdout.is_empty(), "{selector}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("oafmt: cannot discover through symlink in write mode: {selector}\n"),
            "{selector}"
        );
        assert!(
            fs::symlink_metadata(&root_link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{selector}"
        );
        assert!(
            fs::symlink_metadata(&intermediate_link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{selector}"
        );
        assert_eq!(
            fs::read(&direct).unwrap(),
            input_yaml().as_bytes(),
            "{selector}"
        );
        assert_eq!(
            fs::read(&nested).unwrap(),
            input_yaml().as_bytes(),
            "{selector}"
        );
        assert_eq!(
            fs::read(&other).unwrap(),
            input_yaml().as_bytes(),
            "{selector}"
        );
    }
}

#[test]
fn write_rejects_absolute_parent_and_repeated_separator_symlink_prefixes_without_mutation() {
    let directory = TestDir::new();
    let target = directory.write("tree/api.yaml", input_yaml());
    let other = directory.write("workspace/other.yaml", input_yaml());
    let link = directory.path().join("root-link");
    let current = directory.path().join("workspace/current");
    fs::create_dir_all(&current).unwrap();
    symlink(directory.path().join("tree"), &link).unwrap();
    let selectors = [
        "../..//root-link/./*.yaml".to_owned(),
        format!("{}//root-link/./*.yaml", directory.path().display()),
    ];

    for selector in selectors {
        fs::write(&target, input_yaml()).unwrap();
        fs::write(&other, input_yaml()).unwrap();
        let output = command()
            .current_dir(&current)
            .arg("--write")
            .arg(&selector)
            .arg("../other.yaml")
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2), "{selector}");
        assert!(output.stdout.is_empty(), "{selector}");
        assert_eq!(fs::read(&target).unwrap(), input_yaml().as_bytes());
        assert_eq!(fs::read(&other).unwrap(), input_yaml().as_bytes());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}

#[test]
fn read_only_modes_follow_symlink_rooted_native_globs() {
    let directory = TestDir::new();
    let direct = directory.write("tree/api.yaml", input_yaml());
    let nested = directory.write("tree/nested/api.yaml", input_yaml());
    let root_link = directory.path().join("root-link");
    symlink(directory.path().join("tree"), &root_link).unwrap();

    let checked = command()
        .current_dir(directory.path())
        .args(["--check", "root-link/*.yaml"])
        .output()
        .unwrap();
    assert_eq!(checked.status.code(), Some(1));
    assert!(checked.stdout.is_empty());
    assert_eq!(
        String::from_utf8(checked.stderr).unwrap(),
        "oafmt: formatting changes required: root-link/api.yaml\n"
    );

    let diffed = command()
        .current_dir(directory.path())
        .args(["--diff", "root-link/*.yaml"])
        .output()
        .unwrap();
    assert_eq!(diffed.status.code(), Some(0));
    assert!(diffed.stderr.is_empty());
    let stdout = String::from_utf8(diffed.stdout).unwrap();
    assert!(stdout.contains("--- root-link/api.yaml\n+++ root-link/api.yaml\n"));
    assert!(!stdout.contains("root-link/nested/api.yaml"));
    assert_eq!(fs::read(&direct).unwrap(), input_yaml().as_bytes());
    assert_eq!(fs::read(&nested).unwrap(), input_yaml().as_bytes());
    assert!(
        fs::symlink_metadata(root_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn read_only_parent_after_symlink_glob_uses_the_traversed_candidate_identity() {
    let directory = TestDir::new();
    let target = directory.write("target/api.yaml", input_yaml());
    let decoy = directory.write("api.yaml", expected_yaml());
    fs::create_dir_all(directory.path().join("target/nested")).unwrap();
    let root_link = directory.path().join("root-link");
    symlink(directory.path().join("target/nested"), &root_link).unwrap();

    let checked = command()
        .current_dir(directory.path())
        .args(["--check", "root-link/../*.yaml"])
        .output()
        .unwrap();
    assert_eq!(checked.status.code(), Some(1));
    assert!(checked.stdout.is_empty());
    assert_eq!(
        String::from_utf8(checked.stderr).unwrap(),
        "oafmt: formatting changes required: root-link/../api.yaml\n"
    );

    let diffed = command()
        .current_dir(directory.path())
        .args(["--diff", "root-link/../*.yaml"])
        .output()
        .unwrap();
    assert_eq!(diffed.status.code(), Some(0));
    assert!(diffed.stderr.is_empty());
    let stdout = String::from_utf8(diffed.stdout).unwrap();
    assert!(stdout.starts_with("--- root-link/../api.yaml\n+++ root-link/../api.yaml\n@@ "));
    assert_eq!(fs::read(&target).unwrap(), input_yaml().as_bytes());
    assert_eq!(fs::read(&decoy).unwrap(), expected_yaml().as_bytes());
    assert!(
        fs::symlink_metadata(root_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn literal_symlink_parent_selectors_keep_distinct_native_file_identities() {
    let directory = TestDir::new();
    let malformed = directory.write("target/api.yaml", "openapi: [\n");
    let decoy = directory.write("api.yaml", expected_yaml());
    fs::create_dir_all(directory.path().join("target/nested")).unwrap();
    let root_link = directory.path().join("root-link");
    symlink(directory.path().join("target/nested"), &root_link).unwrap();
    let malformed_before = fs::read(&malformed).unwrap();
    let decoy_before = fs::read(&decoy).unwrap();

    let output = command()
        .current_dir(directory.path())
        .args(["--check", "api.yaml", "root-link/../api.yaml"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("cannot format root-link/../api.yaml"));
    assert_eq!(fs::read(&malformed).unwrap(), malformed_before);
    assert_eq!(fs::read(&decoy).unwrap(), decoy_before);
}

#[test]
fn write_native_glob_in_ordinary_directory_still_formats_files() {
    let directory = TestDir::new();
    let input = directory.write("ordinary/api.yaml", input_yaml());

    let output = command()
        .current_dir(directory.path())
        .args(["--write", "ordinary/*.yaml"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(input).unwrap(), expected_yaml());
}

#[test]
fn write_native_glob_still_skips_symlinks_discovered_after_metacharacter() {
    let directory = TestDir::new();
    let real = directory.write("tree/real/api.yaml", input_yaml());
    let outside = directory.write("outside/api.yaml", input_yaml());
    let file_link = directory.path().join("tree/links/api.yaml");
    let directory_link = directory.path().join("tree/nested-link");
    fs::create_dir_all(directory.path().join("tree/links")).unwrap();
    symlink(&outside, &file_link).unwrap();
    symlink(directory.path().join("outside"), &directory_link).unwrap();

    let output = command()
        .current_dir(directory.path())
        .args(["--write", "tree/*/*.yaml"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(real).unwrap(), expected_yaml());
    assert_eq!(fs::read(&outside).unwrap(), input_yaml().as_bytes());
    assert!(
        fs::symlink_metadata(file_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(directory_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn discovered_format_failure_prevents_all_writes() {
    let directory = TestDir::new();
    let valid = directory.write("a/openapi.yaml", input_yaml());
    let malformed = directory.write("z/openapi.yaml", "openapi: [\n");

    let output = command()
        .arg("--write")
        .arg(directory.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_to_string(&valid).unwrap(), input_yaml());
    assert_eq!(fs::read_to_string(malformed).unwrap(), "openapi: [\n");

    let output = command()
        .arg("--write")
        .arg(&valid)
        .arg(directory.path().join("missing/**/*.yaml"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(valid).unwrap(), input_yaml());
}

#[test]
fn write_discovers_and_deduplicates_directory_and_glob_inputs() {
    let directory = TestDir::new();
    let yaml = directory.write("a/openapi.yaml", input_yaml());
    let json = directory.write("b/openapi.json", input_json());

    let output = command()
        .current_dir(directory.path())
        .args(["--write", ".", "**/openapi.*"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(yaml).unwrap(), expected_yaml());
    assert_eq!(fs::read_to_string(json).unwrap(), expected_json());
}

#[test]
fn write_leading_parent_directory_selector_formats_only_the_selected_file() {
    let directory = TestDir::new();
    let selected = directory.write("shared/openapi.yaml", input_yaml());
    let decoy = directory.write("workspace/shared/openapi.yaml", input_yaml());
    let unselected = directory.write("workspace/other.yaml", input_yaml());
    let current = directory.path().join("workspace");
    let decoy_before = fs::read(&decoy).unwrap();
    let unselected_before = fs::read(&unselected).unwrap();

    let output = command()
        .current_dir(&current)
        .args(["--write", "../shared"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(&selected).unwrap(), expected_yaml().as_bytes());
    assert_eq!(fs::read(&decoy).unwrap(), decoy_before);
    assert_eq!(fs::read(&unselected).unwrap(), unselected_before);
}

#[test]
fn read_only_modes_follow_explicit_symlinks_but_write_rejects_them() {
    let directory = TestDir::new();
    let target = directory.write("target.yaml", input_yaml());
    let link = directory.path().join("link.yaml");
    symlink(&target, &link).unwrap();

    let output = command().arg(&link).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_yaml().as_bytes());
    assert!(output.stderr.is_empty());

    let output = command().arg("--check").arg(&link).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains(&link.display().to_string())
    );

    let output = command().arg("--diff").arg(&link).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with(&format!("--- {}\n", link.display()))
    );

    fs::write(&target, expected_yaml()).unwrap();
    let output = command().arg("--write").arg(&link).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains(&link.display().to_string())
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), expected_yaml());
}

#[test]
fn write_is_atomic_in_place_and_preserves_permissions() {
    let directory = TestDir::new();
    let path = directory.write("api.json", input_json());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    let output = command().args(["--write"]).arg(&path).output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), expected_json());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let remaining: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(remaining, [path.file_name().unwrap()]);
}

#[test]
fn stdin_filepath_infers_format_and_rejects_write() {
    let mut child = command()
        .args(["--stdin-filepath", "virtual/api.yml"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input_yaml().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_yaml().as_bytes());
    assert!(output.stderr.is_empty());

    let output = run_with_stdin(
        &["--check", "--stdin-filepath", "virtual/api.yml"],
        input_yaml(),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "oafmt: formatting changes required: virtual/api.yml\n"
    );

    let output = run_with_stdin(
        &["--diff", "--stdin-filepath", "virtual/api.yml"],
        input_yaml(),
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("--- virtual/api.yml\n+++ virtual/api.yml\n")
    );

    let output = command()
        .args(["--write", "--stdin-filepath", "api.yaml"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn modes_extensions_directories_and_bad_input_fail_with_exit_two() {
    let directory = TestDir::new();
    let yaml = directory.write("api.yaml", input_yaml());
    let text = directory.write("api.txt", input_yaml());
    let malformed = directory.write("bad.yaml", "openapi: [\n");

    for args in [
        vec!["--check", "--diff", yaml.to_str().unwrap()],
        vec![text.to_str().unwrap()],
        vec![directory.path().to_str().unwrap()],
        vec![malformed.to_str().unwrap()],
    ] {
        let output = command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oafmt"))
}

fn run_with_stdin(args: &[&str], input: &str) -> std::process::Output {
    run_with_stdin_command(command().args(args), input)
}

fn run_with_stdin_in(directory: &Path, args: &[&str], input: &str) -> std::process::Output {
    run_with_stdin_command(command().current_dir(directory).args(args), input)
}

fn run_with_stdin_command(command: &mut Command, input: &str) -> std::process::Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

const fn input_yaml() -> &'static str {
    include_str!("../../oafmt-core/tests/fixtures/basic.input.yaml")
}

const fn expected_yaml() -> &'static str {
    include_str!("../../oafmt-core/tests/fixtures/basic.expected.yaml")
}

const fn input_json() -> &'static str {
    include_str!("../../oafmt-core/tests/fixtures/basic.input.json")
}

const fn expected_json() -> &'static str {
    include_str!("../../oafmt-core/tests/fixtures/basic.expected.json")
}

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "oafmt-cli-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
