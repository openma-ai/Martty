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
        additional_directories: Vec::new(),
    };
    let (tx, _rx) = mpsc::channel();
    App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx)
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
        additional_directories: Vec::new(),
    };
    let (tx, _rx) = mpsc::channel();
    App::new(Some(Theme::dark()), cfg, "pending".into(), false, true, tx)
}

#[test]
fn file_menu_popup_renders_above_the_composer() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.input.insert_str("@");
    // A small controlled workspace: [../, src/, README.md].
    let ws = std::env::temp_dir().join(format!(
        "dsh-tui-ui-file-menu-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(ws.join("src")).expect("mkdir");
    std::fs::write(ws.join("src/main.rs"), "").expect("write");
    std::fs::write(ws.join("README.md"), "").expect("write");
    let token = crate::file_ref::active_at_token("@", 1).expect("token");
    app.cfg.workspace = ws.to_string_lossy().into_owned();
    app.file_menu =
        Some(crate::file_ref::FileMenu::open(&ws, 0, &token).expect("menu opens"));
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    let theme = app.theme;
    let row_text = |y: u16| -> String {
        (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect()
    };
    // The popup sits above the composer box (rows 11..15 on this layout:
    // 12 list rows cap to 3 entries + 2 border rows).
    let title_row = row_text(11);
    assert!(
        title_row.contains("@") && title_row.contains("."),
        "popup title shows the relative cwd: {title_row:?}"
    );
    let joined = [row_text(12), row_text(13), row_text(14)].join("\n");
    assert!(
        joined.contains("src/") && joined.contains("README.md"),
        "entries listed: {joined:?}"
    );
    // The panel surface colors the popup; the selected row uses chip_bg.
    assert_eq!(buf[(4, 12)].bg, theme.chip_bg, "selected row highlight");
    assert_eq!(buf[(4, 13)].bg, theme.panel, "unselected rows keep panel");
    // The highlight is unmistakable: a solid ▶ marker in the brand accent
    // and the selected row text in brand + bold.
    assert_eq!(buf[(3, 12)].symbol(), "▶", "bigger highlight marker");
    assert_eq!(buf[(3, 12)].fg, theme.brand, "marker takes the accent");
    let _ = std::fs::remove_dir_all(&ws);
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
    app.selected_model = Some("deepseek-flash".into());
    let s = flat(status_right(&app));
    assert!(s.contains("deepseek-flash"), "{s}");
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
    assert!(!pending.contains("/keys"), "{pending}");
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
    assert!(bound_left.contains("minimal"), "{bound_left}");
    assert!(bound_left.contains("Full access"), "{bound_left}");
    assert!(!flat_spans(status_right(&app)).contains("deepseek-chat"));
    app.handle(
        crate::bus::AppEvent::Ui(crate::events::UiEvent::SessionModel {
            session: "acp-session".into(), model: "deepseek-chat".into(),
        }), &ctl,
    );
    let bound_right = flat_spans(status_right(&app));
    assert!(bound_right.contains("deepseek-chat"), "{bound_right}");
    assert!(bound_right.contains("high"), "{bound_right}");
}

#[test]
fn codex_model_chip_waits_for_acp_then_uses_the_reported_session_model() {
    let flat = |spans: Vec<Span>| -> String {
        spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    };
    let mut app = live_test_app();
    app.cfg.bin = "npx -y @agentclientprotocol/codex-acp".into();
    app.server_info = Some("@agentclientprotocol/codex-acp".into());
    app.session_bound = true;

    let pending = flat(status_right(&app));
    assert!(!pending.contains("deepseek-chat"), "{pending}");

    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Ui(crate::events::UiEvent::SessionModel {
            session: app.session_id.clone(),
            model: "gpt-5.6-codex".into(),
        }),
        &ctl,
    );

    let reported = flat(status_right(&app));
    assert!(reported.contains("gpt-5.6-codex"), "{reported}");
    assert!(!reported.contains("deepseek-chat"), "{reported}");
}

#[test]
fn live_deepseek_landing_also_uses_acp_instead_of_startup_provider_and_model() {
    let mut app = live_test_app();
    app.server_info = Some("dsh-acp".into());
    assert_eq!(welcome_model(&app), "waiting for ACP");
    assert_eq!(displayed_model(&app), None);
    app.session_model = Some("current-deepseek-model".into());
    assert_eq!(welcome_model(&app), "current-deepseek-model");
}

#[test]
fn failed_connection_landing_has_target_and_error_not_waiting_or_credentials() {
    let mut app = live_test_app();
    app.server_info = Some("Cline".into());
    app.connection_error = Some("process exited (1)".into());
    app.session_id = "unavailable".into();
    let text = welcome_info_lines(&app).iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<String>();
    assert!(text.contains("Cline"), "{text}");
    assert!(text.contains("connection failed"), "{text}");
    assert!(!text.contains("waiting for ACP"), "{text}");
    assert!(!text.contains("source not reported"), "{text}");
}

#[test]
fn failed_connection_landing_keeps_only_the_first_error_line() {
    let mut app = live_test_app();
    app.server_info = Some("dsh-acp".into());
    // The full error (including the agent stderr tail) renders wrapped in
    // the transcript notice; the one-line status row can only carry the
    // headline, so an embedded newline must not reach the span.
    app.connection_error = Some("exited (1)\nAgent stderr:\nlong detail".into());
    let text = welcome_info_lines(&app).iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<String>();
    assert!(text.contains("exited (1)"), "{text}");
    assert!(!text.contains("Agent stderr"), "{text}");

    app.connection_error = Some(String::new());
    let text = welcome_info_lines(&app).iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<String>();
    assert!(text.contains("connection failed"), "{text}");
}

#[test]
fn a_refused_attach_landing_says_initialize_refused_not_failed() {
    let mut app = live_test_app();
    app.server_info = Some("acp".into());
    // An answered refusal: the agent is attached — the landing row names the
    // refusal instead of the dead-attach "connection failed".
    app.connection_error = Some("unsupported client".into());
    app.attach_refused = true;
    app.session_id = "unavailable".into();
    let text = welcome_info_lines(&app).iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<String>();
    assert!(text.contains("initialize refused"), "{text}");
    assert!(!text.contains("connection failed"), "{text}");
}

#[test]
fn attached_landing_does_not_guess_runtime_before_initialize_or_after_failure() {
    let mut app = live_test_app();
    app.cfg.bin = "dsh-acp".into();
    app.server_info = None;
    for state in [crate::app::RunState::Starting, crate::app::RunState::Idle] {
        app.state = state;
        let text = welcome_info_lines(&app).iter().flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref()).collect::<String>();
        assert!(!text.contains("dsh-acp"), "{text}");
        assert!(!text.contains("deepseek-chat"), "{text}");
        assert!(text.contains("waiting for ACP"), "{text}");
    }
}

#[test]
fn new_tab_landing_never_falls_back_to_old_runtime_or_auth() {
    let mut app = live_test_app();
    app.server_info = Some("dsh-acp".into());
    app.auth.status = crate::acp_auth::AuthStatus::Configured;
    app.auth.method_name = Some("Old credential".into());
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(crate::bus::AppEvent::Ctl(crate::bus::CtlEvent::NewSessionRequested), &ctl);
    let text = welcome_info_lines(&app).iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<String>();
    assert!(!text.contains("dsh-acp"), "{text}");
    assert!(!text.contains("deepseek-chat"), "{text}");
    assert!(!text.contains("Old credential"), "{text}");
}

#[test]
fn welcome_auth_unknown_source_does_not_claim_authenticate() {
    let mut app = live_test_app();
    app.auth = crate::acp_auth::configured_snapshot(vec![], None);
    let lines = welcome_info_lines(&app);
    let text = lines.iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<String>();
    assert!(text.contains("source not reported"), "{text}");
    assert!(!text.contains("ACP authenticate"), "{text}");
    assert!(text.contains("credentials "), "label and value need spacing: {text}");
}

#[test]
fn welcome_auth_with_methods_reports_ready_not_unreported() {
    // An agent that advertised auth methods did report its providers, so
    // a configured status without a named method reads "ready", not
    // "credential source not reported" — and the "credentials" label is
    // not repeated in the value (issue #160).
    let mut app = live_test_app();
    let methods = crate::acp_auth::parse_auth_methods(
        &serde_json::json!([
            { "id": "api-key:deepseek", "name": "DeepSeek API key",
              "_meta": { "api-key": { "provider": "deepseek" } } }
        ]),
        &["dsh-acp".into()],
        "/tmp",
        &Default::default(),
    );
    app.auth = crate::acp_auth::configured_snapshot(methods, None);
    let lines = welcome_info_lines(&app);
    // The credentials row's value is "ready" — the label is not repeated.
    let row = lines
        .iter()
        .find(|line| {
            line.spans
                .first()
                .is_some_and(|span| span.content.contains("credentials"))
        })
        .expect("credentials row");
    let value = row
        .spans
        .last()
        .map(|span| span.content.as_ref())
        .unwrap_or_default();
    assert_eq!(value, "ready");
    let text = lines.iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<String>();
    assert!(!text.contains("not reported"), "{text}");
}

#[test]
fn welcome_auth_distinguishes_pending_and_failed_authenticate() {
    let flat = |lines: Vec<Line>| lines.iter().flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref()).collect::<Vec<_>>().join("\n");
    let mut app = live_test_app();
    app.auth.status = crate::acp_auth::AuthStatus::SigningIn;
    app.auth.method_name = Some("Log in with Google".into());
    let pending = flat(welcome_info_lines(&app));
    assert!(pending.contains("signing in"), "{pending}");
    assert!(!pending.contains("sign-in needed"), "{pending}");
    app.auth.status = crate::acp_auth::AuthStatus::Failed;
    let failed = flat(welcome_info_lines(&app));
    assert!(failed.contains("sign-in failed"), "{failed}");
    assert!(failed.contains("ctrl+g"), "{failed}");
    assert!(!failed.contains("host dsh"), "{failed}");
}

#[test]
fn welcome_info_uses_the_active_acp_runtime_and_reported_session_model() {
    let flat = |lines: Vec<Line>| -> String {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut app = live_test_app();
    app.server_info = Some("@agentclientprotocol/codex-acp".into());

    let pending = flat(welcome_info_lines(&app));
    assert!(pending.contains("waiting for ACP"), "{pending}");
    assert!(pending.contains("codex-acp"), "{pending}");
    assert!(!pending.contains("deepseek-chat"), "{pending}");

    app.session_model = Some("gpt-5.6-sol".into());
    let reported = flat(welcome_info_lines(&app));
    assert!(reported.contains("gpt-5.6-sol"), "{reported}");
    assert!(!reported.contains("deepseek-chat"), "{reported}");
}

#[test]
fn status_chips_show_values_without_inline_key_hints() {
    let flat = |line: &Line| -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    let mut app = test_app();

    app.locale = crate::locale::Locale::Zh;
    let zh = flat(&status_title(&app));
    // Issue #148: values only — the bindings live in /keys, not the dock.
    assert!(zh.contains("Standard mode · 工作区可写"), "{zh}");
    assert!(!zh.contains("ctrl+shift+a"), "{zh}");
    assert!(!zh.contains("shift+tab"), "{zh}");
    assert!(!zh.contains("permission"), "{zh}");
    assert!(!zh.contains("权限"), "{zh}");

    app.locale = crate::locale::Locale::En;
    let en_line = status_title(&app);
    let en = flat(&en_line);
    assert!(en.contains("Standard mode · Workspace Write"), "{en}");
    assert!(!en.contains("ctrl+shift+a"), "{en}");
    assert!(!en.contains("shift+tab"), "{en}");
    assert!(!en.contains("permission"), "{en}");
    assert!(!en.contains("access"), "{en}");

    // No chord-looking spans remain anywhere in the chip row.
    assert!(
        en_line
            .spans
            .iter()
            .all(|span| !span.content.contains('+')),
        "{en}"
    );
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
}

#[test]
fn status_chips_render_optional_approval_plan_and_fallback_labels() {
    let flat = |line: &Line| -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    let mut app = test_app();

    // Approval and plan chips render when the host reports them — still
    // values only, no key glued on (#148).
    app.modes.approval = Some("on-request".into());
    app.modes.plan = true;
    let line = status_title(&app);
    let s = flat(&line);
    assert!(s.contains(" · on-request"), "{s}");
    assert!(s.contains(" · plan"), "{s}");
    assert!(!s.contains('+'), "{s}");

    // zh: a permission id outside the renamed trio falls back to the
    // shared label, and a sandbox-only mode feeds the same chip.
    app.locale = crate::locale::Locale::Zh;
    app.modes.permission = Some("custom-perm".into());
    let s = flat(&status_title(&app));
    assert!(s.contains("Custom Perm"), "{s}");
    app.modes.permission = None;
    app.modes.sandbox = Some("sandbox-mode".into());
    let s = flat(&status_title(&app));
    assert!(s.contains("Sandbox Mode"), "{s}");
    assert!(!s.contains('+'), "{s}");
}

#[test]
fn cordis_protocol_id_is_rendered_as_creator() {
    let mut app = test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Ctl(crate::bus::CtlEvent::Catalog {
            session_id: None,
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
fn meta_row_hints_only_advertise_the_running_interrupt() {
    let flat = |spans: Vec<Span>| -> String {
        spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    };
    let mut app = test_app();
    // Issue #148: only the mid-turn interrupt survives in the dock — every
    // other binding lives in /keys. Idle states render nothing, whatever
    // the draft holds (plain text, slash command, shell, pending FIFO).
    for draft in ["", "hello", "/mo", "!ls"] {
        app.input.set(draft.into());
        let s = flat(context_hints(&app));
        assert!(s.is_empty(), "idle draft {draft:?} rendered {s:?}");
    }
    app.input.clear();
    app.queued = 2;
    assert!(flat(context_hints(&app)).is_empty());
    app.queued = 0;
    // Running, draft or not: esc interrupt, and nothing else.
    app.state = RunState::Running;
    for draft in ["", "follow-up"] {
        app.input.set(draft.into());
        let s = flat(context_hints(&app));
        assert!(s.contains("esc interrupt"), "{s}");
        assert!(!s.contains("⏎"), "{s}");
        assert!(!s.contains("queue"), "{s}");
        assert!(!s.contains("steer"), "{s}");
        assert!(!s.contains("send"), "{s}");
    }
    // zh localizes the surviving label.
    app.locale = crate::locale::Locale::Zh;
    let s = flat(context_hints(&app));
    assert!(s.contains("esc 中断"), "{s}");
    assert!(!s.contains("⏎"), "{s}");
}

#[test]
fn meta_row_hints_stay_quiet_while_queue_modals_are_open() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    let flat = |spans: Vec<Span>| -> String {
        spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    };
    let mut app = test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.state = RunState::Running;
    app.input.set("queued draft".into());
    // Plain running state still advertises the interrupt (#148 keeps it).
    assert!(flat(context_hints(&app)).contains("esc interrupt"));
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

    // Modal flows still work — they just aren't advertised in the dock,
    // and an open modal suppresses the interrupt label since esc then
    // closes the modal rather than the turn.
    assert!(app.queue_selecting(), "queue picker should be open");
    assert!(flat(context_hints(&app)).is_empty());
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    assert!(app.queue_editing(), "queue editor should be open");
    assert!(flat(context_hints(&app)).is_empty());

    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        ))),
        &ctl,
    );
    assert!(app.queue_delete_confirming(), "delete confirm should be open");
    assert!(flat(context_hints(&app)).is_empty());
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
fn native_agent_rail_auto_closes_once_every_task_has_ended() {
    // Issue #80: the native rail is only a live-progress surface. When no
    // subagent is running and none failed, it closes instead of leaving a
    // static summary; an inline selection reopens it.
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
    app.subagents.push(crate::app::SubagentView {
        id: "child-2".into(),
        parent: "dsh-test".into(),
        label: "subagent 2".into(),
        running: false,
        failed: false,
        transcript: crate::transcript::Transcript::new("child-2".into()),
    });

    let frame = dump_frame(&mut app, 100, 24);
    assert!(
        !frame.contains("Agents ·"),
        "all tasks finished → the native rail auto-closes:\n{frame}"
    );

    // An active inline selection keeps the rail visible after the tasks end.
    app.agent_selection = Some("child-1".into());
    let expanded = dump_frame(&mut app, 100, 24);
    assert!(
        expanded.contains("subagent 1"),
        "selection reopens the rail:\n{expanded}"
    );

    // A failed task keeps the summary visible as a failure surface.
    app.agent_selection = None;
    app.subagents[1].failed = true;
    let failed = dump_frame(&mut app, 100, 24);
    assert!(failed.contains("Agents · 2/2"), "{failed}");
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
            url: None,            message: "The agent needs your input.".into(),
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

/// The caret is a buffer cell (a steady reversed block), so it moves
/// atomically with the frame diff and can never be blinked off by a
/// scroll-heavy redraw. Returns the reversed cells of one drawn frame.
fn caret_cells(app: &mut App, width: u16, height: u16) -> Vec<(u16, u16)> {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, app)).expect("draw frame");
    let buf = terminal.backend().buffer();
    let mut cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if buf[(x, y)].modifier.contains(Modifier::REVERSED) {
                cells.push((x, y));
            }
        }
    }
    cells
}

#[test]
fn empty_composer_paints_the_caret_on_the_placeholder() {
    let mut app = test_app();
    app.show_banner = false;
    let cells = caret_cells(&mut app, 80, 20);
    assert_eq!(cells, vec![(3, 16)], "caret right of the ❯ prompt");
}

#[test]
fn composer_caret_follows_the_cursor_end_and_mid_draft() {
    let mut app = test_app();
    app.show_banner = false;
    app.input.set("hello".into());
    app.input.set_cursor_char(5);
    // One past the draft: a reversed blank cell, never a hardware cursor.
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(8, 16)]);
    app.input.set_cursor_char(2);
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(5, 16)]);
}

#[test]
fn composer_caret_sits_on_wide_graphemes() {
    let mut app = test_app();
    app.show_banner = false;
    app.input.set("中".into());
    app.input.set_cursor_char(0);
    // The reversed style rides the grapheme's start cell — terminals paint
    // the full two-column glyph with it, so the block caret covers 中.
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(3, 16)]);
    // Cursor after the wide char: the caret is the blank cell past it.
    app.input.set_cursor_char(1);
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(5, 16)]);
}

/// The hardware-cursor park target (`app.caret_cell`) must always equal the
/// cell the frame painted the soft caret on: `main` parks the hidden
/// hardware cursor there after every draw so IME candidate popups anchor at
/// the caret (issue #66).
#[test]
fn caret_cell_tracks_the_painted_caret_in_the_composer() {
    let mut app = test_app();
    app.show_banner = false;
    // Empty well: caret right of the ❯ prompt.
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(3, 16)]);
    assert_eq!(app.caret_cell, Some((3, 16)));
    // Draft end and mid-draft.
    app.input.set("hello".into());
    app.input.set_cursor_char(5);
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(8, 16)]);
    assert_eq!(app.caret_cell, Some((8, 16)));
    app.input.set_cursor_char(2);
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(5, 16)]);
    assert_eq!(app.caret_cell, Some((5, 16)));
}

#[test]
fn caret_cell_follows_wrapped_rows_and_hides_with_the_caret() {
    let mut app = test_app();
    app.show_banner = false;
    // The well is 76 cells wide at 80x20; 77 chars wrap to a second row.
    app.input.set("a".repeat(77));
    app.input.set_cursor_char(77);
    assert_eq!(caret_cells(&mut app, 80, 20), vec![(4, 17)]);
    assert_eq!(app.caret_cell, Some((4, 17)));
    // While elicitation owns input the composer paints no caret and no
    // park target — the field's own caret takes over: the frame's only
    // reversed cell is the field caret and the park target rides it, never
    // the composer well row (y=16 at 80x20).
    let mut app = test_app();
    app.show_banner = false;
    app.input.set("draft".into());
    app.elicitation_ask = Some(crate::app::ElicitationAskOverlay {
        form: crate::elicitation::ElicitationFormState::new(crate::elicitation::ElicitationForm {
            url: None,            message: "m".into(),
            fields: vec![crate::elicitation::ElicitationField {
                name: "f".into(),
                custom_name: None,
                title: "t".into(),
                description: None,
                required: true,
                secret: false,
                kind: crate::elicitation::ElicitationFieldKind::Text { default: None },
            }],
        }),
        scroll: 0,
        reply: None,
    });
    let cells = caret_cells(&mut app, 80, 20);
    assert_eq!(app.caret_cell, cells.first().copied(), "park rides the field caret");
    assert!(
        cells.iter().all(|(_, y)| *y != 16),
        "composer well shows no caret while elicitation owns input"
    );
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
fn harness_new_tab_displays_the_landing_page_and_preserves_old_transcript() {
    let mut app = live_test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.show_banner = false;
    app.transcript.push_user("old Harness turn".into(), false);

    app.handle(crate::bus::AppEvent::Ctl(crate::bus::CtlEvent::NewSessionRequested), &ctl);

    assert!(app.show_banner);
    assert!(app.transcript.cells.is_empty());
    assert_eq!(app.session_tabs().len(), 2);
    let frame = dump_frame(&mut app, 140, 60);
    assert!(frame.contains("https://martty.sh"), "landing page is visible:\n{frame}");
    assert!(!frame.contains("old Harness turn"), "{frame}");
    // Selecting the first tab uses the same public command path as the TUI.
    app.input.set("/session prev".into());
    app.handle(crate::bus::AppEvent::Term(crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE,
    ))), &ctl);
    let frame = dump_frame(&mut app, 140, 60);
    assert!(frame.contains("old Harness turn"), "{frame}");
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
    assert!(frame.contains("martty"), "session facts remain:\n{frame}");
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
        !frame.contains("martty "),
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
fn draw_records_the_pet_anchor_for_the_kitty_reconciler() {
    let mut app = test_app();
    app.show_banner = false;
    app.pet_visible = true;
    app.pet_pixels = true;
    // One draw: the anchor is recorded with the idle flag; main() reconciles
    // the kitty placement against exactly this value.
    let _ = dump_frame(&mut app, 100, 34);
    let (rect, working) = app.pet_want.expect("pet anchor recorded");
    assert_eq!(
        Some(rect),
        pet_rect(Rect::new(0, 0, 100, 34), &app),
        "anchor matches the pet geometry at this size"
    );
    assert!(!working, "idle app draws the idle sprite");

    app.state = RunState::Running;
    let _ = dump_frame(&mut app, 100, 34);
    assert!(app.pet_want.expect("pet anchor").1, "working flag follows");

    // Char-whale fallback (no kitty protocol): nothing recorded, the kitty
    // reconciler stays silent.
    app.pet_pixels = false;
    let _ = dump_frame(&mut app, 100, 34);
    assert_eq!(app.pet_want, None, "no kitty pixels, no anchor");
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
        offset: 0,
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
        offset: 0,
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
        offset: 0,
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
        offset: 0,
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
        offset: 0,
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

/// The ↥ jump flash (issue #103): for ~5 s after a jump the jumped
/// prompt's bubble rows are highlighted like a picker's selected row —
/// chip background wash and the text in the brand tone; after the window
/// the next tick clears it and the next frame restores the normal bubble.
#[test]
fn prompt_jump_flash_washes_the_jumped_prompt_then_restores() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.transcript.push_user("alpha prompt".into(), false);
    app.transcript.push_notice(
        crate::transcript::NoticeLevel::Info,
        "filler row".into(),
    );
    app.transcript.push_user("beta prompt".into(), false);
    let beta_cell = app
        .transcript
        .cells
        .iter()
        .rposition(|cell| matches!(cell.kind, crate::transcript::CellKind::User { .. }))
        .expect("second user prompt");

    // Locate the first cell of an ASCII needle in the rendered buffer.
    let text_cell = |buf: &ratatui::buffer::Buffer, needle: &str| -> Option<(u16, u16)> {
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect();
            if let Some(x) = row.find(needle) {
                return Some((x as u16, y as u16));
            }
        }
        None
    };

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = app.theme;
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let (bx, by) = text_cell(terminal.backend().buffer(), "beta prompt")
        .expect("beta prompt rendered");

    // Frame 1, no flash: the bubble keeps its normal colors.
    let buf = terminal.backend().buffer().clone();
    assert_eq!(buf[(bx, by)].bg, theme.bubble_bg, "idle bubble background");
    assert_eq!(buf[(bx, by)].fg, theme.bubble_fg, "idle bubble text");

    // Arm the flash for the second prompt and redraw: its rows carry the
    // chip wash with the text in the brand tone, while the other prompt
    // stays untouched.
    app.prompt_flash = Some((
        beta_cell,
        std::time::Instant::now() + crate::app::PROMPT_FLASH_TTL,
    ));
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    assert_eq!(buf[(bx, by)].bg, theme.chip_bg, "jumped prompt washed");
    assert_eq!(buf[(bx, by)].fg, theme.brand, "jumped prompt text brand");
    let (ax, ay) = text_cell(terminal.backend().buffer(), "alpha prompt")
        .expect("alpha prompt rendered");
    assert_ne!(
        buf[(ax, ay)].bg, theme.chip_bg,
        "the other prompt keeps its background"
    );
    assert_ne!(
        buf[(ax, ay)].fg, theme.brand,
        "the other prompt keeps its text color"
    );

    // Expire the flash: the tick clears it and the next frame restores the
    // ordinary bubble look.
    app.prompt_flash = Some((
        beta_cell,
        std::time::Instant::now() - crate::app::PROMPT_FLASH_TTL,
    ));
    app.tick();
    assert!(app.prompt_flash.is_none(), "tick cleared the expired flash");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    assert_eq!(
        buf[(bx, by)].bg, theme.bubble_bg,
        "flash restored the normal bubble background"
    );
    assert_eq!(
        buf[(bx, by)].fg, theme.bubble_fg,
        "flash restored the normal bubble text"
    );
}

#[test]
fn model_picker_marks_the_streamed_model_when_it_differs_from_config() {
    use crate::app::{Picker, PickerItem, PickerKind};
    let mut app = test_app();
    app.show_banner = false;
    // A turn streamed on `deepseek-v4-pro` while the config still names the
    // fallback: the picker must mark the running model (issue #102).
    app.cfg.model = "deepseek-flash".into();
    app.transcript.last_model = Some("deepseek-v4-pro".into());
    app.picker = Some(Picker {
        offset: 0,
        kind: PickerKind::Model,
        title: " model ".into(),
        sel: 1,
        items: vec![
            PickerItem {
                id: "deepseek-flash".into(),
                label: "deepseek-flash".into(),
                meta: String::new(),
                provider: None,
            },
            PickerItem {
                id: "deepseek-v4-pro".into(),
                label: "deepseek-v4-pro".into(),
                meta: String::new(),
                provider: None,
            },
        ],
    });

    let frame = dump_frame(&mut app, 100, 24);
    let marked: Vec<&str> = frame.lines().filter(|line| line.contains("✓")).collect();
    assert_eq!(marked.len(), 1, "one current model:\n{frame}");
    assert!(
        marked[0].contains("deepseek-v4-pro"),
        "the running model is marked: {}\n{frame}",
        marked[0]
    );
    assert!(
        marked[0].contains("▸"),
        "highlight follows the running model: {}",
        marked[0]
    );
}

#[test]
fn effort_picker_marks_and_highlights_the_current_effort() {
    use crate::app::{Picker, PickerItem, PickerKind};
    let mut app = test_app();
    app.show_banner = false;
    app.modes.effort = Some("max".into());
    app.picker = Some(Picker {
        offset: 0,
        kind: PickerKind::Effort,
        title: " reasoning effort · enter select · esc close ".into(),
        sel: 2,
        items: ["off", "high", "max"]
            .iter()
            .map(|e| PickerItem {
                id: e.to_string(),
                label: e.to_string(),
                meta: String::new(),
                provider: None,
            })
            .collect(),
    });

    let frame = dump_frame(&mut app, 100, 24);
    let max_row = frame
        .lines()
        .find(|l| l.contains("max"))
        .expect("max row listed");
    assert!(max_row.contains("max ✓"), "current effort marked: {max_row}");
    assert!(
        max_row.contains("▸"),
        "highlight on the active effort: {max_row}"
    );
}

#[test]
fn slash_effort_menu_marks_and_highlights_the_active_effort() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.show_banner = false;
    app.modes.effort = Some("max".into());
    app.input.set("/effort".into());
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    // The typed `/effort ` option menu opens on the effort in effect.
    let frame = dump_frame(&mut app, 100, 30);
    let marked: Vec<&str> = frame.lines().filter(|line| line.contains('✓')).collect();
    assert_eq!(marked.len(), 1, "only the active effort is marked:\n{frame}");
    assert!(
        marked[0].contains("max") && marked[0].contains("▸"),
        "highlight and ✓ sit on the active effort: {}\n{frame}",
        marked[0]
    );
}

#[test]
fn slash_model_menu_marks_the_running_model() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.show_banner = false;
    app.cfg.model = "deepseek-flash".into();
    app.transcript.last_model = Some("deepseek-v4-pro".into());
    app.input.set("/model".into());
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::NONE,
        ))),
        &ctl,
    );

    // The inline `/model ` list leads with the running model, marked ✓ and
    // highlighted, instead of the config default.
    let frame = dump_frame(&mut app, 100, 30);
    let marked: Vec<&str> = frame.lines().filter(|line| line.contains('✓')).collect();
    assert_eq!(marked.len(), 1, "only the running model is marked:\n{frame}");
    assert!(
        marked[0].contains("deepseek-v4-pro") && marked[0].contains("▸"),
        "highlight and ✓ sit on the running model: {}\n{frame}",
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
    assert!(frame.contains("↓"), "scroll indicator missing:\n{frame}");
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
            url: None,            message: "The agent needs your input.".into(),
            fields: vec![ElicitationField {
                name: "question_0".into(),
                custom_name: Some("question_0_custom".into()),
                title: "Target".into(),
                description: Some("Where should this run?".into()),
                required: true,
                secret: false,
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
            url: None,        message: "The agent needs your input.".into(),
        fields: vec![ElicitationField {
            name: "answer".into(),
            custom_name: None,
            title: "Answer".into(),
            description: None,
            required: true,
            secret: false,
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
                .map(|x| buffer[(x as u16, *y)].symbol())
                .collect::<String>()
                .contains("overlay answer")
        })
        .expect("elicitation text field");
    let answer_line = (0..100)
        .map(|x| buffer[(x, answer_row)].symbol())
        .collect::<String>();
    let answer_byte = answer_line.find("overlay answer").expect("answer text");
    let answer_start = answer_line[..answer_byte].width() as u16;
    // The caret is a buffer cell: a reversed block one past the field text
    // (the cursor sits at the end of "overlay answer").
    let caret_x = answer_start + "overlay answer".width() as u16;
    assert!(
        buffer[(caret_x, answer_row)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "elicitation field paints its own caret cell"
    );
    assert_eq!(
        app.caret_cell,
        Some((caret_x, answer_row)),
        "hardware cursor parks on the elicitation field caret"
    );
    // The composer draft keeps no caret while the overlay owns input.
    let composer_row = (0..30)
        .find(|y| {
            (0..100)
                .map(|x| buffer[(x as u16, *y)].symbol())
                .collect::<String>()
                .contains("composer draft")
        })
        .expect("composer draft visible behind the overlay");
    let draft_line = (0..100)
        .map(|x| buffer[(x, composer_row)].symbol())
        .collect::<String>();
    let draft_byte = draft_line.find("composer draft").expect("draft text");
    let draft_start = draft_line[..draft_byte].width() as u16;
    let draft_end = draft_start + "composer draft".width() as u16;
    assert!(
        !(0..100).any(|x| {
            x >= draft_start && x <= draft_end && buffer[(x, composer_row)].modifier.contains(ratatui::style::Modifier::REVERSED)
        }),
        "composer shows no caret while elicitation owns input"
    );
}

fn box_line(line: &str) -> &str {
    let trimmed = line.trim_start_matches(' ');
    trimmed
        .strip_prefix('│')
        .and_then(|rest| rest.strip_suffix('│'))
        .unwrap_or(trimmed)
}

fn grouped_select_test_app(count: usize, selected: usize) -> App {
    let mut app = test_app();
    let options: Vec<_> = (0..count).map(|index| serde_json::json!({
        "value": format!("item-{index:02}"), "label": format!("Candidate {index:02}"),
        "description": "An ACP agent",
        "group": if index < count / 2 { "Downloaded" } else { "Not downloaded" },
    })).collect();
    let mut select: crate::app::SelectOverlay = serde_json::from_value(serde_json::json!({
        "id": "catalog", "title": "Harnesses", "value": format!("item-{selected:02}"),
        "searchable": true, "options": options,
    })).expect("select snapshot");
    select.sel = selected;
    app.select_overlay = Some(select);
    app
}

fn select_only_frame(app: &App, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| draw_plugin_select(frame, app, frame.area())).expect("select frame");
    let buffer = terminal.backend().buffer();
    (0..height).map(|y| (0..width).map(|x| buffer[(x, y)].symbol())
        .collect::<String>()).collect::<Vec<_>>().join("\n")
}

#[test]
fn grouped_select_overlay_separates_groups_without_selectable_heading_rows() {
    let app = grouped_select_test_app(4, 0);
    let frame = select_only_frame(&app, 70, 22);
    assert_eq!(frame.matches("Downloaded").count(), 1, "one first-group heading:\n{frame}");
    assert_eq!(frame.matches("Not downloaded").count(), 1, "one second-group heading:\n{frame}");
    let rows: Vec<_> = frame.lines().collect();
    for label in ["Downloaded", "Not downloaded"] {
        let heading = rows.iter().find(|line| line.contains(label)).expect("heading");
        assert!(heading.contains('─'), "heading has a divider:\n{frame}");
        assert!(!heading.contains('○') && !heading.contains('●'), "heading is not an option:\n{frame}");
    }
    assert_eq!(frame.matches('○').count(), 3, "only real unselected options:\n{frame}");
    assert_eq!(frame.matches('●').count(), 1, "one real selected option:\n{frame}");
}

#[test]
fn grouped_select_overlay_scrolls_with_the_selection_and_repeats_visible_group_title() {
    let mut app = grouped_select_test_app(40, 39);
    let frame = select_only_frame(&app, 70, 12);
    assert!(frame.contains("Not downloaded"), "window repeats its group heading:\n{frame}");
    assert!(frame.contains("▸ ● Candidate 39"), "selected row stays visible:\n{frame}");
    assert!(!frame.contains("Downloaded"), "offscreen group heading is not retained:\n{frame}");
    app.select_overlay.as_mut().unwrap().query = "candidate 39".into();
    let frame = select_only_frame(&app, 70, 12);
    assert!(frame.contains("Not downloaded"), "filtered result keeps its heading:\n{frame}");
    assert!(!frame.contains("Candidate 38"), "nonmatches excluded:\n{frame}");
    app.select_overlay.as_mut().unwrap().query = "no such candidate".into();
    let frame = select_only_frame(&app, 70, 12);
    assert!(frame.contains("No matches"), "empty filter state:\n{frame}");
    assert!(!frame.contains("Not downloaded"), "no empty group headings:\n{frame}");
}

#[test]
fn grouped_select_overlay_small_terminals_prioritize_the_actual_choice() {
    let mut app = grouped_select_test_app(40, 39);
    // The overlay can omit decorative headings when only one body row remains.
    let frame = select_only_frame(&app, 34, 7);
    assert!(frame.contains("▸ ● Candidate 39"), "heading cannot replace the selected row:\n{frame}");
    assert_eq!(frame.lines().count(), 7);
    app.select_overlay.as_mut().unwrap().searchable = false;
    let frame = select_only_frame(&app, 22, 12);
    assert!(frame.contains("Not downloa…"), "long heading ellipsized within the narrow panel:\n{frame}");
    assert!(frame.lines().all(|line| line.chars().count() == 22), "all rows stay inside the screen:\n{frame}");
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
        searchable: false,
        query: String::new(),
        value: "long".into(),
        sel: 0,
        options: vec![
            SelectOption {
                deletable: false,
                value: "long".into(),
                disabled: false,
                label: long_label,
                description: Some(long_desc),
                group: None,
            },
            SelectOption {
                deletable: false,
                value: "short".into(),
                disabled: false,
                label: "Short".into(),
                description: Some("second option".into()),
                group: None,
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
        searchable: false,
        query: String::new(),
        value: "v".into(),
        sel: 0,
        options: vec![SelectOption {
            deletable: false,
            disabled: false,
            value: "v".into(),
            label: "x".repeat(400),
            description: None,
            group: None,
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
    let spans = compact_node_spans(&node, &theme, crate::markdown::ToneMode::Single, 60, '⣋').expect("generic spans");
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

    let soft = compact_node_spans(&node, &app.theme, crate::markdown::ToneMode::Single, 60, '⠋').expect("generic spans");
    let bright = compact_node_spans(&node, &app.theme, crate::markdown::ToneMode::Single, 60, '⠴').expect("generic spans");
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

#[test]
fn empty_plugin_dock_snapshot_hides_the_agents_row_after_tasks_end() {
    // Issue #80 end-to-end shape: all subagent views are finished and the
    // Client panel published an EMPTY navigation.dock snapshot (the
    // auto-close payload). The native rail must not take over as a
    // fallback while the plugin owns the slot, so the row disappears.
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
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::SLOTS_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "slot": "conversation.navigation.dock",
                "rev": 7,
                "nodes": [],
            }),
        },
        &ctl,
    );

    let frame = dump_frame(&mut app, 100, 24);
    assert!(
        !frame.contains("Agents"),
        "plugin-owned empty dock must hide the row, not fall back to the native rail:\n{frame}"
    );
    assert!(
        !frame.contains("subagent 1"),
        "no native rail entries either:\n{frame}"
    );
}

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

/// Issue #147: the composer card's whole top border is the resize handle
/// — the ⛶ button is gone. Pressing it arms a drag that pins the well
/// height, clamped to [auto default, 5/8 of the main area]; a plain click
/// (Down+Up, no Drag) changes nothing.
#[test]
fn composer_top_border_drags_to_resize_and_clamps() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");

    let area = ratatui::layout::Rect::new(0, 0, 100, 34);
    let auto = resolved_composer_height(area, &app);
    let cap_y = app.composer_box.expect("records the composer box").y;
    assert!(cap_y < app.input_area.y, "the border rides above the well");
    // The ⛶ glyph is gone — every cap-row cell is plain border or title.
    let buf = terminal.backend().buffer().clone();
    for x in 0..100u16 {
        assert_ne!(buf[(x, cap_y)].symbol(), "⛶", "maximize glyph gone at {x}");
    }

    // A plain click arms then releases the resize without moving the well.
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 50, cap_y), &ctl);
    assert_eq!(app.composer_resize, Some((cap_y, auto)));
    assert_eq!(app.composer_rows, None, "a press alone pins nothing");
    app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 50, cap_y), &ctl);
    assert!(app.composer_resize.is_none());
    assert_eq!(app.composer_rows, None, "a click without drag keeps auto");

    // Dragging up grows the well by exactly the pointer's travel.
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 50, cap_y), &ctl);
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, cap_y - 9), &ctl);
    assert_eq!(app.composer_rows, Some(auto + 9));
    assert_eq!(resolved_composer_height(area, &app), auto + 9);
    // Past the ceiling clamps at 5/8 of the main area — the conversation
    // always keeps at least its remaining 3/8.
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, 0), &ctl);
    assert_eq!(app.composer_rows, Some(34 * 5 / 8));
    // Dragging below the grab clamps at the auto default, never under it.
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, 33), &ctl);
    assert_eq!(app.composer_rows, Some(auto));
    app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 50, 33), &ctl);
    assert!(app.composer_resize.is_none());
    assert_eq!(resolved_composer_height(area, &app), auto);
}

/// Hovering the top border lights the whole cap line — the resize
/// affordance; leaving restores the quiet border tone. The ↥ glyph's
/// cells hover on their own instead (jump button, issue #103).
#[test]
fn composer_border_hover_highlights_the_cap_row() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    let theme = app.theme;
    let cap_y = app.composer_box.expect("composer box").y;
    assert_eq!(buf[(0, cap_y)].symbol(), "╭", "cap row of the card");
    assert_eq!(buf[(99, cap_y)].symbol(), "╮");
    assert_eq!(buf[(0, cap_y)].fg, theme.border, "idle border tone");
    let dash_x = (0..100u16)
        .find(|&x| buf[(x, cap_y)].symbol() == "─")
        .expect("cap row keeps border dashes");

    // Hover: dashes and corners brighten to the strongest foreground.
    app.handle_mouse(mouse(MouseEventKind::Moved, dash_x, cap_y), &ctl);
    assert!(app.hover_composer_border);
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    assert_eq!(buf[(0, cap_y)].fg, theme.fg, "left corner brightens");
    assert_eq!(buf[(dash_x, cap_y)].fg, theme.fg, "dash brightens");
    assert_eq!(buf[(99, cap_y)].fg, theme.fg, "right corner brightens");
    assert_eq!(buf[(50, cap_y + 1)].bg, theme.panel, "the well below is untouched");

    // The pointer leaves: quiet border again.
    app.handle_mouse(mouse(MouseEventKind::Moved, 5, 5), &ctl);
    assert!(!app.hover_composer_border);
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    assert_eq!(buf[(dash_x, cap_y)].fg, theme.border, "idle tone restored");
    assert_eq!(buf[(0, cap_y)].fg, theme.border);
}

/// The border's hit-test never reaches the text: grabbing it must not
/// move the caret.
#[test]
fn composer_border_drag_does_not_place_the_caret() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.input.set("hello".into());
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let cap_y = app.composer_box.expect("composer box").y;
    let before = app.input.cursor_char();
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 50, cap_y), &ctl);
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, cap_y - 2), &ctl);
    assert_eq!(
        app.input.cursor_char(),
        before,
        "border drag resizes the well, never the caret"
    );
    assert!(app.composer_resize.is_some());
    assert!(app.composer_rows.is_some());
}

/// A full draft cannot reach the border: with the well completely filled,
/// the cap row still takes the drag and carries no button glyph.
#[test]
fn composer_border_survives_a_full_draft() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    // Fill the well: auto height 4 rows, well 3 rows × ~96 columns.
    app.input.set(" word".repeat(60));
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let cap_y = app.composer_box.expect("composer box").y;
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 50, cap_y), &ctl);
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, cap_y - 5), &ctl);
    assert!(
        app.composer_rows.is_some(),
        "full draft: the border still resizes"
    );
    let buf = terminal.backend().buffer().clone();
    for x in 0..100u16 {
        assert_ne!(buf[(x, app.input_area.y)].symbol(), "↥", "well rows carry no button glyph");
    }
}

/// ^↑/^↓ step the composer for non-mouse terminals (issue #147): the
/// first press pins the resolved height, steps clamp inside the frame's
/// legal range, and shrinking on the floor releases back to auto.
#[test]
fn composer_keyboard_steps_clamp_and_release() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let area = ratatui::layout::Rect::new(0, 0, 100, 34);
    let auto = resolved_composer_height(area, &app);
    assert_eq!(app.composer_bounds, (auto, 34 * 5 / 8));

    app.resize_composer_step(1);
    assert_eq!(app.composer_rows, Some(auto + 1));
    app.resize_composer_step(100);
    assert_eq!(app.composer_rows, Some(34 * 5 / 8), "grow clamps at the ceiling");
    app.resize_composer_step(-100);
    assert_eq!(app.composer_rows, Some(auto), "shrink clamps at the floor");
    app.resize_composer_step(-1);
    assert_eq!(app.composer_rows, None, "shrinking on the floor releases to auto");
    assert_eq!(resolved_composer_height(area, &app), auto);
}

/// A pinned composer height re-clamps into the legal range when the
/// terminal shrinks — the stored value survives, the render never strands.
#[test]
fn pinned_composer_height_reclamps_on_terminal_resize() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    app.composer_rows = Some(34 * 5 / 8);

    // Shrink to 16 rows: bounds become (4, 10); the pinned 21 renders 10.
    let backend = TestBackend::new(100, 16);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let small = ratatui::layout::Rect::new(0, 0, 100, 16);
    assert_eq!(app.composer_bounds, (4, 10));
    assert_eq!(resolved_composer_height(small, &app), 10);
    assert_eq!(app.composer_rows, Some(34 * 5 / 8), "the pin itself survives");

    // Grow back: the pin renders at its full height again.
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let area = ratatui::layout::Rect::new(0, 0, 100, 34);
    assert_eq!(resolved_composer_height(area, &app), 34 * 5 / 8);
}

/// The ↥ user-prompt jump button (issue #103) keeps the cap row's
/// top-right corner now that ⛶ is gone — one cell inside the corner — and
/// hovers the same way: quiet caption tone idle, brightest foreground on
/// hover.
#[test]
fn prompt_jump_button_sits_at_the_corner_and_hovers() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    let theme = app.theme;
    let composer_box = app.composer_box.expect("composer box");
    let jump = app.prompt_jump_btn.expect("jump rect");

    // Geometry: (space) ↥ ╮ — the glyph one cell inside the corner.
    assert_eq!(jump.y, composer_box.y, "the ↥ rides the cap row");
    assert_eq!(buf[(jump.x + 1, jump.y)].symbol(), "↥");
    assert_eq!(buf[(jump.x + 2, jump.y)].symbol(), "╮", "↥ sits right next to the corner");
    assert_eq!(buf[(jump.x, jump.y)].symbol(), " ", "a space before ↥");
    assert_eq!(buf[(jump.x + 1, jump.y)].fg, theme.caption, "idle tone");

    // Hover brightens the ↥ glyph.
    app.handle_mouse(mouse(MouseEventKind::Moved, jump.x + 1, jump.y), &ctl);
    assert!(app.hover_prompt_jump_btn);
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    assert_eq!(
        buf[(jump.x + 1, jump.y)].fg,
        theme.fg,
        "hover brightens the ↥ glyph"
    );
    // Leaving restores the quiet glyph.
    app.handle_mouse(mouse(MouseEventKind::Moved, 5, 5), &ctl);
    assert!(!app.hover_prompt_jump_btn);
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    assert_eq!(
        buf[(jump.x + 1, jump.y)].fg,
        theme.caption,
        "idle tone restored when the pointer leaves"
    );
}

/// While the drag is armed the cap line stays bright even if the pointer
/// row leaves the border — mid-drag frames paint the highlight, not just
/// the hover state.
#[test]
fn composer_border_highlight_paints_while_dragging() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let cap_y = app.composer_box.expect("composer box").y;
    let theme = app.theme;

    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 50, cap_y), &ctl);
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, cap_y - 3), &ctl);
    assert!(app.composer_resize.is_some());
    // No hover flag during a drag — the resize state alone lights the cap.
    app.hover_composer_border = false;
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame mid-drag");
    let drag_cap_y = app.composer_box.expect("composer box").y;
    assert_eq!(drag_cap_y, cap_y - 3, "the border rode up with the well");
    let buf = terminal.backend().buffer().clone();
    let dash_x = (0..100u16)
        .find(|&x| buf[(x, drag_cap_y)].symbol() == "─")
        .expect("cap row keeps border dashes");
    assert_eq!(buf[(dash_x, drag_cap_y)].fg, theme.fg, "cap stays bright mid-drag");
    assert_eq!(buf[(0, drag_cap_y)].fg, theme.fg);
    assert_eq!(
        buf[(0, cap_y)].symbol(),
        "│",
        "the old cap row is inside the taller well now"
    );
}

/// Dragging back to exactly the armed height resolves to the same pin —
/// the setter short-circuits instead of writing an identical value.
#[test]
fn composer_border_redrag_to_the_same_height_is_a_noop() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let cap_y = app.composer_box.expect("composer box").y;
    let auto = resolved_composer_height(Rect::new(0, 0, 100, 34), &app);

    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 50, cap_y), &ctl);
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, cap_y - 6), &ctl);
    app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 50, cap_y - 6), &ctl);
    assert_eq!(app.composer_rows, Some(auto + 6));

    // A frame bakes the pin into composer_h; re-grabbing the moved border
    // then dragging zero rows resolves to the armed height — a no-op.
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let cap_y = app.composer_box.expect("composer box").y;
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 50, cap_y), &ctl);
    app.needs_redraw = false;
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, cap_y), &ctl);
    assert_eq!(app.composer_rows, Some(auto + 6));
    assert!(!app.needs_redraw, "same-height drag stays a no-op");
}

/// Below the boxed layout's floor the composer falls back to the
/// borderless inner view: no drag handle exists there, and a stale hover
/// flag cannot make the chrome painter wander outside the frame.
#[test]
fn composer_chrome_survives_the_borderless_fallback() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    let backend = TestBackend::new(100, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    app.hover_composer_border = true;
    terminal.draw(|f| draw(f, &mut app)).expect("draw fallback frame");
    assert!(app.composer_box.is_none(), "borderless fallback has no drag handle");
    assert!(app.prompt_jump_btn.is_some(), "↥ still lands on the inner cap");
}

/// `layout_jump_btn` yields no rect when the cap row is too narrow for the
/// glyph plus its margins — the painter must not scribble over the well.
#[test]
fn composer_cap_layout_hides_the_jump_glyph_on_tiny_widths() {
    let mut app = test_app();
    layout_jump_btn(&mut app, Rect::new(0, 0, 3, 1));
    assert!(app.prompt_jump_btn.is_none());
    layout_jump_btn(&mut app, Rect::new(0, 0, 4, 1));
    assert_eq!(app.prompt_jump_btn, Some(Rect::new(1, 0, 2, 1)));
}

/// `paint_composer_chrome` writes nothing when the ↥ rect is absent or
/// points outside the frame — stale rects cannot scribble past the border.
#[test]
fn composer_chrome_tolerates_a_missing_or_stray_jump_rect() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|f| {
            app.prompt_jump_btn = Some(Rect::new(95, 40, 2, 1)); // below the frame
            paint_composer_chrome(f, &mut app);
            app.prompt_jump_btn = None;
            paint_composer_chrome(f, &mut app);
        })
        .expect("paint with stray and absent jump rects");
}

/// Regression: a full queue shelf plus an expanded draft used to push the
/// composer past the frame; `draw_input` then wrote the prompt gutter at
/// y == frame height and ratatui's `Buffer::index_of` panicked
/// (`index outside of buffer`). The chrome stack must compress instead.
#[test]
fn overflowing_chrome_stack_keeps_the_composer_inside_the_frame() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.queued = 12;
    app.input
        .set((0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"));
    let backend = TestBackend::new(80, 22);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    assert!(
        app.input_area.height > 0,
        "the composer must keep at least one row: {:?}",
        app.input_area,
    );
    assert!(
        app.input_area.bottom() <= 22,
        "composer escaped the 22-row frame: {:?}",
        app.input_area,
    );
    assert!(app.chat_view.area.bottom() <= 22);
}

/// The 20-row / 8-queued configuration sat exactly at the old boundary; it
/// must keep fitting (and must not panic) without extra compression.
#[test]
fn queue_shelf_and_expanded_draft_at_the_old_boundary_still_fit() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.queued = 8;
    app.input
        .set((0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"));
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    assert!(
        app.input_area.bottom() <= 20,
        "composer escaped the 20-row frame: {:?}",
        app.input_area,
    );
}

/// A roomy terminal must keep a real conversation area: the compression pass
/// only engages when the chrome stack actually overflows.
#[test]
fn roomy_chrome_stack_keeps_the_conversation_visible() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.queued = 3;
    app.input.set("one\ntwo\nthree\nfour\nfive".into());
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    assert!(
        app.chat_view.area.height > 0,
        "chat collapsed on a terminal with room to spare",
    );
    assert!(app.input_area.bottom() <= 30);
}

#[test]
fn harness_name_precedes_model_without_registry_image() {
    let mut app = live_test_app();
    app.session_bound = true;
    app.server_info = Some("Custom Harness".into());
    app.selected_model = Some("example-model".into());
    let text: String = status_right(&app).iter().map(|span| span.content.as_ref()).collect();
    assert!(text.contains("Custom Harness · example-model"), "{text}");
}

#[test]
fn harness_icon_is_session_scoped_and_falls_back_on_non_pixel_terminals() {
    let mut app = live_test_app();
    app.session_bound = true;
    app.session_id = "alpha".into();
    app.server_info = Some("Alpha ACP".into());
    app.selected_model = Some("example-model".into());
    let snapshot = crate::slots::parse_snapshot(&serde_json::json!({
        "protocol": 0, "slot": "conversation.harness", "nodes": [{
            "id": "harness:alpha", "kind": "image", "name": "Alpha Harness",
            "mime": "image/png", "dataBase64": crate::pet::base64(crate::pet::LIANG_IDLE_PNG)
        }]
    })).unwrap().unwrap();
    app.harness_badge = crate::harness_badge::parse(&snapshot);
    assert!(status_right(&app).iter().any(|s| s.content == "Alpha Harness · "));
    app.pet_pixels = true;
    // Use a small valid PNG; the painter rejects oversized Registry images.
    let mut png = Vec::new();
    image::DynamicImage::new_rgba8(16, 16).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let snapshot = crate::slots::parse_snapshot(&serde_json::json!({
        "protocol": 0, "slot": "conversation.harness", "nodes": [{
            "id": "harness:alpha", "kind": "image", "name": "Alpha Harness",
            "mime": "image/png", "dataBase64": crate::pet::base64(&png)
        }]
    })).unwrap().unwrap();
    app.harness_badge = crate::harness_badge::parse(&snapshot);
    let line = meta_line(&app, 100);
    layout_harness_icon(&mut app, &line, Rect::new(1, 20, 100, 1));
    assert!(app.harness_thumb.is_some());
    assert_eq!(app.harness_thumb.as_ref().unwrap().rect.height, 1);
    let mut pixel_terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    pixel_terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let rect = app.harness_thumb.as_ref().unwrap().rect;
    assert_eq!(pixel_terminal.backend().buffer()[(rect.x, rect.y)].symbol(), "\u{2007}");
    app.session_id = "beta".into();
    app.server_info = Some("Beta Harness".into());
    let text: String = status_right(&app).iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("Beta Harness · example-model"));
    assert!(!text.contains(HARNESS_IMAGE_SPACE));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(app.harness_thumb.is_none());
}

/// #145: the OSC sequence that ties the terminal's *default* background to
/// the canvas color. Rgb canvases emit `OSC 11 ; #rrggbb`; `Reset` (kitty
/// backdrop, cells intentionally left Reset) and named colors restore the
/// profile default via `OSC 111`.
#[test]
fn canvas_bg_sequence_bytes() {
    assert_eq!(
        canvas_bg_sequence(ratatui::style::Color::Rgb(0x0f, 0x11, 0x15)),
        "\x1b]11;#0f1115\x1b\\"
    );
    assert_eq!(
        canvas_bg_sequence(ratatui::style::Color::Reset),
        "\x1b]111\x1b\\"
    );
    assert_eq!(
        canvas_bg_sequence(ratatui::style::Color::Black),
        "\x1b]111\x1b\\"
    );
    assert_eq!(OSC_BG_RESET, "\x1b]111\x1b\\");
}

/// #145: every cell of every frame must carry a painted background. A row
/// left at `Color::Reset` is exactly the "gray band" the terminal fills
/// with its own default bg — sweep heights, widths, themes, banner and the
/// composer-dock status row on/off so no layout path can skip a row again.
#[test]
fn every_frame_cell_has_a_painted_background() {
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;

    let dock_slot: crate::slots::SlotSnapshot = serde_json::from_value(serde_json::json!({
        "protocol": 0,
        "slot": "conversation.composer.dock",
        "rev": 1,
        "nodes": [
            { "id": "stats:counts", "kind": "generic", "title": "1 turn · 3 steps", "body": "" }
        ]
    }))
    .expect("dock slot snapshot");

    for theme in [Theme::dark(), Theme::light()] {
        for &(w, h) in &[
            (20u16, 4u16),   // below the drawable minimum: pre-fill still paints
            (24, 6),
            (40, 10),
            (80, 24),
            (100, 33),
            (120, 50),
        ] {
            for banner in [true, false] {
                for dock in [false, true] {
                    let mut app = test_app();
                    app.theme = theme;
                    app.show_banner = banner;
                    if dock {
                        app.slot_snapshots
                            .insert("conversation.composer.dock".into(), dock_slot.clone());
                    }
                    let mut terminal =
                        Terminal::new(TestBackend::new(w, h)).expect("test terminal");
                    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
                    let buf = terminal.backend().buffer();
                    for y in 0..h {
                        for x in 0..w {
                            assert_ne!(
                                buf[(x, y)].bg,
                                Color::Reset,
                                "unpainted cell ({x},{y}) at {w}x{h} theme={:?} banner={banner} dock={dock}",
                                theme.mode
                            );
                        }
                    }
                }
            }
        }
    }
}

/// #149 regression: with colour output disabled, `CanvasBgSync` must not
/// write any OSC sequence — the terminal's default fg draws on its default
/// bg, and forcing a canvas bg (e.g. light-theme white) blanks the window.
#[test]
fn canvas_bg_sync_disabled_writes_nothing() {
    let mut sync = CanvasBgSync::new(false);
    let mut out = Vec::new();
    sync.sync(&mut out, ratatui::style::Color::Rgb(0xff, 0xff, 0xff))
        .expect("sync");
    sync.sync(&mut out, ratatui::style::Color::Reset).expect("sync");
    assert!(out.is_empty(), "disabled sync must emit nothing");
}

#[test]
fn canvas_bg_sync_emits_once_and_recovers_on_change() {
    let mut sync = CanvasBgSync::new(true);
    let mut out = Vec::new();
    sync.sync(&mut out, ratatui::style::Color::Rgb(0x0f, 0x11, 0x15))
        .expect("sync");
    assert_eq!(out, b"\x1b]11;#0f1115\x1b\\");
    out.clear();
    // Unchanged canvas colour: no re-emit.
    sync.sync(&mut out, ratatui::style::Color::Rgb(0x0f, 0x11, 0x15))
        .expect("sync");
    assert!(out.is_empty(), "unchanged colour must not re-emit");
    // Change to Reset (kitty backdrop): emits the profile restore.
    sync.sync(&mut out, ratatui::style::Color::Reset).expect("sync");
    assert_eq!(out, b"\x1b]111\x1b\\");
}

/// Mirrors crossterm `Colored::ansi_color_disabled`: only a *non-empty*
/// NO_COLOR suppresses colour output.
#[test]
fn ansi_color_disabled_matches_crossterm_semantics() {
    let prev = std::env::var_os("NO_COLOR");
    std::env::set_var("NO_COLOR", "1");
    assert!(ansi_color_disabled());
    std::env::set_var("NO_COLOR", "anything");
    assert!(ansi_color_disabled());
    std::env::set_var("NO_COLOR", "");
    assert!(!ansi_color_disabled());
    match prev {
        Some(v) => std::env::set_var("NO_COLOR", v),
        None => std::env::remove_var("NO_COLOR"),
    }
}

fn url_or_secret_form(url: Option<String>, secret_value: Option<&str>) -> crate::elicitation::ElicitationFormState {
    let fields = match secret_value {
        Some(_) => vec![crate::elicitation::ElicitationField {
            name: "api-key".into(),
            custom_name: None,
            title: "API key".into(),
            description: None,
            required: true,
            secret: true,
            kind: crate::elicitation::ElicitationFieldKind::Text { default: None },
        }],
        None => Vec::new(),
    };
    let mut form = crate::elicitation::ElicitationFormState::new(crate::elicitation::ElicitationForm {
        message: "sign in".into(),
        url,
        fields,
    });
    if let (Some(value), Some(field)) = (secret_value, form.fields.first_mut()) {
        field.input.buf = value.to_string();
    }
    form
}

#[test]
fn auth_form_masks_secret_values_as_bullets() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    app.auth_form = Some(crate::app::AuthFormOverlay {
        method_id: "api-key:deepseek".into(),
        form: url_or_secret_form(None, Some("sk-live-secret")),
    });
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer();
    let text: String = (0..30)
        .flat_map(|y| {
            (0..100)
                .map(move |x| buf[(x, y)].symbol().to_string())
                .chain(std::iter::once("\n".into()))
        })
        .collect();
    assert!(!text.contains("sk-live-secret"), "secret leaked: {text}");
    assert!(text.contains('•'), "masked bullets rendered: {text}");
}

#[test]
fn url_elicitation_renders_the_link_fieldlessly() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    app.elicitation_ask = Some(crate::app::ElicitationAskOverlay {
        form: url_or_secret_form(Some("https://auth.example/login".into()), None),
        scroll: 0,
        reply: None,
    });
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer();
    let text: String = (0..30)
        .flat_map(|y| {
            (0..100)
                .map(move |x| buf[(x, y)].symbol().to_string())
                .chain(std::iter::once("\n".into()))
        })
        .collect();
    assert!(
        text.contains("https://auth.example/login"),
        "url rendered: {text}"
    );
}

#[test]
fn fieldless_form_without_url_renders_confirm_hint() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    let form = url_or_secret_form(None, None);
    app.elicitation_ask = Some(crate::app::ElicitationAskOverlay {
        form,
        scroll: 0,
        reply: None,
    });
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer();
    let text: String = (0..30)
        .flat_map(|y| {
            (0..100)
                .map(move |x| buf[(x, y)].symbol().to_string())
                .chain(std::iter::once("\n".into()))
        })
        .collect();
    assert!(text.contains("enter confirm"), "{text}");
}

#[test]
fn welcome_needs_auth_line_in_english_and_chinese() {
    let flat = |lines: Vec<Line>| -> String {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut app = live_test_app();
    app.auth = crate::acp_auth::needs_auth_snapshot(
        Vec::new(),
        None,
        Some("token missing".into()),
    );
    let en = flat(welcome_info_lines(&app));
    assert!(en.contains("sign-in needed"), "{en}");
    assert!(en.contains("ctrl+g"), "{en}");
    // No known method stays unspecific — never the type word (issue #160).
    assert!(!en.contains("agent"), "{en}");
    // A named method appears in place of the old "agent" fallback.
    app.auth.method_name = Some("DeepSeek API key".into());
    let named = flat(welcome_info_lines(&app));
    assert!(named.contains("sign-in needed · DeepSeek API key"), "{named}");
    app.locale = crate::locale::Locale::Zh;
    let zh = flat(welcome_info_lines(&app));
    assert!(zh.contains("需要登录"), "{zh}");
}

#[test]
fn fielded_form_renders_the_error_line() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = test_app();
    app.show_banner = false;
    let mut form = url_or_secret_form(None, Some("api-key"));
    form.error = Some("too short".into());
    app.elicitation_ask = Some(crate::app::ElicitationAskOverlay {
        form,
        scroll: 0,
        reply: None,
    });
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer();
    let text: String = (0..30)
        .flat_map(|y| {
            (0..100)
                .map(move |x| buf[(x, y)].symbol().to_string())
                .chain(std::iter::once("\n".into()))
        })
        .collect();
    assert!(text.contains("too short"), "{text}");
}

#[test]
fn auth_picker_clamps_long_labels_before_the_meta_column() {
    // pi-acp advertises ~40-char method names; they must ellipsize instead
    // of overflowing into the type column (issue #160).
    use crate::app::{Picker, PickerItem, PickerKind, PICKER_LABEL_COL};
    let mut app = test_app();
    app.show_banner = false;
    // Theme is a kind that can mark a current entry: item `a` exercises the
    // `✓` row (clamp to COL-3), item `b` the plain row (clamp to COL-1).
    app.active_palette_id = "a".into();
    app.picker = Some(Picker {
        offset: 0,
        kind: PickerKind::Theme,
        title: " sign in ".into(),
        sel: 0,
        items: vec![
            PickerItem {
                id: "a".into(),
                label: "Sign in with a very long provider name that overflows".into(),
                meta: "agent".into(),
                provider: None,
            },
            PickerItem {
                id: "b".into(),
                label: "Another long method name that also overflows here".into(),
                meta: "agent".into(),
                provider: None,
            },
        ],
    });
    let frame = dump_frame(&mut app, 100, 30);
    for line in frame.lines().filter(|l| l.contains('…') && l.contains("agent")) {
        // The picker floats inside the frame — measure the meta column
        // relative to the row's own leading marker.
        let start = line.find('▸').or_else(|| line.find('│')).unwrap_or(0);
        let meta_col = line[start..]
            .find("agent")
            .map(|index| line[start..start + index].chars().count())
            .expect("meta column still rendered");
        assert!(
            meta_col <= 2 + PICKER_LABEL_COL + 2,
            "label overflowed the {PICKER_LABEL_COL}-char column: {line}"
        );
    }
    assert!(frame.contains('…'), "long labels ellipsize:\n{frame}");
}

#[test]
fn the_slash_argument_hint_paints_as_muted_ghost_text() {
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use ratatui::Terminal;
    let mut app = test_app();
    app.show_banner = false;
    app.skills = vec![crate::bus::SkillInfo {
        name: "review".into(),
        description: "Review the current changes.".into(),
        input_hint: Some("optional review instructions".into()),
        config_action: None,
        client_command: false,
    }];
    app.input.insert_str("/review");
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    let buf = terminal.backend().buffer().clone();
    let theme = app.theme;

    // The composer well is where the ghost belongs: the hint follows the
    // typed command on its first row.
    let well = app.input_area;
    assert!(well.height > 0, "the composer well was recorded");
    let row: String = (0..80).map(|x| buf[(x, well.y)].symbol().to_string()).collect();
    assert!(
        row.contains("/review optional review instructions"),
        "the ghost follows the typed command: {row:?}"
    );
    // Muted like the placeholder: caption color, italic.
    let x = row.find("optional review instructions").expect("ghost cell") as u16;
    let cell = &buf[(x, well.y)];
    assert_eq!(cell.fg, theme.caption, "the ghost is muted");
    assert!(
        cell.modifier.contains(Modifier::ITALIC),
        "the ghost is italic"
    );
    // The hint is a ghost only — the buffer itself never carries it.
    assert_eq!(app.input.buf(), "/review");
}
