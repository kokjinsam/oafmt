//! Command-line boundary for deterministic, syntax-preserving OpenAPI formatting.
//!
//! The CLI owns argument parsing, discovery, diagnostics, preflight, and
//! permission-preserving atomic replacement. Write mode preflights the complete
//! selection before mutation. Before replacing each changed file, it compares
//! the current raw bytes with the retained preflight snapshot. A mismatch is a
//! write conflict; each successful replacement is atomic on the supported
//! filesystem, but the comparison and rename are separate operations. Files
//! are replaced independently, failures are reported in deterministic path
//! order, and the aggregate exit lattice is `3 > 2 > 1 > 0`.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oafmt_core::{FormatError, InputFormat, format};

/// Strict discovery configuration loading.
pub mod config;
/// Deterministic file discovery and selector resolution.
pub mod discovery;

use discovery::{FileInput, normalize_lexically};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Stdout,
    Write,
    Check,
    Diff,
}

struct Options {
    mode: Mode,
    input: Input,
}

enum Input {
    Files {
        files: Vec<FileInput>,
        discovery_failures: Vec<discovery::Failure>,
    },
    Stdin(PathBuf),
}

struct PreparedFile {
    path: PathBuf,
    display: PathBuf,
    source: String,
    output: String,
    changed: bool,
}

struct Failure {
    severity: u8,
    message: String,
}

enum Preflight {
    Ready(PreparedFile),
    Failed(Failure),
}

enum WriteEvent {
    PreflightComplete,
    TemporaryPrepared,
}

enum ReplaceError {
    Conflict,
    Io(io::Error),
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err((code, message)) => {
            diagnostic(message);
            ExitCode::from(code)
        }
    }
}

fn run() -> Result<u8, (u8, String)> {
    let options = parse_args(env::args_os().skip(1))?;
    match options.input {
        Input::Stdin(path) => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .map_err(|error| user_error(format!("cannot read stdin: {error}")))?;
            let format_kind = infer_format(&path)?;
            let result = format(&source, format_kind).map_err(classify_format_error)?;
            match options.mode {
                Mode::Stdout => {
                    io::stdout()
                        .write_all(result.output.as_bytes())
                        .map_err(|error| user_error(format!("cannot write stdout: {error}")))?;
                    Ok(0)
                }
                Mode::Check if result.changed => {
                    diagnostic(format_args!(
                        "formatting changes required: {}",
                        path.display()
                    ));
                    Ok(1)
                }
                Mode::Check => Ok(0),
                Mode::Diff => {
                    if result.changed {
                        let diff =
                            unified_diff(&path.display().to_string(), &source, &result.output);
                        io::stdout()
                            .write_all(diff.as_bytes())
                            .map_err(|error| user_error(format!("cannot write stdout: {error}")))?;
                    }
                    Ok(0)
                }
                Mode::Write => Err((
                    3,
                    "internal invariant: write mode received stdin".to_owned(),
                )),
            }
        }
        Input::Files {
            files,
            discovery_failures,
        } => run_files(options.mode, files, discovery_failures),
    }
}

fn run_files(
    mode: Mode,
    files: Vec<FileInput>,
    discovery_failures: Vec<discovery::Failure>,
) -> Result<u8, (u8, String)> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut hook = |_event: WriteEvent| {};
    run_files_with_writers(
        mode,
        files,
        discovery_failures,
        &mut stdout,
        &mut stderr,
        &mut hook,
    )
}

fn run_files_with_writers(
    mode: Mode,
    files: Vec<FileInput>,
    discovery_failures: Vec<discovery::Failure>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    hook: &mut impl FnMut(WriteEvent),
) -> Result<u8, (u8, String)> {
    let preflight = collect_preflight(mode, files, discovery_failures);
    if mode == Mode::Write {
        hook(WriteEvent::PreflightComplete);
    }

    match mode {
        Mode::Stdout => match preflight.into_iter().next().ok_or_else(|| {
            (
                3,
                "internal invariant: stdout mode has no preflight result".to_owned(),
            )
        })? {
            Preflight::Ready(file) => {
                stdout
                    .write_all(file.output.as_bytes())
                    .map_err(|error| user_error(format!("cannot write stdout: {error}")))?;
                Ok(0)
            }
            Preflight::Failed(failure) => Err((failure.severity, failure.message)),
        },
        Mode::Write => run_write(preflight, stderr, hook),
        Mode::Check => Ok(run_check(preflight, stderr)),
        Mode::Diff => run_diff(preflight, stdout, stderr),
    }
}

fn collect_preflight(
    mode: Mode,
    files: Vec<FileInput>,
    discovery_failures: Vec<discovery::Failure>,
) -> Vec<Preflight> {
    let mut keyed_preflight: Vec<_> = files
        .into_iter()
        .map(|file| {
            let key = file.key.clone();
            (key, preflight_file(mode, &file))
        })
        .collect();
    keyed_preflight.extend(discovery_failures.into_iter().map(|failure| {
        (
            failure.key,
            Preflight::Failed(Failure {
                severity: 2,
                message: failure.message,
            }),
        )
    }));
    keyed_preflight.sort_by(|left, right| left.0.cmp(&right.0));
    keyed_preflight
        .into_iter()
        .map(|(_, result)| result)
        .collect()
}

fn preflight_file(mode: Mode, input: &FileInput) -> Preflight {
    match prepare_file(mode, &input.path, &input.display) {
        Ok(file) => Preflight::Ready(file),
        Err((severity, message)) => Preflight::Failed(Failure { severity, message }),
    }
}

fn prepare_file(mode: Mode, path: &Path, display: &Path) -> Result<PreparedFile, (u8, String)> {
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|error| user_error(format!("cannot read {}: {error}", display.display())))?;
    if mode == Mode::Write && link_metadata.file_type().is_symlink() {
        return Err(user_error(format!(
            "cannot replace symlink: {}",
            display.display()
        )));
    }
    let metadata = fs::metadata(path)
        .map_err(|error| user_error(format!("cannot read {}: {error}", display.display())))?;
    if !metadata.is_file() {
        return Err(user_error(format!(
            "input is not a file: {}",
            display.display()
        )));
    }
    let format_kind = infer_format(display)?;
    let source = fs::read_to_string(path)
        .map_err(|error| user_error(format!("cannot read {}: {error}", display.display())))?;
    let result = format(&source, format_kind).map_err(|error| {
        let (severity, message) = classify_format_error(error);
        (
            severity,
            format!("cannot format {}: {message}", display.display()),
        )
    })?;
    Ok(PreparedFile {
        path: path.to_path_buf(),
        display: display.to_path_buf(),
        source,
        output: result.output,
        changed: result.changed,
    })
}

fn run_check(preflight: Vec<Preflight>, stderr: &mut impl Write) -> u8 {
    let mut severities = Vec::with_capacity(preflight.len());
    for result in preflight {
        match result {
            Preflight::Ready(file) if file.changed => {
                write_diagnostic(
                    stderr,
                    format_args!("formatting changes required: {}", file.display.display()),
                );
                severities.push(1);
            }
            Preflight::Ready(_) => severities.push(0),
            Preflight::Failed(failure) => {
                write_diagnostic(stderr, failure.message);
                severities.push(failure.severity);
            }
        }
    }
    aggregate_exit(severities)
}

fn run_diff(
    preflight: Vec<Preflight>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> Result<u8, (u8, String)> {
    let mut output = String::new();
    let mut severities = Vec::with_capacity(preflight.len());
    for result in preflight {
        match result {
            Preflight::Ready(file) => {
                if file.changed {
                    output.push_str(&unified_diff(
                        &file.display.display().to_string(),
                        &file.source,
                        &file.output,
                    ));
                }
                severities.push(0);
            }
            Preflight::Failed(failure) => {
                write_diagnostic(stderr, failure.message);
                severities.push(failure.severity);
            }
        }
    }
    let collected_severity = aggregate_exit(severities);
    write_diff_output(stdout, &output, collected_severity)
}

fn run_write(
    preflight: Vec<Preflight>,
    stderr: &mut impl Write,
    hook: &mut impl FnMut(WriteEvent),
) -> Result<u8, (u8, String)> {
    let preflight_exit = aggregate_exit(preflight.iter().filter_map(|result| match result {
        Preflight::Ready(_) => None,
        Preflight::Failed(failure) => Some(failure.severity),
    }));
    if preflight_exit != 0 {
        for result in preflight {
            if let Preflight::Failed(failure) = result {
                write_diagnostic(stderr, failure.message);
            }
        }
        return Ok(preflight_exit);
    }

    let mut severities = Vec::with_capacity(preflight.len());
    for result in preflight {
        let Preflight::Ready(file) = result else {
            return Err((
                3,
                "internal invariant: failed preflight reached replacement".to_owned(),
            ));
        };
        if file.changed {
            match atomic_replace(
                &file.path,
                file.source.as_bytes(),
                file.output.as_bytes(),
                hook,
            ) {
                Ok(()) => severities.push(0),
                Err(ReplaceError::Conflict) => {
                    write_diagnostic(
                        stderr,
                        format_args!(
                            "write conflict: {}: file changed since preflight",
                            file.display.display()
                        ),
                    );
                    severities.push(2);
                }
                Err(ReplaceError::Io(error)) => {
                    write_diagnostic(
                        stderr,
                        format_args!("cannot replace {}: {error}", file.display.display()),
                    );
                    severities.push(2);
                }
            }
        } else {
            severities.push(0);
        }
    }
    Ok(aggregate_exit(severities))
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Options, (u8, String)> {
    let mut args = args;
    let mut mode = None;
    let mut stdin_path = None;
    let mut config_path = None;
    let mut files = Vec::new();

    while let Some(argument) = args.next() {
        if argument == "--write" {
            set_mode(&mut mode, Mode::Write)?;
        } else if argument == "--check" {
            set_mode(&mut mode, Mode::Check)?;
        } else if argument == "--diff" {
            set_mode(&mut mode, Mode::Diff)?;
        } else if argument == "--stdin-filepath" {
            if stdin_path.is_some() {
                return Err(user_error("--stdin-filepath may be supplied only once"));
            }
            stdin_path =
                Some(PathBuf::from(args.next().ok_or_else(|| {
                    user_error("--stdin-filepath requires a path")
                })?));
        } else if argument == "--config" {
            if config_path.is_some() {
                return Err(user_error("--config may be supplied only once"));
            }
            config_path = Some(PathBuf::from(
                args.next()
                    .ok_or_else(|| user_error("--config requires a path"))?,
            ));
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(user_error(format!(
                "unknown option: {}",
                argument.to_string_lossy()
            )));
        } else {
            files.push(PathBuf::from(argument));
        }
    }

    if !files.is_empty() && stdin_path.is_some() {
        return Err(user_error(
            "a file and --stdin-filepath cannot be used together",
        ));
    }
    let mode = mode.unwrap_or(Mode::Stdout);
    if mode == Mode::Stdout && config_path.is_some() {
        return Err(user_error(
            "--config is available only with --write, --check, or --diff",
        ));
    }
    if mode == Mode::Write && stdin_path.is_some() {
        return Err(user_error("--write cannot be used with --stdin-filepath"));
    }
    if config_path.is_some() && stdin_path.is_some() {
        return Err(user_error("--config cannot be used with --stdin-filepath"));
    }
    if mode == Mode::Stdout && files.len() > 1 {
        return Err(user_error("stdout mode accepts exactly one input file"));
    }
    let input = if let Some(path) = stdin_path {
        Input::Stdin(path)
    } else if files.is_empty() {
        return Err(user_error("one input file is required"));
    } else if mode == Mode::Stdout {
        Input::Files {
            files: sort_and_deduplicate(files)?,
            discovery_failures: Vec::new(),
        }
    } else {
        let cwd = env::current_dir().ok();
        let config = config::load(config_path.as_deref(), cwd.as_deref()).map_err(user_error)?;
        let resolution = discovery::resolve(files, mode, cwd.as_deref(), &config)
            .map_err(|message| (3, message))?;
        Input::Files {
            files: resolution.files,
            discovery_failures: resolution.failures,
        }
    };
    Ok(Options { mode, input })
}

fn sort_and_deduplicate(paths: Vec<PathBuf>) -> Result<Vec<FileInput>, (u8, String)> {
    let current_directory = paths
        .iter()
        .any(|path| path.is_relative())
        .then(|| {
            env::current_dir()
                .map_err(|error| user_error(format!("cannot determine current directory: {error}")))
        })
        .transpose()?;
    let mut files = paths
        .into_iter()
        .map(|path| {
            let absolute = current_directory
                .as_ref()
                .map_or_else(|| path.clone(), |directory| directory.join(&path));
            FileInput {
                key: normalize_lexically(&absolute),
                path: path.clone(),
                display: path,
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.display.as_os_str().cmp(right.display.as_os_str()))
    });
    files.dedup_by(|right, left| right.key == left.key);
    Ok(files)
}

fn set_mode(current: &mut Option<Mode>, requested: Mode) -> Result<(), (u8, String)> {
    if current.replace(requested).is_some() {
        return Err(user_error(
            "--write, --check, and --diff are mutually exclusive",
        ));
    }
    Ok(())
}

fn infer_format(path: &Path) -> Result<InputFormat, (u8, String)> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("yaml" | "yml") => Ok(InputFormat::Yaml),
        Some("json") => Ok(InputFormat::Json),
        _ => Err(user_error(format!(
            "cannot infer input format from {}",
            path.display()
        ))),
    }
}

fn atomic_replace(
    path: &Path,
    expected: &[u8],
    contents: &[u8],
    hook: &mut impl FnMut(WriteEvent),
) -> Result<(), ReplaceError> {
    let metadata = fs::symlink_metadata(path).map_err(ReplaceError::Io)?;
    if !metadata.file_type().is_file() {
        return Err(ReplaceError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination is not a regular file",
        )));
    }
    let permissions = metadata.permissions();
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("input");

    for attempt in 0..100 {
        let temporary = directory.join(format!(
            ".{file_name}.oafmt.{}.{attempt}.tmp",
            std::process::id()
        ));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match opened {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ReplaceError::Io(error)),
        };
        let result = (|| -> Result<(), ReplaceError> {
            file.set_permissions(permissions)
                .map_err(ReplaceError::Io)?;
            file.write_all(contents).map_err(ReplaceError::Io)?;
            file.sync_all().map_err(ReplaceError::Io)?;
            drop(file);
            hook(WriteEvent::TemporaryPrepared);

            let final_metadata = fs::symlink_metadata(path).map_err(ReplaceError::Io)?;
            if !final_metadata.file_type().is_file() {
                return Err(ReplaceError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "destination is not a regular file",
                )));
            }

            let read_limit = expected
                .len()
                .checked_add(1)
                .and_then(|length| u64::try_from(length).ok())
                .ok_or_else(|| {
                    ReplaceError::Io(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "snapshot is too large",
                    ))
                })?;
            let mut current = Vec::with_capacity(expected.len().saturating_add(1));
            fs::File::open(path)
                .map_err(ReplaceError::Io)?
                .take(read_limit)
                .read_to_end(&mut current)
                .map_err(ReplaceError::Io)?;
            if current != expected {
                return Err(ReplaceError::Conflict);
            }
            fs::rename(&temporary, path).map_err(ReplaceError::Io)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    Err(ReplaceError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary file",
    )))
}

fn unified_diff(path: &str, before: &str, after: &str) -> String {
    let before_count = line_count(before);
    let after_count = line_count(after);
    let mut output = format!("--- {path}\n+++ {path}\n@@ -1,{before_count} +1,{after_count} @@\n");
    append_diff_lines(&mut output, '-', before);
    append_diff_lines(&mut output, '+', after);
    output
}

fn line_count(text: &str) -> usize {
    text.lines().count()
}

fn append_diff_lines(output: &mut String, prefix: char, text: &str) {
    for line in text.split_inclusive('\n') {
        output.push(prefix);
        output.push_str(line);
    }
    if !text.is_empty() && !text.ends_with('\n') {
        output.push('\n');
        output.push_str("\\ No newline at end of file\n");
    }
}

fn user_error(message: impl Into<String>) -> (u8, String) {
    (2, message.into())
}

fn classify_format_error(error: FormatError) -> (u8, String) {
    match error {
        FormatError::Input(message) => user_error(message),
        FormatError::InternalInvariant(message) => (3, message),
    }
}

fn write_diff_output(
    writer: &mut impl Write,
    output: &str,
    collected_severity: u8,
) -> Result<u8, (u8, String)> {
    writer.write_all(output.as_bytes()).map_err(|error| {
        (
            aggregate_exit([collected_severity, 2]),
            format!("cannot write stdout: {error}"),
        )
    })?;
    Ok(collected_severity)
}

fn aggregate_exit(severities: impl IntoIterator<Item = u8>) -> u8 {
    severities.into_iter().max().unwrap_or(0)
}

fn diagnostic(message: impl fmt::Display) {
    drop(writeln!(io::stderr().lock(), "oafmt: {message}"));
}

fn write_diagnostic(writer: &mut impl Write, message: impl fmt::Display) {
    drop(writeln!(writer, "oafmt: {message}"));
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "filesystem fixture setup uses unwrap so setup failures stop the test"
    )]

    use std::fs;
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{Mode, WriteEvent, aggregate_exit, run_files_with_writers, write_diff_output};
    use crate::discovery::FileInput;

    const INPUT_YAML: &str = include_str!("../../oafmt-core/tests/fixtures/basic.input.yaml");
    const EXPECTED_YAML: &str = include_str!("../../oafmt-core/tests/fixtures/basic.expected.yaml");

    type CapturedWrite = (Result<u8, (u8, String)>, Vec<u8>, Vec<u8>);

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed pipe"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn exit_aggregation_uses_the_highest_severity() {
        assert_eq!(aggregate_exit([]), 0);
        assert_eq!(aggregate_exit([0, 1, 2, 3]), 3);
        assert_eq!(aggregate_exit([3, 2, 1, 0]), 3);
        assert_eq!(aggregate_exit([1, 2, 1]), 2);
        assert_eq!(aggregate_exit([0, 1, 0]), 1);
    }

    #[test]
    fn diff_stdout_failure_preserves_collected_severity() {
        for (collected, expected) in [(3, 3), (2, 2), (0, 2)] {
            let (severity, message) =
                write_diff_output(&mut FailingWriter, "diff", collected).unwrap_err();

            assert_eq!(severity, expected);
            assert!(message.contains("cannot write stdout"));
            assert!(message.contains("closed pipe"));
        }
    }

    #[test]
    fn write_conflicts_on_raw_changes_after_preflight_and_temp_preparation() {
        let same_length = INPUT_YAML.replace("Pets", "Cats").into_bytes();
        assert_eq!(same_length.len(), INPUT_YAML.len());
        let cases = [
            ("same-length edit", same_length, false),
            ("invalid UTF-8 edit", vec![0xff, 0x00], false),
            (
                "already formatted bytes",
                EXPECTED_YAML.as_bytes().to_vec(),
                false,
            ),
            ("edit after temp preparation", vec![0xff, 0xfe], true),
        ];

        for (label, external, after_temp) in cases {
            let directory = TestDir::new();
            let path = directory.write("api.yaml", INPUT_YAML.as_bytes());
            let mut changed = false;
            let (result, stdout, stderr) =
                run_write_with_hook(vec![path.clone()], |event| match event {
                    WriteEvent::PreflightComplete if !after_temp && !changed => {
                        fs::write(&path, &external).unwrap();
                        changed = true;
                    }
                    WriteEvent::TemporaryPrepared if after_temp && !changed => {
                        let temporary_files = directory.temporary_files();
                        assert_eq!(temporary_files.len(), 1);
                        assert_eq!(
                            fs::read(&temporary_files[0]).unwrap(),
                            EXPECTED_YAML.as_bytes()
                        );
                        assert_eq!(fs::read(&path).unwrap(), INPUT_YAML.as_bytes());
                        fs::write(&path, &external).unwrap();
                        changed = true;
                    }
                    _ => {}
                });

            assert!(changed, "{label}: mutation hook did not run");
            assert_eq!(result.unwrap(), 2, "{label}");
            assert!(stdout.is_empty(), "{label}: write mode wrote stdout");
            assert_eq!(
                stderr,
                format!(
                    "oafmt: write conflict: {}: file changed since preflight\n",
                    path.display()
                )
                .as_bytes(),
                "{label}"
            );
            assert_eq!(fs::read(&path).unwrap(), external, "{label}");
            directory.assert_no_temporary_files();
        }
    }

    #[test]
    fn byte_identical_rewrite_remains_eligible_for_replacement() {
        let directory = TestDir::new();
        let path = directory.write("api.yaml", INPUT_YAML.as_bytes());
        let (result, stdout, stderr) = run_write_with_hook(vec![path.clone()], |event| {
            if matches!(event, WriteEvent::PreflightComplete) {
                fs::write(&path, INPUT_YAML.as_bytes()).unwrap();
            }
        });

        assert_eq!(result.unwrap(), 0);
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert_eq!(fs::read(&path).unwrap(), EXPECTED_YAML.as_bytes());
        directory.assert_no_temporary_files();
    }

    #[test]
    fn write_continues_after_a_conflict_without_rolling_back_other_files() {
        let directory = TestDir::new();
        let first = directory.write("a.yaml", INPUT_YAML.as_bytes());
        let second = directory.write("b.yaml", INPUT_YAML.as_bytes());
        let third = directory.write("c.yaml", INPUT_YAML.as_bytes());
        let external = INPUT_YAML.replace("Pets", "Cats").into_bytes();

        let (result, stdout, stderr) = run_write_with_hook(
            vec![third.clone(), first.clone(), second.clone()],
            |event| {
                if matches!(event, WriteEvent::PreflightComplete) {
                    fs::write(&second, &external).unwrap();
                }
            },
        );

        assert_eq!(result.unwrap(), 2);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            format!(
                "oafmt: write conflict: {}: file changed since preflight\n",
                second.display()
            )
            .as_bytes()
        );
        assert_eq!(fs::read(first).unwrap(), EXPECTED_YAML.as_bytes());
        assert_eq!(fs::read(second).unwrap(), external);
        assert_eq!(fs::read(third).unwrap(), EXPECTED_YAML.as_bytes());
        directory.assert_no_temporary_files();
    }

    #[test]
    fn non_regular_destination_failure_keeps_it_intact_and_continues() {
        let directory = TestDir::new();
        let first = directory.write("a.yaml", INPUT_YAML.as_bytes());
        let second = directory.write("b.yaml", INPUT_YAML.as_bytes());
        let third = directory.write("c.yaml", INPUT_YAML.as_bytes());
        let mut replaced = false;
        let mut prepared_count = 0;

        let (result, stdout, stderr) = run_write_with_hook(
            vec![first.clone(), second.clone(), third.clone()],
            |event| {
                if matches!(event, WriteEvent::TemporaryPrepared) {
                    prepared_count += 1;
                    if prepared_count == 2 && !replaced {
                        fs::remove_file(&second).unwrap();
                        fs::create_dir(&second).unwrap();
                        fs::write(second.join("sentinel"), b"preserve").unwrap();
                        replaced = true;
                    }
                }
            },
        );

        assert!(replaced);
        assert_eq!(result.unwrap(), 2);
        assert!(stdout.is_empty());
        let diagnostic = String::from_utf8(stderr).unwrap();
        assert!(diagnostic.starts_with(&format!("oafmt: cannot replace {}:", second.display())));
        assert!(diagnostic.contains("destination is not a regular file"));
        assert_eq!(fs::read(first).unwrap(), EXPECTED_YAML.as_bytes());
        assert_eq!(fs::read(second.join("sentinel")).unwrap(), b"preserve");
        assert_eq!(fs::read(third).unwrap(), EXPECTED_YAML.as_bytes());
        directory.assert_no_temporary_files();
    }

    #[cfg(unix)]
    #[test]
    fn final_symlink_substitution_does_not_change_its_target() {
        use std::os::unix::fs::symlink;

        let directory = TestDir::new();
        let path = directory.write("api.yaml", INPUT_YAML.as_bytes());
        let target = directory.write("target.yaml", EXPECTED_YAML.as_bytes());
        let mut substituted = false;

        let (result, stdout, stderr) = run_write_with_hook(vec![path.clone()], |event| {
            if matches!(event, WriteEvent::TemporaryPrepared) && !substituted {
                fs::remove_file(&path).unwrap();
                symlink(&target, &path).unwrap();
                substituted = true;
            }
        });

        assert!(substituted);
        assert_eq!(result.unwrap(), 2);
        assert!(stdout.is_empty());
        let diagnostic = String::from_utf8(stderr).unwrap();
        assert!(diagnostic.starts_with(&format!("oafmt: cannot replace {}:", path.display())));
        assert!(diagnostic.contains("destination is not a regular file"));
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target).unwrap(), EXPECTED_YAML.as_bytes());
        directory.assert_no_temporary_files();
    }

    fn run_write_with_hook(paths: Vec<PathBuf>, mut hook: impl FnMut(WriteEvent)) -> CapturedWrite {
        let files = paths
            .into_iter()
            .map(|path| FileInput {
                key: path.clone(),
                display: path.clone(),
                path,
            })
            .collect();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = run_files_with_writers(
            Mode::Write,
            files,
            Vec::new(),
            &mut stdout,
            &mut stderr,
            &mut hook,
        );
        (result, stdout, stderr)
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "oafmt-write-contract-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }

        fn write(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, contents).unwrap();
            path
        }

        fn assert_no_temporary_files(&self) {
            assert!(self.temporary_files().is_empty());
        }

        fn temporary_files(&self) -> Vec<PathBuf> {
            fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .filter(|name| name.to_string_lossy().contains(".oafmt."))
                .map(|name| self.0.join(name))
                .collect()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
