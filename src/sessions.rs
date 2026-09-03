//! Durable-session discovery and loading for `/resume`.
//!
//! The harness persists one JSONL event log per session:
//!
//! ```text
//! <root>/<workspace-slug>/<session-id>/session.jsonl[.zstd]
//! ```
//!
//! Roots: the configured `session_root` and the local dsh store
//! `~/.dsh/sessions`. A flat `<root>/<session-id>/session.jsonl` layout is tolerated
//! too. The workspace slug is the absolute path with `/` mapped to `-`,
//! wrapped in `-…--` (observed: `/Users/x/proj` → `--Users-x-proj--`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde_json::Value;

/// One resumable session, summarized for the picker.
#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub id: String,
    pub file: PathBuf,
    pub modified: SystemTime,
    pub turns: usize,
    /// First real user prompt (truncated) — the session's human handle.
    pub preview: String,
    /// Harness-generated title from `session/title` events, when present.
    /// The LLM ("provider" source) title wins over the truncated-prompt
    /// "fallback" stub.
    pub title: Option<String>,
}

/// `/Users/x/proj` → `--Users-x-proj--` (the harness's directory slug).
pub fn workspace_slug(workspace: &str) -> String {
    format!("-{}--", workspace.replace('/', "-"))
}

/// Candidate session roots, existing ones only: the configured root plus
/// the local dsh store.
fn session_roots_from(cfg_root: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(cfg_root)];
    if let Some(home) = home {
        roots.push(home.join(".dsh").join("sessions"));
        roots.push(home.join(".dsh-tui").join("sessions"));
    }
    roots.sort();
    roots.dedup();
    roots.retain(|r| r.is_dir());
    roots
}

fn session_roots(cfg_root: &str) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    session_roots_from(cfg_root, home.as_deref())
}

/// The session log inside one session directory, preferring the live
/// uncompressed file over the compacted `.zstd`.
fn session_file(dir: &Path) -> Option<PathBuf> {
    for name in ["session.jsonl", "session.jsonl.zstd"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// List resumable sessions for `workspace`, newest first, excluding
/// `skip_id` (the currently active session). Best effort: unreadable or
/// foreign files are skipped, never an error.
/// Default cap for the `/resume` picker when no explicit count is given
/// (`/resume` behaves like `/resume 50`).
pub const DEFAULT_SESSION_LIST_LIMIT: usize = 50;

pub fn list_sessions(
    cfg_root: &str,
    workspace: &str,
    skip_id: &str,
    limit: usize,
) -> Vec<SessionSummary> {
    let slug = workspace_slug(workspace);
    let mut candidate_files: Vec<(PathBuf, SystemTime)> = Vec::new();
    for root in session_roots(cfg_root) {
        // <root>/<slug>/<id>/session.jsonl[.zstd]
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root.join(&slug)) {
            dirs.extend(entries.flatten().map(|e| e.path()));
        }
        // Flat fallback: <root>/<id>/session.jsonl[.zstd]
        if let Ok(entries) = std::fs::read_dir(&root) {
            dirs.extend(entries.flatten().map(|e| e.path()));
        }
        for dir in dirs {
            if dir.file_name().and_then(|n| n.to_str()) == Some(skip_id) {
                continue;
            }
            if let Some(file) = session_file(&dir) {
                let mtime = std::fs::metadata(&file)
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                candidate_files.push((file, mtime));
            }
        }
    }
    candidate_files.sort_by(|(f1, m1), (f2, m2)| m2.cmp(m1).then_with(|| f1.cmp(f2)));
    candidate_files.dedup_by(|(f1, _), (f2, _)| f1 == f2);

    let mut out: Vec<SessionSummary> = Vec::new();
    for (file, _) in candidate_files {
        let Some(summary) = summarize(&file) else {
            continue;
        };
        if summary.id == skip_id || out.iter().any(|s| s.id == summary.id) {
            continue;
        }
        out.push(summary);
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Read and parse every JSONL event in a session log (zstd-aware).
pub fn read_session_events(file: &Path) -> Result<Vec<Value>> {
    let text = read_session_text(file)?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect())
}

fn read_session_text(file: &Path) -> Result<String> {
    let raw = std::fs::File::open(file).with_context(|| format!("open {}", file.display()))?;
    let mut text = String::new();
    if file.extension().is_some_and(|e| e == "zstd") {
        // Appended logs are a sequence of concatenated zstd frames (one per
        // flush); a single StreamingDecoder stops at the first frame
        // boundary, so keep decoding until the reader is exhausted.
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(raw);
        while !reader
            .fill_buf()
            .with_context(|| format!("read {}", file.display()))?
            .is_empty()
        {
            let mut dec = ruzstd::decoding::StreamingDecoder::new(&mut reader)
                .with_context(|| format!("zstd frame of {}", file.display()))?;
            dec.read_to_string(&mut text)
                .with_context(|| format!("decompress {}", file.display()))?;
        }
    } else {
        let mut raw = raw;
        raw.read_to_string(&mut text)
            .with_context(|| format!("read {}", file.display()))?;
    }
    Ok(text)
}

/// The first real user prompt inside a `user/message` event, if any.
pub fn user_text(event: &Value) -> Option<String> {
    if event.get("type").and_then(Value::as_str) != Some("user/message") {
        return None;
    }
    let data = event.get("data")?;
    if data.pointer("/source/kind").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let mut out = String::new();
    for block in data.get("content")?.as_array()? {
        if block.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(t) = block.get("text").and_then(Value::as_str) {
                out.push_str(t);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn summarize(file: &Path) -> Option<SessionSummary> {
    let events = read_session_events(file).ok()?;
    let header = events.first()?;
    if header.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    let id = header.get("id").and_then(Value::as_str)?.to_string();
    let turns = events
        .iter()
        .filter(|e| e.get("type").and_then(Value::as_str) == Some("turn/start"))
        .count();
    let preview = events
        .iter()
        .find_map(user_text)
        .map(|t| {
            let one_line = t.replace('\n', " ");
            let mut p: String = one_line.chars().take(40).collect();
            if one_line.chars().count() > 40 {
                p.push('…');
            }
            p
        })
        .unwrap_or_default();
    // The harness titles sessions asynchronously: a "fallback" stub derived
    // from the first prompt arrives first, the LLM-generated "provider"
    // title later. Keep the provider title when it exists.
    let mut title: Option<String> = None;
    let mut provider_title: Option<String> = None;
    for ev in &events {
        if ev.get("type").and_then(Value::as_str) != Some("session/title") {
            continue;
        }
        let Some(t) = ev.pointer("/data/title").and_then(Value::as_str) else {
            continue;
        };
        let t = t.trim().to_string();
        if t.is_empty() {
            continue;
        }
        if ev.pointer("/data/source/kind").and_then(Value::as_str) == Some("provider") {
            provider_title = Some(t);
        } else {
            title = Some(t);
        }
    }
    let title = provider_title.or(title);
    let modified = std::fs::metadata(file)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Some(SessionSummary {
        id,
        file: file.to_path_buf(),
        modified,
        turns,
        preview,
        title,
    })
}

/// "just now" / "5m" / "3h" / "2d" — picker-sized age.
pub fn age_label(modified: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(modified)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

#[cfg(test)]
#[path = "../tests/unit/sessions__tests.rs"]
mod tests;
