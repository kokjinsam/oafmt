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
fn directories_and_unexpanded_globs_are_each_reported_as_errors() {
    let directory = TestDir::new();
    let _existing = directory.write("api.yaml", input_yaml());
    let literal_glob = directory.path().join("*.yaml");

    let output = command()
        .arg("--check")
        .arg(&literal_glob)
        .arg(directory.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&literal_glob.display().to_string()));
    assert!(stderr.contains(&directory.path().display().to_string()));
    assert_eq!(stderr.lines().count(), 2);
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
    let mut child = command()
        .args(args)
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

fn input_yaml() -> &'static str {
    include_str!("../../oafmt-core/tests/fixtures/basic.input.yaml")
}

fn expected_yaml() -> &'static str {
    include_str!("../../oafmt-core/tests/fixtures/basic.expected.yaml")
}

fn input_json() -> &'static str {
    include_str!("../../oafmt-core/tests/fixtures/basic.input.json")
}

fn expected_json() -> &'static str {
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
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
