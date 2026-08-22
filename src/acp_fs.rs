//! ACP `fs/read_text_file` / `fs/write_text_file` (Backchat policy).
//!
//! Reads any path. Writes inside the session cwd are silent; writes outside
//! ask through the permission overlay. Plugins never see the TTY.

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::Sender;

use agent_client_protocol::Error as AcpError;

use crate::bus::{AppEvent, PermissionAskOption, PermissionAskReply};

pub fn slice_text(text: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let start = (line.unwrap_or(1).saturating_sub(1) as usize).min(lines.len());
    let end = match limit {
        Some(n) => start.saturating_add(n as usize).min(lines.len()),
        None => lines.len(),
    };
    lines[start..end].join("\n")
}

/// Backchat `path.resolve` without requiring the path to exist.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn resolve_with_cwd(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&cwd.join(path))
    }
}

/// True when `target` is the cwd or a path under it (not a sibling prefix).
pub fn is_inside_cwd(target: &Path, cwd: &str) -> bool {
    if cwd.is_empty() {
        return false;
    }
    let resolved = resolve_with_cwd(target, Path::new(cwd));
    let root = normalize_path(Path::new(cwd));
    if resolved == root {
        return true;
    }
    let mut rest = resolved.components();
    for component in root.components() {
        match rest.next() {
            Some(next) if next == component => {}
            _ => return false,
        }
    }
    true
}

pub fn read_text_file(
    path: &Path,
    line: Option<u32>,
    limit: Option<u32>,
) -> Result<String, AcpError> {
    let text = std::fs::read_to_string(path).map_err(io_err)?;
    Ok(slice_text(&text, line, limit))
}

pub fn write_text_file(path: &Path, content: &str) -> Result<(), AcpError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
    }
    std::fs::write(path, content).map_err(io_err)
}

/// Ask through the existing permission overlay: AllowOnce vs reject.
pub async fn confirm_write_outside(bus: &Sender<AppEvent>, path: &Path) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let options = vec![
        PermissionAskOption {
            option_id: "deny".into(),
            kind: "reject_once".into(),
            name: "Deny".into(),
        },
        PermissionAskOption {
            option_id: "allow".into(),
            kind: "allow_once".into(),
            name: "Allow write".into(),
        },
    ];
    if bus
        .send(AppEvent::PermissionAsk {
            title: format!("write {} · outside workspace", path.display()),
            options,
            reply: tx,
        })
        .is_err()
    {
        return false;
    }
    matches!(
        rx.await.unwrap_or(PermissionAskReply::Cancelled),
        PermissionAskReply::Selected(id) if id == "allow"
    )
}

pub fn denied_write() -> AcpError {
    AcpError::new(-32603, "user denied write")
}

fn io_err(err: std::io::Error) -> AcpError {
    AcpError::new(-32603, err.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/acp_fs__tests.rs"]
mod tests;

