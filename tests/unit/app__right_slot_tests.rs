use super::*;
use serde_json::json;
use std::sync::mpsc::Receiver;

fn test_app() -> (App, Controller, Receiver<AppEvent>) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: std::env::temp_dir()
            .join(format!("dsh-tui-right-slot-{}", std::process::id()))
            .to_string_lossy()
            .into_owned(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl, rx)
}

fn snapshot(nodes: serde_json::Value, rev: u64) -> AppEvent {
    AppEvent::Rpc {
        method: crate::cordis::SLOTS_UPDATE.into(),
        params: json!({
            "protocol": 0,
            "slot": "chrome.right",
            "rev": rev,
            "nodes": nodes,
        }),
    }
}

#[test]
fn chrome_right_snapshot_renders_as_an_independent_sidebar() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.handle(
            snapshot(
                json!([
                    { "id": "build:title", "kind": "markdown", "text": "# Build monitor" },
                    {
                        "id": "build:checks",
                        "kind": "group",
                        "title": "Checks",
                        "children": [
                            { "id": "build:test", "kind": "generic", "title": "Tests", "body": "97 passed", "status": "ok" },
                            { "id": "build:log", "kind": "terminal", "title": "Deploy", "body": "$ ship\ncomplete", "exit": 0 }
                        ]
                    }
                ]),
                1,
            ),
            &ctl,
        );

    let frame = crate::ui::dump_frame(&mut app, 120, 28);

    assert!(frame.contains("Build monitor"), "markdown node:\n{frame}");
    assert!(frame.contains("Checks"), "group node:\n{frame}");
    assert!(frame.contains("97 passed"), "generic node:\n{frame}");
    assert!(frame.contains("complete"), "terminal node:\n{frame}");
    assert!(
        app.chat_view.area.width < 90,
        "the sidebar must own a separate right-hand pane, chat width was {}:\n{frame}",
        app.chat_view.area.width,
    );
}

#[test]
fn empty_chrome_right_snapshot_gives_the_full_width_back_to_chat() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.handle(
        snapshot(
            json!([{ "id": "build:title", "kind": "markdown", "text": "Build monitor" }]),
            1,
        ),
        &ctl,
    );
    let _ = crate::ui::dump_frame(&mut app, 120, 28);
    let narrowed = app.chat_view.area.width;

    app.handle(snapshot(json!([]), 2), &ctl);
    let frame = crate::ui::dump_frame(&mut app, 120, 28);

    assert!(
        app.chat_view.area.width > narrowed,
        "empty slot restores width"
    );
    assert!(
        !frame.contains("Build monitor"),
        "disposed nodes disappear:\n{frame}"
    );
}

#[test]
fn snapshots_without_revision_still_replace_the_previous_view() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    for text in ["first panel", "second panel"] {
        app.handle(
            AppEvent::Rpc {
                method: crate::cordis::SLOTS_UPDATE.into(),
                params: json!({
                    "protocol": 0,
                    "slot": "chrome.right",
                    "nodes": [{ "id": "demo:title", "kind": "markdown", "text": text }],
                }),
            },
            &ctl,
        );
    }

    let frame = crate::ui::dump_frame(&mut app, 120, 28);
    assert!(
        frame.contains("second panel"),
        "latest unversioned snapshot wins:\n{frame}"
    );
    assert!(
        !frame.contains("first panel"),
        "old snapshot was replaced:\n{frame}"
    );
}

#[test]
fn conversation_input_dock_is_compact_and_does_not_claim_the_sidebar() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.show_banner = false;
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: json!({
                "protocol": 0,
                "slot": "conversation.input.dock",
                "rev": 1,
                "nodes": [{
                    "id": "plan-view:summary",
                    "kind": "generic",
                    "title": "Plan · 1/2 · Implement",
                    "body": "",
                    "status": "running",
                    "action": { "kind": "command", "name": "plan-view", "args": "" }
                }]
            }),
        },
        &ctl,
    );

    let frame = crate::ui::dump_frame(&mut app, 100, 24);

    assert!(
        frame.contains("Plan · 1/2 · Implement"),
        "dock row:\n{frame}"
    );
    let lines = frame.lines().collect::<Vec<_>>();
    let plan_y = lines
        .iter()
        .position(|line| line.contains("Plan · 1/2 · Implement"))
        .expect("Plan row") as u16;
    assert!(
        lines[plan_y as usize].starts_with('╭'),
        "the dock owns the single cap row:\n{frame}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("Tip ·")),
        "a dock present means no tip line — the cap stays one row:\n{frame}"
    );
    assert_eq!(
        lines[plan_y as usize + 1].chars().next(),
        Some('│'),
        "the input well starts right below the one cap row:\n{frame}"
    );

    app.handle(
        AppEvent::Term(crossterm::event::Event::Mouse(
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 5,
                row: plan_y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
        )),
        &ctl,
    );
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::InvokePluginCommand { name, args })
            if name == "plan-view" && args.is_empty()
    ));
    assert!(app.slot_snapshots.contains_key("conversation.input.dock"));
    assert!(
        !app.slot_snapshots.contains_key("chrome.right"),
        "composer dock must not claim chrome.right"
    );
    let (mut baseline, _baseline_ctl, _baseline_rx) = test_app();
    baseline.show_banner = false;
    let _ = crate::ui::dump_frame(&mut baseline, 100, 24);
    assert_eq!(
        app.chat_view.area.width, baseline.chat_view.area.width,
        "dock must keep the full-width shell",
    );
}

#[test]
fn conversation_input_dock_routes_each_visible_action() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.show_banner = false;
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: json!({
                "protocol": 0,
                "slot": "conversation.input.dock",
                "rev": 1,
                "nodes": [
                    {
                        "id": "plan-view:summary",
                        "kind": "generic",
                        "title": "Plan",
                        "body": "",
                        "action": { "kind": "command", "name": "plan-view", "args": "" }
                    },
                    {
                        "id": "goal-view:summary",
                        "kind": "generic",
                        "title": "Goal",
                        "body": "",
                        "action": { "kind": "command", "name": "goal-view", "args": "active" }
                    }
                ]
            }),
        },
        &ctl,
    );

    let frame = crate::ui::dump_frame(&mut app, 100, 24);
    let lines = frame.lines().collect::<Vec<_>>();
    let dock_y = lines
        .iter()
        .position(|line| line.contains("Plan | Goal"))
        .expect("combined dock row") as u16;
    let goal_x = lines[dock_y as usize].find("Goal").expect("Goal title") as u16;

    app.handle(
        AppEvent::Term(crossterm::event::Event::Mouse(
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: goal_x,
                row: dock_y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
        )),
        &ctl,
    );

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::InvokePluginCommand { name, args })
            if name == "goal-view" && args == "active"
    ));
}

#[test]
fn status_slash_fallback_shows_run_state_without_transcript_stats() {
    let (mut app, ctl, _rx) = test_app();
    app.modes.effort = Some("high".into());
    // Seed the transcript accumulators: the fallback must not render them.
    app.transcript.usage.input = 1834;
    app.transcript.usage.output = 412;
    app.transcript.usage.cached = 1200;
    app.transcript.stats.turns = 3;
    app.transcript.stats.steps = 47;
    app.transcript.stats.turn_millis = 135_000;
    app.transcript.stats.tool_millis = 8_000;
    app.transcript.stats.ttft_total_millis = 4_500;
    app.transcript.stats.ttft_count = 3;

    app.run_slash("status", "", &ctl);

    let last = app.transcript.cells.last().expect("status cell");
    let crate::transcript::CellKind::MarkdownNotice { text } = &last.kind else {
        panic!("/status should be a markdown notice, got {:?}", last.kind);
    };
    assert!(text.contains("## status"), "{text}");
    assert!(text.contains("- state · "), "{text}");
    // ACP facts: demo run shows the demo marker and its session.
    assert!(text.contains("- acp · demo"), "{text}");
    assert!(text.contains("- session · dsh-test"), "{text}");
    assert!(text.contains("- model · deepseek-v4-flash"), "{text}");
    assert!(text.contains("- effort · high"), "{text}");
    assert!(text.contains("- permission · "), "{text}");
    assert!(text.contains("- plan · "), "{text}");
    // The fallback owns no transcript accumulators: token/turn/timing
    // figures belong to the Client `acpSessionStats` snapshot that the
    // status-view overlay renders in live runs.
    assert!(!text.contains("- tokens ·"), "{text}");
    assert!(!text.contains("- turns ·"), "{text}");
    assert!(!text.contains("- LLM ·"), "{text}");
    assert!(!text.contains("- TTFT avg ·"), "{text}");
    assert!(!text.contains("- rate ·"), "{text}");
}

#[test]
fn status_client_command_is_listed_and_invoked_locally() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::COMMANDS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "commands": [{
                    "name": "status",
                    "description": "Session run state and key stats"
                }]
            }),
        },
        &ctl,
    );
    app.input.set("/status".into());

    let menu = app.slash_matches();
    assert_eq!(menu.len(), 1);
    assert!(menu[0].plugin, "/status is the status-view Client command");
    assert!(
        !SLASH_COMMANDS.iter().any(|c| c.name == "status"),
        "/status must not stay a Rust builtin: the Client command renders it"
    );

    app.submit(&ctl);
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::InvokePluginCommand { name, args })
            if name == "status" && args.is_empty()
    ));
    assert!(
        app.transcript.cells.is_empty(),
        "no transcript cell is created for the Client command"
    );
}

#[test]
fn session_slash_shows_effort_when_set() {
    let (mut app, ctl, _rx) = test_app();
    app.modes.effort = Some("max".into());

    app.run_slash("session", "", &ctl);

    let last = app.transcript.cells.last().expect("session cell");
    let crate::transcript::CellKind::MarkdownNotice { text } = &last.kind else {
        panic!("/session should be a markdown notice, got {:?}", last.kind);
    };
    assert!(text.contains("## session"), "{text}");
    assert!(text.contains("- effort · max"), "{text}");

    // Unset effort stays hidden.
    let (mut plain, ctl2, _rx2) = test_app();
    plain.run_slash("session", "", &ctl2);
    let last = plain.transcript.cells.last().expect("session cell");
    let crate::transcript::CellKind::MarkdownNotice { text } = &last.kind else {
        panic!("/session should be a markdown notice, got {:?}", last.kind);
    };
    assert!(!text.contains("- effort ·"), "{text}");
}
