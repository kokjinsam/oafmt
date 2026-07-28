use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oafmt_core::{FormatError, InputFormat, format};

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
    Files(Vec<FileInput>),
    Stdin(PathBuf),
}

struct FileInput {
    path: PathBuf,
    key: PathBuf,
}

struct PreparedFile {
    path: PathBuf,
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

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err((code, message)) => {
            eprintln!("oafmt: {message}");
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
                    eprintln!("oafmt: formatting changes required: {}", path.display());
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
                Mode::Write => {
                    unreachable!("--write with stdin is rejected during argument parsing")
                }
            }
        }
        Input::Files(files) => run_files(options.mode, files),
    }
}

fn run_files(mode: Mode, files: Vec<FileInput>) -> Result<u8, (u8, String)> {
    let preflight: Vec<_> = files
        .into_iter()
        .map(|file| preflight_file(mode, file))
        .collect();

    match mode {
        Mode::Stdout => match preflight
            .into_iter()
            .next()
            .expect("argument parsing requires one input")
        {
            Preflight::Ready(file) => {
                io::stdout()
                    .write_all(file.output.as_bytes())
                    .map_err(|error| user_error(format!("cannot write stdout: {error}")))?;
                Ok(0)
            }
            Preflight::Failed(failure) => Err((failure.severity, failure.message)),
        },
        Mode::Write => run_write(preflight),
        Mode::Check => Ok(run_check(preflight)),
        Mode::Diff => run_diff(preflight),
    }
}

fn preflight_file(mode: Mode, input: FileInput) -> Preflight {
    match prepare_file(mode, &input.path) {
        Ok(file) => Preflight::Ready(file),
        Err((severity, message)) => Preflight::Failed(Failure { severity, message }),
    }
}

fn prepare_file(mode: Mode, path: &Path) -> Result<PreparedFile, (u8, String)> {
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|error| user_error(format!("cannot read {}: {error}", path.display())))?;
    if mode == Mode::Write && link_metadata.file_type().is_symlink() {
        return Err(user_error(format!(
            "cannot replace symlink: {}",
            path.display()
        )));
    }
    let metadata = fs::metadata(path)
        .map_err(|error| user_error(format!("cannot read {}: {error}", path.display())))?;
    if !metadata.is_file() {
        return Err(user_error(format!(
            "input is not a file: {}",
            path.display()
        )));
    }
    let format_kind = infer_format(path)?;
    let source = fs::read_to_string(path)
        .map_err(|error| user_error(format!("cannot read {}: {error}", path.display())))?;
    let result = format(&source, format_kind).map_err(|error| {
        let (severity, message) = classify_format_error(error);
        (
            severity,
            format!("cannot format {}: {message}", path.display()),
        )
    })?;
    Ok(PreparedFile {
        path: path.to_path_buf(),
        source,
        output: result.output,
        changed: result.changed,
    })
}

fn run_check(preflight: Vec<Preflight>) -> u8 {
    let mut severities = Vec::with_capacity(preflight.len());
    for result in preflight {
        match result {
            Preflight::Ready(file) if file.changed => {
                eprintln!(
                    "oafmt: formatting changes required: {}",
                    file.path.display()
                );
                severities.push(1);
            }
            Preflight::Ready(_) => severities.push(0),
            Preflight::Failed(failure) => {
                eprintln!("oafmt: {}", failure.message);
                severities.push(failure.severity);
            }
        }
    }
    aggregate_exit(severities)
}

fn run_diff(preflight: Vec<Preflight>) -> Result<u8, (u8, String)> {
    let mut output = String::new();
    let mut severities = Vec::with_capacity(preflight.len());
    for result in preflight {
        match result {
            Preflight::Ready(file) => {
                if file.changed {
                    output.push_str(&unified_diff(
                        &file.path.display().to_string(),
                        &file.source,
                        &file.output,
                    ));
                }
                severities.push(0);
            }
            Preflight::Failed(failure) => {
                eprintln!("oafmt: {}", failure.message);
                severities.push(failure.severity);
            }
        }
    }
    let collected_severity = aggregate_exit(severities);
    write_diff_output(&mut io::stdout(), &output, collected_severity)
}

fn run_write(preflight: Vec<Preflight>) -> Result<u8, (u8, String)> {
    let preflight_exit = aggregate_exit(preflight.iter().filter_map(|result| match result {
        Preflight::Ready(_) => None,
        Preflight::Failed(failure) => Some(failure.severity),
    }));
    if preflight_exit != 0 {
        for result in preflight {
            if let Preflight::Failed(failure) = result {
                eprintln!("oafmt: {}", failure.message);
            }
        }
        return Ok(preflight_exit);
    }

    let mut severities = Vec::with_capacity(preflight.len());
    for result in preflight {
        let Preflight::Ready(file) = result else {
            unreachable!("preflight failures returned before replacement");
        };
        if file.changed {
            match atomic_replace(&file.path, file.output.as_bytes()) {
                Ok(()) => severities.push(0),
                Err(error) => {
                    eprintln!("oafmt: cannot replace {}: {error}", file.path.display());
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
    let mut args = args.peekable();
    let mut mode = None;
    let mut stdin_path = None;
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
    if mode == Mode::Write && stdin_path.is_some() {
        return Err(user_error("--write cannot be used with --stdin-filepath"));
    }
    if mode == Mode::Stdout && files.len() > 1 {
        return Err(user_error("stdout mode accepts exactly one input file"));
    }
    let input = if let Some(path) = stdin_path {
        Input::Stdin(path)
    } else if files.is_empty() {
        return Err(user_error("one input file is required"));
    } else {
        Input::Files(sort_and_deduplicate(files)?)
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
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                current_directory
                    .as_ref()
                    .expect("relative inputs require a current directory")
                    .join(&path)
            };
            FileInput {
                key: normalize_lexically(&absolute),
                path,
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.path.as_os_str().cmp(right.path.as_os_str()))
    });
    files.dedup_by(|right, left| right.key == left.key);
    Ok(files)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
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

fn atomic_replace(path: &Path, contents: &[u8]) -> io::Result<()> {
    let permissions = fs::metadata(path)?.permissions();
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
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.set_permissions(permissions)?;
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary file",
    ))
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

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{aggregate_exit, write_diff_output};

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
}
