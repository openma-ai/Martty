//! dsh-tui — a terminal-native agent UI for the DeepSeek Harness JSON-RPC
//! stdio runtime.

mod app;
mod attachments;
mod bus;
mod clipboard;
mod controller;
mod demo;
mod events;
mod input;
mod logo;
mod logo_data;
mod markdown;
mod pet;
mod proto;
mod runtime;
mod sessions;
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
use crate::runtime::RuntimeConfig;

const HELP: &str = "\
dsh-tui — terminal-native UI for DeepSeek Harness

USAGE:
  dsh-tui [OPTIONS]

OPTIONS:
  -w, --workspace <dir>     agent workspace (default: cwd)
      --session-root <dir>  session JSONL root (default: ~/.dsh-tui/sessions)
      --session-id <id>     resume/continue a durable session id
      --provider <id>       provider route (default: deepseek-official)
      --model <id>          model id (default: $DSH_MODEL or deepseek-v4-flash)
      --max-tokens <n>      per-request output token cap
      --base-url <url>      sets DEEPSEEK_BASE_URL for the runtime
      --api-key <key>       sets DEEPSEEK_API_KEY for the runtime
      --cordis <file>       cordis composition (default: bundled runtime config)
      --runtime-bin <file>  dsh-jsonrpc-agent binary (default: auto-discover)
      --theme <dark|light>  DeepSeek Web UI palette (default: dark)
      --demo                scripted turns, no runtime / API key needed
      --attach-fds          plugin mode: speak JSON-RPC over inherited fds 3/4
                            (used by `dsh plugin --profile tui add @openma/deepseek-harness-tui`)
      --attach-tcp <addr>   plugin mode: authenticated loopback TCP (Windows)
      --check-runtime       spawn + initialize the runtime, print info, exit
      --dump-frame [WxH]    render one demo frame as text (default 100x34)
  -V, --version             print version
  -h, --help                this help

KEYS (grok-build homage): enter send/queue · ctrl+x send-now ·
esc interrupt / clear draft · ctrl+c clear/quit · ↑ history · ! shell ·
/ commands · ctrl+p model · ctrl+o expand · ctrl+t theme
MOUSE: wheel scrolls · click a tool expands/collapses it · wheel over a
tool scrolls its output · drag selects & copies on release · 2×click
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
    cordis: Option<String>,
    runtime_bin: Option<String>,
    theme: String,
    demo: bool,
    attach_fds: bool,
    attach_tcp: Option<String>,
    check_runtime: bool,
    dump_frame: Option<(u16, u16)>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        workspace: None,
        session_root: None,
        session_id: None,
        provider: None,
        model: None,
        max_tokens: None,
        base_url: None,
        api_key: None,
        cordis: None,
        runtime_bin: None,
        theme: "dark".into(),
        demo: false,
        attach_fds: false,
        attach_tcp: None,
        check_runtime: false,
        dump_frame: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut take = |name: &str| -> Result<String> {
            it.next().with_context(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "-w" | "--workspace" => args.workspace = Some(take("--workspace")?),
            "--session-root" => args.session_root = Some(take("--session-root")?),
            "--session-id" => args.session_id = Some(take("--session-id")?),
            "--provider" => args.provider = Some(take("--provider")?),
            "--model" => args.model = Some(take("--model")?),
            "--max-tokens" => args.max_tokens = Some(take("--max-tokens")?.parse()?),
            "--base-url" => args.base_url = Some(take("--base-url")?),
            "--api-key" => args.api_key = Some(take("--api-key")?),
            "--cordis" => args.cordis = Some(take("--cordis")?),
            "--runtime-bin" => args.runtime_bin = Some(take("--runtime-bin")?),
            "--theme" => args.theme = take("--theme")?,
            "--demo" => args.demo = true,
            "--attach-fds" => args.attach_fds = true,
            "--attach-tcp" => args.attach_tcp = Some(take("--attach-tcp")?),
            "--check-runtime" => args.check_runtime = true,
            "--dump-frame" => {
                let dims = it.next().unwrap_or_else(|| "100x34".into());
                let (w, h) = dims
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .unwrap_or((100, 34));
                args.dump_frame = Some((w, h));
            }
            "-V" | "--version" => {
                println!("dsh-tui {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other} (see --help)"),
        }
    }
    Ok(args)
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
        None => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.dsh-tui/sessions")
        }
    };
    std::fs::create_dir_all(&session_root).ok();

    let attached = args.attach_fds || args.attach_tcp.is_some();
    let (bin, sibling_cordis) = if args.demo {
        (
            args.runtime_bin.clone().unwrap_or_else(|| "demo".into()),
            Some("demo".into()),
        )
    } else if attached {
        ("(host dsh)".into(), Some("(host profile)".into()))
    } else {
        runtime::discover_runtime(args.runtime_bin.as_deref(), &workspace)?
    };
    let cordis = if args.demo {
        "demo".into()
    } else if attached {
        "(host profile)".into()
    } else {
        runtime::resolve_cordis(args.cordis.as_deref(), sibling_cordis)?
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
    // Die quietly on closed pipes (dsh-tui --dump-frame | head) instead of
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

    let cfg = build_config(&args)?;
    let session_id = args
        .session_id
        .clone()
        .unwrap_or_else(|| format!("dsh-{}", app::timestamp()));

    let (bus_tx, bus_rx) = mpsc::channel::<AppEvent>();

    // Plugin mode: Unix uses inherited pipe fds while Windows connects to an
    // authenticated loopback socket. The TTY stays on stdio 0/1/2.
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
        let reader = writer.try_clone().context("clone plugin TCP stream")?;
        Some(std::sync::Arc::new(proto::RuntimeProcess::attach(
            reader,
            writer,
            bus_tx.clone(),
        )))
    } else {
        None
    };

    let controller = Controller::start(cfg.clone(), args.demo, attached_rt, bus_tx.clone());
    let theme = ui::theme_for(&args.theme);
    let mut app = App::new(
        theme,
        cfg,
        session_id,
        args.demo,
        args.attach_fds || args.attach_tcp.is_some(),
        bus_tx.clone(),
    );
    // The composer pet: real pixels (kitty graphics) where the terminal can,
    // half-block art (drawn by ui) where it can't.
    app.pet_pixels = pet::kitty_supported();
    let mut pet = pet::Pet::new(app.pet_pixels);
    // User-image thumbnails in the chat scrollback (kitty graphics, PNG only).
    let mut thumbnails = pet::Thumbnails::new();

    // input pump
    {
        let tx = bus_tx.clone();
        std::thread::Builder::new()
            .name("input".into())
            .spawn(move || loop {
                match crossterm::event::read() {
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
    // Host skills for the slash menu (plugin mode; demo serves samples,
    // standalone answers empty and the menu keeps its builtins).
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
                let working = !matches!(app.state, RunState::Idle);
                let want = ui::pet_rect(area, &app).map(|r| (r, working));
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

fn restore_terminal() {
    let mut stdout = std::io::stdout();
    if pet::kitty_supported() {
        // Drop any pet placement (panic-safe: also runs from the hook).
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

/// `--check-runtime`: raw protocol handshake without the TUI.
fn check_runtime(args: &Args) -> Result<()> {
    let cfg = build_config(args)?;
    if args.demo {
        bail!("--check-runtime is for the real runtime; drop --demo");
    }
    println!("runtime  {}", cfg.bin);
    println!("cordis   {}", cfg.cordis);
    println!("cwd      {}", cfg.workspace);
    let (tx, rx) = mpsc::channel::<AppEvent>();
    let rt = proto::RuntimeProcess::spawn(&cfg.bin, &cfg.child_env(), &cfg.workspace, tx)?;
    let params = serde_json::json!({
        "cwd": cfg.workspace,
        "provider": cfg.provider,
        "model": cfg.model,
    });
    let t0 = std::time::Instant::now();
    let result = rt.request("initialize", Some(params), Duration::from_secs(180))?;
    println!(
        "initialize ok in {:.1}s → {}",
        t0.elapsed().as_secs_f32(),
        serde_json::to_string(&result)?
    );
    rt.shutdown();
    drop(rx);
    println!("shutdown ok");
    Ok(())
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
