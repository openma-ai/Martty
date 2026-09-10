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

    let sessions = list_sessions(tmp.to_str().unwrap(), "/w", "dsh-cur", usize::MAX);
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
fn legacy_dsh_tui_sessions_remain_discoverable_after_the_martty_move() {
    let home = std::env::temp_dir().join(format!("martty-legacy-sessions-{}", std::process::id()));
    let current = home.join(".martty/sessions");
    let legacy = home.join(".dsh-tui/sessions");
    std::fs::create_dir_all(&current).unwrap();
    std::fs::create_dir_all(&legacy).unwrap();

    let roots = session_roots_from(current.to_str().unwrap(), Some(&home));

    assert!(roots.contains(&current));
    assert!(roots.contains(&legacy));
    let _ = std::fs::remove_dir_all(home);
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
    let sessions = list_sessions(tmp.to_str().unwrap(), "/w", "other", usize::MAX);
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
    let sessions = list_sessions(tmp.to_str().unwrap(), "/w", "other", usize::MAX);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].turns, 2, "turns counted across frames");
    assert_eq!(sessions[0].preview, "from frame two");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn list_sessions_limit_keeps_the_most_recent_n() {
    let tmp = std::env::temp_dir().join(format!("dsh-sess-limit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let slug = workspace_slug("/w");
    write_session(&tmp, &slug, "dsh-oldest", &[header("dsh-oldest")]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_session(&tmp, &slug, "dsh-mid", &[header("dsh-mid")]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_session(&tmp, &slug, "dsh-newest", &[header("dsh-newest")]);

    // Default `/resume` (= 50) returns everything; `/resume 2` keeps only
    // the two most recent entries.
    let all = list_sessions(tmp.to_str().unwrap(), "/w", "none", usize::MAX);
    assert_eq!(all.len(), 3, "no limit returns every session");
    let two = list_sessions(tmp.to_str().unwrap(), "/w", "none", 2);
    let ids: Vec<&str> = two.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["dsh-newest", "dsh-mid"], "limit keeps the newest n");
    let one = list_sessions(tmp.to_str().unwrap(), "/w", "none", 1);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].id, "dsh-newest");
    let _ = std::fs::remove_dir_all(&tmp);
}
