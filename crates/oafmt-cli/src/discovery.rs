use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use ignore::WalkBuilder;
use wax::{Glob, Program};

use crate::Mode;
use crate::config::Config;

#[derive(Debug)]
pub(super) struct FileInput {
    pub(super) path: PathBuf,
    pub(super) display: PathBuf,
    pub(super) key: PathBuf,
}

#[derive(Debug)]
pub(super) struct Failure {
    pub(super) key: PathBuf,
    pub(super) message: String,
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    display: PathBuf,
    explicit: bool,
}

#[derive(Debug, Default)]
struct DiscoveryState {
    selected: BTreeMap<PathBuf, Candidate>,
    failures: Vec<Failure>,
}

pub(super) struct Resolution {
    pub(super) files: Vec<FileInput>,
    pub(super) failures: Vec<Failure>,
}

pub(super) fn resolve(
    selectors: Vec<PathBuf>,
    mode: Mode,
    cwd: Option<&Path>,
    config: &Config,
) -> Result<Resolution, String> {
    let include = compile_set(config.include.as_deref().unwrap_or(&[])).map_err(|error| {
        format!("internal invariant: validated include pattern failed to compile: {error}")
    })?;
    let exclude = compile_set(&config.exclude).map_err(|error| {
        format!("internal invariant: validated exclude pattern failed to compile: {error}")
    })?;
    let mut state = DiscoveryState::default();

    for selector in selectors {
        let absolute = absolute(&selector, cwd);
        let key = normalize_lexically(&absolute);
        let link_metadata = fs::symlink_metadata(&selector);
        let is_glob = is_native_glob(&selector);
        if mode == Mode::Write && !is_glob && prefix_contains_symlink(&selector, cwd) {
            state.failures.push(Failure {
                key,
                message: format!(
                    "cannot discover through symlink in write mode: {}",
                    selector.display()
                ),
            });
            continue;
        }
        if is_glob {
            discover_glob(&selector, mode, cwd, config, exclude.as_deref(), &mut state);
        } else {
            match link_metadata {
                Ok(_) => match fs::metadata(&selector) {
                    Ok(metadata) if metadata.is_file() => {
                        let identity = fs::canonicalize(&absolute).unwrap_or(key);
                        insert_candidate(
                            &mut state.selected,
                            identity.clone(),
                            identity,
                            selector,
                            true,
                        );
                    }
                    Ok(metadata) if metadata.is_dir() => {
                        discover_directory(
                            &selector,
                            &absolute,
                            config,
                            include.as_deref(),
                            exclude.as_deref(),
                            &mut state,
                        );
                    }
                    Ok(_) => state.failures.push(Failure {
                        key,
                        message: format!(
                            "input is not a file or directory: {}",
                            selector.display()
                        ),
                    }),
                    Err(error) => state.failures.push(Failure {
                        key,
                        message: format!("cannot read {}: {error}", selector.display()),
                    }),
                },
                Err(_) => {
                    insert_candidate(&mut state.selected, key, selector.clone(), selector, true);
                }
            }
        }
    }

    state.failures.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.message.cmp(&right.message))
    });
    let files = state
        .selected
        .into_iter()
        .map(|(key, candidate)| FileInput {
            path: candidate.path,
            display: candidate.display,
            key,
        })
        .collect();
    Ok(Resolution {
        files,
        failures: state.failures,
    })
}

fn discover_directory(
    display_root: &Path,
    absolute_root: &Path,
    config: &Config,
    include: Option<&[Glob<'static>]>,
    exclude: Option<&[Glob<'static>]>,
    state: &mut DiscoveryState,
) {
    let walk_root = fs::canonicalize(absolute_root).unwrap_or_else(|_| absolute_root.to_path_buf());
    let mut count = 0;
    if !contains_vcs_metadata(display_root) && !contains_vcs_metadata(&walk_root) {
        walk(
            &walk_root,
            config.respect_gitignore,
            |path, file_type| {
                if !file_type.is_file() || file_type.is_symlink() || !supported_extension(path) {
                    return;
                }
                let Ok(relative_to_root) = path.strip_prefix(&walk_root) else {
                    return;
                };
                if contains_vcs_metadata(relative_to_root) {
                    return;
                }
                let display = display_root.join(relative_to_root);
                let identity = normalize_lexically(path);
                let lexical_candidate = absolute_root.join(relative_to_root);
                let config_relative = relative_path(&config.directory, &lexical_candidate);
                let included = include.map_or_else(
                    || {
                        matches!(
                            path.file_name().and_then(OsStr::to_str),
                            Some("openapi.yaml" | "openapi.yml" | "openapi.json")
                        )
                    },
                    |patterns| matches_any(patterns, &config_relative),
                );
                if included
                    && !exclude.is_some_and(|patterns| matches_any(patterns, &config_relative))
                {
                    count += 1;
                    insert_candidate(
                        &mut state.selected,
                        identity,
                        path.to_path_buf(),
                        remove_current_components(&display),
                        false,
                    );
                }
            },
            |message| {
                state.failures.push(Failure {
                    key: normalize_lexically(absolute_root),
                    message,
                });
            },
        );
    }
    if count == 0 {
        state.failures.push(Failure {
            key: normalize_lexically(absolute_root),
            message: format!(
                "selector produced no supported candidates: {}",
                display_root.display()
            ),
        });
    }
}

fn discover_glob(
    selector: &Path,
    mode: Mode,
    cwd: Option<&Path>,
    config: &Config,
    exclude: Option<&[Glob<'static>]>,
    state: &mut DiscoveryState,
) {
    let Some(pattern) = selector.to_str() else {
        state.failures.push(Failure {
            key: normalize_lexically(&absolute(selector, cwd)),
            message: format!("glob selector is not valid UTF-8: {}", selector.display()),
        });
        return;
    };
    let plan = match prepare_glob(pattern, cwd) {
        Ok(plan) => plan,
        Err(error) => {
            state.failures.push(Failure {
                key: normalize_lexically(&absolute(selector, cwd)),
                message: format!("invalid glob selector {pattern:?}: {error}"),
            });
            return;
        }
    };
    if mode == Mode::Write && prefix_contains_symlink(&plan.syntactic_prefix, cwd) {
        state.failures.push(Failure {
            key: normalize_lexically(&absolute(selector, cwd)),
            message: format!(
                "cannot discover through symlink in write mode: {}",
                selector.display()
            ),
        });
        return;
    }
    let root = absolute(&plan.prefix, cwd);
    let walk_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let mut count = 0;
    if !contains_vcs_metadata(&plan.prefix) && !contains_vcs_metadata(&walk_root) {
        walk(
            &walk_root,
            config.respect_gitignore,
            |path, file_type| {
                if !file_type.is_file() || file_type.is_symlink() || !supported_extension(path) {
                    return;
                }
                let Ok(match_path) = path.strip_prefix(&walk_root) else {
                    return;
                };
                if contains_vcs_metadata(match_path) {
                    return;
                }
                let identity = normalize_lexically(path);
                let lexical_candidate = root.join(match_path);
                let config_relative = relative_path(&config.directory, &lexical_candidate);
                if plan.matcher.is_match(match_path)
                    && !exclude.is_some_and(|patterns| matches_any(patterns, &config_relative))
                {
                    count += 1;
                    insert_candidate(
                        &mut state.selected,
                        identity,
                        path.to_path_buf(),
                        glob_display(&plan.prefix, match_path, cwd),
                        false,
                    );
                }
            },
            |message| {
                state.failures.push(Failure {
                    key: normalize_lexically(&absolute(selector, cwd)),
                    message,
                });
            },
        );
    }
    if count == 0 {
        state.failures.push(Failure {
            key: normalize_lexically(&absolute(selector, cwd)),
            message: format!(
                "selector produced no supported candidates: {}",
                selector.display()
            ),
        });
    }
}

#[derive(Debug)]
struct GlobPlan {
    prefix: PathBuf,
    matcher: Glob<'static>,
    syntactic_prefix: PathBuf,
}

fn prepare_glob(pattern: &str, cwd: Option<&Path>) -> Result<GlobPlan, String> {
    let normalized = normalize_pattern(pattern);
    validate_frozen_dialect(&normalized)?;
    let glob = Glob::new(&normalized)
        .map(Glob::into_owned)
        .map_err(|error| error.to_string())?;
    let components = pattern_components(&normalized);
    let first_variant = components
        .iter()
        .position(|component| component_is_variant(component))
        .ok_or_else(|| "glob has no variant component".to_owned())?;
    let syntactic_prefix =
        path_from_pattern_components(normalized.starts_with('/'), &components[..first_variant]);
    let syntactic_matcher = components[first_variant..].join("/");
    let (wax_prefix, wax_matcher) = glob.partition_or_empty();
    let (prefix, matcher) = if wax_prefix == syntactic_prefix {
        (wax_prefix, wax_matcher)
    } else {
        let mut prefix = syntactic_prefix.clone();
        let mut matcher = syntactic_matcher;
        if first_variant > 0
            && !prefix_contains_symlink(&syntactic_prefix, cwd)
            && matches!(
                syntactic_prefix.components().next_back(),
                Some(Component::Normal(_))
            )
        {
            prefix.pop();
            matcher = format!("{}/{matcher}", components[first_variant - 1]);
        }
        let matcher = Glob::new(&matcher)
            .map(Glob::into_owned)
            .map_err(|error| format!("validated glob suffix failed to compile: {error}"))?;
        (prefix, matcher)
    };
    Ok(GlobPlan {
        prefix,
        matcher,
        syntactic_prefix,
    })
}

fn prefix_contains_symlink(invariant_prefix: &Path, cwd: Option<&Path>) -> bool {
    let mut prefix = if invariant_prefix.is_absolute() {
        PathBuf::new()
    } else {
        cwd.unwrap_or_else(|| Path::new(".")).to_path_buf()
    };
    for component in invariant_prefix.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                prefix.push(component.as_os_str());
            }
            Component::ParentDir | Component::Normal(_) => {
                prefix.push(component.as_os_str());
                if fs::symlink_metadata(&prefix)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return true;
                }
            }
        }
    }
    false
}

fn walk(
    root: &Path,
    respect_gitignore: bool,
    mut visit: impl FnMut(&Path, fs::FileType),
    mut fail: impl FnMut(String),
) {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(false)
        .ignore(false)
        .git_ignore(respect_gitignore)
        .git_global(false)
        .git_exclude(false)
        .require_git(true)
        .filter_entry(|entry| !is_vcs_metadata(entry.path()));
    for entry in builder.build() {
        match entry {
            Ok(entry) => {
                if let Some(file_type) = entry.file_type() {
                    visit(entry.path(), file_type);
                }
            }
            Err(error) => fail(format!("cannot traverse {}: {error}", root.display())),
        }
    }
}

fn is_vcs_metadata(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(".git" | ".hg" | ".svn")
    )
}

fn contains_vcs_metadata(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(name) if matches!(name.to_str(), Some(".git" | ".hg" | ".svn"))
        )
    })
}

fn glob_display(prefix: &Path, match_path: &Path, cwd: Option<&Path>) -> PathBuf {
    let display = prefix.join(match_path);
    if symlink_precedes_parent(prefix, cwd) {
        remove_current_components(&display)
    } else {
        let identity = normalize_lexically(&absolute(&display, cwd));
        discovered_display(&identity, cwd)
    }
}

pub(super) fn symlink_precedes_parent(path: &Path, cwd: Option<&Path>) -> bool {
    let mut prefix = if path.is_absolute() {
        PathBuf::new()
    } else {
        cwd.unwrap_or_else(|| Path::new(".")).to_path_buf()
    };
    let mut saw_symlink = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                prefix.push(component.as_os_str());
            }
            Component::ParentDir => {
                if saw_symlink {
                    return true;
                }
                prefix.push(component.as_os_str());
            }
            Component::Normal(_) => {
                prefix.push(component.as_os_str());
                saw_symlink |= fs::symlink_metadata(&prefix)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink());
            }
        }
    }
    false
}

fn remove_current_components(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        if component != Component::CurDir {
            cleaned.push(component.as_os_str());
        }
    }
    cleaned
}

fn insert_candidate(
    selected: &mut BTreeMap<PathBuf, Candidate>,
    key: PathBuf,
    path: PathBuf,
    display: PathBuf,
    explicit: bool,
) {
    selected
        .entry(key)
        .and_modify(|current| {
            if (explicit && !current.explicit)
                || (explicit == current.explicit
                    && display.as_os_str() < current.display.as_os_str())
            {
                current.path.clone_from(&path);
                current.display.clone_from(&display);
                current.explicit = explicit;
            }
        })
        .or_insert(Candidate {
            path,
            display,
            explicit,
        });
}

fn compile_set(patterns: &[String]) -> Result<Option<Vec<Glob<'static>>>, String> {
    if patterns.is_empty() {
        return Ok(None);
    }
    patterns
        .iter()
        .map(|pattern| compile_frozen_pattern(pattern))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(super) fn validate_pattern(pattern: &str) -> Result<(), String> {
    compile_frozen_pattern(pattern).map(|_| ())
}

fn matches_any(patterns: &[Glob<'static>], path: &Path) -> bool {
    patterns.iter().any(|pattern| pattern.is_match(path))
}

fn compile_frozen_pattern(pattern: &str) -> Result<Glob<'static>, String> {
    let pattern = normalize_pattern(pattern);
    validate_frozen_dialect(&pattern)?;
    Glob::new(&pattern)
        .map(Glob::into_owned)
        .map_err(|error| error.to_string())
}

fn normalize_pattern(pattern: &str) -> String {
    let rooted = pattern.starts_with('/');
    let components = pattern_components(pattern)
        .into_iter()
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>();
    let normalized = components.join("/");
    if rooted {
        format!("/{normalized}")
    } else {
        normalized
    }
}

fn validate_frozen_dialect(pattern: &str) -> Result<(), String> {
    let components = pattern_components(pattern);
    for component in &components {
        let mut in_character_class = false;
        let mut characters = component.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '[' if !in_character_class => {
                    in_character_class = true;
                }
                ']' if in_character_class => in_character_class = false,
                '{' | '}' if !in_character_class => {
                    return Err("brace expansion is not supported".to_owned());
                }
                '<' | '>' if !in_character_class => {
                    return Err("repetition is not supported".to_owned());
                }
                '$' if !in_character_class => {
                    return Err("Wax zero-or-more wildcards are not supported".to_owned());
                }
                '\\' => return Err("escapes are not supported".to_owned()),
                '(' if !in_character_class && characters.peek() == Some(&'?') => {
                    return Err("flags are not supported".to_owned());
                }
                _ => {}
            }
        }
    }
    if components
        .iter()
        .scan(false, |seen, component| {
            let after_variant = *seen && *component == "..";
            *seen |= component_is_variant(component);
            Some(after_variant)
        })
        .any(|after_variant| after_variant)
    {
        return Err("parent components after a wildcard are not supported".to_owned());
    }
    Ok(())
}

fn pattern_components(pattern: &str) -> Vec<&str> {
    let mut components = Vec::new();
    let mut start = 0;
    let mut in_character_class = false;
    for (index, character) in pattern.char_indices() {
        match character {
            '[' if !in_character_class => in_character_class = true,
            ']' if in_character_class => in_character_class = false,
            '/' if !in_character_class => {
                components.push(&pattern[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    components.push(&pattern[start..]);
    components
}

fn component_is_variant(component: &str) -> bool {
    let mut in_character_class = false;
    for character in component.chars() {
        match character {
            '[' | '*' | '?' if !in_character_class => return true,
            ']' if in_character_class => in_character_class = false,
            _ => {}
        }
    }
    false
}

fn path_from_pattern_components(rooted: bool, components: &[&str]) -> PathBuf {
    let mut path = if rooted {
        PathBuf::from("/")
    } else {
        PathBuf::new()
    };
    for component in components {
        path.push(component);
    }
    path
}

fn is_native_glob(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|path| path.contains(['*', '?', '[']))
}

fn supported_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("yaml" | "yml" | "json")
    )
}

fn discovered_display(identity: &Path, cwd: Option<&Path>) -> PathBuf {
    cwd.and_then(|cwd| identity.strip_prefix(cwd).ok())
        .filter(|path| !path.as_os_str().is_empty())
        .map_or_else(|| identity.to_path_buf(), Path::to_path_buf)
}

fn absolute(path: &Path, cwd: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.unwrap_or_else(|| Path::new(".")).join(path)
    }
}

fn relative_path(base: &Path, target: &Path) -> PathBuf {
    let base = normalize_lexically(base);
    let target = normalize_lexically(target);
    let mut base_components = base.components().peekable();
    let mut target_components = target.components().peekable();
    while base_components.peek() == target_components.peek() && base_components.peek().is_some() {
        base_components.next();
        target_components.next();
    }
    let mut relative = PathBuf::new();
    for component in base_components {
        if !matches!(component, Component::RootDir | Component::Prefix(_)) {
            relative.push("..");
        }
    }
    for component in target_components {
        relative.push(component.as_os_str());
    }
    relative
}

pub(super) fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
