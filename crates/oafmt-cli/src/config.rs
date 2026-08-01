use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::discovery::{normalize_lexically, symlink_precedes_parent, validate_pattern};

#[derive(Debug)]
pub(super) struct Config {
    pub(super) directory: PathBuf,
    pub(super) include: Option<Vec<String>>,
    pub(super) exclude: Vec<String>,
    pub(super) respect_gitignore: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    discovery: Option<DiscoveryConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoveryConfig {
    include: Option<Vec<String>>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default = "default_true")]
    respect_gitignore: bool,
}

const fn default_true() -> bool {
    true
}

pub(super) fn load(explicit: Option<&Path>, cwd: Option<&Path>) -> Result<Config, String> {
    let path = match explicit {
        Some(path) => {
            let unresolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.ok_or_else(|| "cannot determine current directory".to_owned())?
                    .join(path)
            };
            if symlink_precedes_parent(&unresolved, None) {
                return Err(format!(
                    "configuration path contains '..' after a symlink: {}",
                    path.display()
                ));
            }
            let path = normalize_lexically(&unresolved);
            if !path.is_file() {
                return Err(format!(
                    "configuration file does not exist: {}",
                    path.display()
                ));
            }
            Some(path)
        }
        None => cwd.and_then(find_nearest),
    };

    let Some(path) = path else {
        return Ok(default_config(
            cwd.unwrap_or_else(|| Path::new(".")).to_path_buf(),
        ));
    };
    let source = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read configuration {}: {error}", path.display()))?;
    let parsed: FileConfig = toml::from_str(&source)
        .map_err(|error| format!("invalid configuration {}: {error}", path.display()))?;
    let discovery = parsed.discovery.unwrap_or(DiscoveryConfig {
        include: None,
        exclude: Vec::new(),
        respect_gitignore: true,
    });
    if discovery.include.as_ref().is_some_and(Vec::is_empty) {
        return Err(format!(
            "invalid configuration {}: discovery.include must not be empty",
            path.display()
        ));
    }
    for pattern in discovery
        .include
        .iter()
        .flatten()
        .chain(discovery.exclude.iter())
    {
        validate_pattern(pattern).map_err(|error| {
            format!(
                "invalid configuration {}: pattern {pattern:?}: {error}",
                path.display()
            )
        })?;
    }
    Ok(Config {
        directory: path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        include: discovery.include,
        exclude: discovery.exclude,
        respect_gitignore: discovery.respect_gitignore,
    })
}

const fn default_config(directory: PathBuf) -> Config {
    Config {
        directory,
        include: None,
        exclude: Vec::new(),
        respect_gitignore: true,
    }
}

fn find_nearest(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .map(|directory| directory.join("oafmt.toml"))
        .find(|path| path.is_file())
}
