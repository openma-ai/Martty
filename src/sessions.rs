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
//!
//! Listing stays cheap without any persisted state: a log is streamed line
//! by line (a summary never holds the whole decompressed transcript) and
//! skipped with a cheap needle test unless the line can contribute, cache
//! misses are summarized a few files at a time in parallel, `/resume <id>`
//! reads only session headers before summarizing the chosen log, and the
//! ACP picker summarizes only the ids the agent listed.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// How a typed id prefix resolved against the durable store.
#[derive(Debug)]
pub enum SessionResolution {
    /// Exactly one session matched the prefix.
    One(SessionSummary),
    /// Several matched, but one id equals the typed prefix exactly.
    Exact(SessionSummary),
    /// No session matched.
    None,
    /// Several sessions matched and none is an exact id.
    Ambiguous(usize),
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

/// One session log found by the directory scan.
#[derive(Clone, Debug)]
struct Candidate {
    file: PathBuf,
    mtime: SystemTime,
}

/// Scan the session roots for this workspace's logs, newest first. No file
/// contents are read: only directory entries and one `stat` per log.
fn collect_candidates(cfg_root: &str, workspace: &str, skip_id: &str) -> Vec<Candidate> {
    let slug = workspace_slug(workspace);
    let mut out: Vec<Candidate> = Vec::new();
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
            let Some(file) = session_file(&dir) else {
                continue;
            };
            let mtime = std::fs::metadata(&file)
                .and_then(|meta| meta.modified())
                .unwrap_or(UNIX_EPOCH);
            out.push(Candidate { file, mtime });
        }
    }
    out.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.file.cmp(&b.file)));
    out.dedup_by(|a, b| a.file == b.file);
    out
}

/// Below this many logs, reading sequentially beats spawning threads.
const PARALLEL_MIN_FILES: usize = 2;
/// Cap the fan-out: `/resume` blocks the UI thread, so leave headroom.
const PARALLEL_MAX_THREADS: usize = 4;

/// Default cap for the `/resume` picker when no explicit count is given
/// (`/resume` behaves like `/resume 50`).
pub const DEFAULT_SESSION_LIST_LIMIT: usize = 50;

/// List resumable sessions for `workspace`, newest first, excluding
/// `skip_id` (the currently active session). Best effort: unreadable or
/// foreign files are skipped, never an error.
pub fn list_sessions(
    cfg_root: &str,
    workspace: &str,
    skip_id: &str,
    limit: usize,
) -> Vec<SessionSummary> {
    let candidates = collect_candidates(cfg_root, workspace, skip_id);
    let mut out: Vec<SessionSummary> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut pos = 0;
    while out.len() < limit && pos < candidates.len() {
        // Summarize at most the rows still missing, in parallel. A log that
        // turns out to be unreadable or foreign does not consume a limit
        // slot, so keep walking until `limit` real sessions are found.
        let end = pos
            .saturating_add(limit - out.len())
            .min(candidates.len());
        let files: Vec<PathBuf> = candidates[pos..end]
            .iter()
            .map(|candidate| candidate.file.clone())
            .collect();
        pos = end;
        for summary in summarize_many(&files).into_iter().flatten() {
            if summary.id == skip_id || !seen.insert(summary.id.clone()) {
                continue;
            }
            out.push(summary);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/// Local summaries for the given session ids, keyed by id. Used to enrich
/// ACP `session/list` rows without summarizing the whole local store.
pub fn sessions_for_ids(
    cfg_root: &str,
    workspace: &str,
    skip_id: &str,
    ids: &[String],
) -> HashMap<String, SessionSummary> {
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut found: HashMap<String, SessionSummary> = HashMap::new();
    if wanted.is_empty() {
        return found;
    }
    let candidates = collect_candidates(cfg_root, workspace, skip_id);
    let mut claimed: HashSet<String> = HashSet::new();
    let mut files: Vec<PathBuf> = Vec::new();
    let mut pending_ids: Vec<String> = Vec::new();
    for candidate in candidates {
        if claimed.len() >= wanted.len() {
            break;
        }
        let Some(id) = read_session_id(&candidate.file) else {
            continue;
        };
        if id == skip_id || !wanted.contains(id.as_str()) || !claimed.insert(id.clone()) {
            continue;
        }
        files.push(candidate.file);
        pending_ids.push(id);
    }
    for (id, summary) in pending_ids.into_iter().zip(summarize_many(&files)) {
        if let Some(summary) = summary {
            found.insert(id, summary);
        }
    }
    found
}

/// Resolve `/resume <id>` without listing the store: every candidate header
/// is read (cheap) but only the chosen log is fully summarized.
pub fn resolve_session(
    cfg_root: &str,
    workspace: &str,
    skip_id: &str,
    prefix: &str,
) -> SessionResolution {
    let candidates = collect_candidates(cfg_root, workspace, skip_id);
    let mut matches: Vec<(PathBuf, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for candidate in candidates {
        let Some(id) = read_session_id(&candidate.file) else {
            continue;
        };
        if id == skip_id || !id.starts_with(prefix) || !seen.insert(id.clone()) {
            continue;
        }
        matches.push((candidate.file, id));
    }
    match matches.len() {
        0 => SessionResolution::None,
        1 => match summarize(&matches[0].0) {
            Some(summary) => SessionResolution::One(summary),
            None => SessionResolution::None,
        },
        many => {
            let exact = matches
                .iter()
                .find(|(_, id)| id == prefix)
                .and_then(|(file, _)| summarize(file));
            match exact {
                Some(summary) => SessionResolution::Exact(summary),
                None => SessionResolution::Ambiguous(many),
            }
        }
    }
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

/// Walk a session log line by line without ever holding the whole file:
/// plain logs are read through a buffered line reader, zstd logs one frame
/// at a time with a carry buffer for lines split across frames. `f` returns
/// false to stop early. Empty lines are still passed to `f`; it decides
/// whether they matter.
fn for_each_line(file: &Path, mut f: impl FnMut(&str) -> bool) -> Result<()> {
    let raw = std::fs::File::open(file).with_context(|| format!("open {}", file.display()))?;
    if file.extension().is_some_and(|e| e == "zstd") {
        let mut reader = BufReader::new(raw);
        let mut pending = String::new();
        loop {
            if reader
                .fill_buf()
                .with_context(|| format!("read {}", file.display()))?
                .is_empty()
            {
                break;
            }
            let mut dec = ruzstd::decoding::StreamingDecoder::new(&mut reader)
                .with_context(|| format!("zstd frame of {}", file.display()))?;
            let mut chunk = String::new();
            dec.read_to_string(&mut chunk)
                .with_context(|| format!("decompress {}", file.display()))?;
            pending.push_str(&chunk);
            let mut consumed = 0;
            let mut stop = false;
            loop {
                let Some(pos) = pending[consumed..].find('\n') else {
                    break;
                };
                let end = consumed + pos;
                if !f(&pending[consumed..end]) {
                    stop = true;
                    break;
                }
                consumed = end + 1;
            }
            pending.drain(..consumed);
            if stop {
                return Ok(());
            }
        }
        let tail = pending.strip_suffix('\n').unwrap_or(&pending);
        let tail = tail.strip_suffix('\r').unwrap_or(tail);
        if !tail.trim().is_empty() {
            f(tail);
        }
        Ok(())
    } else {
        let mut reader = BufReader::new(raw);
        let mut line = String::new();
        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .with_context(|| format!("read {}", file.display()))?;
            if read == 0 {
                break;
            }
            let text = line.strip_suffix('\n').unwrap_or(&line);
            let text = text.strip_suffix('\r').unwrap_or(text);
            if !f(text) {
                break;
            }
        }
        Ok(())
    }
}

/// The header id of a session log, reading only the first parseable line.
/// Matches [`summarize`]: the first parseable line must be the `session`
/// header, unparseable lines before it are skipped.
fn read_session_id(file: &Path) -> Option<String> {
    let mut id = None;
    for_each_line(file, |line| {
        let line = line.trim();
        if line.is_empty() {
            return true;
        }
        if let Ok(event) = serde_json::from_str::<Value>(line) {
            if event.get("type").and_then(Value::as_str) == Some("session") {
                id = event.get("id").and_then(Value::as_str).map(str::to_string);
            }
            return false;
        }
        true
    })
    .ok()?;
    id
}

/// Summarize one session log, streaming it line by line. Lines that cannot
/// contribute to the summary are rejected with a substring test before the
/// (comparatively expensive) JSON parse.
fn summarize(file: &Path) -> Option<SessionSummary> {
    let mut id: Option<String> = None;
    let mut header_done = false;
    let mut turns = 0usize;
    let mut preview: Option<String> = None;
    let mut title: Option<String> = None;
    let mut provider_title: Option<String> = None;
    for_each_line(file, |line| {
        let line = line.trim();
        if line.is_empty() {
            return true;
        }
        if !header_done {
            // The first parseable line must be the session header.
            if let Ok(event) = serde_json::from_str::<Value>(line) {
                header_done = true;
                if event.get("type").and_then(Value::as_str) == Some("session") {
                    id = event.get("id").and_then(Value::as_str).map(str::to_string);
                }
            }
            return true;
        }
        if id.is_none() {
            return false; // the first parseable line was not a session header
        }
        // Cheap needle test first; only the event types that can contribute
        // to the summary are worth a JSON parse.
        let interesting = line.contains("turn/start")
            || line.contains("session/title")
            || (preview.is_none() && line.contains("user/message"));
        if !interesting {
            return true;
        }
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return true;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("turn/start") => turns += 1,
            Some("session/title") => {
                if let Some(t) = event.pointer("/data/title").and_then(Value::as_str) {
                    let t = t.trim();
                    if !t.is_empty() {
                        let t = t.to_string();
                        if event.pointer("/data/source/kind").and_then(Value::as_str)
                            == Some("provider")
                        {
                            provider_title = Some(t);
                        } else {
                            title = Some(t);
                        }
                    }
                }
            }
            Some("user/message") if preview.is_none() => {
                preview = user_text(&event);
            }
            _ => {}
        }
        true
    })
    .ok()?;

    let id = id?;
    let preview = preview
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
    let title = provider_title.or(title);
    Some(SessionSummary {
        id,
        file: file.to_path_buf(),
        modified: std::fs::metadata(file)
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH),
        turns,
        preview,
        title,
    })
}

/// Summarize several logs, in order. Decompression dominates and is
/// independent per file, so larger batches fan out over a few threads.
fn summarize_many(files: &[PathBuf]) -> Vec<Option<SessionSummary>> {
    if files.is_empty() {
        return Vec::new();
    }
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(PARALLEL_MAX_THREADS)
        .min(files.len());
    if files.len() < PARALLEL_MIN_FILES || threads <= 1 {
        return files.iter().map(|file| summarize(file)).collect();
    }
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<SessionSummary>>> =
        Mutex::new((0..files.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= files.len() {
                    break;
                }
                let summary = summarize(&files[i]);
                if let Ok(mut guard) = out.lock() {
                    guard[i] = summary;
                }
            });
        }
    });
    out.into_inner().unwrap_or_default()
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
