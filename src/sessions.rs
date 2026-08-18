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
fn session_roots(cfg_root: &str) -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(cfg_root)];
    if let Ok(home) = std::env::var("HOME") {
        roots.push(Path::new(&home).join(".dsh").join("sessions"));
    }
    roots.sort();
    roots.dedup();
    roots.retain(|r| r.is_dir());
    roots
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
pub fn list_sessions(cfg_root: &str, workspace: &str, skip_id: &str) -> Vec<SessionSummary> {
    let slug = workspace_slug(workspace);
    let mut out: Vec<SessionSummary> = Vec::new();
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
            let Some(file) = session_file(&dir) else {
                continue;
            };
            let Some(summary) = summarize(&file) else {
                continue;
            };
            if summary.id == skip_id || out.iter().any(|s| s.id == summary.id) {
                continue;
            }
            out.push(summary);
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.modified));
    out.truncate(50);
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
mod tests {
    use super::*;

    fn write_session(root: &Path, slug: &str, id: &str, lines: &[String]) -> PathBuf {
        let dir = root.join(slug).join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session.jsonl");
        std::fs::write(&file, lines.join("\n")).unwrap();
        file
    }

    fn header(id: &str) -> String {
        format!(r#"{{"type":"session","version":0,"id":"{id}","createdAt":1,"cwd":"/w"}}"#)
    }

    fn user_msg(text: &str) -> String {
        format!(
            r#"{{"type":"user/message","seq":7,"data":{{"content":[{{"text":"{text}","type":"text"}}],"source":{{"kind":"user"}},"role":"user","id":"x"}}}}"#
        )
    }

    fn title_event(kind: &str, text: &str) -> String {
        format!(
            r#"{{"type":"session/title","seq":8,"data":{{"title":"{text}","source":{{"kind":"{kind}"}}}}}}"#
        )
    }

    #[test]
    fn slug_matches_observed_host_layout() {
        assert_eq!(
            workspace_slug("/Users/minimax/oos-proj/deepseek-harness-tui"),
            "--Users-minimax-oos-proj-deepseek-harness-tui--"
        );
        assert_eq!(workspace_slug("/Users/minimax"), "--Users-minimax--");
    }

    #[test]
    fn lists_workspace_sessions_newest_first_and_skips_current() {
        let tmp = std::env::temp_dir().join(format!("dsh-sess-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let slug = workspace_slug("/w");
        write_session(
            &tmp,
            &slug,
            "dsh-old",
            &[header("dsh-old"), user_msg("older prompt")],
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_session(
            &tmp,
            &slug,
            "dsh-new",
            &[
                header("dsh-new"),
                r#"{"type":"turn/start","seq":1,"data":{"turn":1}}"#.into(),
                user_msg("修复失败的测试 with a long tail that should be truncated away entirely"),
                title_event("fallback", "修复失败的测试"),
                title_event("provider", "fix failing tests"),
                r#"{"type":"turn/start","seq":9,"data":{"turn":2}}"#.into(),
            ],
        );
        write_session(&tmp, &slug, "dsh-cur", &[header("dsh-cur")]);

        let sessions = list_sessions(tmp.to_str().unwrap(), "/w", "dsh-cur");
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["dsh-new", "dsh-old"], "newest first, current skipped");
        assert_eq!(sessions[0].turns, 2);
        assert!(sessions[0].preview.starts_with("修复失败的测试"));
        assert!(sessions[0].preview.ends_with('…'), "long preview truncated");
        assert_eq!(
            sessions[0].title.as_deref(),
            Some("fix failing tests"),
            "provider title beats the fallback stub"
        );
        assert_eq!(sessions[1].title, None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn user_text_skips_injected_context() {
        let injected: Value = serde_json::from_str(
            r#"{"type":"user/message","data":{"content":[{"text":"runtime context","type":"text"}],"source":{"kind":"runtime-context"}}}"#,
        )
        .unwrap();
        assert_eq!(user_text(&injected), None);
        let real: Value = serde_json::from_str(&user_msg("hi")).unwrap();
        assert_eq!(user_text(&real).as_deref(), Some("hi"));
    }

    #[test]
    fn title_falls_back_to_non_provider_and_skips_blanks() {
        let tmp = std::env::temp_dir().join(format!("dsh-title-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let slug = workspace_slug("/w");
        write_session(
            &tmp,
            &slug,
            "dsh-fb",
            &[
                header("dsh-fb"),
                user_msg("hello"),
                title_event("fallback", "hello world"),
            ],
        );
        write_session(
            &tmp,
            &slug,
            "dsh-blank",
            &[
                header("dsh-blank"),
                user_msg("hi"),
                title_event("fallback", "  "),
            ],
        );
        let sessions = list_sessions(tmp.to_str().unwrap(), "/w", "other");
        assert_eq!(
            sessions
                .iter()
                .find(|s| s.id == "dsh-fb")
                .unwrap()
                .title
                .as_deref(),
            Some("hello world"),
            "fallback title used when no provider title exists"
        );
        assert_eq!(
            sessions.iter().find(|s| s.id == "dsh-blank").unwrap().title,
            None,
            "blank titles are dropped"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Appended session logs are concatenated zstd frames — every frame
    /// must decode, not just the first (the header-only regression).
    #[test]
    fn reads_all_frames_of_concatenated_zstd_logs() {
        let tmp = std::env::temp_dir().join(format!("dsh-zstd-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let dir = tmp.join(workspace_slug("/w")).join("dsh-z");
        std::fs::create_dir_all(&dir).unwrap();
        let frames: Vec<u8> = [
            format!("{}\n", header("dsh-z")),
            format!(
                "{}\n{}\n",
                r#"{"type":"turn/start","seq":1,"data":{"turn":1}}"#,
                user_msg("from frame two")
            ),
            format!("{}\n", r#"{"type":"turn/start","seq":9,"data":{"turn":2}}"#),
        ]
        .iter()
        .flat_map(|chunk| {
            ruzstd::encoding::compress_to_vec(
                chunk.as_bytes(),
                ruzstd::encoding::CompressionLevel::Fastest,
            )
        })
        .collect();
        std::fs::write(dir.join("session.jsonl.zstd"), frames).unwrap();

        let events = read_session_events(&dir.join("session.jsonl.zstd")).unwrap();
        assert_eq!(events.len(), 4, "header + 3 events across 3 frames");
        let sessions = list_sessions(tmp.to_str().unwrap(), "/w", "other");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].turns, 2, "turns counted across frames");
        assert_eq!(sessions[0].preview, "from frame two");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
