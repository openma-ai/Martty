//! Martty — a terminal-native ACP client UI.

mod acp;
mod acp_auth;
mod acp_fs;
mod acp_term;
mod app;
mod attachments;
mod bus;
mod clipboard;
mod controller;
mod cordis;
mod deepseek_logo;
mod demo;
mod elicitation;
mod events;
mod input;
mod locale;
mod logo;
mod markdown;
mod pet;
mod proto;
mod runtime;
mod sessions;
mod slots;
mod theme;
mod transcript;
mod ui;

use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

use crate::app::{App, RunState};
use crate::bus::{AppEvent, Cmd};
use crate::controller::Controller;
use crate::runtime::{default_session_root, RuntimeConfig};

const HELP: &str = "\
martty — terminal-native ACP client UI

USAGE:
  martty [OPTIONS]

OPTIONS:
  -w, --workspace <dir>     agent workspace (default: cwd)
      --session-root <dir>  session JSONL root (default: $MARTTY_HOME/sessions)
      --session-id <id>     resume/continue a durable session id
      --provider <id>       provider route (default: deepseek-official)
      --model <id>          model id (default: $DSH_MODEL or deepseek-v4-flash)
      --max-tokens <n>      per-request output token cap
      --base-url <url>      sets DEEPSEEK_BASE_URL for a spawned agent
      --api-key <key>       sets DEEPSEEK_API_KEY for a spawned agent
      --agent <cmd>         ACP agent command (default: dsh-acp or $DSH_TUI_AGENT)
      --agent-arg <arg>     extra argument for --agent (repeatable)
      --theme <dark|light>  DeepSeek Web UI palette (default: dark)
      --demo                scripted turns, no runtime / API key needed
      --demo-skin           ember gallery palette via the plugin runner (implies --demo)
      --attach-fds          speak ACP over inherited fds 3/4 (Node mux / demo-skin)
      --attach-tcp <addr>   authenticated loopback TCP (Windows)
      --check-runtime       spawn + initialize the ACP agent, print info, exit
      --dump-frame [WxH]    render one demo frame as text (default 100x34)
  -V, --version             print version
  -h, --help                this help

KEYS (grok-build homage): enter send/queue · ctrl+x send-now ·
esc interrupt / clear draft · ctrl+c clear/quit · ↑ history · ! shell ·
/ commands · ctrl+p model · ctrl+o expand · ctrl+t theme
MOUSE: wheel scrolls the conversation · click a tool expands/collapses it ·
drag selects & copies on release · 2×click
copies a word · shift+drag uses the terminal's native selection
";

struct Args {
    workspace: Option<String>,
    session_root: Option<String>,
    session_id: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    max_tokens: Option<u64>,
    base_url: Option<String>,
    api_key: Option<String>,
    agent: Option<String>,
    agent_args: Vec<String>,
    theme: String,
    demo: bool,
    demo_skin: bool,
    attach_fds: bool,
    attach_tcp: Option<String>,
    check_runtime: bool,
    dump_frame: Option<(u16, u16)>,
}

fn parse_args() -> Result<Args> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut args_out = Args {
        workspace: None,
        session_root: None,
        session_id: None,
        provider: None,
        model: None,
        max_tokens: None,
        base_url: None,
        api_key: None,
        agent: None,
        agent_args: Vec::new(),
        theme: "dark".into(),
        demo: false,
        demo_skin: false,
        attach_fds: false,
        attach_tcp: None,
        check_runtime: false,
        dump_frame: None,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut take = |name: &str| -> Result<String> {
            it.next().with_context(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "-w" | "--workspace" => args_out.workspace = Some(take("--workspace")?),
            "--session-root" => args_out.session_root = Some(take("--session-root")?),
            "--session-id" => args_out.session_id = Some(take("--session-id")?),
            "--provider" => args_out.provider = Some(take("--provider")?),
            "--model" => args_out.model = Some(take("--model")?),
            "--max-tokens" => args_out.max_tokens = Some(take("--max-tokens")?.parse()?),
            "--base-url" => args_out.base_url = Some(take("--base-url")?),
            "--api-key" => args_out.api_key = Some(take("--api-key")?),
            "--agent" => args_out.agent = Some(take("--agent")?),
            "--agent-arg" => args_out.agent_args.push(take("--agent-arg")?),
            "--theme" => args_out.theme = take("--theme")?,
            "--demo" => args_out.demo = true,
            "--demo-skin" => {
                args_out.demo_skin = true;
                args_out.demo = true;
            }
            "--attach-fds" => args_out.attach_fds = true,
            "--attach-tcp" => args_out.attach_tcp = Some(take("--attach-tcp")?),
            "--check-runtime" => args_out.check_runtime = true,
            "--dump-frame" => {
                let dims = it.next().unwrap_or_else(|| "100x34".into());
                let (w, h) = dims
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .unwrap_or((100, 34));
                args_out.dump_frame = Some((w, h));
            }
            "-V" | "--version" => {
                println!("martty {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other} (see --help)"),
        }
    }
    Ok(args_out)
}

fn agent_argv(args: &Args) -> Vec<String> {
    if let Some(cmd) = &args.agent {
        let mut out = vec![cmd.clone()];
        out.extend(args.agent_args.iter().cloned());
        return out;
    }
    if let Ok(env) = std::env::var("DSH_TUI_AGENT") {
        let tokens: Vec<String> = env.split_whitespace().map(str::to_string).collect();
        if !tokens.is_empty() {
            return tokens;
        }
    }
    vec!["dsh-acp".into()]
}

fn build_config(args: &Args) -> Result<RuntimeConfig> {
    let local = runtime::local_dsh();
    let workspace = match &args.workspace {
        Some(w) => std::fs::canonicalize(w)
            .with_context(|| format!("workspace not found: {w}"))?
            .to_string_lossy()
            .into_owned(),
        None => std::env::current_dir()?.to_string_lossy().into_owned(),
    };
    let session_root = match &args.session_root {
        Some(r) => r.clone(),
        None => default_session_root().to_string_lossy().into_owned(),
    };
    std::fs::create_dir_all(&session_root).ok();

    let attached = args.attach_fds || args.attach_tcp.is_some();
    let agent = agent_argv(args);
    let bin = if args.demo {
        "demo".into()
    } else {
        agent.join(" ")
    };
    let cordis = if args.demo {
        "demo".into()
    } else if attached {
        "(host mux)".into()
    } else {
        "acp".into()
    };

    Ok(RuntimeConfig {
        bin,
        cordis,
        workspace,
        session_root,
        // Route defaults borrow the local dsh install's configured default
        // (settings.yaml agent-default-model) before falling back to stock.
        provider: args
            .provider
            .clone()
            .or(local.provider)
            .unwrap_or_else(|| "deepseek-official".into()),
        model: args
            .model
            .clone()
            .or_else(|| std::env::var("DSH_MODEL").ok())
            .or(local.model)
            .unwrap_or_else(|| "deepseek-v4-flash".into()),
        max_tokens: args.max_tokens,
        base_url: args.base_url.clone(),
        api_key: args.api_key.clone(),
    })
}

fn main() -> Result<()> {
    // Die quietly on closed pipes (martty --dump-frame | head) instead of
    // panicking in println!.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let args = parse_args()?;

    if args.attach_fds && args.attach_tcp.is_some() {
        bail!("--attach-fds and --attach-tcp are mutually exclusive");
    }

    if args.check_runtime {
        return check_runtime(&args);
    }
    if let Some((w, h)) = args.dump_frame {
        return dump_frame(&args, w, h);
    }

    if args.demo_skin && !args.attach_fds && args.attach_tcp.is_none() {
        return reexec_demo_skin();
    }

    let cfg = build_config(&args)?;
    let session_id = args
        .session_id
        .clone()
        .unwrap_or_else(|| format!("dsh-{}", app::timestamp()));

    let (bus_tx, bus_rx) = mpsc::channel::<AppEvent>();
    install_termination_handler(bus_tx.clone())?;

    // `--demo` / `--demo-skin`: JSON-RPC attach for palette + scripted turns.
    // Live: official ACP on those fds (Node mux) or a spawned agent.
    let controller = if args.demo {
        let attached_rt = if args.attach_fds {
            #[cfg(unix)]
            {
                use std::os::unix::io::FromRawFd;
                let reader = unsafe { std::fs::File::from_raw_fd(3) };
                let writer = unsafe { std::fs::File::from_raw_fd(4) };
                Some(std::sync::Arc::new(proto::RuntimeProcess::attach(
                    reader,
                    writer,
                    bus_tx.clone(),
                )))
            }
            #[cfg(not(unix))]
            {
                bail!("--attach-fds requires a unix platform");
            }
        } else if let Some(address) = &args.attach_tcp {
            let writer = tcp_attach_stream(address)?;
            let reader = writer.try_clone().context("clone plugin TCP stream")?;
            Some(std::sync::Arc::new(proto::RuntimeProcess::attach(
                reader,
                writer,
                bus_tx.clone(),
            )))
        } else {
            None
        };
        Controller::start(cfg.clone(), true, attached_rt, bus_tx.clone())
    } else {
        let endpoint = if args.attach_fds {
            #[cfg(unix)]
            {
                use std::os::unix::io::FromRawFd;
                let incoming = unsafe { std::fs::File::from_raw_fd(3) };
                let outgoing = unsafe { std::fs::File::from_raw_fd(4) };
                acp::AcpEndpoint::AttachStdio { incoming, outgoing }
            }
            #[cfg(not(unix))]
            {
                bail!("--attach-fds requires a unix platform");
            }
        } else if let Some(address) = &args.attach_tcp {
            acp::AcpEndpoint::AttachTcp(tcp_attach_stream(address)?)
        } else {
            acp::AcpEndpoint::Spawn(agent_argv(&args))
        };
        Controller::start_acp(cfg.clone(), endpoint, bus_tx.clone())
    };
    let theme = ui::theme_for(&args.theme);
    let mut app = App::new(
        theme,
        cfg,
        session_id,
        args.demo,
        args.attach_fds || args.attach_tcp.is_some() || !args.demo,
        bus_tx.clone(),
    );
    // The composer pet: real pixels (kitty graphics) where the terminal can,
    // half-block art (drawn by ui) where it can't.
    app.pet_pixels = pet::kitty_supported();
    let mut pet = pet::Pet::new(app.pet_pixels);
    let mut backdrop = pet::Backdrop::new(app.pet_pixels);
    // User-image thumbnails in the chat scrollback (kitty graphics, PNG only).
    let mut thumbnails = pet::Thumbnails::new();

    // input pump
    {
        let tx = bus_tx.clone();
        std::thread::Builder::new()
            .name("input".into())
            .spawn(move || loop {
                match crossterm::event::read() {
                    Ok(crossterm::event::Event::Key(key)) => {
                        // Recover terminal-lost physical modifiers at read
                        // time, before a quick key release can race the UI
                        // event queue.
                        let ev = crossterm::event::Event::Key(crate::input::rescue_key(key));
                        if tx.send(AppEvent::Term(ev)).is_err() {
                            break;
                        }
                    }
                    Ok(ev) => {
                        if tx.send(AppEvent::Term(ev)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            })
            .expect("spawn input thread");
    }

    // terminal guard
    enter_tui()?;
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        prev_hook(info);
    }));

    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    if let Ok(auto) = std::env::var("DSH_TUI_AUTOPROMPT") {
        if !auto.trim().is_empty() {
            app.auto_prompt(&auto, &controller);
        }
    }
    // ACP commands for the slash menu; demo serves samples.
    controller.send(bus::Cmd::FetchSkills);

    let run = (|| -> Result<()> {
        let mut last_tick = std::time::Instant::now();
        loop {
            if app.needs_redraw {
                terminal.draw(|f| ui::draw(f, &mut app))?;
                app.needs_redraw = false;
                // Reconcile the pixel pet (frame + state) with what was drawn.
                let size = terminal.size()?;
                let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                let _ = backdrop.sync(&mut std::io::stdout(), app.active_background(), area);
                let working = !matches!(app.state, RunState::Idle);
                // Same anchor math as ui::draw: the pet sits inside the
                // composer box, above the stats dock when it is shown.
                let dock_h =
                    ui::composer_dock_height(&app, area.height, app.active_subagent.is_some());
                let pet_area = ratatui::layout::Rect::new(
                    0,
                    0,
                    size.width,
                    size.height.saturating_sub(dock_h),
                );
                let want = ui::pet_rect(pet_area, &app).map(|r| (r, working));
                let _ = pet.sync(&mut std::io::stdout(), want);
                // Sync image thumbnails (chat + composer attachment strip)
                // against the freshly drawn viewport.
                let shots: Vec<pet::ThumbShot> = app
                    .chat_view
                    .images
                    .iter()
                    .chain(app.att_thumbs.iter())
                    .map(|t| pet::ThumbShot {
                        id: t.id,
                        rect: t.rect,
                        data: t.data.as_ref(),
                    })
                    .collect();
                let _ = thumbnails.sync(&mut std::io::stdout(), &shots);
            }
            match bus_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(ev) => {
                    app.handle(ev, &controller);
                    // drain whatever is queued to batch redraws
                    while let Ok(ev) = bus_rx.try_recv() {
                        app.handle(ev, &controller);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last_tick.elapsed() >= Duration::from_millis(100) {
                app.tick();
                last_tick = std::time::Instant::now();
            }
            if let Some(launch) = app.take_terminal_auth() {
                restore_terminal();
                eprintln!(
                    "\n{} — finish setup in this terminal, then Martty resumes.\n",
                    launch.label
                );
                let result = crate::acp_auth::run_terminal_auth(&launch);
                enter_tui()?;
                app.needs_redraw = true;
                while let Ok(ev) = bus_rx.try_recv() {
                    if !matches!(ev, AppEvent::Term(_)) {
                        app.handle(ev, &controller);
                    }
                }
                match result {
                    Ok(()) => controller.send(Cmd::Authenticate {
                        method_id: launch.method_id,
                        values: Default::default(),
                    }),
                    Err(err) => app
                        .transcript
                        .push_notice(crate::transcript::NoticeLevel::Error, err),
                }
            }
            if app.quit {
                break;
            }
        }
        Ok(())
    })();

    controller.send(Cmd::Shutdown);
    restore_terminal();
    run
}

fn enter_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    // Kitty keyboard protocol, pushed blind: terminals that support it
    // (ghostty · kitty · wezterm · iterm2 3.5+) start reporting ⌘/⌥ chords
    // as real SUPER/ALT modifiers and make shift+enter distinguishable;
    // everything else ignores the sequence. (No capability query — the
    // input thread already owns the event stream, a query reply would race.)
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    Ok(())
}

#[cfg(unix)]
fn install_termination_handler(tx: mpsc::Sender<AppEvent>) -> Result<()> {
    let mut signals = signal_hook::iterator::Signals::new([
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ])?;
    std::thread::Builder::new()
        .name("termination-signal".into())
        .spawn(move || {
            if signals.forever().next().is_some() {
                let _ = tx.send(AppEvent::Terminate);
            }
        })
        .context("spawn termination signal handler")?;
    Ok(())
}

#[cfg(not(unix))]
fn install_termination_handler(_tx: mpsc::Sender<AppEvent>) -> Result<()> {
    Ok(())
}

fn restore_terminal() {
    let mut stdout = std::io::stdout();
    if pet::kitty_supported() {
        // Drop any kitty placement (panic-safe: also runs from the hook).
        let _ = stdout.write_all(pet::KITTY_DELETE_ALL.as_bytes());
    }
    let _ = execute!(stdout, PopKeyboardEnhancementFlags);
    let _ = execute!(
        stdout,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
    let _ = stdout.flush();
}

/// `--check-runtime`: ACP initialize without the TUI.
fn check_runtime(args: &Args) -> Result<()> {
    if args.demo {
        bail!("--check-runtime is for a real ACP agent; drop --demo");
    }
    let argv = agent_argv(args);
    println!("agent    {}", argv.join(" "));
    let t0 = std::time::Instant::now();
    let name = acp::check_blocking(argv)?;
    println!(
        "initialize ok in {:.1}s → {name}",
        t0.elapsed().as_secs_f32()
    );
    Ok(())
}

fn tcp_attach_stream(address: &str) -> Result<std::net::TcpStream> {
    use std::net::{SocketAddr, TcpStream};

    let address: SocketAddr = address
        .parse()
        .with_context(|| format!("invalid --attach-tcp address: {address}"))?;
    if !address.ip().is_loopback() {
        bail!("--attach-tcp requires a loopback address");
    }
    let token = std::env::var("DSH_TUI_ATTACH_TOKEN")
        .context("--attach-tcp requires DSH_TUI_ATTACH_TOKEN")?;
    if token.is_empty() {
        bail!("DSH_TUI_ATTACH_TOKEN must not be empty");
    }
    std::env::remove_var("DSH_TUI_ATTACH_TOKEN");
    let mut writer = TcpStream::connect_timeout(&address, Duration::from_secs(10))
        .with_context(|| format!("connect plugin transport at {address}"))?;
    writeln!(writer, "{token}")?;
    writer.flush()?;
    Ok(writer)
}

/// `--dump-frame WxH`: render a canned demo conversation to plain text.
fn dump_frame(args: &Args, w: u16, h: u16) -> Result<()> {
    let mut args_demo = Args {
        demo: true,
        ..parse_args()?
    };
    args_demo.demo = true;
    let cfg = build_config(&args_demo)?;
    let (bus_tx, bus_rx) = mpsc::channel::<AppEvent>();
    let theme = ui::theme_for(&args.theme);
    let mut app = App::new(theme, cfg, "dsh-demo".into(), true, false, bus_tx.clone());

    // Run one scripted demo turn synchronously through the real pipeline.
    app.transcript
        .push_user("查看这个仓库并修复失败的测试".into(), false);
    app.show_banner = false; // dump simulates the post-submit look: no whale
    app.state = RunState::Starting;
    demo::run_demo_turn(bus_tx, "dsh-demo".into(), "inspect the repo".into());
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        match bus_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ev) => {
                let done = matches!(
                    &ev,
                    AppEvent::Rpc { method, params }
                        if method == "session.status"
                            && params.get("status").and_then(|s| s.as_str()) == Some("idle")
                );
                // No controller in dump mode: feed events directly.
                match ev {
                    AppEvent::Ui(ui) => app.transcript.apply(ui),
                    AppEvent::Rpc { method, params } => {
                        for ui_ev in events::parse_notification(&method, &params) {
                            app.transcript.apply(ui_ev);
                        }
                    }
                    _ => {}
                }
                if done {
                    break;
                }
            }
            Err(_) if std::time::Instant::now() > deadline => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) => {}
        }
    }
    app.state = RunState::Idle;
    print!("{}", ui::dump_frame(&mut app, w, h));
    Ok(())
}

fn argv_without_demo_skin(args: impl IntoIterator<Item = String>) -> Vec<String> {
    args.into_iter().filter(|a| a != "--demo-skin").collect()
}

fn demo_skin_script_candidates(
    manifest_dir: &std::path::Path,
    exe: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut out = vec![manifest_dir.join("npm/lib/demo-skin.js")];
    if let Some(dir) = exe.parent() {
        out.push(dir.join("../../lib/demo-skin.js"));
        out.push(dir.join("../lib/demo-skin.js"));
    }
    out
}

fn find_demo_skin_script(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    demo_skin_script_candidates(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), exe)
        .into_iter()
        .find(|p| p.is_file())
}

/// `--demo-skin` without an attach transport: hand off to the Node plugin
/// runner so the palette arrives over `_dsh/cordis/tui/theme/update`. Missing node/script
/// fails loud — never silently paint the built-in default pack.
fn reexec_demo_skin() -> Result<()> {
    let exe = std::env::current_exe()
        .context("martty --demo-skin: cannot resolve the current executable (MARTTY_BIN)")?;
    let looked =
        demo_skin_script_candidates(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), &exe);
    let script = find_demo_skin_script(&exe).ok_or_else(|| {
        anyhow::anyhow!(
            "martty --demo-skin requires npm/lib/demo-skin.js (looked in {}); refusing to fall back to the default palette",
            looked
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let child_args = argv_without_demo_skin(std::env::args().skip(1));
    let status = std::process::Command::new("node")
        .arg(&script)
        .args(&child_args)
        .env("MARTTY_BIN", &exe)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| {
            format!(
                "martty --demo-skin failed to spawn node {} (is node on PATH?); refusing to fall back to the default palette",
                script.display()
            )
        })?;
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
#[path = "../tests/unit/main__cli_args_tests.rs"]
mod cli_args_tests;
