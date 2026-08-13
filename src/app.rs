//! App state and input handling — the grok-build interaction homage.
//!
//! Enter sends (or queues mid-turn, protocol-natively); Alt+Enter interrupts
//! and sends now; Esc cancels a running turn with the draft preserved, and
//! double-Esc clears an idle draft; Ctrl+C clears first, then arms quit;
//! `!` runs a local shell command; `/` opens the slash menu; Up recalls
//! history on an empty prompt.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

use crate::bus::{AppEvent, Cmd, CtlEvent};
use crate::controller::Controller;
use crate::events::parse_notification;
use crate::runtime::RuntimeConfig;
use crate::theme::Theme;
use crate::transcript::{NoticeLevel, Transcript};

pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

const ESC_ARM_WINDOW: Duration = Duration::from_millis(800);
const CTRL_C_QUIT_WINDOW: Duration = Duration::from_millis(1500);
const TIP_TTL: Duration = Duration::from_secs(4);

pub struct SlashCommand {
    pub name: &'static str,
    pub usage: &'static str,
    pub desc: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand { name: "help", usage: "/help", desc: "show help and tips" },
    SlashCommand { name: "keys", usage: "/keys", desc: "keyboard shortcuts" },
    SlashCommand { name: "new", usage: "/new [id]", desc: "start a fresh session" },
    SlashCommand { name: "clear", usage: "/clear", desc: "clear the scrollback" },
    SlashCommand { name: "model", usage: "/model [id]", desc: "switch model (restarts runtime)" },
    SlashCommand { name: "theme", usage: "/theme [dark|light]", desc: "toggle the DeepSeek palette" },
    SlashCommand { name: "session", usage: "/session", desc: "show session + runtime info" },
    SlashCommand { name: "logo", usage: "/logo", desc: "print the whale" },
    SlashCommand { name: "quit", usage: "/quit", desc: "exit dsh-tui" },
];

pub const MODEL_PRESETS: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4",
    "deepseek-v3.2",
    "deepseek-chat",
    "deepseek-reasoner",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Starting,
    Running,
}

pub struct Input {
    pub buf: String,
    pub cursor: usize, // char index
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub stash: String,
}

impl Input {
    fn new() -> Self {
        Input {
            buf: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            stash: String::new(),
        }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.buf
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.buf.len())
    }

    pub fn insert(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.buf.insert(at, ch);
        self.cursor += 1;
        self.hist_pos = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        let at = self.byte_at(self.cursor);
        self.buf.insert_str(at, s);
        self.cursor += s.chars().count();
        self.hist_pos = None;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete_word_back(&mut self) {
        let chars: Vec<char> = self.buf.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let start = self.byte_at(i);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor = i;
    }

    fn kill_to_end(&mut self) {
        let at = self.byte_at(self.cursor);
        self.buf.truncate(at);
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.hist_pos = None;
    }

    fn set(&mut self, s: String) {
        self.cursor = s.chars().count();
        self.buf = s;
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

pub struct App {
    pub theme: Theme,
    pub transcript: Transcript,
    pub input: Input,
    pub state: RunState,
    pub state_note: String,
    pub run_started: Option<Instant>,
    pub spinner_idx: usize,
    pub scroll_up: usize, // lines above the bottom; 0 = follow
    pub slash_sel: usize,
    pub model_picker: Option<(usize, Vec<String>)>,
    pub tip: Option<(String, Instant)>,
    pub ambient_tip_idx: usize,
    pub ambient_tip_at: Instant,
    esc_armed: Option<Instant>,
    ctrl_c_armed: Option<Instant>,
    pub session_id: String,
    pub cfg: RuntimeConfig,
    pub demo: bool,
    pub quit: bool,
    pub queued: usize,
    shell_seq: u64,
    shell_pending: Vec<(u64, usize)>, // (id, cell idx)
    bus_tx: Sender<AppEvent>,
    pub server_info: Option<String>,
    pub needs_redraw: bool,
}

pub const AMBIENT_TIPS: &[&str] = &[
    "Tip · esc interrupts a running turn — your draft survives",
    "Tip · type ! to run a local shell command without the agent",
    "Tip · enter queues a follow-up while DeepSeek works; alt+enter sends it now",
    "Tip · ctrl+e expands every thought and tool result",
    "Tip · /theme light mirrors the Web UI light palette",
    "Tip · ask DeepSeek about the harness docs — e.g. \"how do I add a tool?\"",
    "Tip · /new starts a fresh durable session; the old JSONL log stays on disk",
];

impl App {
    pub fn new(
        theme: Theme,
        cfg: RuntimeConfig,
        session_id: String,
        demo: bool,
        bus_tx: Sender<AppEvent>,
    ) -> Self {
        App {
            theme,
            transcript: Transcript::new(session_id.clone()),
            input: Input::new(),
            state: RunState::Idle,
            state_note: String::new(),
            run_started: None,
            spinner_idx: 0,
            scroll_up: 0,
            slash_sel: 0,
            model_picker: None,
            tip: None,
            ambient_tip_idx: 0,
            ambient_tip_at: Instant::now(),
            esc_armed: None,
            ctrl_c_armed: None,
            session_id,
            cfg,
            demo,
            quit: false,
            queued: 0,
            shell_seq: 0,
            shell_pending: Vec::new(),
            bus_tx,
            server_info: None,
            needs_redraw: true,
        }
    }

    pub fn spinner(&self) -> char {
        SPINNER[self.spinner_idx % SPINNER.len()]
    }

    pub fn tick(&mut self) {
        if self.state != RunState::Idle || self.transcript.streaming() {
            self.spinner_idx = self.spinner_idx.wrapping_add(1);
            self.needs_redraw = true;
        }
        if let Some((_, at)) = &self.tip {
            if at.elapsed() > TIP_TTL {
                self.tip = None;
                self.needs_redraw = true;
            }
        }
        if self.ambient_tip_at.elapsed() > Duration::from_secs(14) {
            self.ambient_tip_at = Instant::now();
            self.ambient_tip_idx = (self.ambient_tip_idx + 1) % AMBIENT_TIPS.len();
            if self.tip.is_none() {
                self.needs_redraw = true;
            }
        }
        // disarm expired chords
        if let Some(t) = self.esc_armed {
            if t.elapsed() > ESC_ARM_WINDOW {
                self.esc_armed = None;
            }
        }
        if let Some(t) = self.ctrl_c_armed {
            if t.elapsed() > CTRL_C_QUIT_WINDOW {
                self.ctrl_c_armed = None;
            }
        }
    }

    pub fn show_tip(&mut self, text: impl Into<String>) {
        self.tip = Some((text.into(), Instant::now()));
        self.needs_redraw = true;
    }

    pub fn slash_matches(&self) -> Vec<&'static SlashCommand> {
        if !self.input.buf.starts_with('/') || self.input.buf.contains(' ') {
            return Vec::new();
        }
        let prefix = &self.input.buf[1..];
        SLASH_COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(prefix))
            .collect()
    }

    pub fn handle(&mut self, ev: AppEvent, ctl: &Controller) {
        match ev {
            AppEvent::Term(term) => self.handle_term(term, ctl),
            AppEvent::Rpc { method, params } => {
                for ui in parse_notification(&method, &params) {
                    if let crate::events::UiEvent::SessionStatus { session, running } = &ui {
                        if *session == self.session_id {
                            self.state = if *running {
                                RunState::Running
                            } else {
                                RunState::Idle
                            };
                            if *running {
                                if self.run_started.is_none() {
                                    self.run_started = Some(Instant::now());
                                }
                            } else {
                                self.run_started = None;
                                self.queued = 0;
                                self.state_note.clear();
                            }
                        }
                    }
                    if let crate::events::UiEvent::TurnStart { session, .. } = &ui {
                        if *session == self.session_id && self.queued > 0 {
                            self.queued = self.queued.saturating_sub(1);
                        }
                    }
                    self.transcript.apply(ui);
                }
                self.needs_redraw = true;
            }
            AppEvent::RuntimeStderr(_line) => {
                // kept in proto's tail buffer for diagnostics; stay quiet here
            }
            AppEvent::RuntimeExited(code) => {
                if self.state != RunState::Idle {
                    self.state = RunState::Idle;
                    self.run_started = None;
                }
                if let Some(c) = code {
                    if c != 0 {
                        self.transcript.push_notice(
                            NoticeLevel::Warn,
                            format!("runtime exited with code {c} — next prompt restarts it"),
                        );
                    }
                }
                self.needs_redraw = true;
            }
            AppEvent::Ctl(ctl_ev) => {
                match ctl_ev {
                    CtlEvent::Starting { runtime } => {
                        self.state = RunState::Starting;
                        self.run_started = Some(Instant::now());
                        self.state_note = "starting runtime".into();
                        let short = runtime.rsplit('/').next().unwrap_or(&runtime).to_string();
                        self.transcript
                            .push_notice(NoticeLevel::Info, format!("⛭ spawning {short}"));
                    }
                    CtlEvent::Ready { server } => {
                        self.server_info = Some(server.clone());
                        self.state_note = "waiting for model".into();
                        self.transcript
                            .push_notice(NoticeLevel::Info, format!("⛭ connected · {server}"));
                    }
                    CtlEvent::PromptQueued { .. } => {
                        if self.state == RunState::Starting {
                            self.state = RunState::Running;
                        }
                        self.state_note.clear();
                    }
                    CtlEvent::Error(err) => {
                        self.state = RunState::Idle;
                        self.run_started = None;
                        self.transcript.push_notice(NoticeLevel::Error, err);
                    }
                    CtlEvent::Interrupted => {
                        self.state = RunState::Idle;
                        self.run_started = None;
                        self.queued = 0;
                        self.transcript.push_notice(
                            NoticeLevel::Warn,
                            "interrupted — runtime stopped; the session log is preserved and the next prompt restarts it".into(),
                        );
                    }
                }
                self.needs_redraw = true;
            }
            AppEvent::ShellDone { id, code, output } => {
                if let Some(pos) = self.shell_pending.iter().position(|(sid, _)| *sid == id) {
                    let (_, cell) = self.shell_pending.remove(pos);
                    self.transcript.finish_shell(cell, code, output);
                    self.needs_redraw = true;
                }
            }
        }
    }

    fn handle_term(&mut self, ev: Event, ctl: &Controller) {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key, ctl),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(..) => self.needs_redraw = true,
            Event::Paste(text) => {
                self.input.insert_str(&text.replace('\n', " "));
                self.needs_redraw = true;
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_by(3),
            MouseEventKind::ScrollDown => self.scroll_by(-3),
            _ => {}
        }
    }

    pub fn scroll_by(&mut self, delta: i64) {
        let cur = self.scroll_up as i64;
        self.scroll_up = (cur + delta).max(0) as usize; // clamped to content in ui::draw
        self.needs_redraw = true;
    }

    fn handle_key(&mut self, key: KeyEvent, ctl: &Controller) {
        self.needs_redraw = true;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // --- model picker overlay steals input first (grok modal semantics)
        if self.model_picker.is_some() {
            self.handle_picker_key(key, ctl);
            return;
        }

        match key.code {
            KeyCode::Enter if alt => self.send_now(ctl),
            KeyCode::Enter => {
                let menu = self.slash_matches();
                if !menu.is_empty() {
                    let cmd = menu[self.slash_sel.min(menu.len() - 1)];
                    self.accept_slash(cmd, ctl);
                } else {
                    self.submit(ctl);
                }
            }
            KeyCode::Esc => self.handle_esc(ctl),
            KeyCode::Char('c') if ctrl => self.handle_ctrl_c(ctl),
            KeyCode::Char('q') if ctrl => self.quit = true,
            KeyCode::Char('l') if ctrl => {
                self.transcript.clear();
                self.transcript
                    .push_notice(NoticeLevel::Info, "scrollback cleared".into());
            }
            KeyCode::Char('t') if ctrl => {
                self.theme = self.theme.toggled();
                self.show_tip(match self.theme.mode {
                    crate::theme::Mode::Dark => "theme: deepseek dark",
                    crate::theme::Mode::Light => "theme: deepseek light",
                });
            }
            KeyCode::Char('e') if ctrl => {
                self.transcript.expand_all = !self.transcript.expand_all;
                self.show_tip(if self.transcript.expand_all {
                    "expanded all thoughts and tool results"
                } else {
                    "collapsed all thoughts and tool results"
                });
            }
            KeyCode::Char('m') if ctrl => self.open_model_picker(),
            KeyCode::Char('u') if ctrl => self.scroll_by(10),
            KeyCode::Char('d') if ctrl => self.scroll_by(-10),
            KeyCode::Char('k') if ctrl => self.input.kill_to_end(),
            KeyCode::Char('w') if ctrl => self.input.delete_word_back(),
            KeyCode::Char('a') if ctrl => self.input.cursor = 0,
            KeyCode::Char('e') if !ctrl && alt => self.input.cursor = self.input.buf.chars().count(),
            KeyCode::PageUp => self.scroll_by(20),
            KeyCode::PageDown => self.scroll_by(-20),
            KeyCode::Home if self.input.is_empty() => self.scroll_up = usize::MAX,
            KeyCode::End if self.input.is_empty() => self.scroll_up = 0,
            KeyCode::Home => self.input.cursor = 0,
            KeyCode::End => self.input.cursor = self.input.buf.chars().count(),
            KeyCode::Up => self.history_prev(),
            KeyCode::Down => self.history_next(),
            KeyCode::Left => self.input.cursor = self.input.cursor.saturating_sub(1),
            KeyCode::Right => {
                let max = self.input.buf.chars().count();
                self.input.cursor = (self.input.cursor + 1).min(max);
            }
            KeyCode::Backspace if alt => self.input.delete_word_back(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Tab => {
                let menu = self.slash_matches();
                if !menu.is_empty() {
                    let cmd = menu[self.slash_sel.min(menu.len() - 1)];
                    self.input.set(format!("/{} ", cmd.name));
                }
            }
            KeyCode::Char(ch) if !ctrl => {
                self.input.insert(ch);
                self.slash_sel = 0;
            }
            _ => {}
        }

        // slash menu selection with arrows while menu is visible
        if self.input.buf.starts_with('/') {
            let n = self.slash_matches().len();
            if n > 0 {
                match key.code {
                    KeyCode::Up => {
                        self.slash_sel = self.slash_sel.checked_sub(1).unwrap_or(n - 1);
                    }
                    KeyCode::Down => self.slash_sel = (self.slash_sel + 1) % n,
                    _ => {}
                }
            }
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent, ctl: &Controller) {
        let Some((sel, items)) = &mut self.model_picker else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.model_picker = None,
            KeyCode::Up => *sel = sel.checked_sub(1).unwrap_or(items.len() - 1),
            KeyCode::Down => *sel = (*sel + 1) % items.len(),
            KeyCode::Enter => {
                let model = items[*sel].clone();
                self.model_picker = None;
                self.set_model(model, ctl);
            }
            _ => {}
        }
    }

    fn open_model_picker(&mut self) {
        let mut items: Vec<String> = MODEL_PRESETS.iter().map(|s| s.to_string()).collect();
        if !items.contains(&self.cfg.model) {
            items.insert(0, self.cfg.model.clone());
        }
        let sel = items.iter().position(|m| *m == self.cfg.model).unwrap_or(0);
        self.model_picker = Some((sel, items));
    }

    fn set_model(&mut self, model: String, ctl: &Controller) {
        if model == self.cfg.model {
            return;
        }
        self.cfg.model = model.clone();
        ctl.send(Cmd::SetModel(model.clone()));
        self.transcript.push_notice(
            NoticeLevel::Info,
            format!("model → {model} (runtime restarts on next prompt)"),
        );
    }

    fn handle_esc(&mut self, ctl: &Controller) {
        if self.model_picker.is_some() {
            self.model_picker = None;
            return;
        }
        if self.input.buf.starts_with('/') && !self.slash_matches().is_empty() {
            self.input.clear();
            return;
        }
        match self.state {
            RunState::Running | RunState::Starting => {
                // grok: Esc cancels immediately; the draft survives.
                if ctl.interrupt_now() {
                    ctl.send(Cmd::Interrupt);
                    self.state_note = "cancelling".into();
                } else if ctl.is_attached() {
                    self.show_tip("plugin mode: the host turn cannot be interrupted from here");
                } else {
                    self.show_tip("demo turn — it finishes on its own");
                }
            }
            RunState::Idle => {
                if !self.input.is_empty() {
                    if self.esc_armed.take().is_some() {
                        self.input.history.push(self.input.buf.clone());
                        self.input.clear();
                        self.show_tip("draft cleared — ↑ recalls it");
                    } else {
                        self.esc_armed = Some(Instant::now());
                        self.show_tip("press esc again to clear the draft");
                    }
                }
            }
        }
    }

    fn handle_ctrl_c(&mut self, ctl: &Controller) {
        if !self.input.is_empty() {
            // grok: Ctrl+C clears the draft first, keeps the turn.
            self.input.history.push(self.input.buf.clone());
            self.input.clear();
            self.show_tip("draft cleared — ↑ recalls it");
            return;
        }
        if matches!(self.state, RunState::Running | RunState::Starting) {
            if ctl.interrupt_now() {
                ctl.send(Cmd::Interrupt);
                self.state_note = "cancelling".into();
            } else if ctl.is_attached() {
                self.show_tip("plugin mode: the host turn cannot be interrupted from here");
            }
            return;
        }
        if self.ctrl_c_armed.take().is_some() {
            self.quit = true;
        } else {
            self.ctrl_c_armed = Some(Instant::now());
            self.show_tip("press ctrl+c again to exit");
        }
    }

    fn history_prev(&mut self) {
        if !self.input.is_empty() && self.input.hist_pos.is_none() {
            return; // grok: history opens from an empty prompt
        }
        if self.input.history.is_empty() {
            return;
        }
        let pos = match self.input.hist_pos {
            None => {
                self.input.stash = self.input.buf.clone();
                self.input.history.len() - 1
            }
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.input.hist_pos = Some(pos);
        self.input.set(self.input.history[pos].clone());
    }

    fn history_next(&mut self) {
        let Some(pos) = self.input.hist_pos else {
            return;
        };
        if pos + 1 >= self.input.history.len() {
            self.input.hist_pos = None;
            let stash = std::mem::take(&mut self.input.stash);
            self.input.set(stash);
        } else {
            self.input.hist_pos = Some(pos + 1);
            self.input.set(self.input.history[pos + 1].clone());
        }
    }

    fn accept_slash(&mut self, cmd: &'static SlashCommand, ctl: &Controller) {
        let line = self.input.buf.clone();
        let rest = line
            .strip_prefix('/')
            .and_then(|s| s.strip_prefix(cmd.name))
            .unwrap_or("")
            .trim()
            .to_string();
        self.input.clear();
        self.slash_sel = 0;
        self.run_slash(cmd.name, &rest, ctl);
    }

    pub fn run_slash(&mut self, name: &str, arg: &str, ctl: &Controller) {
        match name {
            "help" => self.push_help(),
            "keys" => self.push_keys(),
            "clear" => {
                self.transcript.clear();
                self.transcript
                    .push_notice(NoticeLevel::Info, "scrollback cleared".into());
            }
            "quit" => self.quit = true,
            "logo" => self.transcript.push_notice(
                NoticeLevel::Info,
                "the whale lives in the banner — resize the terminal and it adapts".into(),
            ),
            "theme" => {
                let next = match arg {
                    "light" => Theme::light(),
                    "dark" => Theme::dark(),
                    _ => self.theme.toggled(),
                };
                self.theme = next;
            }
            "model" => {
                if arg.is_empty() {
                    self.open_model_picker();
                } else {
                    self.set_model(arg.to_string(), ctl);
                }
            }
            "new" => {
                let id = if arg.is_empty() {
                    format!("dsh-{}", timestamp())
                } else {
                    arg.to_string()
                };
                self.session_id = id.clone();
                self.transcript.set_root_session(id.clone());
                self.transcript
                    .push_notice(NoticeLevel::Info, format!("new session · {id}"));
            }
            "session" => self.push_session_info(),
            other => {
                self.transcript.push_notice(
                    NoticeLevel::Warn,
                    format!("unknown command /{other} — /help lists commands"),
                );
            }
        }
    }

    fn push_help(&mut self) {
        let text = "\
DeepSeek Build (dsh-tui) — deepseek-harness in your terminal

  enter        send · queues a follow-up while a turn runs
  alt+enter    interrupt the turn and send now
  esc          interrupt (draft survives) · 2× clears the draft
  ctrl+c       clear draft · interrupt · 2× quits
  !cmd         run a local shell command (not the agent)
  /model       switch model   /new fresh session   /theme dark|light
  ctrl+e       expand thoughts + tool output   ctrl+l clear
  pgup/pgdn    scroll · mouse wheel works · end follows the tail

Info shown per turn: streamed reasoning, streamed answer, every tool
call with its result, injected context, subagent lifecycles, token
usage (incl. cache hits), and the turn end reason.";
        self.transcript.push_notice(NoticeLevel::Info, text.into());
    }

    fn push_keys(&mut self) {
        let text = "\
keys — grok-build homage set
  enter send · alt+enter send-now · esc / ctrl+c cancel semantics
  ↑ history (empty prompt) · tab completes /commands
  ctrl+m model picker · ctrl+t theme · ctrl+e expand · ctrl+l clear
  ctrl+u/ctrl+d half-page · pgup/pgdn page · home/end top/tail
  ctrl+a/ctrl+k/ctrl+w readline editing · alt+backspace word";
        self.transcript.push_notice(NoticeLevel::Info, text.into());
    }

    fn push_session_info(&mut self) {
        let creds = if self.demo {
            "demo mode (no API calls)".to_string()
        } else if self.cfg.has_credentials() {
            "api key present".to_string()
        } else {
            "DEEPSEEK_API_KEY not set".to_string()
        };
        let text = format!(
            "session · {}\nprovider · {} / {}\nworkspace · {}\nsession root · {}\nruntime · {}\nserver · {}\ncredentials · {}\ntokens · ↑{} ↓{} (cached {}, reasoning {})",
            self.session_id,
            self.cfg.provider,
            self.cfg.model,
            self.cfg.workspace,
            self.cfg.session_root,
            self.cfg.bin,
            self.server_info.as_deref().unwrap_or("not started"),
            creds,
            self.transcript.usage.input,
            self.transcript.usage.output,
            self.transcript.usage.cached,
            self.transcript.usage.reasoning,
        );
        self.transcript.push_notice(NoticeLevel::Info, text);
    }

    fn submit(&mut self, ctl: &Controller) {
        let text = self.input.buf.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(cmdline) = text.strip_prefix('/') {
            let mut parts = cmdline.splitn(2, ' ');
            let name = parts.next().unwrap_or("").to_string();
            let arg = parts.next().unwrap_or("").trim().to_string();
            self.input.history.push(text.clone());
            self.input.clear();
            self.run_slash(&name, &arg, ctl);
            return;
        }
        if let Some(cmd) = text.strip_prefix('!') {
            let cmd = cmd.trim().to_string();
            if !cmd.is_empty() {
                self.input.history.push(text.clone());
                self.input.clear();
                self.run_local_shell(cmd);
            }
            return;
        }

        self.input.history.push(text.clone());
        self.input.clear();
        let running = matches!(self.state, RunState::Running | RunState::Starting);
        self.transcript.push_user(text.clone(), running);
        if running {
            self.queued += 1;
            self.show_tip("queued — lands after this turn · alt+enter would send now");
        } else {
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
            self.state_note = if self.demo { String::new() } else { "contacting runtime".into() };
        }
        self.scroll_up = 0;
        ctl.send(Cmd::Prompt {
            session_id: self.session_id.clone(),
            text,
        });
    }

    /// grok send-now: cancel the current turn and run this message next.
    fn send_now(&mut self, ctl: &Controller) {
        let text = self.input.buf.trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut interrupted = true;
        if matches!(self.state, RunState::Running | RunState::Starting) {
            interrupted = ctl.interrupt_now();
            if interrupted {
                ctl.send(Cmd::Interrupt);
            }
        }
        self.input.history.push(text.clone());
        self.input.clear();
        self.transcript.push_user(text.clone(), !interrupted);
        if !interrupted && matches!(self.state, RunState::Running | RunState::Starting) {
            self.queued += 1;
            self.show_tip("cannot interrupt here — queued for the next turn instead");
        } else {
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
        }
        self.scroll_up = 0;
        ctl.send(Cmd::Prompt {
            session_id: self.session_id.clone(),
            text,
        });
    }

    /// Submit a prompt programmatically (used by DSH_TUI_AUTOPROMPT).
    pub fn auto_prompt(&mut self, text: &str, ctl: &Controller) {
        self.input.set(text.to_string());
        self.submit(ctl);
    }

    fn run_local_shell(&mut self, cmd: String) {
        self.shell_seq += 1;
        let id = self.shell_seq;
        let cell = self.transcript.push_shell(cmd.clone());
        self.shell_pending.push((id, cell));
        let tx = self.bus_tx.clone();
        let cwd = self.cfg.workspace.clone();
        std::thread::spawn(move || {
            let out = std::process::Command::new("sh")
                .arg("-lc")
                .arg(&cmd)
                .current_dir(&cwd)
                .output();
            let (code, text) = match out {
                Ok(o) => {
                    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                    let e = String::from_utf8_lossy(&o.stderr);
                    if !e.trim().is_empty() {
                        if !s.is_empty() {
                            s.push('\n');
                        }
                        s.push_str(&e);
                    }
                    const LIMIT: usize = 16 * 1024;
                    if s.len() > LIMIT {
                        let mut cut = LIMIT;
                        while cut > 0 && !s.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        s.truncate(cut);
                        s.push_str("\n… (truncated)");
                    }
                    (o.status.code(), s)
                }
                Err(err) => (None, format!("failed to run: {err}")),
            };
            let _ = tx.send(AppEvent::ShellDone {
                id,
                code,
                output: text,
            });
        });
    }
}

pub fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs:x}")
}
