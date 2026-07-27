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
    File(PathBuf),
    Stdin(PathBuf),
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
    let (source, display_path, format_kind, file_path) = match &options.input {
        Input::File(path) => {
            let metadata = fs::metadata(path)
                .map_err(|error| user_error(format!("cannot read {}: {error}", path.display())))?;
            if !metadata.is_file() {
                return Err(user_error(format!(
                    "input is not a file: {}",
                    path.display()
                )));
            }
            let source = fs::read_to_string(path)
                .map_err(|error| user_error(format!("cannot read {}: {error}", path.display())))?;
            (
                source,
                path.display().to_string(),
                infer_format(path)?,
                Some(path.as_path()),
            )
        }
        Input::Stdin(path) => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .map_err(|error| user_error(format!("cannot read stdin: {error}")))?;
            (
                source,
                path.display().to_string(),
                infer_format(path)?,
                None,
            )
        }
    };

    let result = format(&source, format_kind).map_err(|error| match error {
        FormatError::Input(message) => user_error(message),
        FormatError::InternalInvariant(message) => (3, message),
    })?;

    match options.mode {
        Mode::Stdout => {
            io::stdout()
                .write_all(result.output.as_bytes())
                .map_err(|error| user_error(format!("cannot write stdout: {error}")))?;
            Ok(0)
        }
        Mode::Write => {
            let path = file_path.expect("--write with stdin is rejected during argument parsing");
            if result.changed {
                atomic_replace(path, result.output.as_bytes()).map_err(|error| {
                    user_error(format!("cannot replace {}: {error}", path.display()))
                })?;
            }
            Ok(0)
        }
        Mode::Check if result.changed => {
            eprintln!("oafmt: formatting changes required: {display_path}");
            Ok(1)
        }
        Mode::Check => Ok(0),
        Mode::Diff => {
            if result.changed {
                let diff = unified_diff(&display_path, &source, &result.output);
                io::stdout()
                    .write_all(diff.as_bytes())
                    .map_err(|error| user_error(format!("cannot write stdout: {error}")))?;
            }
            Ok(0)
        }
    }
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Options, (u8, String)> {
    let mut args = args.peekable();
    let mut mode = None;
    let mut stdin_path = None;
    let mut file = None;

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
        } else if file.replace(PathBuf::from(argument)).is_some() {
            return Err(user_error("exactly one input is supported"));
        }
    }

    if file.is_some() && stdin_path.is_some() {
        return Err(user_error(
            "a file and --stdin-filepath cannot be used together",
        ));
    }
    let input = match (file, stdin_path) {
        (Some(path), None) => Input::File(path),
        (None, Some(path)) => Input::Stdin(path),
        (None, None) => return Err(user_error("one input file is required")),
        (Some(_), Some(_)) => unreachable!("handled above"),
    };
    let mode = mode.unwrap_or(Mode::Stdout);
    if mode == Mode::Write && matches!(input, Input::Stdin(_)) {
        return Err(user_error("--write cannot be used with --stdin-filepath"));
    }
    Ok(Options { mode, input })
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
