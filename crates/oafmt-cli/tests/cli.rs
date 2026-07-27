use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
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
