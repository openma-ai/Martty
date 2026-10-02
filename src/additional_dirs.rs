//! Client-side checks for ACP `additionalDirectories`.
//!
//! The field is a standard session-lifecycle parameter. These checks reject
//! paths the protocol will not accept (relative, filesystem root, `$HOME`)
//! before a session request is sent. They do not depend on a particular agent.

use std::path::{Component, Path, PathBuf};

/// Validate `--add-dir` values against the session `cwd`.
///
/// Entries equal to `cwd` are dropped. An entry that is only the workspace
/// is an error, so a flag that changes nothing is not silent. The returned
/// paths are canonical absolute directories, in first-seen order.
pub fn validate(
    requested: &[&str],
    cwd: &str,
    home: Option<&str>,
) -> Result<Vec<String>, String> {
    let primary = canonicalize_dir(cwd).unwrap_or_else(|_| normalize_lexically(Path::new(cwd)));
    let home = home.and_then(|path| {
        if path.is_empty() {
            None
        } else {
            Some(canonicalize_dir(path).unwrap_or_else(|_| normalize_lexically(Path::new(path))))
        }
    });
    let mut out = Vec::new();
    let mut dropped_workspace = Vec::new();
    for entry in requested {
        match classify(entry, &primary, home.as_deref())? {
            Some(canonical) => {
                if !out.iter().any(|kept: &String| kept == &canonical) {
                    out.push(canonical);
                }
            }
            None => dropped_workspace.push((*entry).to_string()),
        }
    }
    if out.is_empty() && !dropped_workspace.is_empty() {
        return Err(format!(
            "additional directory must not be the workspace: {}",
            dropped_workspace.join(", ")
        ));
    }
    Ok(out)
}

/// `Ok(None)` when `entry` is the workspace and should be omitted.
fn classify(entry: &str, primary: &Path, home: Option<&Path>) -> Result<Option<String>, String> {
    if entry.is_empty() {
        return Err("additional directory must be a non-empty absolute path".into());
    }
    let path = Path::new(entry);
    if !path.is_absolute() {
        return Err(format!(
            "additional directory must be an absolute path: {entry}"
        ));
    }
    if is_filesystem_root(path) {
        return Err(format!(
            "additional directory must not be a filesystem root: {entry}"
        ));
    }
    let canonical = canonicalize_dir(entry).map_err(|_| {
        format!("additional directory is not an existing directory: {entry}")
    })?;
    if is_filesystem_root(&canonical) {
        return Err(format!(
            "additional directory must not be a filesystem root: {entry}"
        ));
    }
    if home.is_some_and(|home| canonical == home) {
        return Err(format!(
            "additional directory must not be the home directory: {entry}"
        ));
    }
    if canonical == primary {
        return Ok(None);
    }
    Ok(Some(canonical.to_string_lossy().into_owned()))
}

fn canonicalize_dir(path: &str) -> Result<PathBuf, ()> {
    let canonical = std::fs::canonicalize(path).map_err(|_| ())?;
    if !canonical.is_dir() {
        return Err(());
    }
    Ok(canonical)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

pub fn is_filesystem_root(path: &Path) -> bool {
    let mut components = path.components();
    match components.next() {
        Some(Component::RootDir) => components.next().is_none(),
        Some(Component::Prefix(_)) => {
            matches!(components.next(), Some(Component::RootDir) | None)
                && components.next().is_none()
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/additional_dirs__tests.rs"]
mod tests;
