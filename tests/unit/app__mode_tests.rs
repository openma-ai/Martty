use super::*;
use crate::bus::{
    CatalogModel, CatalogPreset, CordisPluginItem, PendingCordisApproval, SessionListItem,
    StaticPluginItem,
};
use std::sync::mpsc::Receiver;

/// Unique session root per call — keeps persisted UI fixtures isolated.
fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-mode-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

fn test_cfg() -> RuntimeConfig {
    RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: fresh_root(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    }
}

fn test_app() -> (App, Controller, Receiver<AppEvent>) {
    let cfg = test_cfg();
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl, rx)
}

#[test]
fn legacy_mode_cache_is_neither_read_nor_written() {
    let cfg = test_cfg();
    let cache = std::path::Path::new(&cfg.session_root).join("dsh-tui-modes.json");
    let legacy = serde_json::json!({
        "workspaces": {
            cfg.workspace.clone(): {
                "plan": true,
                "sandbox": "danger-full-access",
                "approval": "never",
                "permission": "danger-full-access",
                "agent_preset": "code",
                "effort": "max"
            }
        }
    })
    .to_string();
    std::fs::write(&cache, &legacy).expect("seed legacy modes cache");

    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let mut app = App::new(
        Theme::dark(),
        cfg.clone(),
        "s1".into(),
        true,
        false,
        tx.clone(),
    );
    assert!(app.modes == Modes::default());

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::AgentPreset {
            session: "s1".into(),
            preset: "standard".into(),
        }),
        &ctl,
    );
    assert_eq!(
        std::fs::read_to_string(cache).expect("legacy file remains inspectable"),
        legacy,
        "live ACP state must never be written into the retired cache",
    );
    drop(rx);
}

#[test]
fn selected_model_clears_once_a_turn_streams_on_it() {
    let (mut app, ctl, _rx) = test_app();
    app.set_model("deepseek-v4-pro".into(), &ctl);
    assert_eq!(app.selected_model.as_deref(), Some("deepseek-v4-pro"));
    // The next turn streams on the picked model → the pick is realized
    // and the stream fact takes over.
    app.transcript.last_model = Some("deepseek-v4-pro".into());
    app.handle(AppEvent::Ctl(CtlEvent::TuiOpDone("noop".into())), &ctl);
    assert_eq!(
        app.selected_model, None,
        "realized pick defers to the stream"
    );
}

#[test]
fn slash_menu_offers_the_dynamic_plugin_manager() {
    let (mut app, _ctl, _rx) = test_app();
    app.input.set("/plug".into());

    let matches = app.slash_matches();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "plugins");
    assert_eq!(matches[0].usage, "/plugins");
}

#[test]
fn slash_menu_offers_the_client_language_switch() {
    let (mut app, _ctl, _rx) = test_app();
    app.input.set("/lang".into());

    let matches = app.slash_matches();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "lang");
    assert_eq!(matches[0].usage, "/lang [zh|en]");
}

#[test]
fn lang_switch_repaints_immediately_and_persists_for_the_workspace() {
    let cfg = test_cfg();
    let legacy = std::path::Path::new(&cfg.session_root).join("dsh-tui-settings.json");
    std::fs::write(
        &legacy,
        r#"{"language":"en","uiPreset":"deepseek","theme":"ember"}"#,
    )
    .expect("seed UI preset selection");
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let mut app = App::new(
        Theme::dark(),
        cfg.clone(),
        "s1".into(),
        true,
        false,
        tx.clone(),
    );
    app.show_banner = false;

    app.run_slash("lang", "zh", &ctl);

    let frame = crate::ui::dump_frame(&mut app, 100, 24);
    assert!(
        frame.replace(' ', "").contains("描述你想构建的内容"),
        "{frame}"
    );

    let mut restarted = App::new(Theme::dark(), cfg, "s2".into(), true, false, tx);
    restarted.show_banner = false;
    let frame = crate::ui::dump_frame(&mut restarted, 100, 24);
    assert!(
        frame.replace(' ', "").contains("描述你想构建的内容"),
        "{frame}"
    );
    assert_eq!(restarted.ui_preset, "deepseek", "/lang preserves UI Preset");
    let current = std::path::Path::new(&restarted.cfg.session_root).join("settings.json");
    assert!(
        current.is_file(),
        "legacy settings migrate to the Martty filename"
    );
    assert!(
        legacy.is_file(),
        "migration preserves the legacy settings file"
    );
    let saved: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(current).expect("read migrated Martty settings"),
    )
    .unwrap();
    assert_eq!(
        saved["theme"], "ember",
        "/lang preserves Client-owned settings"
    );
}

#[test]
fn liang_toggle_is_transient_and_keeps_the_empty_welcome_centered() {
    let (mut app, ctl, _rx) = test_app();

    app.run_slash("liang", "off", &ctl);

    assert!(!app.pet_visible);
    assert!(
        app.transcript.cells.is_empty(),
        "a local pet toggle must not become conversation history"
    );
    let _ = crate::ui::dump_frame(&mut app, 140, 60);
    let first = app
        .chat_view
        .lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .expect("welcome content");
    let last = app
        .chat_view
        .lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .expect("welcome content");
    let bottom = app.chat_view.area.height as usize - last - 1;
    assert!(
        first.abs_diff(bottom) <= 1,
        "closing Liang must not top-align the welcome: top={first}, bottom={bottom}"
    );
}

#[test]
fn plugins_slash_fetches_the_static_loader_inventory() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();

    app.run_slash("plugins", "", &ctl);

    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("plugins slash sends a command");
    assert!(matches!(command, Cmd::FetchStaticPlugins));
}

#[test]
fn cordis_plugins_slash_fetches_the_dynamic_inventory() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();

    app.run_slash("cordis-plugins", "", &ctl);

    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("cordis-plugins slash sends a command");
    assert!(matches!(command, Cmd::FetchCordisPlugins { agent_id } if agent_id == "dsh-test"));
}

#[test]
fn static_plugin_inventory_is_read_only_and_matches_web_status_fields() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Ctl(CtlEvent::StaticPlugins {
            plugins: vec![StaticPluginItem {
                entry_id: "root/include".into(),
                module_name: "include".into(),
                enabled: true,
                fiber_phase: Some("active".into()),
            }],
        }),
        &ctl,
    );

    let tree = app.plugin_tree.as_ref().expect("plugin tree opens");
    assert!(
        tree.title.contains("静态") || tree.title.contains("static"),
        "{}",
        tree.title
    );

    // The inventory renders as a provider → plugin tree: the unscoped row
    // lands under `core`, scoped rows under their npm scope, and the leaf
    // keeps the enabled/phase meta of the old flat picker.
    let (mut grouped, ctl2, _rx2) = test_app();
    grouped.handle(
        AppEvent::Ctl(CtlEvent::StaticPlugins {
            plugins: vec![
                StaticPluginItem {
                    entry_id: "root/include".into(),
                    module_name: "include".into(),
                    enabled: true,
                    fiber_phase: Some("active".into()),
                },
                StaticPluginItem {
                    entry_id: "root/tool-bash".into(),
                    module_name: "@deepseek-ai/dsh-tool-bash".into(),
                    enabled: false,
                    fiber_phase: None,
                },
            ],
        }),
        &ctl2,
    );
    let frame = crate::ui::dump_frame(&mut grouped, 100, 20);
    assert!(frame.contains("core"), "provider bucket:\n{frame}");
    assert!(frame.contains("@deepseek-ai"), "scoped provider:\n{frame}");
    assert!(frame.contains("include"), "plugin leaf:\n{frame}");
    assert!(frame.contains("dsh-tool-bash"), "scoped leaf:\n{frame}");
    assert!(frame.contains("enabled"), "leaf meta:\n{frame}");
    assert!(frame.contains("disabled"), "leaf meta:\n{frame}");
}

#[test]
fn cordis_plugin_inventory_opens_a_running_stopped_and_pending_picker() {
    let (mut app, ctl, _rx) = test_app();

    app.handle(
        AppEvent::Ctl(CtlEvent::CordisPlugins {
            plugins: vec![
                CordisPluginItem {
                    id: "panel-1".into(),
                    name: "Status panel".into(),
                    package_id: "pkg-1".into(),
                    status: "running".into(),
                    approval_request_id: None,
                },
                CordisPluginItem {
                    id: "theme-1".into(),
                    name: "Clay theme".into(),
                    package_id: "pkg-2".into(),
                    status: "stopped".into(),
                    approval_request_id: None,
                },
                CordisPluginItem {
                    id: "dock-1".into(),
                    name: "Composer dock".into(),
                    package_id: "pkg-3".into(),
                    status: "awaiting-approval".into(),
                    approval_request_id: Some("approval-3".into()),
                },
            ],
        }),
        &ctl,
    );

    let picker = app.picker.as_ref().expect("plugin picker opens");
    assert!(matches!(picker.kind, PickerKind::CordisPlugin));
    assert_eq!(picker.items[0].label, "Status panel");
    assert_eq!(picker.items[0].meta, "dynamic · running · enter stop");
    assert_eq!(picker.items[1].meta, "dynamic · stopped · enter restore");
    assert_eq!(
        picker.items[2].meta,
        "dynamic · awaiting approval · enter review"
    );
}

#[test]
fn enter_toggles_a_plugin_and_reopens_the_backend_inventory() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Ctl(CtlEvent::CordisPlugins {
            plugins: vec![CordisPluginItem {
                id: "panel-1".into(),
                name: "Status panel".into(),
                package_id: "pkg-1".into(),
                status: "running".into(),
                approval_request_id: None,
            }],
        }),
        &ctl,
    );

    app.handle_picker_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("plugin picker sends a toggle");
    assert!(matches!(
        command,
        Cmd::SetCordisPluginEnabled { agent_id, plugin_id, enabled }
            if agent_id == "dsh-test" && plugin_id == "panel-1" && !enabled
    ));
}

#[test]
fn pending_cordis_approval_renders_above_tips_and_alt_shortcut_answers_it() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::APPROVALS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "approvals": [{
                    "requestId": "approval-1",
                    "agentId": "dsh-test",
                    "pluginId": "panel-1",
                    "packageId": "pkg-1",
                    "mode": "run",
                    "name": "Right sidebar",
                    "purpose": "Show jobs and web fetches"
                }]
            }),
        },
        &ctl,
    );
    assert_eq!(
        app.pending_cordis_approvals,
        vec![PendingCordisApproval {
            request_id: "approval-1".into(),
            agent_id: "dsh-test".into(),
            plugin_id: "panel-1".into(),
            package_id: "pkg-1".into(),
            mode: "run".into(),
            name: "Right sidebar".into(),
            purpose: "Show jobs and web fetches".into(),
        }]
    );
    let frame = crate::ui::dump_frame(&mut app, 120, 24);
    let approval = frame.find("Right sidebar").expect("approval row");
    let tips = frame.find("Tip").expect("tip row");
    assert!(approval < tips, "approval must render above tips");

    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char('2'),
            KeyModifiers::ALT,
        ))),
        &ctl,
    );
    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("approval shortcut sends a decision");
    assert!(matches!(
        command,
        Cmd::RespondCordisApproval { request_id, decision }
            if request_id == "approval-1" && decision == "allow-future"
    ));
}

#[test]
fn child_session_updates_do_not_enter_the_parent_transcript() {
    let (mut app, _ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    app.apply_ui(crate::events::UiEvent::TextDelta {
        session: "child-1".into(),
        text: "child-only output".into(),
    });

    let rendered = app
        .transcript
        .lines(&Theme::dark(), 80, '⠋')
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(
        !rendered.contains("child-only output"),
        "child content must stay out of the parent transcript: {rendered}"
    );
}

#[test]
fn subagent_started_updates_navigation_without_a_timeline_notice() {
    let (mut app, _ctl, _rx) = test_app();

    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });

    assert_eq!(app.subagents.len(), 1);
    assert!(app.subagents[0].running);
    let rendered = app
        .transcript
        .lines(&Theme::dark(), 80, '⠋')
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(
        !rendered.contains("subagent 1 started"),
        "lifecycle belongs in the Agent dock, not the timeline: {rendered}"
    );
}

#[test]
fn subagent_finished_updates_navigation_without_a_timeline_notice() {
    let (mut app, _ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });

    app.apply_ui(crate::events::UiEvent::SubagentFinished {
        child: "child-1".into(),
        failed: false,
    });

    assert!(!app.subagents[0].running);
    let rendered = app
        .transcript
        .lines(&Theme::dark(), 80, '⠋')
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(
        !rendered.contains("subagent 1 finished"),
        "lifecycle belongs in the Agent dock, not the timeline: {rendered}"
    );
}

#[test]
fn subagent_tool_call_keeps_its_request_block_when_it_also_carries_lifecycle_metadata() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;

    app.handle(
        AppEvent::Rpc {
            method: "session/update".into(),
            params: serde_json::json!({
                "sessionId": "dsh-test",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "subagent:run-1",
                    "title": "Start subagent child-1",
                    "status": "in_progress",
                    "rawInput": {
                        "childSessionId": "child-1",
                        "provider": "codex",
                        "local": false
                    },
                    "_meta": {"dsh": {"subagent": {
                        "state": "started",
                        "childSessionId": "child-1"
                    }}}
                }
            }),
        },
        &ctl,
    );

    let frame = crate::ui::dump_frame(&mut app, 100, 24);
    assert!(frame.contains("Start subagent child-1"), "{frame}");
    assert!(
        frame.lines().any(|line| line.contains("│ request")),
        "the pending ACP tool call must expose its request before the child finishes:\n{frame}"
    );
    assert!(frame.contains("childSessionId"), "{frame}");
}

#[test]
fn metadata_subagent_failure_updates_only_the_agent_status() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;

    for update in [
        serde_json::json!({
            "sessionUpdate": "session_info_update",
            "_meta": {"dsh": {
                "event": "subagent/lifecycle",
                "subagent": {"state": "started", "childSessionId": "child-1"}
            }}
        }),
        serde_json::json!({
            "sessionUpdate": "session_info_update",
            "_meta": {"dsh": {
                "event": "subagent/lifecycle",
                "subagent": {
                    "state": "finished",
                    "childSessionId": "child-1",
                    "stopReason": "error"
                }
            }}
        }),
    ] {
        app.handle(
            AppEvent::Rpc {
                method: "session/update".into(),
                params: serde_json::json!({"sessionId": "dsh-test", "update": update}),
            },
            &ctl,
        );
    }

    assert!(!app.subagents[0].running);
    assert!(app.subagents[0].failed);
    assert_eq!(app.agents_snapshot().items[1].status, "failed");
    let frame = crate::ui::dump_frame(&mut app, 100, 24);
    assert!(frame.contains("Agents · 1/1"), "{frame}");
    assert!(!frame.contains("Start subagent"), "{frame}");
    assert!(!frame.contains("│ request"), "{frame}");
}

#[test]
fn first_subagent_of_a_new_root_turn_archives_the_previous_batch() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;

    app.apply_ui(crate::events::UiEvent::TurnStart {
        session: "dsh-test".into(),
        turn: 1,
    });
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    app.apply_ui(crate::events::UiEvent::SubagentFinished {
        child: "child-1".into(),
        failed: false,
    });
    app.apply_ui(crate::events::UiEvent::TurnStart {
        session: "dsh-test".into(),
        turn: 2,
    });
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-2".into(),
    });

    let collapsed = crate::ui::dump_frame(&mut app, 100, 24);
    assert!(collapsed.contains("Agents · 0/1"), "{collapsed}");

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    let expanded = crate::ui::dump_frame(&mut app, 100, 24);
    assert!(!expanded.contains("subagent 1"), "{expanded}");
    assert!(expanded.contains("subagent 2"), "{expanded}");
    assert!(expanded.contains("History (1)"), "{expanded}");

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    let picker = app.picker.as_ref().expect("History opens a picker form");
    assert!(matches!(picker.kind, PickerKind::AgentHistory));
    assert_eq!(picker.items.len(), 1);
    assert_eq!(picker.items[0].id, "child-1");

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(app.active_subagent.as_deref(), Some("child-1"));
}

#[test]
fn a_running_subagent_keeps_the_spinner_advancing() {
    let (mut app, _ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    let before = app.spinner_idx;

    app.tick();

    assert_ne!(app.spinner_idx, before);
}

#[test]
fn down_on_an_empty_prompt_enters_inline_agent_navigation() {
    let (mut app, ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);

    assert!(app.picker.is_none(), "Agent navigation has no popup");
    assert_eq!(app.agent_selection.as_deref(), Some("dsh-test"));
}

#[test]
fn agent_navigation_is_inline_when_the_view_plugin_is_available() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::COMMANDS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "commands": [{
                    "name": "agents",
                    "description": "Switch the visible Agent transcript"
                }]
            }),
        },
        &ctl,
    );

    app.handle(
        AppEvent::Term(crossterm::event::Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    assert!(
        app.picker.is_none(),
        "Agent navigation must not open a popup"
    );

    app.handle(
        AppEvent::Term(crossterm::event::Event::Key(KeyEvent::new(
            KeyCode::Right,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );
    app.handle(
        AppEvent::Term(crossterm::event::Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    assert_eq!(
        app.active_subagent.as_deref(),
        Some("child-1"),
        "↓ enters the rail, → focuses the child, and Enter opens its transcript"
    );
    while let Ok(command) = commands.try_recv() {
        assert!(
            !matches!(command, Cmd::InvokePluginCommand { name, .. } if name == "agents"),
            "keyboard navigation stays inline instead of reopening the Client overlay"
        );
    }
}

#[test]
fn the_client_agents_selection_can_open_any_subagent_or_return_to_main() {
    let (mut app, ctl, _rx) = test_app();
    for child in ["child-1", "child-2"] {
        app.apply_ui(crate::events::UiEvent::SubagentStarted {
            parent: "dsh-test".into(),
            child: child.into(),
        });
    }

    app.handle(
        AppEvent::Rpc {
            method: "_dsh/cordis/tui/agents/select".into(),
            params: serde_json::json!({ "protocol": 0, "id": "child-2" }),
        },
        &ctl,
    );
    assert_eq!(app.active_subagent.as_deref(), Some("child-2"));

    app.handle(
        AppEvent::Rpc {
            method: "_dsh/cordis/tui/agents/select".into(),
            params: serde_json::json!({ "protocol": 0, "id": "dsh-test" }),
        },
        &ctl,
    );
    assert!(app.active_subagent.is_none());

    for action in ["begin", "next", "confirm"] {
        app.handle(
            AppEvent::Rpc {
                method: crate::cordis::AGENTS_NAVIGATE.into(),
                params: serde_json::json!({ "protocol": 0, "action": action }),
            },
            &ctl,
        );
    }
    assert_eq!(
        app.active_subagent.as_deref(),
        Some("child-1"),
        "the Client /agents command enters the same inline state machine"
    );
    assert!(app.picker.is_none());
}

#[test]
fn live_subagent_changes_publish_a_client_compositor_snapshot() {
    let cfg = test_cfg();
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let mut app = App::new(Theme::dark(), cfg, "live-session".into(), false, true, tx);
    let (ctl, commands) = crate::controller::tests::test_controller();

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SubagentStarted {
            parent: "live-session".into(),
            child: "child-1".into(),
        }),
        &ctl,
    );

    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("Agent compositor snapshot");
    let crate::bus::Cmd::AgentsSnapshot { snapshot } = command else {
        panic!("wrong command: {command:?}");
    };
    assert_eq!(snapshot.active_id, "live-session");
    assert_eq!(snapshot.selected_id, None);
    assert_eq!(snapshot.items[0].id, "live-session");
    assert_eq!(snapshot.items[0].kind, "main");
    assert_eq!(snapshot.items[1].id, "child-1");
    assert_eq!(snapshot.items[1].kind, "subagent");
    assert_eq!(snapshot.items[1].status, "running");
}

#[test]
fn enter_from_the_agent_switcher_opens_the_child_transcript() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.transcript.push_user("main-only text".into(), false);
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    app.apply_ui(crate::events::UiEvent::TextDelta {
        session: "child-1".into(),
        text: "child-only output".into(),
    });
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    let frame = crate::ui::dump_frame(&mut app, 100, 24);
    assert!(frame.contains("child-only output"), "{frame}");
    assert!(!frame.contains("main-only text"), "{frame}");
}

#[test]
fn esc_from_a_child_transcript_returns_to_main() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.transcript.push_user("main-only text".into(), false);
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    app.apply_ui(crate::events::UiEvent::TextDelta {
        session: "child-1".into(),
        text: "child-only output".into(),
    });
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);

    let frame = crate::ui::dump_frame(&mut app, 100, 24);
    assert!(frame.contains("main-only text"), "{frame}");
    assert!(!frame.contains("child-only output"), "{frame}");
}

#[test]
fn child_transcript_view_does_not_accept_composer_input() {
    let (mut app, ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert!(
        app.input.is_empty(),
        "child view must not edit the main draft"
    );
    assert!(
        app.transcript.cells.iter().all(|cell| !matches!(
            &cell.kind,
            crate::transcript::CellKind::User { text, .. } if text == "x"
        )),
        "child view must not submit a main-session prompt"
    );
}

#[test]
fn down_from_a_child_view_reenters_inline_agent_navigation() {
    let (mut app, ctl, _rx) = test_app();
    for child in ["child-1", "child-2"] {
        app.apply_ui(crate::events::UiEvent::SubagentStarted {
            parent: "dsh-test".into(),
            child: child.into(),
        });
    }
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(app.active_subagent.as_deref(), Some("child-1"));

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    assert_eq!(app.agent_selection.as_deref(), Some("child-1"));
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(app.active_subagent.as_deref(), Some("child-2"));
}

#[test]
fn q_from_a_child_transcript_returns_to_main() {
    let (mut app, ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE), &ctl);

    assert!(app.active_subagent.is_none());
}

#[test]
fn a_new_session_clears_the_previous_agent_views() {
    let (mut app, ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "child-1".into(),
    });
    assert_eq!(app.subagents.len(), 1);

    app.run_slash("new", "fresh", &ctl);

    assert!(app.subagents.is_empty());
    assert!(app.active_subagent.is_none());
}

#[test]
fn slash_agent_opens_the_agent_preset_picker() {
    let (mut app, ctl, _rx) = test_app();

    app.run_slash("agent", "", &ctl);

    let picker = app.picker.as_ref().expect("agent picker opens");
    assert!(matches!(picker.kind, PickerKind::Mode));
    let ids: Vec<&str> = picker.items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["standard", "code", "minimal", "cordis"]);
    assert_eq!(picker.sel, 0, "defaults to standard");
    assert_eq!(picker.items[0].label, "Standard mode");
}

#[test]
fn slash_menu_exposes_agent_instead_of_mode() {
    let (mut app, _ctl, _rx) = test_app();

    app.input.set("/agent".into());
    let agent = app.slash_matches();
    assert_eq!(agent.len(), 1);
    assert_eq!(agent[0].name, "agent");

    app.input.set("/mode".into());
    assert!(app.slash_matches().iter().all(|entry| entry.name != "mode"));
}

#[test]
fn ctrl_shift_a_cycles_the_agent_directly_without_touching_the_draft() {
    let (mut app, ctl, rx) = test_app();
    app.input.set("keep this draft".into());

    app.handle_key(
        KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
        &ctl,
    );

    assert_eq!(app.input.buf, "keep this draft");
    assert!(app.picker.is_none(), "the shortcut must not open a form");

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while app.modes.agent_preset.as_deref() != Some("code") {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("ctrl+shift+a switches to the next agent preset");
        let ev = rx.recv_timeout(remaining).expect("agent preset event");
        app.handle(ev, &ctl);
    }
    assert_eq!(app.current_mode(), "code");
}

#[test]
fn cmd_left_moves_to_the_current_wrapped_line_start_not_the_draft_start() {
    let (mut app, ctl, _rx) = test_app();
    app.input.set("abcdefghij".into());
    app.input.cursor = 6;
    app.composer_wrap_width = 4;

    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER), &ctl);

    assert_eq!(app.input.cursor, 4, "second visual row starts before 'e'");
}

#[test]
fn cmd_right_moves_to_the_current_wrapped_line_end_not_the_draft_end() {
    let (mut app, ctl, _rx) = test_app();
    app.input.set("abcdefghij".into());
    app.input.cursor = 6;
    app.composer_wrap_width = 4;

    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER), &ctl);

    assert_eq!(app.input.cursor, 8, "second visual row ends after 'h'");
    assert_eq!(
        app.input.visual_cursor(4),
        (1, 4),
        "the wrap boundary keeps its upstream line-end affinity"
    );
}

#[test]
fn ctrl_u_kills_only_to_the_current_wrapped_line_start() {
    let (mut app, ctl, _rx) = test_app();
    app.input.set("abcdefghij".into());
    app.input.cursor = 6;
    app.composer_wrap_width = 4;

    app.handle_key(
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        &ctl,
    );

    assert_eq!(app.input.buf, "abcdghij");
    assert_eq!(app.input.cursor, 4, "the first visual row is preserved");
}

#[test]
fn ctrl_k_kills_only_to_the_current_wrapped_line_end() {
    let (mut app, ctl, _rx) = test_app();
    app.input.set("abcdefghij".into());
    app.input.cursor = 6;
    app.composer_wrap_width = 4;

    app.handle_key(
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        &ctl,
    );

    assert_eq!(app.input.buf, "abcdefij");
    assert_eq!(app.input.cursor, 6, "the final visual row is preserved");
}

#[test]
fn live_mode_picker_uses_advertised_composition_not_stock() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.handle(
        AppEvent::Ctl(CtlEvent::Catalog {
            models: Vec::new(),
            presets: vec![CatalogPreset {
                id: "cordis".into(),
                name: "Creator from ACP".into(),
                description: "inspect".into(),
                broken: false,
            }],
        }),
        &ctl,
    );
    app.run_slash("agent", "", &ctl);
    let picker = app.picker.as_ref().expect("mode picker opens");
    let ids: Vec<&str> = picker.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["cordis"]);
    assert_eq!(picker.items[0].label, "Creator from ACP");
}

#[test]
fn host_catalog_replaces_mode_picker_items() {
    let (mut app, ctl, _rx) = test_app();
    app.modes.agent_preset = Some("code".into());
    app.run_slash("agent", "", &ctl);
    app.handle(
        AppEvent::Ctl(CtlEvent::Catalog {
            models: Vec::new(),
            presets: vec![
                CatalogPreset {
                    id: "standard".into(),
                    name: "Standard mode".into(),
                    description: "full".into(),
                    broken: false,
                },
                CatalogPreset {
                    id: "code".into(),
                    name: "Code mode".into(),
                    description: "ts".into(),
                    broken: false,
                },
                CatalogPreset {
                    id: "custom".into(),
                    name: "Custom".into(),
                    description: "mine".into(),
                    broken: true,
                },
            ],
        }),
        &ctl,
    );
    let picker = app.picker.as_ref().expect("picker still open");
    assert_eq!(picker.items.len(), 3);
    assert_eq!(picker.sel, 1, "selection lands on the current mode");
    assert!(picker.items[2].meta.contains("broken"));
}

#[test]
fn host_catalog_model_picker_distinguishes_duplicate_ids_by_provider() {
    let (mut app, ctl, _rx) = test_app();
    app.cfg.provider = "coding-plan-b".into();
    app.cfg.model = "deepseek-v4".into();
    app.open_model_picker(&ctl);
    app.handle(
        AppEvent::Ctl(CtlEvent::Catalog {
            models: vec![
                CatalogModel {
                    provider: "coding-plan-a".into(),
                    id: "deepseek-v4".into(),
                    name: "DeepSeek V4".into(),
                    vision: false,
                },
                CatalogModel {
                    provider: "coding-plan-b".into(),
                    id: "deepseek-v4".into(),
                    name: "DeepSeek V4".into(),
                    vision: false,
                },
            ],
            presets: Vec::new(),
        }),
        &ctl,
    );

    let picker = app.picker.as_ref().expect("model picker stays open");
    assert_eq!(picker.items[0].meta, "coding-plan-a · DeepSeek V4");
    assert_eq!(picker.items[1].meta, "coding-plan-b · DeepSeek V4");
    assert_eq!(picker.sel, 1, "current provider and model identify the row");
}

#[test]
fn selecting_same_model_id_from_another_provider_switches_provider() {
    let (mut app, ctl, _rx) = test_app();
    app.cfg.provider = "coding-plan-a".into();
    app.cfg.model = "deepseek-v4".into();

    app.select_model(
        PickerItem {
            id: "deepseek-v4".into(),
            label: "deepseek-v4".into(),
            meta: "coding-plan-b · DeepSeek V4".into(),
            provider: Some("coding-plan-b".into()),
        },
        &ctl,
    );

    assert_eq!(app.cfg.provider, "coding-plan-b");
    assert_eq!(app.selected_model.as_deref(), Some("deepseek-v4"));
}

#[test]
fn demo_mode_selection_round_trips_the_durable_event() {
    let (mut app, ctl, rx) = test_app();
    app.run_slash("agent", "minimal", &ctl);
    // The demo controller synthesizes the agent-preset/selected fact.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while app.modes.agent_preset.is_none() {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("demo preset event before timeout");
        let ev = rx.recv_timeout(remaining).expect("bus event");
        app.handle(ev, &ctl);
    }
    assert_eq!(app.modes.agent_preset.as_deref(), Some("minimal"));
    assert_eq!(app.current_mode(), "minimal");
}

#[test]
fn preset_ack_folds_the_chip_and_new_session_waits_for_the_host_mode() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Ctl(CtlEvent::PresetSet {
            preset: "cordis".into(),
        }),
        &ctl,
    );
    assert_eq!(app.modes.agent_preset.as_deref(), Some("cordis"));
    app.run_slash("new", "fresh", &ctl);
    assert_eq!(app.session_id, "fresh");
    assert!(
        app.modes.agent_preset.is_none(),
        "/new must not reuse the previous session's agent preset"
    );
    assert_eq!(app.current_mode(), "standard");
}

#[test]
fn live_acp_new_does_not_invent_a_local_id() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    let before = app.session_id.clone();
    app.run_slash("new", "fresh", &ctl);
    assert_eq!(app.session_id, before, "ACP /new waits for session/new");
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-9".into(),
            notice: Some("new session · acp-9".into()),
        }),
        &ctl,
    );
    assert_eq!(app.session_id, "acp-9");
}

#[test]
fn agent_preset_event_updates_chrome_without_adding_a_transcript_row() {
    let (mut app, _ctl, _rx) = test_app();
    let cells_before = app.transcript.cells.len();

    app.apply_ui(crate::events::UiEvent::AgentPreset {
        session: app.session_id.clone(),
        preset: "cordis".into(),
    });

    assert_eq!(app.modes.agent_preset.as_deref(), Some("cordis"));
    assert_eq!(app.transcript.cells.len(), cells_before);
}

#[test]
fn acp_session_list_opens_picker_and_prefix_loads() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.load_session = true;
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionList {
            sessions: vec![
                SessionListItem {
                    id: "s-old".into(),
                    title: Some("hello".into()),
                    updated_at: Some("yesterday".into()),
                },
                SessionListItem {
                    id: "s-other".into(),
                    title: None,
                    updated_at: None,
                },
            ],
            prefix: None,
        }),
        &ctl,
    );
    let picker = app.picker.as_ref().expect("ACP resume picker");
    assert!(app.resume_via_acp);
    assert_eq!(picker.items[0].label, "hello");
    assert_eq!(picker.items[1].label, "s-other");

    app.picker = None;
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionList {
            sessions: vec![SessionListItem {
                id: "s-old".into(),
                title: None,
                updated_at: None,
            }],
            prefix: Some("s-old".into()),
        }),
        &ctl,
    );
    assert_eq!(app.session_id, "s-old");
    assert!(app.picker.is_none());
}

#[test]
fn acp_session_list_enriches_rows_with_local_summaries() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.load_session = true;
    // A local JSONL log for the same workspace (slug of "/tmp").
    let slug = crate::sessions::workspace_slug("/tmp");
    let dir = std::path::Path::new(&app.cfg.session_root)
        .join(&slug)
        .join("s-local");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
            dir.join("session.jsonl"),
            [
                r#"{"type":"session","version":0,"id":"s-local","createdAt":1,"cwd":"/tmp"}"#,
                r#"{"type":"turn/start","seq":1,"data":{"turn":1}}"#,
                r#"{"type":"user/message","seq":2,"data":{"content":[{"text":"local prompt","type":"text"}],"source":{"kind":"user"},"role":"user","id":"m1"}}"#,
                r#"{"type":"turn/start","seq":3,"data":{"turn":2}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

    app.handle(
        AppEvent::Ctl(CtlEvent::SessionList {
            sessions: vec![SessionListItem {
                id: "s-local".into(),
                title: None,
                updated_at: None,
            }],
            prefix: None,
        }),
        &ctl,
    );
    let picker = app.picker.as_ref().expect("picker");
    assert_eq!(
        picker.items[0].label, "local prompt",
        "local preview becomes the label"
    );
    assert!(
        picker.items[0].meta.contains("2 turns"),
        "{}",
        picker.items[0].meta
    );
    assert!(
        picker.items[0].meta.contains("s-local"),
        "short id in meta: {}",
        picker.items[0].meta
    );
}

#[test]
fn agent_caps_gate_resume_to_session_list() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.handle(
        AppEvent::Ctl(CtlEvent::AgentCaps { load_session: true }),
        &ctl,
    );
    assert!(app.load_session);
    app.run_slash("resume", "", &ctl);
    assert!(
        app.picker.is_none(),
        "live ACP /resume waits for session/list"
    );
}

#[test]
fn slash_permission_opens_picker_marking_current() {
    let (mut app, ctl, _rx) = test_app();
    app.run_slash("permission", "", &ctl);
    let picker = app.picker.as_ref().expect("permission picker opens");
    assert!(matches!(picker.kind, PickerKind::Permission));
    let ids: Vec<&str> = picker.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["read-only", "workspace-write", "danger-full-access"]);
    assert_eq!(picker.sel, 1, "workspace-write is the default");
    assert!(
        picker.items[1].meta.contains("default"),
        "unreported → marked default"
    );

    app.modes.permission = Some("danger-full-access".into());
    app.run_slash("permission", "", &ctl);
    let picker = app.picker.as_ref().expect("picker reopens");
    assert_eq!(picker.sel, 2, "selection lands on the reported preset");
    assert!(picker.items[2].meta.contains("current"));
}

#[test]
fn permission_aliases_normalize() {
    assert_eq!(normalize_permission("full"), Some("danger-full-access"));
    assert_eq!(normalize_permission("YOLO"), Some("danger-full-access"));
    assert_eq!(normalize_permission(" ws "), Some("workspace-write"));
    assert_eq!(normalize_permission("read-only"), Some("read-only"));
    assert_eq!(normalize_permission("RO"), Some("read-only"));
    assert_eq!(permission_label("read-only"), "Read Only");
    assert_eq!(permission_label("workspace-write"), "Workspace Write");
    assert_eq!(permission_label("danger-full-access"), "Full access");
}

#[test]
fn image_media_type_maps_extensions() {
    assert_eq!(media_type_for("a.png"), Some("image/png"));
    assert_eq!(media_type_for("a.JPEG"), Some("image/jpeg"));
    assert_eq!(media_type_for("/tmp/x.webp"), Some("image/webp"));
    assert_eq!(media_type_for("x.gif"), Some("image/gif"));
    assert_eq!(media_type_for("notes.txt"), None);
}

#[test]
fn slash_permission_alias_round_trips_the_durable_event() {
    let (mut app, ctl, rx) = test_app();
    app.run_slash("permission", "full", &ctl);
    // The demo controller synthesizes the permission/sandbox/approval triplet.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while app.modes.permission.is_none() || app.modes.approval.is_none() {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("demo permission event before timeout");
        let ev = rx.recv_timeout(remaining).expect("bus event");
        app.handle(ev, &ctl);
    }
    assert_eq!(app.modes.permission.as_deref(), Some("danger-full-access"));
    assert_eq!(app.modes.approval.as_deref(), Some("never"));
}

#[test]
fn shift_tab_cycles_between_the_stock_presets() {
    let (mut app, ctl, rx) = test_app();
    assert_eq!(
        app.current_permission(),
        "workspace-write",
        "assumed default"
    );

    let wait_for = |app: &mut App, target: &str| {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while app.modes.permission.as_deref() != Some(target) {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect(target);
            let ev = rx.recv_timeout(remaining).expect("bus event");
            app.handle(ev, &ctl);
        }
    };

    app.cycle_permission(&ctl);
    wait_for(&mut app, "danger-full-access");
    app.cycle_permission(&ctl);
    wait_for(&mut app, "read-only");
    app.cycle_permission(&ctl);
    wait_for(&mut app, "workspace-write");
    assert_eq!(app.current_permission(), "workspace-write");
}

#[test]
fn staged_images_live_as_inline_tokens_and_esc_clears_them() {
    let (mut app, ctl, _rx) = test_app();
    assert!(app.pending_images.is_empty());
    app.stage_image(
        "clipboard.png".into(),
        "clipboard".into(),
        "image/png".into(),
        vec![0u8; 8],
        String::new(),
    );
    app.stage_image(
        "shot-2.png".into(),
        "clipboard".into(),
        "image/png".into(),
        vec![1u8; 8],
        String::new(),
    );
    assert_eq!(
        app.pending_images.len(),
        2,
        "images stage instead of sending"
    );
    assert!(app.input.buf.contains("[image 1]") && app.input.buf.contains("[image 2]"));
    app.handle_esc(&ctl);
    assert!(app.input.is_empty(), "esc clears the draft");
    assert!(app.pending_images.is_empty(), "chips go with the draft");
}

#[test]
fn backspace_on_a_chip_cuts_the_whole_token() {
    let (mut app, ctl, _rx) = test_app();
    app.stage_image(
        "a.png".into(),
        "p".into(),
        "image/png".into(),
        vec![0u8; 4],
        String::new(),
    );
    app.stage_image(
        "b.png".into(),
        "p".into(),
        "image/png".into(),
        vec![1u8; 4],
        String::new(),
    );
    // Cursor sits right after "[image 2]" — one backspace eats the
    // whole token and un-stages that image only.
    app.handle_key(
        crossterm::event::KeyEvent::new(KeyCode::Backspace, crossterm::event::KeyModifiers::NONE),
        &ctl,
    );
    assert_eq!(
        app.pending_images.len(),
        1,
        "backspace pops the chip under the cursor"
    );
    assert_eq!(app.pending_images.get(0).unwrap().name, "a.png");
    assert!(app.input.buf.contains("[image 1]"));
    assert!(!app.input.buf.contains("[image 2]"));
}

#[test]
fn editing_a_token_away_unstages_its_image() {
    let (mut app, ctl, _rx) = test_app();
    app.stage_image(
        "a.png".into(),
        "p".into(),
        "image/png".into(),
        vec![0u8; 4],
        String::new(),
    );
    // Simulate a kill that leaves a broken token, then any key event.
    app.input.set("[image 1".into());
    app.handle_key(
        crossterm::event::KeyEvent::new(KeyCode::Char('x'), crossterm::event::KeyModifiers::NONE),
        &ctl,
    );
    assert!(
        app.pending_images.is_empty(),
        "broken token reconciles the tray"
    );
}

#[test]
fn esc_clears_draft_when_idle() {
    let (mut app, ctl, _rx) = test_app();
    app.input.buf = "hello".into();
    app.input.cursor = 5;
    app.handle_esc(&ctl);
    assert!(app.input.is_empty(), "single esc clears the draft");
}

#[test]
fn cancel_requested_stops_in_flight_tools() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.transcript.apply(crate::events::UiEvent::ToolCall {
        session: app.session_id.clone(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: r#"{"command":"grep"}"#.into(),
    });
    app.handle(AppEvent::Ctl(CtlEvent::CancelRequested), &ctl);
    assert_eq!(app.state_note, "cancelling");
    assert!(
        matches!(app.state, RunState::Running),
        "prompt has not unwound yet"
    );
    match &app.transcript.cells.last().unwrap().kind {
        crate::transcript::CellKind::Tool { ok, error, .. } => {
            assert_eq!(*ok, Some(false));
            assert_eq!(error.as_deref(), Some("cancelled"));
        }
        other => panic!("expected tool, got {other:?}"),
    }
}

#[test]
fn send_now_while_running_is_a_steer_not_a_cancelled_queue_item() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.input.set("change course".into());

    app.send_now(&ctl);

    assert!(matches!(app.state, RunState::Running));
    assert_eq!(app.queued, 0);
    assert_ne!(app.state_note, "cancelling");
    assert!(matches!(
        app.transcript.cells.last().map(|cell| &cell.kind),
        Some(crate::transcript::CellKind::User { text, queued: false })
            if text == "change course"
    ));
}

#[test]
fn enter_on_an_empty_composer_sends_the_queue_head_now() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first queued followup".into(), &ctl);
    app.send_agent_text("second queued followup".into(), &ctl);
    assert!(app.input.is_empty());

    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Steer { text, .. }) if text == "first queued followup"
    ));
    assert_eq!(app.queued, 1);
    assert_eq!(app.queue_previews(1)[0].summary, "second queued followup");
    assert!(matches!(
        app.transcript.cells.last().map(|cell| &cell.kind),
        Some(crate::transcript::CellKind::User { text, queued: false })
            if text == "first queued followup"
    ));
}

#[test]
fn a_deferred_queue_head_steer_returns_to_the_front() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first queued followup".into(), &ctl);
    app.send_agent_text("second queued followup".into(), &ctl);

    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );
    let message_id = match commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("queue head steer")
    {
        Cmd::Steer { message_id, .. } => message_id,
        other => panic!("expected queue head steer, got {other:?}"),
    };

    app.handle(
        AppEvent::Ctl(CtlEvent::SteerSettled {
            message_id,
            deferred: true,
        }),
        &ctl,
    );

    let previews = app.queue_previews(2);
    assert_eq!(previews[0].summary, "first queued followup");
    assert_eq!(previews[1].summary, "second queued followup");
}

#[test]
fn rejected_send_now_becomes_visible_fifo_without_stopping_the_active_turn() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.input.set("change course".into());

    app.send_now(&ctl);
    let message_id = *app
        .pending_steer_cells
        .keys()
        .next()
        .expect("tracked steer command");
    app.handle(
        AppEvent::Ctl(CtlEvent::SteerSettled {
            message_id,
            deferred: true,
        }),
        &ctl,
    );

    assert!(matches!(app.state, RunState::Running));
    assert_eq!(app.queued, 1);
    assert!(app.transcript.cells.iter().any(|cell| {
        cell.hidden
            && matches!(
                &cell.kind,
                crate::transcript::CellKind::User { text, .. } if text == "change course"
            )
    }));
    assert!(crate::ui::dump_frame(&mut app, 90, 24).contains("change course"));
}

#[test]
fn send_now_with_an_image_keeps_the_active_turn_running() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.stage_image(
        "shot.png".into(),
        "clipboard".into(),
        "image/png".into(),
        vec![1, 2, 3],
        String::new(),
    );

    app.send_now(&ctl);

    assert!(matches!(app.state, RunState::Running));
    assert_eq!(app.queued, 0);
    assert!(matches!(
        app.transcript.cells.last().map(|cell| &cell.kind),
        Some(crate::transcript::CellKind::Image { queued: false, .. })
    ));
}

#[test]
fn first_prompt_after_agent_ready_is_not_marked_queued() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Ctl(CtlEvent::Starting {
            runtime: "dsh-acp".into(),
        }),
        &ctl,
    );
    app.handle(
        AppEvent::Ctl(CtlEvent::Ready {
            server: "dsh-acp".into(),
        }),
        &ctl,
    );

    app.send_agent_text("first".into(), &ctl);

    assert_eq!(app.queued, 0);
    assert!(matches!(
        app.transcript.cells.last().map(|cell| &cell.kind),
        Some(crate::transcript::CellKind::User { queued: false, .. })
    ));
}

#[test]
fn startup_lifecycle_updates_state_without_adding_transcript_rows() {
    let (mut app, ctl, _rx) = test_app();
    let cells_before = app.transcript.cells.len();

    app.handle(
        AppEvent::Ctl(CtlEvent::Starting {
            runtime: "/usr/local/bin/dsh-acp".into(),
        }),
        &ctl,
    );
    assert!(matches!(app.state, RunState::Starting));
    assert_eq!(app.transcript.cells.len(), cells_before);

    app.handle(
        AppEvent::Ctl(CtlEvent::Ready {
            server: "dsh-acp".into(),
        }),
        &ctl,
    );
    assert!(matches!(app.state, RunState::Idle));
    assert_eq!(app.server_info.as_deref(), Some("dsh-acp"));
    assert_eq!(app.transcript.cells.len(), cells_before);
}

#[test]
fn first_prompt_during_runtime_start_is_active_and_only_the_second_queues() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Ctl(CtlEvent::Starting {
            runtime: "dsh-acp".into(),
        }),
        &ctl,
    );

    app.send_agent_text("first".into(), &ctl);
    app.send_agent_text("second".into(), &ctl);

    assert_eq!(app.queued, 1);
    let users = app
        .transcript
        .cells
        .iter()
        .filter_map(|cell| match &cell.kind {
            crate::transcript::CellKind::User { text, queued } => Some((text.as_str(), *queued)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(users, [("first", false)]);
    assert_eq!(app.queue_previews(1)[0].summary, "second");
}

#[test]
fn terminal_lifecycle_events_release_a_pending_first_prompt() {
    let events = vec![
        AppEvent::RuntimeExited(None),
        AppEvent::Ctl(CtlEvent::Error("failed".into())),
        AppEvent::Ctl(CtlEvent::Interrupted),
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: "dsh-test".into(),
            running: false,
        }),
    ];

    for event in events {
        let (mut app, ctl, _rx) = test_app();
        app.send_agent_text("first".into(), &ctl);
        app.handle(event, &ctl);
        app.send_agent_text("retry".into(), &ctl);

        assert_eq!(app.queued, 0);
        assert!(matches!(
            app.transcript.cells.last().map(|cell| &cell.kind),
            Some(crate::transcript::CellKind::User {
                text,
                queued: false
            }) if text == "retry"
        ));
    }
}

#[test]
fn bracketed_paste_wrapped_csi_u_ctrl_c_still_quits() {
    let (mut app, ctl, _rx) = test_app();

    for _ in 0..2 {
        app.handle(
            AppEvent::Term(Event::Paste("\u{1b}[99;5u".to_string())),
            &ctl,
        );
    }

    assert!(app.quit, "two Ctrl+C presses should quit from idle");
    assert!(
        app.input.is_empty(),
        "the CSI-u bytes must never enter the composer"
    );
}

#[test]
fn ordinary_and_mixed_paste_payloads_are_not_treated_as_keys() {
    let (mut app, ctl, _rx) = test_app();

    app.handle(
        AppEvent::Term(Event::Paste("hello\nworld".to_string())),
        &ctl,
    );
    app.handle(
        AppEvent::Term(Event::Paste(" literal \u{1b}[99;5u".to_string())),
        &ctl,
    );

    assert_eq!(app.input.buf, "hello world literal \u{1b}[99;5u");
    assert!(!app.quit);
}

#[test]
fn resetting_the_session_releases_a_pending_first_prompt() {
    let (mut app, ctl, _rx) = test_app();
    app.send_agent_text("old session".into(), &ctl);

    app.reset_session_ui();
    app.send_agent_text("new session".into(), &ctl);

    assert_eq!(app.queued, 0);
    assert!(matches!(
        app.transcript.cells.last().map(|cell| &cell.kind),
        Some(crate::transcript::CellKind::User {
            text,
            queued: false
        }) if text == "new session"
    ));
}

#[test]
fn resetting_the_session_discards_unsettled_steer_bookkeeping() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.input.set("old steer".into());
    app.send_now(&ctl);
    let message_id = *app
        .pending_steer_cells
        .keys()
        .next()
        .expect("steer is awaiting settlement");

    app.reset_session_ui();
    app.handle(
        AppEvent::Ctl(CtlEvent::SteerSettled {
            message_id,
            deferred: true,
        }),
        &ctl,
    );

    assert!(app.pending_steer_cells.is_empty());
    assert_eq!(app.queued, 0, "late settlement cannot taint a new session");
}

#[test]
fn resetting_the_session_discards_the_client_queue_and_active_edit() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.send_agent_text("old queued prompt".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(app.queue_edit.is_some());

    app.reset_session_ui();

    assert!(app.prompt_queue.is_empty());
    assert!(app.queue_edit.is_none());
    assert_eq!(app.queued, 0);
}

#[test]
fn runtime_exit_discards_delivery_state_owned_by_the_dead_actor() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.send_agent_text("queued followup".into(), &ctl);
    app.input.set("unsettled steer".into());
    app.send_now(&ctl);
    app.input.clear();
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(app.queued, 1);
    assert!(app.queue_edit.is_some());
    assert_eq!(app.pending_steer_cells.len(), 1);

    app.handle(AppEvent::RuntimeExited(Some(1)), &ctl);

    assert_eq!(app.queued, 0);
    assert!(app.prompt_queue.is_empty());
    assert!(app.queue_edit.is_none());
    assert!(app.pending_steer_cells.is_empty());
}

#[test]
fn interrupted_advances_one_client_followup_and_queues_later_input_behind_it() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.state_note = "cancelling".into();
    app.send_agent_text("first followup".into(), &ctl);
    app.handle(AppEvent::Ctl(CtlEvent::Interrupted), &ctl);
    assert!(matches!(app.state, RunState::Starting));
    assert_eq!(app.queued, 0);
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "first followup"
    ));

    app.send_agent_text("second followup".into(), &ctl);
    assert_eq!(
        app.queued, 1,
        "new input queues behind the followup already handed to the actor"
    );
    assert!(app.transcript.cells.iter().all(|cell| !matches!(
        &cell.kind,
        crate::transcript::CellKind::User { text, .. } if text == "second followup"
    )));
}

#[test]
fn interrupted_turn_renders_one_specific_terminal_notice() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Ui(crate::events::UiEvent::TurnEnd {
            session: app.session_id.clone(),
            kind: "interrupted".into(),
        }),
        &ctl,
    );
    app.handle(AppEvent::Ctl(CtlEvent::Interrupted), &ctl);

    let notices = app
        .transcript
        .cells
        .iter()
        .filter_map(|cell| match &cell.kind {
            crate::transcript::CellKind::Notice { text, .. } if text.contains("interrupt") => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notices, ["interrupted — turn cancelled"]);
}

#[test]
fn staged_input_joins_a_surviving_fifo_after_interrupt() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.send_agent_text("first followup".into(), &ctl);
    app.handle(AppEvent::Ctl(CtlEvent::Interrupted), &ctl);

    app.send_staged(
        vec![StagedBlock::Image(crate::attachments::Attachment {
            id: crate::attachments::KITTY_ID_BASE + 1,
            token: "[image 1]".into(),
            name: "shot.png".into(),
            path: "clipboard".into(),
            media_type: "image/png".into(),
            data: std::sync::Arc::from([1_u8, 2, 3]),
        })],
        &ctl,
    );

    assert_eq!(app.queued, 1);
    assert!(app
        .transcript
        .cells
        .iter()
        .all(|cell| !matches!(&cell.kind, crate::transcript::CellKind::Image { .. })));
    assert_eq!(app.queue_previews(1)[0].summary, "▣ shot.png");
}

#[test]
fn send_now_can_steer_while_a_surviving_fifo_is_advancing() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.send_agent_text("ordinary followup".into(), &ctl);
    app.handle(AppEvent::Ctl(CtlEvent::Interrupted), &ctl);
    app.input.set("urgent correction".into());

    app.send_now(&ctl);

    assert_eq!(app.queued, 0, "the ordinary followup is already advancing");
    assert_eq!(app.pending_steer_cells.len(), 1);
    assert!(matches!(
        app.transcript.cells.last().map(|cell| &cell.kind),
        Some(crate::transcript::CellKind::User {
            text,
            queued: false,
        }) if text == "urgent correction"
    ));
}

#[test]
fn followup_stays_in_the_client_until_the_active_turn_is_idle() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;

    app.send_agent_text("client-only followup".into(), &ctl);

    assert_eq!(app.queued, 1);
    assert!(
        app.transcript.cells.iter().all(|cell| !matches!(
            &cell.kind,
            crate::transcript::CellKind::User { text, .. }
                if text == "client-only followup"
        )),
        "a client-owned followup must not enter the timeline before dequeue"
    );
    assert!(
        matches!(
            commands.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "queueing must not issue session/prompt before the active turn settles"
    );
}

#[test]
fn idle_status_delivers_exactly_one_client_queued_prompt() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first followup".into(), &ctl);
    app.send_agent_text("second followup".into(), &ctl);

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "first followup"
    ));
    assert!(matches!(
        app.transcript
            .cells
            .iter()
            .find_map(|cell| match &cell.kind {
                crate::transcript::CellKind::User { text, queued } if text == "first followup" =>
                    Some(*queued),
                _ => None,
            }),
        Some(false)
    ));
    assert!(
        app.transcript.cells.iter().all(|cell| !matches!(
            &cell.kind,
            crate::transcript::CellKind::User { text, .. }
                if text == "second followup"
        )),
        "only the dequeued followup enters the timeline"
    );
    assert_eq!(app.queued, 1);
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn repeated_idle_status_cannot_release_two_client_queued_prompts() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first followup".into(), &ctl);
    app.send_agent_text("second followup".into(), &ctl);

    let idle = AppEvent::Ui(crate::events::UiEvent::SessionStatus {
        session: app.session_id.clone(),
        running: false,
    });
    app.handle(idle, &ctl);
    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "first followup"
    ));
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert_eq!(app.queued, 1);
}

#[test]
fn interrupted_turn_releases_one_client_queued_prompt() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first followup".into(), &ctl);
    app.send_agent_text("second followup".into(), &ctl);

    app.handle(AppEvent::Ctl(CtlEvent::Interrupted), &ctl);

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "first followup"
    ));
    assert_eq!(app.queued, 1);
}

#[test]
fn alt_up_preselects_the_latest_client_queued_prompt() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first followup".into(), &ctl);
    app.send_agent_text("second followup".into(), &ctl);

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    assert!(app.input.is_empty());
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert_eq!(app.input.buf, "second followup");
    assert_eq!(app.queued, 2, "editing keeps the original queue slot");
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn alt_up_then_arrows_can_edit_any_client_queued_prompt() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first followup".into(), &ctl);
    app.send_agent_text("second followup".into(), &ctl);
    app.send_agent_text("third followup".into(), &ctl);

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    assert!(
        app.input.is_empty(),
        "Alt+Up opens queue selection before editing"
    );
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert_eq!(app.input.buf, "second followup");
    assert_eq!(app.queued, 3, "selection keeps every FIFO slot in place");
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn queue_selector_keeps_and_highlights_the_selected_shelf_row() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.state = RunState::Running;
    for ordinal in ["first", "second", "third", "fourth", "fifth"] {
        app.send_agent_text(format!("{ordinal} shelf item"), &ctl);
    }

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);

    let frame = crate::ui::dump_frame(&mut app, 90, 24);
    assert!(
        frame.contains("▸ 5  fifth shelf item"),
        "selected queue row should remain visible and highlighted:\n{frame}"
    );
}

#[test]
fn saving_a_selected_queue_edit_preserves_fifo_position() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first followup".into(), &ctl);
    app.send_agent_text("second followup".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    app.input.set("first followup edited".into());

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert!(app.input.is_empty());
    assert_eq!(app.queued, 2, "saving replaces rather than appends");
    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "first followup edited"
    ));

    app.handle(
        AppEvent::Ctl(CtlEvent::PromptQueued {
            message_id: "first".into(),
        }),
        &ctl,
    );
    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "second followup"
    ));
}

#[test]
fn ctrl_d_then_enter_deletes_the_edited_queue_item() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("keep this".into(), &ctl);
    app.send_agent_text("delete this".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    app.handle_key(
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        &ctl,
    );
    assert_eq!(app.queued, 2, "Ctrl+D only asks for confirmation");
    assert_eq!(app.input.buf, "delete this");

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(app.queued, 1);
    assert!(app.input.is_empty());
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "keep this"
    ));
}

#[test]
fn queued_prompts_render_in_a_shelf_above_the_composer_not_the_timeline() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.state = RunState::Running;
    app.send_agent_text("queue-shelf-marker".into(), &ctl);
    app.input.set("composer-draft-marker".into());

    assert!(
        app.transcript.cells.iter().all(|cell| !matches!(
            &cell.kind,
            crate::transcript::CellKind::User { text, .. }
                if text == "queue-shelf-marker"
        )),
        "queued content belongs to the client shelf, not the transcript"
    );
    let frame = crate::ui::dump_frame(&mut app, 90, 24);
    assert!(frame.contains("Queue · 1"), "missing queue shelf:\n{frame}");
    assert!(
        frame.contains("enter send first"),
        "Queue should advertise empty Enter's immediate-send behavior:\n{frame}"
    );
    let queue_row = frame
        .lines()
        .position(|line| line.contains("queue-shelf-marker"))
        .expect("queued prompt preview");
    let composer_row = frame
        .lines()
        .position(|line| line.contains("composer-draft-marker"))
        .expect("composer draft");
    assert!(
        queue_row < composer_row,
        "queue shelf must sit above composer"
    );
    assert!(
        composer_row - queue_row <= 3,
        "queue shelf should be attached to the composer, not float in the timeline:\n{frame}"
    );
}

#[test]
fn native_queue_fallback_shows_every_item_when_space_allows() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.state = RunState::Running;
    for ordinal in 1..=7 {
        app.send_agent_text(format!("native queued prompt {ordinal}"), &ctl);
    }

    let frame = crate::ui::dump_frame(&mut app, 100, 28);
    for ordinal in 1..=7 {
        assert!(
            frame.contains(&format!("native queued prompt {ordinal}")),
            "native Queue row {ordinal} should remain visible when space allows:\n{frame}"
        );
    }
}

#[test]
fn deleting_a_queued_prompt_removes_it_from_the_queue_shelf() {
    let (mut app, ctl, _rx) = test_app();
    app.show_banner = false;
    app.state = RunState::Running;
    app.send_agent_text("keep-marker".into(), &ctl);
    app.send_agent_text("remove-marker".into(), &ctl);
    assert!(crate::ui::dump_frame(&mut app, 90, 24).contains("remove-marker"));
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    app.handle_key(
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        &ctl,
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    let frame = crate::ui::dump_frame(&mut app, 90, 24);
    assert!(
        frame.contains("keep-marker"),
        "unrelated queue item remains"
    );
    assert!(
        !frame.contains("remove-marker"),
        "deleted queue item must disappear:\n{frame}"
    );
}

#[test]
fn live_queue_changes_publish_a_client_compositor_snapshot() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    let cfg = test_cfg();
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let mut app = App::new(Theme::dark(), cfg, "live-session".into(), false, true, tx);
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.input.set("queued from composer".into());

    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    let Cmd::QueueSnapshot { snapshot } = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("Queue compositor snapshot")
    else {
        panic!("queueing a follow-up must publish Queue state")
    };
    assert_eq!(snapshot.count, 1);
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].ordinal, 1);
    assert_eq!(snapshot.items[0].summary, "queued from composer");
    assert_eq!(snapshot.selected_id, None);

    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT))),
        &ctl,
    );
    let Cmd::QueueSnapshot { snapshot } = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("Queue selection snapshot")
    else {
        panic!("opening Queue selection must publish its focus")
    };
    assert_eq!(snapshot.selected_id, Some(snapshot.items[0].id));
}

#[test]
fn live_queue_snapshot_contains_every_queued_prompt() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    let cfg = test_cfg();
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let mut app = App::new(Theme::dark(), cfg, "live-session".into(), false, true, tx);
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;

    let mut latest = None;
    for ordinal in 1..=7 {
        app.input.set(format!("queued prompt {ordinal}"));
        app.handle(
            AppEvent::Term(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            ))),
            &ctl,
        );
        let Cmd::QueueSnapshot { snapshot } = commands
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("Queue compositor snapshot")
        else {
            panic!("queueing a follow-up must publish Queue state")
        };
        latest = Some(snapshot);
    }

    let snapshot = latest.expect("last Queue snapshot");
    assert_eq!(snapshot.count, 7);
    assert_eq!(snapshot.items.len(), 7);
    assert_eq!(snapshot.items[0].summary, "queued prompt 1");
    assert_eq!(snapshot.items[6].summary, "queued prompt 7");
}

#[test]
fn escape_steps_back_from_delete_confirmation_then_cancels_queue_edit() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("original queued text".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    app.input.set("unsaved edit".into());
    app.handle_key(
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        &ctl,
    );

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert_eq!(app.input.buf, "unsaved edit");
    assert_eq!(app.queued, 1);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert!(app.input.is_empty());
    assert_eq!(app.queued, 1);
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(
        app.input.buf, "original queued text",
        "cancelling restores the unmodified queue item"
    );
}

#[test]
fn dismissed_completion_inside_queue_edit_does_not_open_history() {
    let (mut app, ctl, _rx) = test_app();
    app.input.history.push("old history".into());
    app.state = RunState::Running;
    app.send_agent_text("/model".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(app.slash_completion_open());
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &ctl);
    assert_eq!(
        app.input.buf, "/model",
        "queue editing owns cursor motion after its completion closes"
    );
}

#[test]
fn queue_delete_confirmation_closes_slash_recommendations() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.send_agent_text("/model".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(app.slash_completion_open());

    app.handle_key(
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        &ctl,
    );

    assert!(!app.slash_completion_open());
    assert!(app.queue_delete_confirming());
}

#[test]
fn queue_waits_when_the_item_at_the_head_is_being_edited() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("queued original".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    app.input.set("queued edited".into());

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert_eq!(app.queued, 1);

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "queued edited"
    ));
    assert_eq!(app.queued, 0);
}

#[test]
fn queue_waits_if_the_active_turn_ends_before_a_queue_choice_is_made() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("first queued".into(), &ctl);
    app.send_agent_text("second queued".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );

    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert_eq!(app.queued, 2);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "first queued"
    ));
    assert_eq!(app.queued, 1);
}

#[test]
fn cancelling_a_paused_head_edit_releases_the_original_prompt() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_agent_text("queued original".into(), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    app.input.set("unsaved edit".into());
    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "queued original"
    ));
    assert_eq!(app.queued, 0);
}

#[test]
fn queued_image_prompt_stays_client_side_and_restores_its_chip_for_editing() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.send_staged(
        vec![
            StagedBlock::Text("inspect ".into()),
            StagedBlock::Image(crate::attachments::Attachment {
                id: crate::attachments::KITTY_ID_BASE + 1,
                token: "[image 1]".into(),
                name: "shot.png".into(),
                path: "clipboard".into(),
                media_type: "image/png".into(),
                data: std::sync::Arc::from([1_u8, 2, 3]),
            }),
        ],
        &ctl,
    );

    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(app.input.buf.contains("inspect"));
    assert!(app.input.buf.contains("[image 1]"));
    assert_eq!(app.pending_images.len(), 1);
}

#[test]
fn saving_a_queued_image_edit_updates_preview_and_dequeued_blocks() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.show_banner = false;
    app.state = RunState::Running;
    app.send_staged(
        vec![
            StagedBlock::Text("inspect ".into()),
            StagedBlock::Image(crate::attachments::Attachment {
                id: crate::attachments::KITTY_ID_BASE + 1,
                token: "[image 1]".into(),
                name: "shot.png".into(),
                path: "clipboard".into(),
                media_type: "image/png".into(),
                data: std::sync::Arc::from([1_u8, 2, 3]),
            }),
        ],
        &ctl,
    );
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    app.input.insert_str(" revised-marker");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert!(
        crate::ui::dump_frame(&mut app, 90, 24).contains("revised-marker"),
        "the queued preview reflects the saved image draft"
    );
    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );
    let blocks = match commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("dequeued image prompt")
    {
        Cmd::PromptImages { blocks, .. } => blocks,
        _ => panic!("expected image prompt"),
    };
    assert!(blocks.iter().any(
        |block| matches!(block, crate::bus::PromptBlock::Text(text) if text.contains("revised-marker"))
    ));
}

#[test]
fn rejected_steer_falls_back_into_the_client_owned_queue() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.input.set("urgent correction".into());
    app.send_now(&ctl);
    let message_id = match commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("steer command")
    {
        Cmd::Steer {
            message_id, text, ..
        } => {
            assert_eq!(text, "urgent correction");
            message_id
        }
        _ => panic!("expected text steer"),
    };
    app.handle(
        AppEvent::Ctl(CtlEvent::SteerSettled {
            message_id,
            deferred: true,
        }),
        &ctl,
    );
    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: app.session_id.clone(),
            running: false,
        }),
        &ctl,
    );

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::Prompt { text, .. }) if text == "urgent correction"
    ));
}

#[test]
fn ctrl_c_with_a_draft_clears_it_before_starting_a_fresh_double_press_to_quit() {
    let (mut app, ctl, _rx) = test_app();
    app.ctrl_c_armed = Some(CtrlCQuitChord {
        started: Instant::now(),
        presses: 1,
        required: 2,
    });
    app.input.set("unfinished draft".into());

    app.handle_ctrl_c(&ctl);
    assert!(app.input.is_empty());
    assert!(
        app.ctrl_c_armed.is_none(),
        "clearing is not the first quit press"
    );
    assert!(!app.quit);

    app.handle_ctrl_c(&ctl);
    assert!(app.ctrl_c_armed.is_some());
    assert!(!app.quit);

    app.handle_ctrl_c(&ctl);
    assert!(app.quit);
}

#[test]
fn ctrl_c_while_starting_without_a_prompt_quits_after_two_empty_presses() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Starting;

    app.handle_ctrl_c(&ctl);
    assert!(!app.quit, "the first empty Ctrl+C arms the idle quit chord");

    app.handle_ctrl_c(&ctl);
    assert!(
        app.quit,
        "startup without an active turn uses the two-press chord"
    );
}

#[test]
fn ctrl_c_while_running_never_interrupts_and_two_empty_presses_quit() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_interruptible_controller();
    app.state = RunState::Running;

    app.handle_ctrl_c(&ctl);
    assert!(
        app.ctrl_c_armed.is_some(),
        "an empty Ctrl+C should arm quit even while the turn is running"
    );
    assert!(!app.quit);
    assert_eq!(app.state, RunState::Running);
    assert!(
        matches!(
            commands.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "Ctrl+C must not send Cmd::Interrupt"
    );

    app.handle_ctrl_c(&ctl);
    assert!(app.quit);
}

#[test]
fn skills_merge_into_slash_menu_and_builtins_shadow() {
    let (mut app, _ctl, _rx) = test_app();
    app.skills = vec![
        crate::bus::SkillInfo {
            name: "commit-helper".into(),
            description: "draft a commit".into(),
            input_hint: None,
            config_action: None,
            client_command: false,
        },
        crate::bus::SkillInfo {
            name: "help".into(),
            description: "shadowed by builtin".into(),
            input_hint: None,
            config_action: None,
            client_command: false,
        },
    ];
    app.input.set("/".into());
    let menu = app.slash_matches();
    let skills: Vec<&str> = menu
        .iter()
        .filter(|e| e.skill)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(
        skills,
        ["commit-helper"],
        "builtin /help shadows the skill name"
    );
    app.input.set("/commit".into());
    let menu = app.slash_matches();
    assert_eq!(menu.len(), 1);
    assert!(menu[0].skill);
    assert_eq!(menu[0].usage, "/commit-helper");
}

#[test]
fn acp_client_command_is_listed_and_invoked_locally() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.skills = crate::events::skills_from_available_commands(&serde_json::json!([{
        "name": "plan-view",
        "description": "Open the current ACP plan",
        "_meta": {
            "commandAction": {
                "kind": "clientCommand",
                "presentation": "view"
            }
        }
    }]));
    app.input.set("/plan-view".into());

    let menu = app.slash_matches();
    assert_eq!(menu.len(), 1);
    assert!(
        menu[0].plugin,
        "ACP client command stays on the Client plane"
    );
    assert!(
        !menu[0].skill,
        "ACP client command must not be presented as a host skill"
    );

    app.submit(&ctl);
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::InvokePluginCommand { name, args })
            if name == "plan-view" && args.is_empty()
    ));
    assert!(
        app.transcript.cells.is_empty(),
        "no agent prompt cell is created"
    );
}

#[test]
fn client_plugin_command_catalog_does_not_interpret_legacy_theme_metadata() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::COMMANDS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "commands": [{
                    "name": "liang-effort",
                    "description": "slide the reasoning effort",
                    "whenTheme": "liang"
                }]
            }),
        },
        &ctl,
    );

    app.input.set("/liang-eff".into());
    let matches = app.slash_matches();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "liang-effort");
    assert_eq!(matches[0].desc, "slide the reasoning effort");
}

#[test]
fn client_plugin_command_invocation_stays_out_of_the_agent_prompt() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::COMMANDS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "commands": [{
                    "name": "liang-effort",
                    "description": "slide the reasoning effort"
                }]
            }),
        },
        &ctl,
    );
    app.input.set("/liang-effort".into());

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("plugin command is sent to the compositor");
    assert!(matches!(
        command,
        Cmd::InvokePluginCommand { name, args }
            if name == "liang-effort" && args.is_empty()
    ));
    assert!(app.input.is_empty());
}

#[test]
fn ui_plugin_catalog_reuses_the_upward_slash_menu_and_selects_over_acp() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.show_banner = false;
    app.handle(
            AppEvent::Rpc {
                method: crate::cordis::UI_UPDATE.into(),
                params: serde_json::json!({
                    "protocol": 0,
                    "plugins": [
                        { "id": "default", "label": "Martty", "source": "static", "status": "active" },
                        { "id": "deepseek", "label": "DeepSeek", "source": "dynamic", "status": "stopped" }
                    ]
                }),
            },
            &ctl,
        );
    app.input.set("/ui ".into());

    let menu = app.slash_matches();
    assert_eq!(menu.len(), 2);
    assert_eq!(menu[0].usage, "Martty");
    assert_eq!(menu[1].usage, "DeepSeek");
    assert_eq!(menu[1].desc, "dynamic · stopped");
    assert_eq!(menu[1].completion.as_deref(), Some("/ui deepseek"));

    let frame = crate::ui::dump_frame(&mut app, 100, 28);
    let menu_y = frame
        .lines()
        .position(|line| line.contains("DeepSeek"))
        .unwrap();
    let input_y = frame
        .lines()
        .position(|line| line.contains("/ui "))
        .unwrap();
    assert!(
        menu_y < input_y,
        "argument candidates stay above the composer:\n{frame}"
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginUiSelected { agent_id, id })
            if agent_id == "dsh-test" && id == "deepseek"
    ));
}

#[test]
fn tab_completes_a_slash_argument_without_running_it() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.input.set("/plan o".into());

    let menu = app.slash_matches();
    assert_eq!(
        menu.iter()
            .map(|entry| entry.usage.as_str())
            .collect::<Vec<_>>(),
        ["on", "off"]
    );

    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &ctl);
    assert_eq!(app.input.buf, "/plan on");
    assert!(matches!(
        commands.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn escape_dismisses_slash_completion_then_arrows_browse_history() {
    let (mut app, ctl, _rx) = test_app();
    app.input.history.push("previous prompt".into());
    app.input.set("/mo".into());

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert_eq!(
        app.input.buf, "/mo",
        "closing recommendations must preserve the current slash draft"
    );

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &ctl);
    assert_eq!(app.input.buf, "previous prompt");

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    assert_eq!(
        app.input.buf, "/mo",
        "leaving history restores the dismissed slash draft"
    );
}

#[test]
fn editing_a_dismissed_slash_draft_reopens_completion() {
    let (mut app, ctl, _rx) = test_app();
    app.input.set("/mo".into());
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert!(!app.slash_completion_open());

    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE), &ctl);
    assert!(app.slash_completion_open());

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    app.handle(
        AppEvent::Term(crossterm::event::Event::Paste("e".into())),
        &ctl,
    );
    assert!(
        app.slash_completion_open(),
        "pasting into a dismissed draft is also an edit"
    );
}

#[test]
fn plugin_slider_moves_between_effort_marks_for_material_preview() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "slider",
                    "id": "liang-effort",
                    "title": "Liang reasoning effort",
                    "min": 0,
                    "max": 30,
                    "step": 1,
                    "marks": [
                        { "value": 0, "id": "off", "label": "Off" },
                        { "value": 15, "id": "high", "label": "High" },
                        { "value": 30, "id": "max", "label": "Max" }
                    ],
                    "value": 15
                }
            }),
        },
        &ctl,
    );

    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);

    assert_eq!(
        app.slider_overlay.as_ref().map(|slider| slider.value),
        Some(16.0),
        "the preview axis must have values between effort marks"
    );
    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("slider changes notify the client plugin");
    assert!(matches!(
        command,
        Cmd::PluginOverlayEvent { id, event, value }
            if id == "liang-effort"
                && event == "change"
                && value == Some(serde_json::json!(16.0))
    ));
}

#[test]
fn plugin_select_form_renders_rows_and_submits_the_selected_value() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "select",
                    "id": "ui-preset",
                    "title": "UI preset",
                    "value": "default",
                    "options": [
                        {
                            "value": "default",
                            "label": "Martty",
                            "description": "Ocean blue terminal identity"
                        },
                        {
                            "value": "deepseek",
                            "label": "DeepSeek",
                            "description": "Classic Harness identity"
                        }
                    ]
                }
            }),
        },
        &ctl,
    );

    let frame = crate::ui::dump_frame(&mut app, 100, 30);
    assert!(frame.contains("UI preset"), "form title:\n{frame}");
    assert!(frame.contains("Martty"), "first option:\n{frame}");
    assert!(frame.contains("DeepSeek"), "second option:\n{frame}");
    assert!(
        frame.contains("Ocean blue terminal identity"),
        "description:\n{frame}"
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert!(app.select_overlay.is_none());
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginOverlayEvent { id, event, value })
            if id == "ui-preset"
                && event == "change"
                && value == Some(serde_json::json!("deepseek"))
    ));
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginOverlayEvent { id, event, value })
            if id == "ui-preset"
                && event == "submit"
                && value == Some(serde_json::json!("deepseek"))
    ));
}

#[test]
fn plugin_slider_enter_submits_the_effort_and_closes() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "slider",
                    "id": "liang-effort",
                    "title": "Liang reasoning effort",
                    "min": 0,
                    "max": 30,
                    "step": 1,
                    "marks": [
                        { "value": 0, "id": "off", "label": "Off" },
                        { "value": 15, "id": "high", "label": "High" },
                        { "value": 30, "id": "max", "label": "Max" }
                    ],
                    "snapToMarks": true,
                    "value": 16
                }
            }),
        },
        &ctl,
    );

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert!(app.slider_overlay.is_none());
    let command = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("slider submit notifies the client plugin");
    assert!(matches!(
        command,
        Cmd::PluginOverlayEvent { id, event, value }
            if id == "liang-effort"
                && event == "submit"
                && value == Some(serde_json::json!(15.0))
    ));
}

#[test]
fn plugin_slider_supports_a_plain_numeric_axis_without_marks() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "slider",
                    "id": "generic-threshold",
                    "title": "Threshold",
                    "min": 0,
                    "max": 100,
                    "step": 5,
                    "value": 40
                }
            }),
        },
        &ctl,
    );

    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &ctl);

    assert_eq!(
        app.slider_overlay.as_ref().map(|slider| slider.value),
        Some(45.0)
    );
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginOverlayEvent { id, event, value })
            if id == "generic-threshold"
                && event == "change"
                && value == Some(serde_json::json!(45.0))
    ));
}

#[test]
fn plugin_slider_left_steps_on_the_numeric_axis() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "slider",
                    "id": "generic-threshold",
                    "title": "Threshold",
                    "min": 0,
                    "max": 100,
                    "step": 5,
                    "value": 40
                }
            }),
        },
        &ctl,
    );

    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), &ctl);

    assert_eq!(
        app.slider_overlay.as_ref().map(|slider| slider.value),
        Some(35.0)
    );
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginOverlayEvent { id, event, value })
            if id == "generic-threshold"
                && event == "change"
                && value == Some(serde_json::json!(35.0))
    ));
}

#[test]
fn plugin_slider_escape_cancels_and_closes() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "slider",
                    "id": "generic-threshold",
                    "title": "Threshold",
                    "min": 0,
                    "max": 100,
                    "step": 5,
                    "value": 40
                }
            }),
        },
        &ctl,
    );

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);

    assert!(app.slider_overlay.is_none());
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginOverlayEvent { id, event, value })
            if id == "generic-threshold"
                && event == "cancel"
                && value == Some(serde_json::json!(40.0))
    ));
}

#[test]
fn plugin_view_escape_closes_the_generic_node_modal() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "view",
                    "id": "plan-view",
                    "title": "Plan",
                    "nodes": [{
                        "id": "step-1",
                        "kind": "generic",
                        "title": "Inspect",
                        "body": "priority · high",
                        "status": "running"
                    }]
                }
            }),
        },
        &ctl,
    );

    assert!(app.view_overlay.is_some());
    let frame = crate::ui::dump_frame(&mut app, 100, 30);
    assert!(frame.contains("Plan"), "view title:\n{frame}");
    assert!(frame.contains("Inspect"), "view node:\n{frame}");

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);

    assert!(app.view_overlay.is_none());
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginOverlayEvent { id, event, value })
            if id == "plan-view" && event == "cancel" && value.is_none()
    ));
}

#[test]
fn exact_agent_slash_command_tab_completes() {
    let (mut app, ctl, _rx) = test_app();
    app.input.set("/agent".into());

    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &ctl);

    assert_eq!(app.input.buf, "/agent ");
}

#[test]
fn advertised_plan_config_action_switches_without_starting_a_prompt() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![crate::bus::SkillInfo {
        name: "plan".into(),
        description: "Enter plan mode".into(),
        input_hint: None,
        config_action: Some(crate::bus::CommandConfigAction {
            config_id: "collaboration_mode".into(),
            value: "plan".into(),
            reset_value: Some("default".into()),
        }),
        client_command: false,
    }];
    let cells_before = app.transcript.cells.len();

    app.run_slash("plan", "", &ctl);

    assert!(matches!(app.state, RunState::Idle));
    assert_eq!(
        app.transcript.cells.len(),
        cells_before,
        "client commands do not create prompt transcript cells"
    );
}

#[test]
fn advertised_plan_command_toggles_off_when_plan_is_active() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.skills = vec![crate::bus::SkillInfo {
        name: "plan".into(),
        description: "Enter plan mode".into(),
        input_hint: None,
        config_action: Some(crate::bus::CommandConfigAction {
            config_id: "collaboration_mode".into(),
            value: "plan".into(),
            reset_value: Some("default".into()),
        }),
        client_command: false,
    }];
    app.modes.plan = true;

    app.run_slash("plan", "", &ctl);

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::SetConfigOption { config_id, value })
            if config_id == "collaboration_mode" && value == "default"
    ));
}

#[test]
fn direct_plan_mode_facts_fold_once_into_client_state() {
    let (mut app, ctl, _rx) = test_app();

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::PlanMode {
            session: "dsh-test".into(),
            active: true,
        }),
        &ctl,
    );
    assert!(app.modes.plan);
    let cells_after_first = app.transcript.cells.len();

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::PlanMode {
            session: "dsh-test".into(),
            active: true,
        }),
        &ctl,
    );
    assert_eq!(
        app.transcript.cells.len(),
        cells_after_first,
        "the same config_option_update is idempotent"
    );
}

#[test]
fn initial_default_plan_mode_does_not_add_an_off_notice() {
    let (mut app, ctl, _rx) = test_app();
    let cells_before = app.transcript.cells.len();

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::PlanMode {
            session: "dsh-test".into(),
            active: false,
        }),
        &ctl,
    );

    assert_eq!(app.transcript.cells.len(), cells_before);
}

#[test]
fn direct_ui_turn_facts_update_client_lifecycle() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.run_started = Some(Instant::now());
    app.state_note = "working".into();

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::TurnStart {
            session: "dsh-test".into(),
            turn: 1,
        }),
        &ctl,
    );
    app.handle(
        AppEvent::Ctl(CtlEvent::PromptQueued {
            message_id: "dsh-test".into(),
        }),
        &ctl,
    );

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: "dsh-test".into(),
            running: false,
        }),
        &ctl,
    );
    assert!(matches!(app.state, RunState::Idle));
    assert!(app.run_started.is_none());
    assert!(app.state_note.is_empty());
}

#[test]
fn plan_message_keeps_the_slash_prompt_transport() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![crate::bus::SkillInfo {
        name: "plan".into(),
        description: "Enter plan mode".into(),
        input_hint: None,
        config_action: Some(crate::bus::CommandConfigAction {
            config_id: "collaboration_mode".into(),
            value: "plan".into(),
            reset_value: Some("default".into()),
        }),
        client_command: false,
    }];

    app.run_slash("plan", "focus on the parser", &ctl);

    assert!(matches!(app.state, RunState::Starting));
    assert!(matches!(
        &app.transcript.cells[0].kind,
        crate::transcript::CellKind::User { text, .. }
            if text == "/plan focus on the parser"
    ));
}

#[test]
fn skill_line_ships_as_prompt_not_unknown_command() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![crate::bus::SkillInfo {
        name: "commit-helper".into(),
        description: "draft a commit".into(),
        input_hint: None,
        config_action: None,
        client_command: false,
    }];
    app.input.set("/commit-helper for the last change".into());
    app.submit(&ctl);
    assert!(
        matches!(app.state, RunState::Starting),
        "skill line starts a turn"
    );
    assert!(app.input.is_empty());
}

#[test]
fn accepting_a_skill_completes_then_sends() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![crate::bus::SkillInfo {
        name: "commit-helper".into(),
        description: "draft a commit".into(),
        input_hint: None,
        config_action: None,
        client_command: false,
    }];
    app.input.set("/commit".into());
    let entry = app.slash_matches()[0].clone();
    app.accept_slash(&entry, &ctl);
    assert_eq!(
        app.input.buf, "/commit-helper ",
        "first accept completes the name"
    );
    assert!(matches!(app.state, RunState::Idle));
    app.accept_slash(&entry, &ctl);
    assert!(
        matches!(app.state, RunState::Starting),
        "second accept ships the prompt"
    );
}

#[test]
fn login_is_not_a_tui_builtin_so_the_agent_slash_ships() {
    let (mut app, ctl, _rx) = test_app();
    assert!(
        !SLASH_COMMANDS.iter().any(|c| c.name == "login"),
        "/login belongs to the agent, like Backchat's composer"
    );
    app.skills = vec![crate::bus::SkillInfo {
        name: "login".into(),
        description: "Save a DeepSeek API key into the harness credential store".into(),
        input_hint: None,
        config_action: None,
        client_command: false,
    }];
    app.input.set("/log".into());
    assert!(
        app.slash_matches()
            .iter()
            .any(|e| e.skill && e.name == "login"),
        "agent /login stays in the slash menu"
    );
    app.input.set("/login sk-test".into());
    app.submit(&ctl);
    assert!(
        matches!(app.state, RunState::Starting),
        "agent /login is a prompt"
    );
}

#[test]
fn auth_slash_queues_terminal_launch_like_backchat_sign_in() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    let methods = crate::acp_auth::parse_auth_methods(
        &serde_json::json!([{
            "id": "terminal-login",
            "name": "Log in with a DeepSeek API key",
            "_meta": { "terminal-auth": { "args": ["login"], "env": {} } }
        }]),
        &["dsh-acp".into()],
        "/tmp",
        &Default::default(),
    );
    app.auth = crate::acp_auth::needs_auth_snapshot(methods.clone(), methods.first(), None);
    app.run_slash("auth", "", &ctl);
    let launch = app.take_terminal_auth().expect("terminal auth");
    assert_eq!(launch.command, "dsh-acp");
    assert_eq!(launch.args, ["login"]);
    assert_eq!(launch.method_id, "terminal-login");
}

#[test]
fn auth_in_demo_does_not_leave_the_tui() {
    let (mut app, ctl, _rx) = test_app();
    app.run_slash("auth", "", &ctl);
    assert!(app.take_terminal_auth().is_none());
}

#[test]
fn logout_is_hidden_from_the_slash_menu() {
    let (mut app, _ctl, _rx) = test_app();
    app.skills = vec![
        crate::bus::SkillInfo {
            name: "logout".into(),
            description: "sign out".into(),
            input_hint: None,
            config_action: None,
            client_command: false,
        },
        crate::bus::SkillInfo {
            name: "login".into(),
            description: "agent login".into(),
            input_hint: None,
            config_action: None,
            client_command: false,
        },
    ];
    app.input.set("/".into());
    let matches = app.slash_matches();
    let names: Vec<&str> = matches
        .iter()
        .filter(|e| e.skill)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["login"]);
    assert!(!SLASH_COMMANDS.iter().any(|c| c.name == "logout"));
}

#[test]
fn empty_auth_with_several_methods_opens_the_picker() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    let methods = crate::acp_auth::parse_auth_methods(
        &serde_json::json!([
            {
                "id": "terminal-login",
                "name": "Terminal",
                "_meta": { "terminal-auth": { "args": ["login"], "env": {} } }
            },
            {
                "id": "api-key",
                "name": "API key",
                "_meta": { "api-key": { "provider": "openai" } }
            }
        ]),
        &["dsh-acp".into()],
        "/tmp",
        &Default::default(),
    );
    app.auth = crate::acp_auth::needs_auth_snapshot(methods.clone(), methods.first(), None);
    app.run_slash("auth", "", &ctl);
    let picker = app.picker.as_ref().expect("auth picker");
    assert!(matches!(picker.kind, PickerKind::Auth));
    assert_eq!(picker.items.len(), 2);
    assert!(app.take_terminal_auth().is_none());
}

#[test]
fn open_auth_preserves_draft_and_pending_fifo() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.state = RunState::Running;
    app.send_agent_text("queued followup".into(), &ctl);
    assert_eq!(app.queued, 1);
    assert_eq!(app.prompt_queue.len(), 1);
    app.input.set("keep this draft".into());
    let methods = crate::acp_auth::parse_auth_methods(
        &serde_json::json!([
            {
                "id": "terminal-login",
                "name": "Terminal",
                "_meta": { "terminal-auth": { "args": ["login"], "env": {} } }
            },
            {
                "id": "api-key",
                "name": "API key",
                "_meta": { "api-key": { "provider": "openai" } }
            }
        ]),
        &["dsh-acp".into()],
        "/tmp",
        &Default::default(),
    );
    app.auth = crate::acp_auth::needs_auth_snapshot(methods.clone(), methods.first(), None);
    app.handle(AppEvent::Ctl(CtlEvent::OpenAuth), &ctl);
    assert_eq!(app.input.buf, "keep this draft");
    assert!(matches!(app.state, RunState::Idle));
    assert_eq!(app.queued, 1);
    assert_eq!(app.prompt_queue.len(), 1);
    assert!(app.transcript.cells.iter().all(|cell| !matches!(
        &cell.kind,
        crate::transcript::CellKind::User { text, .. } if text == "queued followup"
    )));
    assert!(matches!(
        app.picker.as_ref().map(|p| p.kind),
        Some(PickerKind::Auth)
    ));
    let tip = app.tip.as_ref().map(|(t, _)| t.as_str()).unwrap_or("");
    assert!(tip.contains("/auth"), "tip: {tip}");
    assert!(!tip.contains("/login"), "tip: {tip}");
}

#[test]
fn auth_retry_does_not_consume_the_followup_fifo_marker() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.state = RunState::Running;
    app.send_agent_text("queued followup".into(), &ctl);

    app.handle(
        AppEvent::Ctl(CtlEvent::Auth(crate::acp_auth::AuthSnapshot {
            status: crate::acp_auth::AuthStatus::NeedsAuth,
            method_id: Some("login".into()),
            method_name: Some("Login".into()),
            methods: Vec::new(),
            message: Some("authentication required".into()),
        })),
        &ctl,
    );
    app.handle(
        AppEvent::Ctl(CtlEvent::PromptQueued {
            message_id: "dsh-test".into(),
        }),
        &ctl,
    );

    assert_eq!(
        app.queued, 1,
        "the retry belongs to the original active prompt, not the FIFO"
    );
    assert_eq!(app.prompt_queue.len(), 1);
}

#[test]
fn demo_open_auth_does_not_start_acp_sign_in() {
    let (mut app, ctl, _rx) = test_app();
    app.input.set("draft".into());
    app.handle(AppEvent::Ctl(CtlEvent::OpenAuth), &ctl);
    assert_eq!(app.input.buf, "draft");
    assert!(app.picker.is_none());
    assert!(app.take_terminal_auth().is_none());
}

#[test]
fn staged_images_send_together_with_token_free_caption() {
    let (mut app, ctl, _rx) = test_app();
    app.stage_image(
        "clipboard.png".into(),
        "clipboard".into(),
        "image/png".into(),
        vec![0u8; 8],
        String::new(),
    );
    app.stage_image(
        "shot-2.png".into(),
        "clipboard".into(),
        "image/png".into(),
        vec![1u8; 8],
        String::new(),
    );
    app.input.insert_str("look");
    app.submit(&ctl);
    assert!(app.pending_images.is_empty(), "tray cleared after send");
    assert!(app.input.is_empty());
    assert!(
        matches!(app.state, RunState::Starting),
        "sending starts the turn"
    );
}

#[test]
fn draft_split_keeps_text_and_images_interleaved() {
    let mut staged = crate::attachments::Staged::default();
    staged
        .add(
            "a.png".into(),
            "/tmp/a.png".into(),
            "image/png".into(),
            vec![1],
        )
        .unwrap();
    staged
        .add(
            "b.png".into(),
            "/tmp/b.png".into(),
            "image/png".into(),
            vec![2],
        )
        .unwrap();
    let blocks =
        split_draft_into_staged_blocks("see [image 1] then [image 2] done", staged.drain());
    assert_eq!(blocks.len(), 5);
    assert!(matches!(&blocks[0], StagedBlock::Text(t) if t == "see"));
    assert!(matches!(&blocks[1], StagedBlock::Image(a) if a.name == "a.png"));
    assert!(matches!(&blocks[2], StagedBlock::Text(t) if t == " then "));
    assert!(matches!(&blocks[3], StagedBlock::Image(a) if a.name == "b.png"));
    assert!(matches!(&blocks[4], StagedBlock::Text(t) if t == "done"));
    let prompt = prompt_blocks_from_staged(blocks);
    assert!(matches!(&prompt[0], crate::bus::PromptBlock::Text(t) if t == "see"));
    assert!(matches!(&prompt[1], crate::bus::PromptBlock::Image(a) if a.path == "/tmp/a.png"));
    assert!(matches!(&prompt[2], crate::bus::PromptBlock::Text(t) if t == " then "));
    assert!(matches!(&prompt[3], crate::bus::PromptBlock::Image(a) if a.path == "/tmp/b.png"));
    assert!(matches!(&prompt[4], crate::bus::PromptBlock::Text(t) if t == "done"));
}

#[test]
fn draft_split_does_not_append_chips_missing_from_the_draft() {
    let mut staged = crate::attachments::Staged::default();
    staged
        .add(
            "kept.png".into(),
            "/tmp/kept.png".into(),
            "image/png".into(),
            vec![1],
        )
        .unwrap();
    staged
        .add(
            "orphan.png".into(),
            "/tmp/orphan.png".into(),
            "image/png".into(),
            vec![2],
        )
        .unwrap();
    let blocks = split_draft_into_staged_blocks("hello [image 1]", staged.drain());
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], StagedBlock::Text(t) if t == "hello"));
    assert!(matches!(&blocks[1], StagedBlock::Image(a) if a.name == "kept.png"));
}

#[test]
fn submit_echoes_interleaved_transcript_not_caption_then_images() {
    let (mut app, ctl, _rx) = test_app();
    app.stage_image(
        "a.png".into(),
        "/tmp/a.png".into(),
        "image/png".into(),
        vec![0u8; 4],
        String::new(),
    );
    app.stage_image(
        "b.png".into(),
        "/tmp/b.png".into(),
        "image/png".into(),
        vec![1u8; 4],
        String::new(),
    );
    app.input.set("see [image 1] then [image 2] done".into());
    app.submit(&ctl);
    let kinds: Vec<String> = app
        .transcript
        .cells
        .iter()
        .map(|c| match &c.kind {
            crate::transcript::CellKind::User { text, .. } => format!("text:{text}"),
            crate::transcript::CellKind::Image { name, caption, .. } => {
                format!("image:{name}:{caption}")
            }
            _ => "other".into(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "text:see",
            "image:a.png:",
            "text: then ",
            "image:b.png:",
            "text:done"
        ]
    );
}

fn ask_options() -> Vec<crate::bus::PermissionAskOption> {
    vec![
        crate::bus::PermissionAskOption {
            option_id: "reject".into(),
            kind: "reject_once".into(),
            name: "Reject".into(),
        },
        crate::bus::PermissionAskOption {
            option_id: "allow".into(),
            kind: "allow_once".into(),
            name: "Allow once".into(),
        },
    ]
}

#[test]
fn acp_permission_ask_enter_selects_option_id() {
    let (mut app, ctl, _rx) = test_app();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::PermissionAsk {
            title: "bash".into(),
            options: ask_options(),
            reply: tx,
        },
        &ctl,
    );
    let ask = app.permission_ask.as_ref().expect("overlay opens");
    assert_eq!(ask.title, "bash");
    assert_eq!(ask.sel, 1, "allow_once is preselected, not auto-chosen");
    assert_eq!(ask.options[0].name, "Reject");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(app.permission_ask.is_none());
    assert_eq!(
        rx.blocking_recv().expect("reply"),
        crate::bus::PermissionAskReply::Selected("allow".into())
    );
}

#[test]
fn acp_permission_ask_esc_cancels() {
    let (mut app, ctl, _rx) = test_app();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::PermissionAsk {
            title: "bash".into(),
            options: ask_options(),
            reply: tx,
        },
        &ctl,
    );
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert!(app.permission_ask.is_none());
    assert_eq!(
        rx.blocking_recv().expect("reply"),
        crate::bus::PermissionAskReply::Cancelled
    );
}

#[test]
fn acp_elicitation_form_opens_and_returns_the_selected_value() {
    use crate::elicitation::{
        ElicitationField, ElicitationFieldKind, ElicitationForm, ElicitationOption,
        ElicitationReply, ElicitationValue,
    };

    let (mut app, ctl, _rx) = test_app();
    let draft = app.input.buf.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::ElicitationAsk {
            form: ElicitationForm {
                message: "The agent needs your input.".into(),
                fields: vec![ElicitationField {
                    name: "question_0".into(),
                    custom_name: None,
                    title: "Target".into(),
                    description: Some("Where should this run?".into()),
                    required: true,
                    kind: ElicitationFieldKind::Single {
                        options: vec![
                            ElicitationOption {
                                value: "local".into(),
                                label: "Local".into(),
                                description: None,
                                custom: false,
                            },
                            ElicitationOption {
                                value: "remote".into(),
                                label: "Remote".into(),
                                description: None,
                                custom: false,
                            },
                        ],
                        default: None,
                    },
                }],
            },
            reply: tx,
        },
        &ctl,
    );
    assert!(app.elicitation_ask.is_some(), "form overlay opens");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert!(app.elicitation_ask.is_none());
    assert_eq!(
        app.input.buf, draft,
        "form input never overwrites the composer"
    );
    assert_eq!(
        rx.blocking_recv().expect("reply"),
        ElicitationReply::Accepted(std::collections::BTreeMap::from([(
            "question_0".into(),
            ElicitationValue::String("remote".into()),
        )]))
    );
}
