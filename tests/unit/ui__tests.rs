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
fn head_branch_parses_a_regular_repo_head() {
    let root = fresh_root();
    let git = std::path::Path::new(&root).join(".git");
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    assert_eq!(crate::ui::head_branch(&root), Some("main".into()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn head_branch_detached_head_has_no_branch() {
    let root = fresh_root();
    let git = std::path::Path::new(&root).join(".git");
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(
        git.join("HEAD"),
        "9fceb32d0f2d1d2f1d0f2d3a4b5c6d7e8f9a0b1c2\n",
    )
    .unwrap();
    assert_eq!(crate::ui::head_branch(&root), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn head_branch_follows_a_gitdir_pointer_file() {
    let root = fresh_root();
    // Real `git worktree add` layout: the worktree holds only a `.git`
    // file pointing at the main repo's `.git/worktrees/<name>` gitdir.
    let gitdir = std::path::Path::new(&root).join("main/.git/worktrees/wt");
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/feature/x\n").unwrap();
    let worktree = std::path::Path::new(&root).join("wt");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();
    assert_eq!(
        crate::ui::head_branch(&worktree.to_string_lossy()),
        Some("feature/x".into())
    );

    // Submodules write a relative pointer, resolved against `.git`'s parent.
    let sub = std::path::Path::new(&root).join("sub");
    std::fs::create_dir_all(sub.join("modules/m")).unwrap();
    std::fs::write(sub.join("modules/m/HEAD"), "ref: refs/heads/dev\n").unwrap();
    std::fs::write(sub.join(".git"), "gitdir: ./modules/m\n").unwrap();
    assert_eq!(
        crate::ui::head_branch(&sub.to_string_lossy()),
        Some("dev".into())
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn head_branch_without_git_or_head_means_no_branch() {
    let root = fresh_root();
    // No `.git` at all: not a repository.
    assert_eq!(crate::ui::head_branch(&root), None);
    // `.git` present but HEAD missing (fresh `git init` always writes
    // HEAD, so this is a corrupt/partial checkout).
    let git = std::path::Path::new(&root).join(".git");
    std::fs::create_dir_all(&git).unwrap();
    assert_eq!(crate::ui::head_branch(&root), None);
    let _ = std::fs::remove_dir_all(&root);
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
    // idle · empty → /keys discovery hint (ctrl+k belongs to the editor)
    let s = flat(context_hints(&app));
    assert!(s.contains("/keys keys"), "{s}");
    assert!(!s.contains("^k"), "{s}");
    // idle · draft → enter sends
    app.input.set("hello".into());
    let s = flat(context_hints(&app));
    assert!(s.contains("⏎ send"), "{s}");
    assert!(!s.contains("keys"), "{s}");
    // idle · slash recommendations own Enter/Esc; shell drafts relabel Enter
    app.input.set("/mo".into());
    let s = flat(context_hints(&app));
    assert!(s.contains("⏎ select") && s.contains("esc close"), "{s}");
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
fn meta_row_hints_explain_queue_edit_and_delete_confirmation() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    let flat = |spans: Vec<Span>| -> String {
        spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    };
    let mut app = test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.input.set("queued draft".into());
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT))),
        &ctl,
    );

    let selecting = flat(context_hints(&app));
    assert!(selecting.contains("↑↓ choose"), "{selecting}");
    assert!(selecting.contains("⏎ edit"), "{selecting}");
    assert!(selecting.contains("esc close"), "{selecting}");
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    let edit = flat(context_hints(&app));
    assert!(edit.contains("⏎ save"), "{edit}");
    assert!(edit.contains("^d delete"), "{edit}");
    assert!(edit.contains("esc cancel"), "{edit}");

    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        ))),
        &ctl,
    );
    let confirm = flat(context_hints(&app));
    assert!(confirm.contains("⏎ delete"), "{confirm}");
    assert!(confirm.contains("esc back"), "{confirm}");
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
fn native_agent_rail_expands_only_during_selection() {
    let mut app = test_app();
    app.show_banner = false;
    app.subagents.push(crate::app::SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: true,
        failed: false,
        transcript: crate::transcript::Transcript::new("child-1".into()),
    });
    app.subagents.push(crate::app::SubagentView {
        id: "child-2".into(),
        parent: "dsh-test".into(),
        label: "subagent 2".into(),
        running: false,
        failed: false,
        transcript: crate::transcript::Transcript::new("child-2".into()),
    });

    let collapsed = dump_frame(&mut app, 100, 24);
    assert!(
        collapsed.contains("· Agents · 1/2  ↓ expand"),
        "the keyboard hint stays beside the summary:\n{collapsed}"
    );
    assert!(!collapsed.contains("click"), "{collapsed}");
    assert!(!collapsed.contains("subagent 1"), "{collapsed}");
    assert!(!collapsed.contains("subagent 2"), "{collapsed}");
    let before_spinner = collapsed
        .lines()
        .find(|line| line.contains("Agents ·"))
        .expect("collapsed Agent rail")
        .to_string();
    app.tick();
    let after_tick = dump_frame(&mut app, 100, 24);
    let after_spinner = after_tick
        .lines()
        .find(|line| line.contains("Agents ·"))
        .expect("collapsed Agent rail after tick");
    assert_eq!(
        before_spinner, after_spinner,
        "the collapsed summary has no redundant spinner prefix"
    );
    let collapsed_lines = collapsed.lines().collect::<Vec<_>>();
    let collapsed_agents_y = collapsed_lines
        .iter()
        .position(|line| line.contains("Agents ·"))
        .expect("collapsed Agent rail");
    let collapsed_meta_y = collapsed_lines
        .iter()
        .position(|line| line.contains("· Standard"))
        .expect("composer meta");
    assert_eq!(
        collapsed_agents_y + 1,
        collapsed_meta_y,
        "collapsed Agent rail sits immediately above composer meta:\n{collapsed}"
    );

    app.agent_selection = Some("dsh-test".into());
    let running_marker = app.spinner();
    let expanded = dump_frame(&mut app, 100, 24);
    assert!(
        expanded.contains(&format!("{running_marker} subagent 1")),
        "{expanded}"
    );
    assert!(expanded.contains("✓ subagent 2"), "{expanded}");
    assert!(expanded.contains("esc close"), "{expanded}");
}

#[test]
fn plugin_agent_navigation_sits_above_composer_meta_with_general_theme_tokens() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    app.subagents.push(crate::app::SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: true,
        failed: false,
        transcript: crate::transcript::Transcript::new("child-1".into()),
    });
    app.subagents.push(crate::app::SubagentView {
        id: "child-2".into(),
        parent: "dsh-test".into(),
        label: "subagent 2".into(),
        running: false,
        failed: false,
        transcript: crate::transcript::Transcript::new("child-2".into()),
    });
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "slot": "conversation.navigation.dock",
                "rev": 1,
                "nodes": [
                    { "id": "agents-view:label", "kind": "generic", "title": "· Agents", "body": "", "tone": "caption" },
                    { "id": "agents-view:agent-dsh-test", "kind": "generic", "title": "▸ main", "body": "", "tone": "brand", "selected": true },
                    { "id": "agents-view:agent-child-1", "kind": "generic", "title": "subagent 1", "body": "", "status": "running", "tone": "fg" },
                    { "id": "agents-view:agent-child-2", "kind": "generic", "title": "subagent 2", "body": "", "status": "done", "tone": "fg" },
                    { "id": "agents-view:switch", "kind": "generic", "title": "←/→ · enter · esc close", "body": "", "tone": "brand_soft" }
                ]
            }),
        },
        &ctl,
    );
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "slot": "conversation.composer.dock",
                "rev": 1,
                "nodes": [{ "id": "stats:counts", "kind": "generic", "title": "4 turns", "body": "" }]
            }),
        },
        &ctl,
    );

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    let frame = (0..24)
        .map(|y| (0..100).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>();
    let meta_y = frame
        .iter()
        .position(|line| line.contains("Standard"))
        .expect("composer meta");
    let agents_y = frame
        .iter()
        .position(|line| line.contains("· Agents"))
        .expect("Agent dock");
    let stats_y = frame
        .iter()
        .position(|line| line.contains("4 turns"))
        .expect("stats dock");

    assert_eq!(
        agents_y + 1,
        meta_y,
        "Agent navigation sits immediately above composer meta:\n{}",
        frame.join("\n")
    );
    assert_eq!(
        stats_y,
        meta_y + 1,
        "stats remain outside navigation:\n{}",
        frame.join("\n")
    );
    let meta_x = frame[meta_y].find("· Standard").expect("meta alignment");
    let agents_x = frame[agents_y].find("· Agents").expect("Agent alignment");
    assert_eq!(
        agents_x,
        meta_x,
        "TUI rows share one left content edge:\n{}",
        frame.join("\n")
    );
    assert_eq!(
        buf[(0, meta_y as u16)].symbol(),
        "╰",
        "composer meta closes the shared card"
    );
    assert_eq!(
        buf[(0, agents_y as u16)].symbol(),
        "│",
        "navigation remains inside the shared card"
    );
    assert_eq!(
        buf[(99, agents_y as u16)].symbol(),
        "│",
        "navigation remains inside the shared card"
    );

    let active_x = frame[agents_y].find("▸ main").expect("active agent") as u16;
    let finished_x = frame[agents_y].find("subagent 2").expect("finished agent") as u16;
    assert_eq!(buf[(active_x, agents_y as u16)].fg, app.theme.brand);
    assert_eq!(buf[(finished_x, agents_y as u16)].fg, app.theme.fg);
    let finished_status_x = (0..100)
        .find(|x| buf[(*x, agents_y as u16)].symbol() == "✓")
        .expect("finished Agent status");
    assert_eq!(
        buf[(finished_status_x, agents_y as u16)].fg,
        app.theme.caption
    );
    assert!(
        frame[agents_y].contains("esc close"),
        "the expanded rail explains how to collapse:\n{}",
        frame.join("\n")
    );

    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "slot": "conversation.navigation.dock",
                "rev": 2,
                "nodes": [
                    { "id": "agents-view:label", "kind": "generic", "title": "· Agents", "body": "", "tone": "caption" },
                    { "id": "agents-view:agent-dsh-test", "kind": "generic", "title": "• main", "body": "", "tone": "brand_soft" },
                    { "id": "agents-view:agent-child-1", "kind": "generic", "title": "subagent 1", "body": "", "status": "running", "tone": "fg" },
                    { "id": "agents-view:agent-child-2", "kind": "generic", "title": "▸ subagent 2", "body": "", "tone": "brand", "selected": true },
                    { "id": "agents-view:switch", "kind": "generic", "title": "←/→ · enter · esc close", "body": "", "tone": "brand_soft" }
                ]
            }),
        },
        &ctl,
    );
    terminal
        .draw(|f| draw(f, &mut app))
        .expect("draw selection frame");
    let selecting = terminal.backend().buffer();
    let selected_x = (0..100)
        .find(|x| selecting[(*x, agents_y as u16)].symbol() == "▸")
        .expect("focused Agent marker");
    assert!(
        selecting[(selected_x, agents_y as u16)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "focused Agent is visibly reversed during inline selection"
    );
}

#[test]
fn native_agent_selection_reverses_the_focused_agent() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    app.subagents.push(crate::app::SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: true,
        failed: false,
        transcript: crate::transcript::Transcript::new("child-1".into()),
    });
    app.agent_selection = Some("child-1".into());

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer();
    let focused = (0..24)
        .flat_map(|y| (0..100).map(move |x| (x, y)))
        .find(|(x, y)| buf[(*x, *y)].symbol() == "▸")
        .expect("focused Agent marker");
    assert!(
        buf[focused]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "native fallback uses the same strong selection state"
    );
}

#[test]
fn native_agent_completion_uses_a_neutral_status_glyph() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    app.subagents.push(crate::app::SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: false,
        failed: false,
        transcript: crate::transcript::Transcript::new("child-1".into()),
    });
    app.agent_selection = Some("dsh-test".into());

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer();
    let status = (0..24)
        .flat_map(|y| (0..100).map(move |x| (x, y)))
        .find(|(x, y)| buf[(*x, *y)].symbol() == "✓")
        .expect("completed Agent glyph");
    assert_eq!(buf[status].fg, app.theme.caption);
    assert_eq!(buf[(status.0 + 2, status.1)].fg, app.theme.fg);
}

#[test]
fn narrow_agent_navigation_keeps_the_active_item_and_switch_action() {
    let mut app = test_app();
    app.show_banner = false;
    app.active_subagent = Some("child-4".into());
    for index in 1..=4 {
        app.subagents.push(crate::app::SubagentView {
            id: format!("child-{index}"),
            parent: "dsh-test".into(),
            label: format!("subagent {index}"),
            running: true,
            failed: false,
            transcript: crate::transcript::Transcript::new(format!("child-{index}")),
        });
    }
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "slot": "conversation.navigation.dock",
                "rev": 1,
                "nodes": [
                    { "id": "agents-view:label", "kind": "generic", "title": "· Agents", "body": "", "tone": "caption" },
                    { "id": "agents-view:agent-dsh-test", "kind": "generic", "title": "main", "body": "", "tone": "fg" },
                    { "id": "agents-view:agent-child-1", "kind": "generic", "title": "subagent 1", "body": "", "tone": "fg" },
                    { "id": "agents-view:agent-child-2", "kind": "generic", "title": "subagent 2", "body": "", "tone": "fg" },
                    { "id": "agents-view:agent-child-3", "kind": "generic", "title": "subagent 3", "body": "", "tone": "fg" },
                    { "id": "agents-view:agent-child-4", "kind": "generic", "title": "▸ subagent 4", "body": "", "tone": "brand", "selected": true },
                    { "id": "agents-view:switch", "kind": "generic", "title": "←/→ · enter · esc close", "body": "", "tone": "brand_soft", "action": { "kind": "command", "name": "agents", "args": "" } }
                ]
            }),
        },
        &ctl,
    );

    let frame = dump_frame(&mut app, 50, 20);
    let rail = frame
        .lines()
        .find(|line| line.contains("Agents"))
        .expect("navigation rail");
    assert!(
        rail.contains("▸ subagent 4"),
        "active item survives:\n{frame}"
    );
    assert!(
        rail.contains("esc close"),
        "trailing action survives:\n{frame}"
    );
}

#[test]
fn short_terminal_keeps_navigation_before_optional_telemetry() {
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    for (slot, id, title) in [
        ("conversation.navigation.dock", "agents:active", "▸ main"),
        ("conversation.composer.dock", "stats:counts", "4 turns"),
    ] {
        app.handle(
            crate::bus::AppEvent::Rpc {
                method: crate::cordis::SLOTS_UPDATE.into(),
                params: serde_json::json!({
                    "protocol": 0,
                    "slot": slot,
                    "rev": 1,
                    "nodes": [{ "id": id, "kind": "generic", "title": title, "body": "" }]
                }),
            },
            &ctl,
        );
    }

    let frame = dump_frame(&mut app, 80, 20);
    assert!(
        frame.contains("▸ main"),
        "navigation remains operable:\n{frame}"
    );
    assert!(
        !frame.contains("4 turns"),
        "optional telemetry yields its row first on short terminals:\n{frame}"
    );
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
        failed: false,
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
        failed: false,
        transcript: crate::transcript::Transcript::new("child-1".into()),
    });
    app.active_subagent = Some("child-1".into());

    let frame = dump_frame(&mut app, 100, 24);
    assert!(frame.contains("working"), "{frame}");
    assert!(!frame.contains("● idle"), "{frame}");
}

#[test]
fn hidden_tool_arguments_replace_thinking_with_the_working_placeholder() {
    let mut app = test_app();
    app.show_banner = false;
    app.state = RunState::Running;
    app.transcript
        .apply(crate::events::UiEvent::ReasoningDelta {
            session: "dsh-test".into(),
            text: "I will launch several tools".into(),
        });
    assert!(
        state_line(&app).is_none(),
        "live reasoning owns the thinking row"
    );

    app.transcript
        .apply(crate::events::UiEvent::ToolCallPreparing {
            session: "dsh-test".into(),
        });

    let line = state_line(&app).expect("working placeholder while tool arguments stream");
    let rendered = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(rendered.contains("working"), "{rendered}");

    app.transcript.apply(crate::events::UiEvent::ToolCall {
        session: "dsh-test".into(),
        call_id: "call-1".into(),
        name: "subagent".into(),
        arguments: r#"{"description":"task"}"#.into(),
    });
    let frame = dump_frame(&mut app, 100, 24);
    assert!(
        frame.contains("subagent"),
        "complete tool request still renders: {frame}"
    );
    assert!(
        frame.contains("│ request"),
        "complete request body remains visible: {frame}"
    );
}

#[test]
fn elicitation_wait_uses_the_state_line_without_claiming_the_agent_is_working() {
    use crate::app::ElicitationAskOverlay;
    use crate::elicitation::{ElicitationForm, ElicitationFormState};

    let mut app = test_app();
    app.state = RunState::Running;
    app.elicitation_ask = Some(ElicitationAskOverlay {
        form: ElicitationFormState::new(ElicitationForm {
            message: "The agent needs your input.".into(),
            fields: Vec::new(),
        }),
        scroll: 0,
        reply: None,
    });

    let line = state_line(&app).expect("input wait state");
    let rendered = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("waiting for input"), "{rendered}");
    assert!(!rendered.contains("working"), "{rendered}");
}

#[test]
fn long_input_wraps_in_the_well() {
    let mut app = test_app();
    app.show_banner = false;
    app.input.set("a".repeat(50));
    app.input.set_cursor_char(50);
    // 40x12 → composer height 4 → a 2-row input well, 36 text cols wide.
    let frame = dump_frame(&mut app, 40, 12);
    let wrapped = frame.lines().filter(|l| l.contains("aaaa")).count();
    assert!(wrapped >= 2, "input should wrap across well rows:\n{frame}");
}

#[test]
fn multiline_input_breaks_on_newlines() {
    let mut app = test_app();
    app.show_banner = false;
    app.input.set("hello\nworld".into());
    app.input.set_cursor_char("hello\nworld".chars().count());
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
fn streaming_preserves_the_viewport_after_the_user_scrolls_up() {
    let mut app = test_app();
    app.show_banner = false;
    app.state = RunState::Running;
    for i in 0..40 {
        app.transcript.push_user(format!("history {i}"), false);
    }
    app.transcript.apply(crate::events::UiEvent::TextDelta {
        session: "dsh-test".into(),
        text: "stream 0".into(),
    });

    let _ = dump_frame(&mut app, 100, 14);
    app.scroll_by(20);
    let _ = dump_frame(&mut app, 100, 14);
    let anchored_top = app.chat_view.top;

    app.transcript.apply(crate::events::UiEvent::TextDelta {
        session: "dsh-test".into(),
        text: (1..=12).map(|i| format!("\nstream {i}")).collect(),
    });
    let frame = dump_frame(&mut app, 100, 14);

    assert_eq!(
        app.chat_view.top, anchored_top,
        "streaming must not move a manually detached viewport:\n{frame}"
    );
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

#[test]
fn elicitation_text_field_owns_the_terminal_cursor_instead_of_the_composer() {
    use crate::app::ElicitationAskOverlay;
    use crate::elicitation::{
        ElicitationField, ElicitationFieldKind, ElicitationForm, ElicitationFormState,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    app.input.insert_str("composer draft");
    let mut form = ElicitationFormState::new(ElicitationForm {
        message: "The agent needs your input.".into(),
        fields: vec![ElicitationField {
            name: "answer".into(),
            custom_name: None,
            title: "Answer".into(),
            description: None,
            required: true,
            kind: ElicitationFieldKind::Text { default: None },
        }],
    });
    form.fields[0].input.insert_str("overlay answer");
    app.elicitation_ask = Some(ElicitationAskOverlay {
        form,
        scroll: 0,
        reply: None,
    });

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buffer = terminal.backend().buffer();
    let answer_row = (0..30)
        .find(|y| {
            (0..100)
                .map(|x| buffer[(x, *y)].symbol())
                .collect::<String>()
                .contains("overlay answer▏")
        })
        .expect("elicitation text field");
    let answer_line = (0..100)
        .map(|x| buffer[(x, answer_row)].symbol())
        .collect::<String>();
    let answer_byte = answer_line.find("overlay answer").expect("answer text");
    let answer_start = answer_line[..answer_byte].width() as u16;
    let cursor = terminal
        .get_cursor_position()
        .expect("terminal cursor position");

    assert_eq!(cursor.y, answer_row);
    assert_eq!(cursor.x, answer_start + "overlay answer".width() as u16);
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
        tone: None,
        selected: false,
        action: None,
    };
    let spans = compact_node_spans(&node, &theme, 60, '⣋').expect("generic spans");
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

#[test]
fn composer_control_rows_share_one_left_gutter() {
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    for (slot, nodes) in [
        (
            "conversation.input.dock",
            serde_json::json!([
                { "id": "plan:summary", "kind": "generic", "title": "· Plan", "body": "2/4", "status": "running", "tone": "caption" },
                { "id": "plan:focus", "kind": "generic", "title": "Collect results", "body": "", "tone": "caption" }
            ]),
        ),
        (
            "conversation.navigation.dock",
            serde_json::json!([
                { "id": "agents:summary", "kind": "generic", "title": "· Agents", "body": "0/4", "status": "running", "tone": "caption" },
                { "id": "agents:switch", "kind": "generic", "title": "↓ expand", "body": "", "tone": "caption" }
            ]),
        ),
    ] {
        app.handle(
            crate::bus::AppEvent::Rpc {
                method: crate::cordis::SLOTS_UPDATE.into(),
                params: serde_json::json!({
                    "protocol": 0,
                    "slot": slot,
                    "rev": 1,
                    "nodes": nodes,
                }),
            },
            &ctl,
        );
    }

    let frame = dump_frame(&mut app, 100, 24);
    assert!(
        frame.contains("· Plan · 2/4 · Collect results"),
        "Plan summary stays quiet while only its progress pulses:\n{frame}"
    );
    assert!(
        frame.contains("· Agents · 0/4  ↓ expand"),
        "Agent summary uses the same compact grammar:\n{frame}"
    );

    let lines = frame.lines().collect::<Vec<_>>();
    let plan_x = lines
        .iter()
        .find(|line| line.contains("· Plan"))
        .and_then(|line| line.find("· Plan"))
        .expect("Plan marker");
    let input_x = lines
        .iter()
        .find(|line| line.contains("describe what you want to build"))
        .and_then(|line| line.find('❯'))
        .expect("composer prompt");
    let agents_x = lines
        .iter()
        .find(|line| line.contains("· Agents"))
        .and_then(|line| line.find("· Agents"))
        .expect("Agents marker");
    let meta_x = lines
        .iter()
        .find(|line| line.contains("· Standard"))
        .and_then(|line| line.find("· Standard"))
        .expect("composer metadata marker");
    assert_eq!(
        (plan_x, input_x, agents_x, meta_x),
        (plan_x, plan_x, plan_x, plan_x)
    );
}

#[test]
fn compact_running_progress_pulses_without_a_spinner_prefix() {
    let app = test_app();
    let node = crate::slots::TuiNode::Generic {
        id: "agents".into(),
        title: "· Agents".into(),
        body: "0/2".into(),
        status: Some("running".into()),
        tone: Some("caption".into()),
        selected: false,
        action: None,
    };

    let soft = compact_node_spans(&node, &app.theme, 60, '⠋').expect("generic spans");
    let bright = compact_node_spans(&node, &app.theme, 60, '⠴').expect("generic spans");
    let text = |spans: &[Span]| {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };

    assert_eq!(text(&soft), "· Agents · 0/2");
    assert_eq!(text(&bright), "· Agents · 0/2");
    assert_eq!(soft[0].style.fg, Some(app.theme.caption));
    assert_eq!(
        soft.last().expect("progress").style.fg,
        Some(app.theme.brand_soft)
    );
    assert_eq!(
        bright.last().expect("progress").style.fg,
        Some(app.theme.brand)
    );
}

#[test]
fn composer_textarea_renders_chips_selection_and_multiline_drafts() {
    // Chip restyle lands on the buffer cells with the theme's bubble colors.
    let mut app = test_app();
    app.show_banner = false;
    app.pending_images
        .add(
            "x.png".into(),
            "/tmp/x.png".into(),
            "image/png".into(),
            vec![137, 80, 78, 71, 13, 10, 26, 10],
        )
        .expect("stage");
    app.input.set("x [image 1] y".into());
    let backend = ratatui::backend::TestBackend::new(50, 15);
    let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer().clone();
    let mut chip_cells = 0usize;
    for row in 0..15u16 {
        for col in 0..50u16 {
            if buf[(col, row)].symbol() == "[" {
                let style = buf[(col, row)].style();
                assert_eq!(style.fg, Some(app.theme.bubble_fg));
                assert_eq!(style.bg, Some(app.theme.bubble_bg));
                chip_cells += 1;
            }
        }
    }
    assert!(chip_cells >= 1, "chip styled cells found");

    // Drag selection paints reversed cells over the dragged range.
    let mut app = test_app();
    app.show_banner = false;
    app.input.set("hello world".into());
    app.input_area = Rect::new(1, 10, 48, 3);
    app.input_top = 0;
    app.input_sel = Some(crate::app::InputSel {
        anchor: (0, 1),
        head: (0, 4),
    });
    let backend = ratatui::backend::TestBackend::new(50, 15);
    let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer().clone();
    let reversed = (0..15u16)
        .flat_map(|row| (0..50u16).map(move |col| (col, row)))
        .filter(|&(col, row)| buf[(col, row)].style().add_modifier.contains(Modifier::REVERSED))
        .count();
    assert!(reversed >= 4, "selection covers the dragged cells: {reversed}");

    // Hard newlines stack draft lines in the well.
    let mut app = test_app();
    app.show_banner = false;
    app.input.set("line one\nline two".into());
    let frame = dump_frame(&mut app, 50, 15);
    let first = frame
        .lines()
        .position(|l| l.contains("line one"))
        .expect("first line");
    let second = frame
        .lines()
        .position(|l| l.contains("line two"))
        .expect("second line");
    assert!(second > first, "hard newline stacks lines");
}
