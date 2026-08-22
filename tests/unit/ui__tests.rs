use super::*;
use crate::runtime::RuntimeConfig;
use std::sync::mpsc;

/// Unique session root per call — keeps the modes cache from leaking
/// between tests and runs.
fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-ui-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

fn test_app() -> App {
    let cfg = RuntimeConfig {
        bin: "dsh-runtime".into(),
        cordis: "cordis".into(),
        workspace: "/tmp".into(),
        session_root: fresh_root(),
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, _rx) = mpsc::channel();
    App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx)
}

fn live_test_app() -> App {
    let cfg = RuntimeConfig {
        bin: "dsh-acp".into(),
        cordis: "cordis".into(),
        workspace: "/tmp".into(),
        session_root: fresh_root(),
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, _rx) = mpsc::channel();
    App::new(Theme::dark(), cfg, "pending".into(), false, true, tx)
}

#[test]
fn composer_is_a_rounded_box_surface() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    let theme = app.theme;
    // 80x20, no composer-dock contribution: the rounded box spans rows
    // 15..19 — top border 15 (tip + · workspace), well 16..18, bottom
    // border 19 carrying the meta row (`╰· Standard … ╯`).
    assert_eq!(buf[(0, 15)].symbol(), "╭", "top-left corner");
    assert_eq!(buf[(79, 15)].symbol(), "╮", "top-right corner");
    assert_eq!(buf[(0, 19)].symbol(), "╰", "bottom-left corner");
    assert_eq!(buf[(79, 19)].symbol(), "╯", "bottom-right corner");
    assert_eq!(buf[(4, 15)].bg, theme.panel, "border row on the card");
    assert_eq!(buf[(40, 16)].bg, theme.panel, "input well on panel surface");
    assert_eq!(buf[(40, 17)].bg, theme.panel, "input well on panel surface");
    assert_eq!(buf[(4, 18)].bg, theme.panel, "well fills the inner rows");
    assert_eq!(
        buf[(1, 19)].symbol(),
        "·",
        "meta row rides the bottom border with small dots"
    );
    assert_eq!(buf[(4, 10)].bg, theme.bg, "chat keeps the base background");
}

#[test]
fn composer_cap_persistently_shows_the_workspace() {
    let mut app = test_app();
    app.show_banner = false;
    app.cfg.workspace = "/work/acme/projects/deepseek-harness-tui-plan-view".into();

    let frame = dump_frame(&mut app, 120, 20);
    let cap = frame
        .lines()
        .find(|line| line.contains("Tip"))
        .expect("composer cap");

    assert!(
        cap.contains("· /work/acme/projects/deepseek-harness-tui-plan-view"),
        "{cap}"
    );
}

#[test]
fn composer_cap_preserves_the_workspace_tail_on_narrow_terminals() {
    let mut app = test_app();
    app.show_banner = false;
    app.cfg.workspace = "/work/acme/very-long-directory-name/deepseek-harness".into();

    let frame = dump_frame(&mut app, 60, 20);
    let cap = frame
        .lines()
        .find(|line| line.contains("Tip"))
        .expect("composer cap");

    assert!(cap.contains("· …/deepseek-harness"), "{cap}");
}

#[test]
fn composer_cap_shows_git_branch_after_the_project_path() {
    let mut app = test_app();
    app.show_banner = false;
    app.cfg.workspace = "/work/acme/martty".into();
    app.git_branch = Some("ui-tweak".into());

    let frame = dump_frame(&mut app, 120, 20);
    let cap = frame
        .lines()
        .find(|line| line.contains("Tip"))
        .expect("composer cap");
    assert!(cap.contains("· /work/acme/martty:ui-tweak"), "{cap}");

    // No git (or not a repository): path only, no branch tag.
    app.git_branch = None;
    let frame = dump_frame(&mut app, 120, 20);
    let cap = frame
        .lines()
        .find(|line| line.contains("Tip"))
        .expect("composer cap");
    assert!(cap.contains("· /work/acme/martty"), "{cap}");
    assert!(!cap.contains("martty:"), "{cap}");
}

#[test]
fn composer_cap_drops_git_branch_on_narrow_terminals() {
    let mut app = test_app();
    app.show_banner = false;
    app.cfg.workspace = "/work/acme/martty".into();
    app.git_branch = Some("a-long-branch-name".into());

    let frame = dump_frame(&mut app, 60, 20);
    let cap = frame
        .lines()
        .find(|line| line.contains("Tip"))
        .expect("composer cap");
    assert!(
        !cap.contains(":a-long-branch-name"),
        "branch must yield when the terminal is narrow: {cap}"
    );
    assert!(cap.contains("· /work/acme/martty"), "{cap}");
}

#[test]
fn active_image_background_clears_only_the_base_canvas() {
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    app.pet_pixels = true;
    let mut palette: serde_json::Value =
        serde_json::from_str(include_str!("../../docs/fixtures/demo-skin.v0.json")).unwrap();
    palette["background"] = serde_json::json!({
        "source": { "kind": "file", "path": "/opt/liang/stage-00.png" },
        "fit": "cover",
        "opacity": 0.42
    });
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "palette": palette,
                "activate": true
            }),
        },
        &ctl,
    );

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer();

    assert_eq!(
        buf[(4, 10)].bg,
        Color::Reset,
        "chat reveals the image layer"
    );
    assert_eq!(
        buf[(0, 15)].symbol(),
        "╭",
        "the composer box sits on the image layer"
    );
    assert_eq!(
        buf[(4, 15)].bg,
        app.theme.panel,
        "the box card is panel, not the image"
    );
    assert_eq!(
        buf[(40, 17)].bg,
        app.theme.panel,
        "composer remains readable"
    );
}

#[test]
fn model_chip_prefers_fresh_pick_until_a_turn_realizes_it() {
    let flat = |spans: Vec<Span>| -> String {
        spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    };
    let mut app = test_app();
    // A previous turn streamed on pro; the user just picked flash.
    app.transcript.last_model = Some("deepseek-v4-pro".into());
    app.selected_model = Some("deepseek-v4-flash".into());
    let s = flat(status_right(&app));
    assert!(s.contains("deepseek-v4-flash"), "{s}");
    assert!(!s.contains("deepseek-v4-pro"), "{s}");
    // Without a pick, the streamed model rules.
    app.selected_model = None;
    let s = flat(status_right(&app));
    assert!(s.contains("deepseek-v4-pro"), "{s}");
}

#[test]
fn live_meta_row_hides_session_options_until_session_bound() {
    let flat_line = |line: Line| -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    let flat_spans = |spans: Vec<Span>| -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    let mut app = live_test_app();
    app.modes.agent_preset = Some("minimal".into());
    app.modes.permission = Some("danger-full-access".into());
    app.modes.effort = Some("high".into());

    assert_eq!(flat_line(status_title(&app)), "");
    let pending = flat_spans(status_right(&app));
    assert!(pending.contains("^k keys"), "{pending}");
    assert!(!pending.contains("deepseek-chat"), "{pending}");
    assert!(!pending.contains("high"), "{pending}");

    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Ctl(crate::bus::CtlEvent::SessionBound {
            session_id: "acp-session".into(),
            notice: None,
        }),
        &ctl,
    );

    let bound_left = flat_line(status_title(&app));
    let bound_right = flat_spans(status_right(&app));
    assert!(bound_left.contains("minimal"), "{bound_left}");
    assert!(bound_left.contains("Full access"), "{bound_left}");
    assert!(bound_right.contains("deepseek-chat"), "{bound_right}");
    assert!(bound_right.contains("high"), "{bound_right}");
}

#[test]
fn status_shortcut_hints_follow_their_values_and_use_key_styling() {
    let flat = |line: &Line| -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    let mut app = test_app();

    app.locale = crate::locale::Locale::Zh;
    let zh_line = status_title(&app);
    let zh = flat(&zh_line);
    assert!(
        zh.contains("Standard mode ctrl+shift+a · 工作区可写 shift+tab"),
        "{zh}"
    );
    assert!(!zh.contains("permission"), "{zh}");
    assert!(!zh.contains("权限"), "{zh}");

    app.locale = crate::locale::Locale::En;
    let en_line = status_title(&app);
    let en = flat(&en_line);
    assert!(
        en.contains("Standard mode ctrl+shift+a · Workspace Write shift+tab"),
        "{en}"
    );
    assert!(!en.contains("permission"), "{en}");
    assert!(!en.contains("access"), "{en}");

    let key_spans = en_line
        .spans
        .iter()
        .filter(|span| span.content.contains('+'))
        .collect::<Vec<_>>();
    assert_eq!(key_spans.len(), 2, "{en}");
    assert!(key_spans.iter().all(|span| {
        span.style.fg == Some(app.theme.brand_soft)
            && span.style.add_modifier.contains(Modifier::BOLD)
    }));
    let value_spans = en_line
        .spans
        .iter()
        .filter(|span| {
            span.content.contains("Standard mode") || span.content.contains("Workspace Write")
        })
        .collect::<Vec<_>>();
    assert_eq!(value_spans.len(), 2, "{en}");
    assert!(value_spans
        .iter()
        .all(|span| span.style.fg == Some(app.theme.fg_tertiary)));
    assert_ne!(key_spans[0].style.fg, value_spans[0].style.fg);
}

#[test]
fn cordis_protocol_id_is_rendered_as_creator() {
    let mut app = test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Ctl(crate::bus::CtlEvent::Catalog {
            models: Vec::new(),
            presets: vec![crate::bus::CatalogPreset {
                id: "cordis".into(),
                name: "Creator from ACP".into(),
                description: String::new(),
                broken: false,
            }],
        }),
        &ctl,
    );
    app.modes.agent_preset = Some("cordis".into());

    let rendered = status_title(&app)
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("Creator from ACP"), "{rendered}");
    assert!(!rendered.contains("cordis"), "{rendered}");
}

#[test]
fn meta_row_hints_follow_the_state_machine() {
    let flat = |spans: Vec<Span>| -> String {
        spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    };
    let mut app = test_app();
    // idle · empty → discovery hint
    assert!(flat(context_hints(&app)).contains("^k keys"));
    // idle · draft → enter sends
    app.input.set("hello".into());
    let s = flat(context_hints(&app));
    assert!(s.contains("⏎ send"), "{s}");
    assert!(!s.contains("keys"), "{s}");
    // idle · slash / shell drafts relabel enter
    app.input.set("/mo".into());
    assert!(flat(context_hints(&app)).contains("⏎ command"));
    app.input.set("!ls".into());
    assert!(flat(context_hints(&app)).contains("⏎ shell"));
    // running · empty → interrupt only
    app.state = RunState::Running;
    app.input.clear();
    let s = flat(context_hints(&app));
    assert!(s.contains("esc interrupt"), "{s}");
    assert!(!s.contains("⏎"), "{s}");
    // running · draft → queue + send-now + interrupt
    app.input.set("follow-up".into());
    let s = flat(context_hints(&app));
    assert!(
        s.contains("⏎ queue") && s.contains("^x steer") && s.contains("esc interrupt"),
        "{s}"
    );
}

#[test]
fn usage_footer_shows_turn_stats_and_cache() {
    let mut app = test_app();
    app.show_banner = false;
    app.slot_snapshots.insert(
            "conversation.composer.dock".into(),
            serde_json::from_value(serde_json::json!({
                "protocol": 0,
                "slot": "conversation.composer.dock",
                "rev": 1,
                "nodes": [
                    { "id": "stats:tokens", "kind": "generic", "title": "Input 1.8K tok · Output 412 tok", "body": "" },
                    { "id": "stats:counts", "kind": "generic", "title": "1 turn · 67 steps", "body": "" },
                    { "id": "stats:cache", "kind": "generic", "title": "Cache hit 65%", "body": "" }
                ]
            })).expect("slot snapshot"),
        );
    let frame = dump_frame(&mut app, 100, 20);
    let footer = frame.lines().last().expect("footer row");
    assert!(footer.contains("1 turn"), "{footer}");
    assert!(footer.contains("67 steps"), "{footer}");
    assert!(footer.contains("Cache hit 65%"), "{footer}");
    assert!(footer.contains("Input 1.8K tok"), "{footer}");
    assert!(footer.contains("Output 412 tok"), "{footer}");
}

#[test]
fn empty_stats_slot_still_reserves_the_composer_footer_row() {
    let mut app = test_app();
    app.show_banner = false;
    let _ = dump_frame(&mut app, 100, 24);
    let without_footer = app.chat_view.area.height;
    app.slot_snapshots.insert(
        "conversation.composer.dock".into(),
        serde_json::from_value(serde_json::json!({
            "protocol": 0,
            "slot": "conversation.composer.dock",
            "rev": 1,
            "nodes": []
        }))
        .expect("empty stats slot snapshot"),
    );

    let _ = dump_frame(&mut app, 100, 24);

    assert_eq!(
        app.chat_view.area.height + 1,
        without_footer,
        "the future status row stays reserved before stats arrive"
    );
}

#[test]
fn usage_footer_drops_sections_on_narrow_screens() {
    let mut app = test_app();
    app.show_banner = false;
    app.slot_snapshots.insert(
            "conversation.composer.dock".into(),
            serde_json::from_value(serde_json::json!({
                "protocol": 0,
                "slot": "conversation.composer.dock",
                "rev": 1,
                "nodes": [
                    { "id": "stats:tokens", "kind": "generic", "title": "Input 1.8K tok · Output 412 tok", "body": "" },
                    { "id": "stats:counts", "kind": "generic", "title": "1 turn · 67 steps", "body": "" },
                    { "id": "stats:cache", "kind": "generic", "title": "Cache hit 65%", "body": "" },
                    { "id": "stats:time", "kind": "generic", "title": "LLM 15m9s · Tool call 0s", "body": "" },
                    { "id": "stats:speed", "kind": "generic", "title": "TTFT avg 1.5s · 0.5 tok/s", "body": "" }
                ]
            })).expect("slot snapshot"),
        );

    // Narrow: low-priority timing sections are dropped, tokens survive.
    let footer = dump_frame(&mut app, 40, 20);
    let footer = footer.lines().last().expect("footer row");
    assert!(footer.contains("Input"), "tokens survive: {footer}");
    assert!(footer.contains("Output"), "tokens survive: {footer}");
    assert!(
        !footer.contains("TTFT"),
        "TTFT dropped when narrow: {footer}"
    );
    assert!(!footer.contains("LLM"), "LLM dropped when narrow: {footer}");
}

#[test]
fn agent_rail_lists_created_subagents_and_their_status() {
    let mut app = test_app();
    app.show_banner = false;
    app.subagents.push(crate::app::SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: true,
        transcript: crate::transcript::Transcript::new("child-1".into()),
    });
    app.subagents.push(crate::app::SubagentView {
        id: "child-2".into(),
        parent: "dsh-test".into(),
        label: "subagent 2".into(),
        running: false,
        transcript: crate::transcript::Transcript::new("child-2".into()),
    });

    let frame = dump_frame(&mut app, 100, 24);
    assert!(frame.contains("agents"), "{frame}");
    assert!(frame.contains("● subagent 1"), "{frame}");
    assert!(frame.contains("✓ subagent 2"), "{frame}");
}

#[test]
fn child_view_replaces_the_composer_with_read_only_navigation() {
    let mut app = test_app();
    app.show_banner = false;
    let mut transcript = crate::transcript::Transcript::new("child-1".into());
    transcript.apply(crate::events::UiEvent::TextDelta {
        session: "child-1".into(),
        text: "child-only output".into(),
    });
    app.subagents.push(crate::app::SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: true,
        transcript,
    });
    app.active_subagent = Some("child-1".into());

    let frame = dump_frame(&mut app, 100, 24);
    assert!(frame.contains("child-only output"), "{frame}");
    assert!(frame.contains("read-only"), "{frame}");
    assert!(frame.contains("esc back"), "{frame}");
    assert!(
        !frame.contains("describe what you want to build"),
        "{frame}"
    );
    assert!(!frame.contains("send a prompt"), "{frame}");
}

#[test]
fn active_running_child_does_not_show_the_main_idle_state() {
    let mut app = test_app();
    app.show_banner = false;
    app.subagents.push(crate::app::SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: true,
        transcript: crate::transcript::Transcript::new("child-1".into()),
    });
    app.active_subagent = Some("child-1".into());

    let frame = dump_frame(&mut app, 100, 24);
    assert!(frame.contains("working"), "{frame}");
    assert!(!frame.contains("● idle"), "{frame}");
}

#[test]
fn long_input_wraps_in_the_well() {
    let mut app = test_app();
    app.show_banner = false;
    app.input.buf = "a".repeat(50);
    app.input.cursor = 50;
    // 40x12 → composer height 4 → a 2-row input well, 36 text cols wide.
    let frame = dump_frame(&mut app, 40, 12);
    let wrapped = frame.lines().filter(|l| l.contains("aaaa")).count();
    assert!(wrapped >= 2, "input should wrap across well rows:\n{frame}");
}

#[test]
fn multiline_input_breaks_on_newlines() {
    let mut app = test_app();
    app.show_banner = false;
    app.input.buf = "hello\nworld".into();
    app.input.cursor = "hello\nworld".chars().count();
    let frame = dump_frame(&mut app, 40, 12);
    let hello = frame
        .lines()
        .position(|l| l.contains("hello"))
        .expect("first line");
    let world = frame
        .lines()
        .position(|l| l.contains("world"))
        .expect("second line");
    assert!(
        world > hello,
        "hard newline pushes the second line down:\n{frame}"
    );
}

#[test]
fn composer_grows_to_show_a_multiline_draft_until_its_cap() {
    let mut app = test_app();
    app.show_banner = false;
    let _ = dump_frame(&mut app, 100, 30);
    let empty_chat_height = app.chat_view.area.height;

    app.input.set("one\ntwo\nthree\nfour\nfive\nsix".into());
    let frame = dump_frame(&mut app, 100, 30);

    assert!(
        app.chat_view.area.height < empty_chat_height,
        "composer should take rows from chat as the draft grows:\n{frame}"
    );
    assert!(frame.contains("one") && frame.contains("six"), "{frame}");
}

#[test]
fn banner_shows_until_first_prompt() {
    let mut app = test_app();
    assert!(app.show_banner, "banner defaults to on");
    assert!(
        dump_frame(&mut app, 84, 40).contains("██████"),
        "wordmark visible on launch"
    );
    app.show_banner = false;
    assert!(
        !dump_frame(&mut app, 84, 40).contains("██████"),
        "whale dives once the banner is dismissed"
    );
}

#[test]
fn empty_welcome_screen_has_balanced_outer_vertical_padding() {
    let mut app = test_app();
    let _ = dump_frame(&mut app, 120, 50);
    let first_content = app
        .chat_view
        .lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .expect("welcome content");
    let last_content = app
        .chat_view
        .lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .expect("welcome content");
    let top = first_content;
    let bottom = app.chat_view.area.height as usize - last_content - 1;

    assert!(
        top.abs_diff(bottom) <= 1,
        "welcome block must be vertically centered: top={top}, bottom={bottom}"
    );
}

#[test]
fn welcome_and_conversation_are_two_states_without_a_mixed_third_state() {
    let mut app = test_app();
    app.transcript.push_notice(
        crate::transcript::NoticeLevel::Info,
        "local UI notice".into(),
    );

    let frame = dump_frame(&mut app, 140, 60);

    assert!(
        frame.contains("https://martty.sh"),
        "Welcome is visible:\n{frame}"
    );
    assert!(
        !frame.contains("local UI notice"),
        "transcript must not share the Welcome state:\n{frame}"
    );
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
        "Welcome remains centered: top={first}, bottom={bottom}"
    );
}

#[test]
fn welcome_hero_slot_replaces_only_the_martty_lockup() {
    let mut app = test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
            "protocol": 0,
            "slot": "welcome.hero",
            "rev": 1,
            "nodes": [
                {
                    "id": "deepseek-logo:logo",
                    "kind": "logo",
                    "name": "deepseek"
                },
                {
                    "id": "deepseek-logo:hint",
                    "kind": "text",
                    "text": "Into the Unknown",
                    "tone": "fg_tertiary"
                }
            ]
            }),
        },
        &ctl,
    );

    assert!(
        app.show_banner,
        "preset changes keep an existing Welcome visible"
    );
    let frame = dump_frame(&mut app, 140, 60);

    assert!(frame.contains("▄▄▄███▀"), "XL whale:\n{frame}");
    assert!(frame.contains('░'), "hollow HARNESS:\n{frame}");
    assert!(frame.contains("Into the Unknown"), "tagline:\n{frame}");
    assert!(
        !frame.contains("https://martty.sh"),
        "Martty hero replaced:\n{frame}"
    );
    assert!(frame.contains("dsh-tui"), "session facts remain:\n{frame}");
    assert!(
        frame.contains("/help commands"),
        "help row remains:\n{frame}"
    );
}

#[test]
fn welcome_slot_updates_do_not_hide_an_existing_conversation() {
    let mut app = test_app();
    app.show_banner = false;
    app.transcript.push_notice(
        crate::transcript::NoticeLevel::Info,
        "conversation remains visible".into(),
    );
    let (ctl, _commands) = crate::controller::tests::test_controller();

    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "slot": "welcome.hero",
                "rev": 1,
                "nodes": [
                    {
                        "id": "deepseek-logo:logo",
                        "kind": "logo",
                        "name": "deepseek"
                    },
                    {
                        "id": "deepseek-logo:hint",
                        "kind": "text",
                        "text": "Into the Unknown",
                        "tone": "fg_tertiary"
                    }
                ]
            }),
        },
        &ctl,
    );

    assert!(
        !app.show_banner,
        "UI preset changes must not re-enter Welcome"
    );
    let frame = dump_frame(&mut app, 140, 60);
    assert!(
        frame.contains("conversation remains visible"),
        "conversation remains rendered after the preset slot changes:\n{frame}"
    );
}

#[test]
fn welcome_info_slot_replaces_only_the_native_information_region() {
    let mut app = test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "slot": "welcome.info",
                "rev": 1,
                "nodes": [{
                    "id": "custom-info:copy",
                    "kind": "text",
                    "text": "Custom welcome information",
                    "tone": "fg_secondary"
                }]
            }),
        },
        &ctl,
    );

    assert!(
        app.show_banner,
        "preset changes keep an existing Welcome visible"
    );
    let frame = dump_frame(&mut app, 140, 60);
    assert!(
        frame.contains("https://martty.sh"),
        "Hero remains:\n{frame}"
    );
    assert!(
        frame.contains("Custom welcome information"),
        "custom info:\n{frame}"
    );
    assert!(
        !frame.contains("dsh-tui"),
        "native version row replaced:\n{frame}"
    );
    assert!(
        !frame.contains("/help commands"),
        "native help row replaced:\n{frame}"
    );
}

#[test]
fn deepseek_hero_preserves_the_original_whale_geometry() {
    let mut app = test_app();
    app.slot_snapshots.insert(
        "welcome.hero".into(),
        serde_json::from_value(serde_json::json!({
            "protocol": 0,
            "slot": "welcome.hero",
            "rev": 1,
            "nodes": [
                { "id": "deepseek-logo:logo", "kind": "logo", "name": "deepseek" },
                {
                    "id": "deepseek-logo:hint",
                    "kind": "text",
                    "text": "Into the Unknown",
                    "tone": "fg_tertiary"
                }
            ]
        }))
        .expect("DeepSeek hero snapshot"),
    );
    let plain = banner_lines(&app, 140)
        .into_iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let first = plain
        .iter()
        .find(|line| line.contains("▄▄▄███▀"))
        .expect("first XL whale row");
    let second = plain
        .iter()
        .find(|line| line.contains("▄▄████████"))
        .expect("second XL whale row");
    let first_x = first.find('▄').expect("first whale pixel");
    let second_x = second.find('▄').expect("second whale pixel");

    assert_eq!(first_x - second_x, 13, "all whale rows share one outer pad");
    assert!(
        plain
            .iter()
            .any(|line| line.contains('█') && line.contains('░')),
        "wide wordmark keeps solid DEEPSEEK beside hollow HARNESS"
    );
}

#[test]
fn composed_deepseek_preset_keeps_balanced_outer_padding() {
    let mut app = test_app();
    for (slot, nodes) in [
        (
            "welcome.hero",
            serde_json::json!([
                { "id": "deepseek:logo", "kind": "logo", "name": "deepseek" },
                { "id": "deepseek:hint", "kind": "text", "text": "Into the Unknown" }
            ]),
        ),
        (
            "welcome.info",
            serde_json::json!([{ "id": "deepseek:info", "kind": "welcomeinfo" }]),
        ),
    ] {
        app.slot_snapshots.insert(
            slot.into(),
            serde_json::from_value(serde_json::json!({
                "protocol": 0,
                "slot": slot,
                "rev": 1,
                "nodes": nodes
            }))
            .expect("preset slot snapshot"),
        );
    }

    let _ = dump_frame(&mut app, 140, 60);
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
        "composed welcome must be centered: top={first}, bottom={bottom}"
    );
}

#[test]
fn pet_rect_geometry() {
    let mut app = test_app();
    let area = Rect::new(0, 0, 100, 34);
    // Off by default (issue #37): `/liang on` summons him.
    assert_eq!(pet_rect(area, &app), None, "liang is off by default");
    app.pet_visible = true;
    // 34 rows → 5-row composer → 4-row pet (192:208 sprite → 7 cols),
    // inset one row/column so the rounded box border stays intact.
    assert_eq!(pet_rect(area, &app), Some(Rect::new(92, 29, 7, 4)));
    app.pet_visible = false;
    assert_eq!(pet_rect(area, &app), None, "/liang off hides him");
    app.pet_visible = true;
    let narrow = Rect::new(0, 0, 50, 34);
    assert_eq!(
        pet_rect(narrow, &app),
        None,
        "narrow terminals keep their columns"
    );
}

#[test]
fn pet_rides_above_the_stats_dock() {
    let mut app = test_app();
    app.pet_visible = true;
    app.slot_snapshots.insert(
        "conversation.composer.dock".into(),
        serde_json::from_value(serde_json::json!({
            "protocol": 0,
            "slot": "conversation.composer.dock",
            "rev": 1,
            "nodes": [
                { "id": "stats:cache", "kind": "generic", "title": "Cache hit 65%", "body": "" }
            ]
        }))
        .expect("slot snapshot"),
    );
    let main = Rect::new(0, 0, 100, 26);
    let dock_h = composer_dock_height(&app, main.height, false);
    assert_eq!(dock_h, 1, "dock fits on a 26-row terminal");
    // Anchored to the box bottom (dock row excluded): one row higher
    // than without the dock.
    let with_dock = Rect::new(
        main.x,
        main.y,
        main.width,
        main.height.saturating_sub(dock_h),
    );
    assert_eq!(pet_rect(with_dock, &app), Some(Rect::new(92, 20, 7, 4)));
    app.slot_snapshots.remove("conversation.composer.dock");
    let without_dock = Rect::new(main.x, main.y, main.width, main.height);
    assert_eq!(pet_rect(without_dock, &app), Some(Rect::new(92, 21, 7, 4)));
}

#[test]
fn pet_stays_inside_the_box_when_the_stats_dock_shows() {
    let mut app = test_app();
    app.pet_visible = true;
    app.show_banner = false;
    app.slot_snapshots.insert(
        "conversation.composer.dock".into(),
        serde_json::from_value(serde_json::json!({
            "protocol": 0,
            "slot": "conversation.composer.dock",
            "rev": 1,
            "nodes": [
                { "id": "stats:cache", "kind": "generic", "title": "Cache hit 65%", "body": "" }
            ]
        }))
        .expect("slot snapshot"),
    );
    let frame = dump_frame(&mut app, 100, 26);
    let dock_line = frame.lines().last().expect("dock row");
    assert!(dock_line.contains("Cache hit 65%"), "{dock_line}");
    assert!(
        !dock_line.contains("▄███"),
        "pet must not overlap the dock: {dock_line}"
    );
    assert!(
        frame.contains("▄███▄█▄▄"),
        "pet still visible inside the box:\n{frame}"
    );
}

#[test]
fn pet_falls_back_to_half_blocks_and_toggles() {
    let mut app = test_app();
    app.pet_visible = true;
    app.show_banner = false;
    // pet_pixels=false (no kitty graphics): XS art at the right edge,
    // inside the box border.
    let frame = dump_frame(&mut app, 100, 34);
    assert!(
        frame.contains("▄███▄█▄▄"),
        "XS whale flush inside the box:\n{frame}"
    );

    // A pixel-protocol terminal draws nothing — the image goes on top.
    app.pet_pixels = true;
    let frame = dump_frame(&mut app, 100, 34);
    assert!(
        !frame.contains("▄███"),
        "cells stay clear for the PNG:\n{frame}"
    );

    // /pet off → gone entirely.
    app.pet_pixels = false;
    app.pet_visible = false;
    let frame = dump_frame(&mut app, 100, 34);
    assert!(!frame.contains("▄███"), "hidden by /pet:\n{frame}");
}

#[test]
fn mode_picker_renders_modes_and_marks_the_current_one() {
    use crate::app::{Picker, PickerItem, PickerKind, AGENT_MODES};
    let mut app = test_app();
    app.show_banner = false;
    app.modes.agent_preset = Some("minimal".into());
    app.picker = Some(Picker {
        kind: PickerKind::Mode,
        title: " agent mode · enter select · esc close ".into(),
        sel: 2,
        items: AGENT_MODES
            .iter()
            .map(|(id, name, desc)| PickerItem {
                id: id.to_string(),
                label: name.to_string(),
                meta: desc.to_string(),
                provider: None,
            })
            .collect(),
    });
    let frame = dump_frame(&mut app, 100, 30);
    for name in ["Standard mode", "Code mode", "Minimal mode", "Creator mode"] {
        assert!(frame.contains(name), "{name} listed in the picker\n{frame}");
    }
    let minimal_row = frame
        .lines()
        .find(|l| l.contains("Minimal mode"))
        .expect("minimal row");
    assert!(
        minimal_row.contains("Minimal mode ✓"),
        "current mode marked: {minimal_row}"
    );
    assert!(
        minimal_row.contains("▸"),
        "selection marker on the current row"
    );
    assert!(
        frame.contains("bash + str_replace_editor"),
        "descriptions visible\n{frame}"
    );
}

#[test]
fn session_picker_rows_align_label_and_meta_columns() {
    use crate::app::{Picker, PickerItem, PickerKind, PICKER_LABEL_COL};
    let mut app = test_app();
    app.show_banner = false;
    app.picker = Some(Picker {
        kind: PickerKind::Session,
        title: " resume session · 2 sessions · enter select · esc close ".into(),
        sel: 0,
        items: vec![
            PickerItem {
                id: "276b7574-b12c-488e-958b-f9673b67fba9".into(),
                label: "查看session历史命令的可行性".into(),
                meta: "276b7574 · 2h · 3 turns".into(),
                provider: None,
            },
            PickerItem {
                id: "dsh-alp".into(),
                label: "fix failing tests".into(),
                meta: "dsh-alp  · just now · 1 turn".into(),
                provider: None,
            },
        ],
    });
    let frame = dump_frame(&mut app, 100, 30);
    let ascii_row = frame
        .lines()
        .find(|l| l.contains("fix failing tests"))
        .expect("ascii row");
    // Wide chars dump as char + continuation cell, so locate the CJK
    // row by its meta instead of the raw label.
    let cjk_row = frame
        .lines()
        .find(|l| l.contains("276b7574 · 2h"))
        .unwrap_or_else(|| panic!("cjk row missing:\n{frame}"));
    // Dump cells contribute exactly one char each, so the column of a
    // marker is the char count before its byte offset (`find` alone
    // returns byte indices, which differ for multi-byte CJK labels).
    let meta_col = |row: &str| row.find('·').map(|i| row[..i].chars().count()).unwrap_or(0);
    assert_eq!(
        meta_col(ascii_row),
        meta_col(cjk_row),
        "meta column lines up:\n{ascii_row}\n{cjk_row}\ncols: {} vs {}",
        meta_col(ascii_row),
        meta_col(cjk_row),
    );
    assert!(
        ascii_row.chars().count() >= 2 + PICKER_LABEL_COL + 8,
        "label column padded to {PICKER_LABEL_COL}"
    );
}

#[test]
fn picker_window_follows_the_selection_and_shows_a_scrollbar() {
    use crate::app::{Picker, PickerItem, PickerKind};
    let mut app = test_app();
    app.show_banner = false;
    let items: Vec<PickerItem> = (0..40)
        .map(|i| PickerItem {
            id: format!("sess-{i:02}"),
            label: format!("session {i:02}"),
            meta: format!("meta {i:02}"),
            provider: None,
        })
        .collect();
    app.picker = Some(Picker {
        kind: PickerKind::Session,
        title: " resume session · 40 sessions · enter select · esc close ".into(),
        sel: 0,
        items,
    });

    // 24-row terminal: the popup caps at 22 rows and must scroll.
    let top = dump_frame(&mut app, 100, 24);
    assert!(top.contains("session 00"), "head row visible:\n{top}");
    assert!(!top.contains("session 39"), "tail not visible yet:\n{top}");
    // The rounded border stays intact: the scrollbar lives in its own
    // column inside the popup, between the rows and the right border.
    assert!(top.contains("╮"), "top-right corner intact:\n{top}");
    assert!(top.contains("║"), "scrollbar track shown:\n{top}");
    assert!(
        top.lines()
            .any(|l| l.contains("session 00") && l.trim_end().ends_with('│')),
        "right border column intact next to the scrollbar:\n{top}"
    );

    // Jump to the tail: the window follows, so the last row is
    // reachable instead of being clipped out of the paragraph.
    app.picker.as_mut().unwrap().sel = 39;
    let tail = dump_frame(&mut app, 100, 24);
    assert!(tail.contains("session 39"), "tail row visible:\n{tail}");
    assert!(!tail.contains("session 00"), "head scrolled away:\n{tail}");
    assert!(tail.contains("█"), "scrollbar thumb shown:\n{tail}");
    assert!(tail.contains("╰"), "bottom corners intact:\n{tail}");
    assert_eq!(app.picker_page_rows, 20, "page size = visible rows");
}

#[test]
fn picker_selection_highlights_the_whole_row() {
    use crate::app::{Picker, PickerItem, PickerKind};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.picker = Some(Picker {
        kind: PickerKind::Session,
        title: " resume session · 2 sessions ".into(),
        sel: 0,
        items: vec![
            PickerItem {
                id: "a".into(),
                label: "first".into(),
                meta: "meta a".into(),
                provider: None,
            },
            PickerItem {
                id: "b".into(),
                label: "second".into(),
                meta: "meta b".into(),
                provider: None,
            },
        ],
    });
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer();
    let chip = app.theme.chip_bg;
    // Cells hold single symbols, so search rows by joining them first.
    let row_of = |needle: &str| {
        (0..20u16)
            .find(|&r| {
                (0..80u16)
                    .map(|c| buf[(c, r)].symbol())
                    .collect::<String>()
                    .contains(needle)
            })
            .unwrap_or_else(|| panic!("row with {needle:?} not found"))
    };
    // The selected row: marker, label, meta and the tail padding all
    // carry the chip background — the highlight spans the whole line.
    let sel_row = row_of("first");
    let marker_col = (0..80u16)
        .find(|&c| buf[(c, sel_row)].symbol() == "▸")
        .expect("selection marker");
    let right_border = (marker_col..80u16)
        .find(|&c| buf[(c, sel_row)].symbol() == "│")
        .expect("popup right border");
    for c in marker_col..right_border {
        assert_eq!(
            buf[(c, sel_row)].style().bg,
            Some(chip),
            "selected row cell {c} carries the highlight bg"
        );
    }
    // The unselected row keeps the panel background.
    let other_row = row_of("second");
    for c in 0..80u16 {
        let cell = &buf[(c, other_row)];
        if !cell.symbol().trim().is_empty() {
            assert_ne!(
                cell.style().bg,
                Some(chip),
                "unselected row cell {c} must not carry the highlight bg"
            );
        }
    }
}

#[test]
fn model_picker_marks_only_the_current_provider_model_pair() {
    use crate::app::{Picker, PickerItem, PickerKind};
    let mut app = test_app();
    app.show_banner = false;
    app.cfg.provider = "coding-plan-b".into();
    app.cfg.model = "deepseek-v4".into();
    app.picker = Some(Picker {
        kind: PickerKind::Model,
        title: " model ".into(),
        sel: 1,
        items: vec![
            PickerItem {
                id: "deepseek-v4".into(),
                label: "deepseek-v4".into(),
                meta: "coding-plan-a · DeepSeek V4".into(),
                provider: Some("coding-plan-a".into()),
            },
            PickerItem {
                id: "deepseek-v4".into(),
                label: "deepseek-v4".into(),
                meta: "coding-plan-b · DeepSeek V4".into(),
                provider: Some("coding-plan-b".into()),
            },
        ],
    });

    let frame = dump_frame(&mut app, 100, 24);
    let marked: Vec<&str> = frame.lines().filter(|line| line.contains("✓")).collect();
    assert_eq!(
        marked.len(),
        1,
        "only one provider/model row is current:\n{frame}"
    );
    assert!(
        marked[0].contains("coding-plan-b"),
        "current marker follows the provider: {}",
        marked[0]
    );
}

#[test]
fn scroll_up_survives_draw_and_shows_indicator() {
    let mut app = test_app();
    app.show_banner = false;
    for i in 0..40 {
        app.transcript.push_user(format!("line {i}"), false);
    }
    app.scroll_by(20);
    let frame = dump_frame(&mut app, 100, 14);
    assert!(app.scroll_up > 0, "scroll_up clamped to zero");
    assert!(frame.contains("▲"), "scroll indicator missing:\n{frame}");
}

#[test]
fn draw_fills_the_chat_view_snapshot() {
    let mut app = test_app();
    let _ = dump_frame(&mut app, 84, 40);
    assert!(app.chat_view.area.width > 0, "chat pane rect captured");
    assert!(
        !app.chat_view.lines.is_empty(),
        "plain-text layout captured for mouse selection"
    );
    assert!(
        app.chat_view
            .lines
            .iter()
            .any(|l| l.contains("https://martty.sh")),
        "snapshot mirrors rendered content"
    );
}

#[test]
fn selection_overlay_reverses_cells() {
    use crate::app::{SelPoint, Selection};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.transcript
        .push_user("hello selection world".into(), false);
    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    // First draw fills chat_view; then select and draw again.
    terminal.draw(|f| draw(f, &mut app)).expect("warmup");
    let line = app
        .chat_view
        .lines
        .iter()
        .position(|l| l.contains("hello"))
        .expect("user line in layout");
    app.sel = Some(Selection {
        anchor: SelPoint { line, col: 0 },
        head: SelPoint { line, col: 8 },
    });
    terminal.draw(|f| draw(f, &mut app)).expect("redraw");
    let buf = terminal.backend().buffer();
    let row = app.chat_view.area.y + (line - app.chat_view.top) as u16;
    let x = app.chat_view.area.x;
    let reversed = (0..9u16)
        .filter(|c| {
            buf[(x + c, row)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        })
        .count();
    assert_eq!(reversed, 9, "anchor..=head cells are highlighted");
    assert_eq!(
        app.selection_text(app.sel.unwrap()),
        crate::app::slice_by_cells(&app.chat_view.lines[line], 0, 9).trim_end(),
        "copied text matches the highlighted cells"
    );
}

#[test]
fn permission_ask_overlay_lists_kind_name_and_title() {
    use crate::app::PermissionAskOverlay;
    use crate::bus::PermissionAskOption;
    let mut app = test_app();
    app.show_banner = false;
    app.permission_ask = Some(PermissionAskOverlay {
        title: "bash".into(),
        sel: 1,
        options: vec![
            PermissionAskOption {
                option_id: "reject".into(),
                kind: "reject_once".into(),
                name: "Reject".into(),
            },
            PermissionAskOption {
                option_id: "allow".into(),
                kind: "allow_once".into(),
                name: "Allow once".into(),
            },
        ],
        reply: None,
    });
    let frame = dump_frame(&mut app, 100, 30);
    assert!(frame.contains("bash"), "tool title in overlay\n{frame}");
    assert!(frame.contains("Reject"), "option name\n{frame}");
    assert!(frame.contains("Allow once"), "option name\n{frame}");
    assert!(frame.contains("reject_once"), "option kind\n{frame}");
    assert!(frame.contains("allow_once"), "option kind\n{frame}");
    let allow_row = frame
        .lines()
        .find(|l| l.contains("Allow once"))
        .expect("allow row");
    assert!(
        allow_row.contains("▸"),
        "selection on allow_once: {allow_row}"
    );
}

#[test]
fn plugin_slider_overlay_renders_track_marks_and_keyboard_affordances() {
    use crate::app::{SliderMark, SliderOverlay};

    let mut app = test_app();
    app.show_banner = false;
    app.slider_overlay = Some(SliderOverlay {
        id: "liang-effort".into(),
        title: "Liang reasoning effort".into(),
        min: 0.0,
        max: 30.0,
        step: 1.0,
        marks: vec![
            SliderMark {
                value: 0.0,
                id: Some("off".into()),
                label: "Off".into(),
            },
            SliderMark {
                value: 15.0,
                id: Some("high".into()),
                label: "High".into(),
            },
            SliderMark {
                value: 30.0,
                id: Some("max".into()),
                label: "Max".into(),
            },
        ],
        snap_to_marks: true,
        value: 16.0,
    });

    let frame = dump_frame(&mut app, 100, 30);

    assert!(frame.contains("Liang reasoning effort"), "title\n{frame}");
    for label in ["Off", "High", "Max"] {
        assert!(frame.contains(label), "mark {label}\n{frame}");
    }
    assert!(frame.contains("16 / 30"), "current numeric value\n{frame}");
    assert!(
        frame.contains("←/→ preview")
            && frame.contains("enter apply")
            && frame.contains("esc cancel"),
        "keyboard affordances\n{frame}"
    );
}

#[test]
fn plugin_view_overlay_renders_markdown_and_uses_wide_screens() {
    use crate::app::ViewOverlay;
    let mut app = test_app();
    app.show_banner = false;
    app.view_overlay = Some(ViewOverlay {
            id: "plan-view".into(),
            title: "Plan".into(),
            nodes: vec![crate::slots::TuiNode::Markdown {
                id: "content".into(),
                text: "## Plan · 1/2\n\n- [x] Inspect · priority · high\n- [ ] **Implement** · priority · medium"
                    .into(),
                streaming: false,
            }],
            scroll: 0,
            notify_plugin: true,
        });
    let frame = dump_frame(&mut app, 160, 30);
    // The plan review renders through the full markdown pipeline: the
    // heading loses its `#` markers, checkboxes become status glyphs,
    // and the task list survives.
    assert!(frame.contains("Plan · 1/2"), "heading:\n{frame}");
    assert!(frame.contains("✓ Inspect"), "checked item:\n{frame}");
    assert!(frame.contains("○ Implement"), "pending item:\n{frame}");
    assert!(frame.contains("Implement"), "pending item:\n{frame}");
    // Wide terminals: the review pane exceeds the old 84-column cap.
    let title_line = frame
        .lines()
        .find(|l| l.contains("esc close"))
        .expect("overlay title row");
    let left = title_line.find('╭').expect("left corner");
    let right = title_line.rfind('╮').expect("right corner");
    assert!(
        right - left > 84,
        "wide overlay expected, got {} cols: {title_line}",
        right - left
    );
}

#[test]
fn elicitation_overlay_renders_a_real_question_form() {
    use crate::app::ElicitationAskOverlay;
    use crate::elicitation::{
        ElicitationField, ElicitationFieldKind, ElicitationForm, ElicitationFormState,
        ElicitationOption,
    };

    let mut app = test_app();
    app.show_banner = false;
    app.elicitation_ask = Some(ElicitationAskOverlay {
        form: ElicitationFormState::new(ElicitationForm {
            message: "The agent needs your input.".into(),
            fields: vec![ElicitationField {
                name: "question_0".into(),
                custom_name: Some("question_0_custom".into()),
                title: "Target".into(),
                description: Some("Where should this run?".into()),
                required: true,
                kind: ElicitationFieldKind::Single {
                    options: vec![
                        ElicitationOption {
                            value: "local".into(),
                            label: "Local".into(),
                            description: Some("Run here.".into()),
                            custom: false,
                        },
                        ElicitationOption {
                            value: "other".into(),
                            label: "Other".into(),
                            description: None,
                            custom: true,
                        },
                    ],
                    default: None,
                },
            }],
        }),
        scroll: 0,
        reply: None,
    });

    let frame = dump_frame(&mut app, 100, 30);

    assert!(frame.contains("Target"), "field header\n{frame}");
    assert!(
        frame.contains("Where should this run?"),
        "question\n{frame}"
    );
    assert!(frame.contains("Local"), "choice\n{frame}");
    assert!(frame.contains("Other"), "custom choice\n{frame}");
    assert!(frame.contains("enter"), "keyboard affordance\n{frame}");
}

fn box_line(line: &str) -> &str {
    let trimmed = line.trim_start_matches(' ');
    trimmed
        .strip_prefix('│')
        .and_then(|rest| rest.strip_suffix('│'))
        .unwrap_or(trimmed)
}

#[test]
fn select_overlay_ellipsizes_long_rows_and_previews_the_selection() {
    use crate::app::{SelectOption, SelectOverlay};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, commands) = crate::controller::tests::test_controller();
    let long_label = "非常长的选项标签，用于验证省略号截断与选中项预览换行".repeat(3);
    let long_desc = "这个描述也很长，说明超宽描述同样会被省略号截断而不是硬切".repeat(3);
    let label_head: String = long_label.chars().take(10).collect();
    app.select_overlay = Some(SelectOverlay {
        id: "ui-preset".into(),
        title: "UI preset".into(),
        value: "long".into(),
        sel: 0,
        options: vec![
            SelectOption {
                value: "long".into(),
                label: long_label,
                description: Some(long_desc),
            },
            SelectOption {
                value: "short".into(),
                label: "Short".into(),
                description: Some("second option".into()),
            },
        ],
    });

    let frame = dump_frame(&mut app, 60, 30);
    let lines: Vec<&str> = frame.lines().collect();
    // Selected row: markers + ellipsized label that stays inside the box.
    let row = lines
        .iter()
        .map(|line| box_line(line))
        .find(|line| line.contains("▸ ●"))
        .expect("selected row");
    assert!(row.contains('…'), "long label ellipsized:\n{frame}");
    // TestBackend renders each double-width cell as `char + phantom
    // space`, so the row's char count equals its cell span.
    assert!(
        row.chars().count() <= 54,
        "row cells {} must fit the box; row: {row:?}\n{frame}",
        row.chars().count()
    );
    // The description row is ellipsized too, never hard-clipped.
    assert!(
        lines
            .iter()
            .map(|line| box_line(line))
            .any(|line| line.starts_with("    ") && line.contains('…')),
        "description ellipsized:\n{frame}"
    );
    // Preview carries the full label head of the highlighted option.
    let hint = lines
        .iter()
        .position(|line| line.contains("enter apply"))
        .expect("hint line");
    let preview: String = lines[hint.saturating_sub(3)..hint]
        .iter()
        .map(|line| box_line(line))
        .collect();
    // Drop the phantom spaces TestBackend inserts after double-width
    // cells before comparing against the CJK head.
    let compact: String = preview.chars().filter(|c| *c != ' ').collect();
    assert!(
        compact.contains(&label_head),
        "preview carries the full label head:\n{frame}"
    );
    assert!(
        preview.contains(" — "),
        "preview joins label and description:\n{frame}"
    );

    // Down moves the highlight; the preview follows the selection.
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))),
        &ctl,
    );
    let frame = dump_frame(&mut app, 60, 30);
    let lines: Vec<&str> = frame.lines().collect();
    let hint = lines
        .iter()
        .position(|line| line.contains("enter apply"))
        .expect("hint line");
    assert_eq!(
        box_line(lines[hint - 1]).trim_end(),
        "Short — second option",
        "preview follows the selection:\n{frame}"
    );
    assert!(
        lines
            .iter()
            .map(|line| box_line(line))
            .any(|line| line.contains("▸ ●") && line.contains("Short")),
        "highlight moved:\n{frame}"
    );

    // Enter submits the highlighted value; the overlay closes.
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );
    assert!(app.select_overlay.is_none(), "overlay closed after submit");
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(crate::bus::Cmd::PluginOverlayEvent { id, event, value })
            if id == "ui-preset"
                && event == "change"
                && value == Some(serde_json::json!("short"))
    ));
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(crate::bus::Cmd::PluginOverlayEvent { id, event, value })
            if id == "ui-preset"
                && event == "submit"
                && value == Some(serde_json::json!("short"))
    ));
}

#[test]
fn select_overlay_preview_wraps_and_caps_three_lines() {
    use crate::app::{SelectOption, SelectOverlay};
    let mut app = test_app();
    app.show_banner = false;
    app.select_overlay = Some(SelectOverlay {
        id: "long".into(),
        title: "Long".into(),
        value: "v".into(),
        sel: 0,
        options: vec![SelectOption {
            value: "v".into(),
            label: "x".repeat(400),
            description: None,
        }],
    });

    let frame = dump_frame(&mut app, 40, 30);
    let lines: Vec<&str> = frame.lines().collect();
    let hint = lines
        .iter()
        .position(|line| line.contains("enter apply"))
        .expect("hint line");
    let preview: Vec<&str> = lines[hint.saturating_sub(3)..hint]
        .iter()
        .map(|line| box_line(line))
        .collect();
    assert_eq!(preview.len(), 3, "preview capped at three lines:\n{frame}");
    assert!(
        preview[0].starts_with("xxx"),
        "preview starts with the label:\n{frame}"
    );
    assert!(
        preview[2].ends_with('…'),
        "capped preview ends with ellipsis:\n{frame}"
    );
    for line in preview {
        assert!(line.width() <= 34, "preview line fits the box:\n{frame}");
    }
}

#[test]
fn composer_glow_bar_is_gone_and_prompt_tints_while_working() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = app.theme;

    // Idle: the prompt stays brand blue.
    app.state = crate::app::RunState::Idle;
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    let prompt_cell = buf
        .content
        .iter()
        .find(|cell| cell.symbol() == "❯")
        .expect("composer prompt");
    assert_eq!(prompt_cell.fg, theme.brand, "idle prompt stays brand blue");

    // Working: the glow bar (brand `▎` column) is gone and the prompt
    // turns amber — the old glow's indicator role (issue #27).
    app.state = crate::app::RunState::Running;
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    for cell in buf.content.iter() {
        assert_ne!(cell.symbol(), "▎", "glow bar must not render while running");
    }
    let prompt_cell = buf
        .content
        .iter()
        .find(|cell| cell.symbol() == "❯")
        .expect("composer prompt");
    assert_eq!(prompt_cell.fg, theme.warn, "working prompt turns amber");
}

#[test]
fn composer_dock_title_uses_the_tertiary_tone() {
    let app = test_app();
    let theme = app.theme;
    let node = crate::slots::TuiNode::Generic {
        id: "stats".into(),
        title: "1.2k tokens".into(),
        body: String::new(),
        status: Some("ok".into()),
        action: None,
    };
    let spans = compact_node_spans(&node, &theme, 60).expect("generic spans");
    let title = spans
        .iter()
        .find(|s| s.content == "1.2k tokens")
        .expect("title span");
    assert_eq!(
        title.style.fg,
        Some(theme.fg_tertiary),
        "dock text sits one tone lighter"
    );
}
