//! App state and input handling — the grok-build interaction homage.
//!
//! Enter sends (or queues mid-turn, client-side); Ctrl+X steers the active
//! turn immediately; Esc cancels a running turn with the draft preserved, and
//! Esc owns interrupt; Ctrl+C clears a draft, then needs two empty presses to quit;
//! `!` runs a command in the session's local shell; `/` opens the slash menu; Up recalls
//! history on an empty prompt.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use unicode_width::UnicodeWidthChar;

use crate::bus::{
    permission_ask_default_sel, AppEvent, Cmd, CtlEvent, PermissionAskOption, PermissionAskReply,
    SessionListItem,
};
use crate::controller::Controller;
use crate::events::parse_notification;
use crate::input::Action;
use crate::locale::{Locale, UiSettings};
use crate::runtime::{legacy_settings_path, settings_path, RuntimeConfig};
use crate::theme::Theme;
use crate::transcript::{clamp_str, NoticeLevel, Transcript};

pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);
const CTRL_C_QUIT_WINDOW: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy)]
struct CtrlCQuitChord {
    started: Instant,
    presses: u8,
    required: u8,
}
const TIP_TTL: Duration = Duration::from_secs(4);

/// Some terminal layers incorrectly wrap Kitty/CSI-u key reports in
/// bracketed-paste markers. Crossterm then exposes the key bytes as a paste,
/// so recover them only when the *entire* payload is made of CSI-u keys.
fn decode_leaked_csi_u_keys(text: &str) -> Option<Vec<KeyEvent>> {
    if text.is_empty() {
        return None;
    }

    let mut rest = text;
    let mut keys = Vec::new();
    while !rest.is_empty() {
        let encoded = rest.strip_prefix("\u{1b}[")?;
        let end = encoded.find('u')?;
        keys.push(decode_csi_u_key(&encoded[..end])?);
        rest = &encoded[end + 1..];
    }
    Some(keys)
}

fn decode_csi_u_key(params: &str) -> Option<KeyEvent> {
    let mut fields = params.split(';');
    let codepoint = fields.next()?.split(':').next()?.parse::<u32>().ok()?;
    let modifier_and_kind = fields.next();
    // Text-as-codepoints and any other trailing fields are deliberately not
    // recovered: falling back to ordinary paste is safer than guessing.
    if fields.next().is_some() {
        return None;
    }

    let (modifier_mask, kind) = match modifier_and_kind {
        Some(field) => {
            let mut parts = field.split(':');
            let mask = parts.next()?.parse::<u32>().ok()?;
            if mask == 0 {
                return None;
            }
            let kind = match parts.next() {
                None | Some("1") => KeyEventKind::Press,
                Some("2") => KeyEventKind::Repeat,
                Some("3") => KeyEventKind::Release,
                Some(_) => return None,
            };
            if parts.next().is_some() {
                return None;
            }
            (mask - 1, kind)
        }
        None => (0, KeyEventKind::Press),
    };

    let mut modifiers = KeyModifiers::NONE;
    if modifier_mask & 1 != 0 {
        modifiers |= KeyModifiers::SHIFT;
    }
    if modifier_mask & 2 != 0 {
        modifiers |= KeyModifiers::ALT;
    }
    if modifier_mask & 4 != 0 {
        modifiers |= KeyModifiers::CONTROL;
    }
    if modifier_mask & 8 != 0 {
        modifiers |= KeyModifiers::SUPER;
    }
    if modifier_mask & 16 != 0 {
        modifiers |= KeyModifiers::HYPER;
    }
    if modifier_mask & 32 != 0 {
        modifiers |= KeyModifiers::META;
    }

    let ch = char::from_u32(codepoint)?;
    let code = match ch {
        '\u{1b}' => KeyCode::Esc,
        '\r' => KeyCode::Enter,
        '\t' if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
        '\t' => KeyCode::Tab,
        '\u{7f}' => KeyCode::Backspace,
        _ => KeyCode::Char(ch),
    };
    Some(KeyEvent::new_with_kind(code, modifiers, kind))
}

pub struct SlashCommand {
    pub name: &'static str,
    pub usage: &'static str,
    pub desc: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "help",
        usage: "/help",
        desc: "show help and tips",
    },
    SlashCommand {
        name: "keys",
        usage: "/keys",
        desc: "keyboard shortcuts",
    },
    SlashCommand {
        name: "new",
        usage: "/new [id]",
        desc: "start a fresh session",
    },
    SlashCommand {
        name: "resume",
        usage: "/resume [id]",
        desc: "resume a durable session from this workspace",
    },
    SlashCommand {
        name: "clear",
        usage: "/clear",
        desc: "clear the scrollback",
    },
    SlashCommand {
        name: "model",
        usage: "/model [id]",
        desc: "switch model · live over ACP",
    },
    SlashCommand {
        name: "agent",
        usage: "/agent [id]",
        desc: "switch agent preset · ctrl+shift+a",
    },
    SlashCommand {
        name: "effort",
        usage: "/effort [off|high|max]",
        desc: "reasoning effort for this session",
    },
    SlashCommand {
        name: "permission",
        usage: "/permission [preset]",
        desc: "permission preset picker · shift+tab cycles",
    },
    SlashCommand {
        name: "plan",
        usage: "/plan [on|off]",
        desc: "toggle host plan mode",
    },
    SlashCommand {
        name: "image",
        usage: "/image <path> [text]",
        desc: "send a local image (png/jpeg/webp/gif)",
    },
    SlashCommand {
        name: "clip",
        usage: "/clip [text]",
        desc: "attach the clipboard image (macOS/Linux)",
    },
    SlashCommand {
        name: "theme",
        usage: "/theme [id]",
        desc: "switch theme plugin · ctrl+t toggles dark/light",
    },
    SlashCommand {
        name: "ui",
        usage: "/ui [id]",
        desc: "switch UI Plugin",
    },
    SlashCommand {
        name: "plugins",
        usage: "/plugins",
        desc: "show static Host plugins",
    },
    SlashCommand {
        name: "cordis-plugins",
        usage: "/cordis-plugins",
        desc: "review or manage dynamic Cordis plugins",
    },
    SlashCommand {
        name: "session",
        usage: "/session",
        desc: "show session + runtime info",
    },
    SlashCommand {
        name: "auth",
        usage: "/auth [method|api-key]",
        desc: "ACP sign-in (Backchat authenticate)",
    },
    SlashCommand {
        name: "lang",
        usage: "/lang [zh|en]",
        desc: "switch interface language",
    },
    SlashCommand {
        name: "liang",
        usage: "/liang [on|off]",
        desc: "召唤小难梁 — 🤫 idle · ⌨︎ working",
    },
    SlashCommand {
        name: "quit",
        usage: "/quit",
        desc: "exit dsh-tui",
    },
];

pub const MODEL_PRESETS: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4",
    "deepseek-v3.2",
    "deepseek-chat",
    "deepseek-reasoner",
];

/// Demo seeds for `/agent` when no agent catalog has arrived. Live ACP
/// replaces these with the extra composition select the agent advertised.
/// Shipped creator id is `cordis`.
pub const AGENT_MODES: &[(&str, &str, &str)] = &[
    (
        "standard",
        "Standard mode",
        "full coding agent · files, shell, search, skills, subagents",
    ),
    (
        "code",
        "Code mode",
        "standard tools driven from one TypeScript program",
    ),
    (
        "minimal",
        "Minimal mode",
        "two tools · persistent bash + str_replace_editor",
    ),
    (
        "cordis",
        "Creator mode",
        "standard + runtime inspection and preset authoring",
    ),
];

/// The stock permission presets (id, one-line meaning) — the default table
/// `@deepseek-ai/dsh-permission-presets` ships. Shift+Tab cycles them;
/// `/permission <name>` passes any other id through for profiles with a
/// custom preset table (the host validates and lists what it knows).
pub const PERMISSION_PRESETS: &[(&str, &str)] = &[
    ("read-only", "read only — no file writes"),
    (
        "workspace-write",
        "write inside the workspace · wider actions ask for approval",
    ),
    (
        "danger-full-access",
        "full file access · approval prompts off — trusted dirs only",
    ),
];

/// Map common spellings onto the stock preset ids (`full` →
/// `danger-full-access`, `ws` → `workspace-write`, `ro` → `read-only`, …).
pub fn normalize_permission(arg: &str) -> Option<&'static str> {
    match arg.trim().to_ascii_lowercase().as_str() {
        "read-only" | "readonly" | "read" | "ro" => Some("read-only"),
        "workspace-write" | "workspace" | "write" | "ws" | "safe" | "sandbox" => {
            Some("workspace-write")
        }
        "danger-full-access" | "full-access" | "full" | "danger" | "yolo" => {
            Some("danger-full-access")
        }
        _ => None,
    }
}

/// User-facing permission label, mirroring the Web's `displayPermissionPreset`:
/// `danger-full-access` → "Full access"; kebab-case keys are title-cased.
pub fn permission_label(id: &str) -> String {
    if id == "danger-full-access" {
        return "Full access".to_string();
    }
    let kebab = !id.is_empty()
        && id.split('-').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if !kebab {
        return id.to_string();
    }
    id.split('-')
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn permission_picker_items(
    modes: &[crate::bus::CatalogPreset],
    reported: Option<&str>,
    current: &str,
) -> Vec<PickerItem> {
    modes
        .iter()
        .map(|p| {
            let mark = if reported == Some(p.id.as_str()) {
                " · current"
            } else if reported.is_none() && p.id == current {
                " · default"
            } else {
                ""
            };
            PickerItem {
                id: p.id.clone(),
                label: permission_label(&p.id),
                meta: format!("{}{mark}", p.description),
                provider: None,
            }
        })
        .collect()
}

/// Map a file extension to the attachment media type the host accepts.
fn media_type_for(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// Read the raster image currently on the system clipboard as (bytes, media
/// type). Terminals don't deliver image paste over stdin, so this shells out
/// to the platform clipboard tool instead.
#[cfg(target_os = "macos")]
fn read_clipboard_image() -> Option<(Vec<u8>, &'static str)> {
    let tmp = std::env::temp_dir().join(format!("dsh-clip-{}.png", std::process::id()));
    let tmp_s = tmp.to_str()?.to_string();
    let script = format!(
        "set out to \"{tmp_s}\"\n\
         set d to (the clipboard as «class PNGf»)\n\
         set h to open for access (POSIX file out) with write permission\n\
         write d to h as «class PNGf»\n\
         close access h\n\
         return out"
    );
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .ok()?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    let bytes = std::fs::read(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    Some((bytes, "image/png"))
}

#[cfg(target_os = "linux")]
fn read_clipboard_image() -> Option<(Vec<u8>, &'static str)> {
    let attempts: &[(&str, &[&str])] = &[
        ("wl-paste", &["--type", "image/png"]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
    ];
    for (cmd, args) in attempts {
        if let Ok(out) = std::process::Command::new(cmd).args(*args).output() {
            if out.status.success() && !out.stdout.is_empty() {
                return Some((out.stdout, "image/png"));
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_clipboard_image() -> Option<(Vec<u8>, &'static str)> {
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Starting,
    Running,
}

pub use crate::input::Input;

/// One endpoint of a mouse selection in chat-layout coordinates: `line`
/// indexes the full wrapped layout (`ChatView::lines`), `col` is a display
/// cell column within the chat pane.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelPoint {
    pub line: usize,
    pub col: usize,
}

/// In-app mouse selection — the grok-build gesture: drag highlights,
/// releasing the button copies (选中完即 copy). `anchor` is where the drag
/// started; `head` follows the pointer and may precede the anchor.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub anchor: SelPoint,
    pub head: SelPoint,
}

impl Selection {
    /// (start, end) in document order; `end` is inclusive (the cell under
    /// the pointer is part of the selection).
    pub fn ordered(&self) -> (SelPoint, SelPoint) {
        if (self.head.line, self.head.col) < (self.anchor.line, self.anchor.col) {
            (self.head, self.anchor)
        } else {
            (self.anchor, self.head)
        }
    }

    fn is_caret(&self) -> bool {
        self.anchor == self.head
    }
}

/// Snapshot of the chat pane layout from the last draw — the seam that
/// mouse hit-testing and copy extraction read (grok-build's resolved
/// selection model, scaled way down): pane rect, index of the first
/// visible layout line, and the plain text of every layout line.
/// A thumbnail the terminal should draw over the chat pane (kitty graphics).
pub struct ThumbPlacement {
    pub id: u32,
    pub rect: ratatui::layout::Rect,
    pub data: std::sync::Arc<[u8]>,
}

#[derive(Default)]
pub struct ChatView {
    pub area: ratatui::layout::Rect,
    pub top: usize,
    pub lines: Vec<String>,
    /// Per layout line, the transcript cell that owns it (only tool cells
    /// claim ownership) — the seam for click-to-expand.
    pub owners: Vec<Option<usize>>,
    /// Visible image thumbnails, filled by `ui::draw_chat` every frame.
    pub images: Vec<ThumbPlacement>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Model,
    Effort,
    Mode,
    Theme,
    UiPlugin,
    Permission,
    Session,
    Subagent,
    Auth,
    StaticPlugin,
    CordisPlugin,
    CordisApproval,
}

#[derive(Clone)]
pub struct PickerItem {
    pub id: String,
    pub label: String,
    pub meta: String,
    pub provider: Option<String>,
}

/// Fixed display width of the picker label column — rows pad/truncate to
/// this so the meta column lines up (char-based padding would misalign
/// CJK labels).
pub(crate) const PICKER_LABEL_COL: usize = 30;

/// One `/` menu entry: a builtin [`SlashCommand`] or a host skill (plugin
/// mode). Builtins win a name collision — the command namespace is closed
/// and resolved client-side before a line ever becomes a prompt; skill
/// lines ship as prompts the host expands.
#[derive(Clone)]
pub struct SlashEntry {
    pub name: String,
    pub usage: String,
    pub desc: String,
    pub skill: bool,
    pub plugin: bool,
    /// Full composer text for an argument candidate. Command-name rows leave
    /// this empty and retain the historical `/name ` tab completion.
    pub completion: Option<String>,
}

#[derive(Clone, serde::Deserialize)]
struct PluginCommand {
    name: String,
    description: String,
    #[serde(default)]
    input: Option<PluginCommandInput>,
}

#[derive(Clone, serde::Deserialize)]
struct PluginCommandInput {
    hint: String,
    #[serde(default)]
    options: Vec<PluginCommandOption>,
}

#[derive(Clone, serde::Deserialize)]
struct PluginCommandOption {
    value: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(serde::Deserialize)]
struct PluginCommandCatalog {
    protocol: u64,
    commands: Vec<PluginCommand>,
}

#[derive(serde::Deserialize)]
struct PluginOverlaySnapshot {
    protocol: u64,
    overlay: Option<PluginOverlay>,
}

#[derive(serde::Deserialize)]
struct CordisApprovalsSnapshot {
    protocol: u64,
    approvals: Vec<crate::bus::PendingCordisApproval>,
}

#[derive(serde::Deserialize)]
struct UiPluginCatalog {
    protocol: u64,
    plugins: Vec<crate::bus::UiPluginItem>,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum PluginOverlay {
    Slider(SliderOverlay),
    Select(SelectOverlay),
    View(ViewOverlay),
}

#[derive(Clone, serde::Deserialize)]
pub struct SelectOverlay {
    pub id: String,
    pub title: String,
    pub value: String,
    pub options: Vec<SelectOption>,
    #[serde(skip)]
    pub sel: usize,
}

#[derive(Clone, serde::Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

fn select_initial_index(select: &SelectOverlay) -> Option<usize> {
    if select.id.is_empty() || select.title.is_empty() || select.options.is_empty() {
        return None;
    }
    let mut values = std::collections::HashSet::new();
    for option in &select.options {
        if option.value.is_empty()
            || option.label.is_empty()
            || !values.insert(option.value.as_str())
        {
            return None;
        }
    }
    select
        .options
        .iter()
        .position(|option| option.value == select.value)
}

#[derive(Clone, serde::Deserialize)]
pub struct SliderOverlay {
    pub id: String,
    pub title: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    #[serde(default)]
    pub marks: Vec<SliderMark>,
    #[serde(rename = "snapToMarks", default)]
    pub snap_to_marks: bool,
    pub value: f64,
}

#[derive(Clone, serde::Deserialize)]
pub struct SliderMark {
    pub value: f64,
    /// Optional host-side mark identity. Parsed for protocol compatibility;
    /// the client renders by value/label and reports the numeric value back,
    /// so the id is not consumed client-side yet.
    #[serde(default)]
    #[allow(dead_code)]
    pub id: Option<String>,
    pub label: String,
}

#[derive(Clone, serde::Deserialize)]
pub struct ViewOverlay {
    pub id: String,
    pub title: String,
    pub nodes: Vec<crate::slots::TuiNode>,
    #[serde(skip)]
    pub scroll: usize,
    /// Plugin-owned views report submit/cancel over the compositor plane;
    /// builtin chrome such as `/keys` closes entirely inside the painter.
    #[serde(skip)]
    pub(crate) notify_plugin: bool,
}

pub struct Picker {
    pub kind: PickerKind,
    pub title: String,
    pub sel: usize,
    pub items: Vec<PickerItem>,
}

pub struct SubagentView {
    pub id: String,
    pub parent: String,
    pub label: String,
    pub running: bool,
    pub transcript: Transcript,
}

/// Overlay for one ACP `session/request_permission` ask.
pub struct PermissionAskOverlay {
    pub title: String,
    pub sel: usize,
    pub options: Vec<PermissionAskOption>,
    pub(crate) reply: Option<tokio::sync::oneshot::Sender<PermissionAskReply>>,
}

/// One live ACP form elicitation. Its editor is separate from the composer.
pub struct ElicitationAskOverlay {
    pub form: crate::elicitation::ElicitationFormState,
    /// Scroll offset of the markdown description pane (render clamps it to
    /// the actual content height, so `usize::MAX` reliably reaches the end).
    pub scroll: usize,
    pub(crate) reply: Option<tokio::sync::oneshot::Sender<crate::elicitation::ElicitationReply>>,
}

impl Drop for ElicitationAskOverlay {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(crate::elicitation::ElicitationReply::Cancelled);
        }
    }
}

impl Drop for PermissionAskOverlay {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(PermissionAskReply::Cancelled);
        }
    }
}

/// Folded per-session mode state (from the durable event stream — the same
/// facts the Web UI chips read). Cached per workspace so chips and pickers
/// show the last-known values immediately on launch.
#[derive(Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Modes {
    pub plan: bool,
    pub sandbox: Option<String>,
    pub approval: Option<String>,
    pub permission: Option<String>,
    pub agent_preset: Option<String>,
    /// Reasoning effort as last requested from this client (`/effort`,
    /// the post-model-pick effort picker); the host doesn't echo one.
    pub effort: Option<String>,
}

struct ShellRequest {
    id: u64,
    command: String,
}

struct ShellWorker {
    tx: Sender<ShellRequest>,
}

impl ShellWorker {
    fn spawn(cwd: String, app_tx: Sender<AppEvent>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<ShellRequest>();
        std::thread::spawn(move || {
            let mut shell: Option<PersistentShell> = None;
            for request in rx {
                if shell.is_none() {
                    match PersistentShell::spawn(&cwd) {
                        Ok(started) => shell = Some(started),
                        Err(err) => {
                            let _ = app_tx.send(AppEvent::ShellDone {
                                id: request.id,
                                code: None,
                                output: format!("failed to start shell: {err}"),
                            });
                            continue;
                        }
                    }
                }

                let result = shell.as_mut().unwrap().run(request.id, &request.command);
                let (code, output, alive) = match result {
                    Ok(result) => result,
                    Err(err) => (None, format!("shell failed: {err}"), false),
                };
                if !alive {
                    shell = None;
                }
                let _ = app_tx.send(AppEvent::ShellDone {
                    id: request.id,
                    code,
                    output,
                });
            }
        });
        Self { tx }
    }

    fn send(&self, request: ShellRequest) -> Result<(), ShellRequest> {
        self.tx.send(request).map_err(|err| err.0)
    }
}

struct PersistentShell {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    marker: String,
}

impl PersistentShell {
    fn spawn(cwd: &str) -> std::io::Result<Self> {
        let mut child = std::process::Command::new("sh")
            .arg("-l")
            .arg("-s")
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "shell stdin unavailable")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "shell stdout unavailable")
        })?;
        // Keep a control fd independent from shell-level stdout redirects.
        stdin.write_all(b"exec 9>&1\n")?;
        stdin.flush()?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            marker: format!("MARTTY_SHELL_{}_{}", std::process::id(), timestamp()),
        })
    }

    fn run(&mut self, id: u64, command: &str) -> std::io::Result<(Option<i32>, String, bool)> {
        let marker = format!("\x1e{}:{id}:", self.marker);
        writeln!(self.stdin, "eval {} 2>&1", shell_quote(command))?;
        writeln!(self.stdin, "__martty_shell_status=$?")?;
        writeln!(
            self.stdin,
            "command printf '\\036{}:{id}:%s\\037' \"$__martty_shell_status\" >&9",
            self.marker
        )?;
        self.stdin.flush()?;

        let mut captured = Vec::new();
        loop {
            let read = self.stdout.read_until(0x1f, &mut captured)?;
            if read == 0 {
                let code = self.child.wait().ok().and_then(|status| status.code());
                return Ok((code, shell_output(captured), false));
            }
            let Some(start) = find_bytes(&captured, marker.as_bytes()) else {
                continue;
            };
            let status_start = start + marker.len();
            let Some(status_len) = captured[status_start..]
                .iter()
                .position(|byte| *byte == 0x1f)
            else {
                continue;
            };
            let status = std::str::from_utf8(&captured[status_start..status_start + status_len])
                .ok()
                .and_then(|value| value.parse::<i32>().ok());
            captured.truncate(start);
            return Ok((status, shell_output(captured), true));
        }
    }
}

impl Drop for PersistentShell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\\"'\\\"'"))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn shell_output(bytes: Vec<u8>) -> String {
    let mut output = String::from_utf8_lossy(&bytes).into_owned();
    const LIMIT: usize = 16 * 1024;
    if output.len() > LIMIT {
        let mut cut = LIMIT;
        while cut > 0 && !output.is_char_boundary(cut) {
            cut -= 1;
        }
        output.truncate(cut);
        output.push_str("\n… (truncated)");
    }
    output
}

pub struct App {
    pub theme: Theme,
    pub locale: Locale,
    pub palettes: Vec<crate::theme::PalettePack>,
    pub active_palette_id: String,
    /// Persisted UI Preset id. The Client compositor owns activation; Rust
    /// keeps this only as a startup fallback before slot snapshots arrive.
    pub ui_preset: String,
    /// Latest compositor-private snapshots, keyed by declared Client slot.
    pub slot_snapshots: HashMap<String, crate::slots::SlotSnapshot>,
    pub transcript: Transcript,
    pub subagents: Vec<SubagentView>,
    pub active_subagent: Option<String>,
    pub input: Input,
    /// Display-cell width of the composer text well from the latest frame.
    pub(crate) composer_wrap_width: usize,
    pub state: RunState,
    pub state_note: String,
    /// Welcome banner (whale + wordmark) — shown until the first real prompt.
    pub show_banner: bool,
    /// Pixel-art Liang at the composer's right edge (`/liang` toggles him).
    pub pet_visible: bool,
    /// True when the terminal speaks the kitty graphics protocol: image
    /// thumbnails and the background layer emit real pixels (set by `main`).
    pub pet_pixels: bool,
    pub run_started: Option<Instant>,
    pub spinner_idx: usize,
    pub scroll_up: usize, // lines above the bottom; 0 = follow
    /// Mouse selection over the chat pane (drag-to-select, copy on release).
    pub sel: Option<Selection>,
    /// A left-button drag is in progress.
    selecting: bool,
    last_click: Option<(Instant, u16, u16)>,
    /// Filled by `ui::draw_chat` every frame.
    pub chat_view: ChatView,
    /// Sessions offered by the open `/resume` picker (id → file lookup).
    resume_candidates: Vec<crate::sessions::SessionSummary>,
    /// `/resume` picker rows came from ACP `session/list` (pick → `session/load`).
    resume_via_acp: bool,
    /// Agent advertised `loadSession`.
    load_session: bool,
    /// Last ACP `session_info_update` title.
    session_title: Option<String>,
    pub slash_sel: usize,
    pub picker: Option<Picker>,
    /// Rows the open picker actually shows (`h - border`), recorded by
    /// `ui::draw_model_picker` — the page size for picker PageUp/PageDown.
    pub picker_page_rows: usize,
    /// ACP tool permission ask (separate from `/permission` session modes).
    pub permission_ask: Option<PermissionAskOverlay>,
    /// Standard ACP form elicitation, above permission and picker overlays.
    pub elicitation_ask: Option<ElicitationAskOverlay>,
    /// Client Plugin modal rendered and driven by the native compositor.
    pub slider_overlay: Option<SliderOverlay>,
    /// Client Plugin single-select form rendered by the native compositor.
    pub select_overlay: Option<SelectOverlay>,
    /// Read-only modal rendered from a semantic TuiNode tree. Client Plugins
    /// and builtin chrome such as `/keys` share this surface.
    pub view_overlay: Option<ViewOverlay>,
    /// Images staged in the composer as inline `[image N]` chips living in
    /// the draft text; editing a token away un-stages its image.
    pub pending_images: crate::attachments::Staged,
    /// Screen rects of the inline chips this frame (hover/cursor preview
    /// hit-testing; recorded by `ui::draw_input`).
    pub att_chips: Vec<(ratatui::layout::Rect, usize)>,
    /// Clickable semantic actions contributed by the current slot frame.
    pub(crate) slot_actions: Vec<(ratatui::layout::Rect, crate::slots::TuiAction)>,
    /// Kitty-graphics placement for the hover-preview popup this frame.
    pub att_thumbs: Vec<ThumbPlacement>,
    /// Chip index under the mouse pointer (grok-style hover preview).
    pub hover_att: Option<usize>,
    pub modes: Modes,
    /// User-invocable host skills (`available_commands_update`); merged into
    /// the slash menu after the builtins.
    pub skills: Vec<crate::bus::SkillInfo>,
    /// Client Plugin commands are compositor-private and live exactly as long
    /// as their owning Plugin Fiber.
    plugin_commands: Vec<PluginCommand>,
    /// Last Host Loader inventory (`/plugins`).
    static_plugins: Vec<crate::bus::StaticPluginItem>,
    /// Last backend-owned dynamic plugin inventory (`/cordis-plugins`).
    cordis_plugins: Vec<crate::bus::CordisPluginItem>,
    /// Model-requested dynamic activations awaiting a decision.
    pub(crate) pending_cordis_approvals: Vec<crate::bus::PendingCordisApproval>,
    /// ACP-carried UI Plugin catalog (`/ui`).
    ui_plugins: Vec<crate::bus::UiPluginItem>,
    /// Last advertised composition select (`/agent`).
    last_presets: Vec<crate::bus::CatalogPreset>,
    /// Last advertised ACP model select (`/model`).
    last_models: Vec<crate::bus::CatalogModel>,
    /// Last advertised session modes (`/permission`, shift+tab).
    permission_choices: Vec<crate::bus::CatalogPreset>,
    /// Last advertised effort catalog for the current model.
    effort_choices: Vec<String>,
    pub tip: Option<(String, Instant)>,
    /// DSH_TUI_KEYDEBUG=1: echo every delivered key event in the tip row.
    key_debug: bool,
    pub ambient_tip_idx: usize,
    pub ambient_tip_at: Instant,
    ctrl_c_armed: Option<CtrlCQuitChord>,
    pub session_id: String,
    pub cfg: RuntimeConfig,
    /// Model explicitly picked this session (`/model`); wins over
    /// `transcript.last_model` in the chip until a turn realizes it.
    pub selected_model: Option<String>,
    pub demo: bool,
    /// A live ACP agent owns runtime, credentials, and its advertised catalog.
    pub attached: bool,
    /// A real `session/new` or `session/load` has supplied this session id.
    /// Cached session options stay hidden until this becomes true.
    pub session_bound: bool,
    /// ACP initialize / authenticate status (live ACP only).
    pub auth: crate::acp_auth::AuthSnapshot,
    /// Leave the TUI and run this agent login, then `authenticate`.
    pending_terminal_auth: Option<crate::acp_auth::TerminalAuthLaunch>,
    pub quit: bool,
    pub queued: usize,
    /// Transcript cells grouped by the client FIFO prompt that owns them.
    queued_cells: VecDeque<Vec<usize>>,
    /// Send Now bubbles awaiting the concurrent ACP request result.
    pending_steer_cells: HashMap<u64, Vec<usize>>,
    next_prompt_id: u64,
    /// A first prompt was handed to the controller but has not reached the
    /// ACP request task yet. Runtime startup alone does not make a turn busy.
    prompt_pending: bool,
    shell_seq: u64,
    shell_pending: Vec<(u64, usize)>, // (id, cell idx)
    shell_worker: Option<ShellWorker>,
    bus_tx: Sender<AppEvent>,
    pub server_info: Option<String>,
    pub needs_redraw: bool,
}

fn ui_session(event: &crate::events::UiEvent) -> Option<&str> {
    use crate::events::UiEvent;
    match event {
        UiEvent::SessionStatus { session, .. }
        | UiEvent::TurnStart { session, .. }
        | UiEvent::TurnEnd { session, .. }
        | UiEvent::TextDelta { session, .. }
        | UiEvent::ReasoningDelta { session, .. }
        | UiEvent::AssistantFinal { session, .. }
        | UiEvent::ToolCall { session, .. }
        | UiEvent::ToolResult { session, .. }
        | UiEvent::Usage { session, .. }
        | UiEvent::UserInjected { session, .. }
        | UiEvent::UserMessage { session, .. }
        | UiEvent::SessionTitle { session, .. }
        | UiEvent::Plan { session, .. }
        | UiEvent::PlanMode { session, .. }
        | UiEvent::SandboxMode { session, .. }
        | UiEvent::ApprovalPolicy { session, .. }
        | UiEvent::PermissionPreset { session, .. }
        | UiEvent::AgentPreset { session, .. }
        | UiEvent::ApprovalAsked { session, .. }
        | UiEvent::ApprovalDecided { session, .. } => Some(session),
        UiEvent::SubagentStarted { .. }
        | UiEvent::SubagentFinished { .. }
        | UiEvent::Palette { .. } => None,
    }
}

/// Byte offset of char index `i` in `s` (end-of-string when past it).
fn byte_of(s: &str, i: usize) -> usize {
    s.char_indices().nth(i).map(|(b, _)| b).unwrap_or(s.len())
}

/// Draft pieces after stripping `[image n]` chips, still in reading order.
enum StagedBlock {
    Text(String),
    Image(crate::attachments::Attachment),
}

fn token_spans_in(
    buf: &str,
    attachments: &[crate::attachments::Attachment],
) -> Vec<(usize, usize, usize)> {
    let mut spans = Vec::new();
    for (idx, att) in attachments.iter().enumerate() {
        if let Some(byte) = buf.find(&att.token) {
            let start = buf[..byte].chars().count();
            spans.push((start, start + att.token.chars().count(), idx));
        }
    }
    spans.sort_unstable();
    spans
}

/// 8-char id prefix — unique enough in the picker while staying readable;
/// `/resume <prefix>` still matches against the full id.
fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// One `/resume` row. The label is the session's human handle — the
/// harness title, else the first real prompt; the meta line carries the id
/// prefix plus age · turns (local logs) or the updated date (ACP rows with
/// no local log).
fn session_picker_row(
    id: &str,
    title: Option<&str>,
    local: Option<&crate::sessions::SessionSummary>,
    updated_at: Option<&str>,
) -> PickerItem {
    let short = short_id(id);
    let label = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| local.map(|s| s.preview.clone()))
        .filter(|s| !s.is_empty())
        .map(|s| clamp_str(&s, PICKER_LABEL_COL))
        .unwrap_or_else(|| short.clone());
    let meta = match local {
        Some(s) => format!(
            "{short:<8} · {} · {} turn{}",
            crate::sessions::age_label(s.modified),
            s.turns,
            if s.turns == 1 { "" } else { "s" },
        ),
        None => format!(
            "{short:<8} · {}",
            updated_at
                .and_then(|u| u.get(..10))
                .filter(|d| !d.is_empty())
                .unwrap_or("?"),
        ),
    };
    PickerItem {
        id: id.to_string(),
        label,
        meta,
        provider: None,
    }
}

fn unique_session_list_match(sessions: &[SessionListItem], prefix: &str) -> Result<String, String> {
    let matches: Vec<&SessionListItem> = sessions
        .iter()
        .filter(|s| s.id.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(format!(
            "no session matches “{prefix}” — /resume lists them"
        )),
        many => match many.iter().find(|s| s.id == prefix) {
            Some(one) => Ok(one.id.clone()),
            None => Err(format!(
                "“{prefix}” is ambiguous ({} matches) — /resume lists them",
                many.len()
            )),
        },
    }
}

fn trim_staged_blocks(blocks: &mut Vec<StagedBlock>) {
    while matches!(blocks.first(), Some(StagedBlock::Text(t)) if t.trim().is_empty()) {
        blocks.remove(0);
    }
    while matches!(blocks.last(), Some(StagedBlock::Text(t)) if t.trim().is_empty()) {
        blocks.pop();
    }
    if let Some(StagedBlock::Text(t)) = blocks.first_mut() {
        *t = t.trim().to_string();
    }
    if let Some(StagedBlock::Text(t)) = blocks.last_mut() {
        *t = t.trim().to_string();
    }
    blocks.retain(|b| !matches!(b, StagedBlock::Text(t) if t.is_empty()));
}

/// Split a composer draft on inline image chips, preserving 图文交替.
fn split_draft_into_staged_blocks(
    buf: &str,
    attachments: Vec<crate::attachments::Attachment>,
) -> Vec<StagedBlock> {
    let spans = token_spans_in(buf, &attachments);
    let chars: Vec<char> = buf.chars().collect();
    let mut slots: Vec<Option<crate::attachments::Attachment>> =
        attachments.into_iter().map(Some).collect();
    let mut blocks = Vec::new();
    let mut char_i = 0usize;
    for &(start, end, idx) in &spans {
        if start > char_i {
            let text: String = chars[char_i..start.min(chars.len())].iter().collect();
            if !text.is_empty() {
                blocks.push(StagedBlock::Text(text));
            }
        }
        if let Some(att) = slots.get_mut(idx).and_then(Option::take) {
            blocks.push(StagedBlock::Image(att));
        }
        char_i = end.min(chars.len());
    }
    if char_i < chars.len() {
        let text: String = chars[char_i..].iter().collect();
        if !text.is_empty() {
            blocks.push(StagedBlock::Text(text));
        }
    }
    trim_staged_blocks(&mut blocks);
    blocks
}

fn image_part_from(att: &crate::attachments::Attachment) -> crate::bus::ImagePart {
    crate::bus::ImagePart {
        data: crate::pet::base64(&att.data),
        media_type: att.media_type.clone(),
        name: att.name.clone(),
        path: att.path.clone(),
    }
}

fn prompt_blocks_from_staged(staged: Vec<StagedBlock>) -> Vec<crate::bus::PromptBlock> {
    staged
        .into_iter()
        .map(|block| match block {
            StagedBlock::Text(text) => crate::bus::PromptBlock::Text(text),
            StagedBlock::Image(att) => crate::bus::PromptBlock::Image(image_part_from(&att)),
        })
        .collect()
}

impl App {
    pub fn active_background(&self) -> Option<&crate::theme::ThemeBackground> {
        self.palettes
            .iter()
            .find(|pack| pack.id == self.active_palette_id && pack.loaded)
            .and_then(|pack| pack.background.as_ref())
    }

    pub fn canvas_background_color(&self) -> ratatui::style::Color {
        if self.pet_pixels && self.active_background().is_some() {
            ratatui::style::Color::Reset
        } else {
            self.theme.bg
        }
    }

    pub fn new(
        theme: Theme,
        cfg: RuntimeConfig,
        session_id: String,
        demo: bool,
        attached: bool,
        bus_tx: Sender<AppEvent>,
    ) -> Self {
        let palettes = vec![crate::theme::PalettePack::builtin_default()];
        let theme = palettes[0].theme(theme.mode);
        let settings = Self::load_settings(&cfg);
        let locale = settings.language;
        App {
            theme,
            locale,
            palettes,
            active_palette_id: "default".into(),
            ui_preset: settings.ui_preset,
            slot_snapshots: HashMap::new(),
            transcript: Transcript::new(session_id.clone()),
            subagents: Vec::new(),
            active_subagent: None,
            input: Input::new(),
            composer_wrap_width: 80,
            state: RunState::Idle,
            state_note: String::new(),
            show_banner: true,
            pet_visible: true,
            pet_pixels: false,
            run_started: None,
            spinner_idx: 0,
            scroll_up: 0,
            sel: None,
            selecting: false,
            last_click: None,
            chat_view: ChatView::default(),
            resume_candidates: Vec::new(),
            resume_via_acp: false,
            load_session: false,
            session_title: None,
            slash_sel: 0,
            picker: None,
            picker_page_rows: 0,
            permission_ask: None,
            elicitation_ask: None,
            slider_overlay: None,
            select_overlay: None,
            view_overlay: None,
            pending_images: crate::attachments::Staged::default(),
            att_chips: Vec::new(),
            slot_actions: Vec::new(),
            att_thumbs: Vec::new(),
            hover_att: None,
            modes: Self::load_modes_cache(&cfg).unwrap_or_default(),
            skills: Vec::new(),
            plugin_commands: Vec::new(),
            static_plugins: Vec::new(),
            cordis_plugins: Vec::new(),
            pending_cordis_approvals: Vec::new(),
            ui_plugins: Vec::new(),
            last_presets: Vec::new(),
            last_models: Vec::new(),
            permission_choices: Vec::new(),
            effort_choices: Vec::new(),
            tip: None,
            key_debug: std::env::var("DSH_TUI_KEYDEBUG").is_ok_and(|v| v == "1"),
            ambient_tip_idx: 0,
            ambient_tip_at: Instant::now(),
            ctrl_c_armed: None,
            session_id,
            cfg,
            selected_model: None,
            demo,
            attached,
            session_bound: demo,
            auth: crate::acp_auth::AuthSnapshot::none(),
            pending_terminal_auth: None,
            quit: false,
            queued: 0,
            queued_cells: VecDeque::new(),
            pending_steer_cells: HashMap::new(),
            next_prompt_id: 1,
            prompt_pending: false,
            shell_seq: 0,
            shell_pending: Vec::new(),
            shell_worker: None,
            bus_tx,
            server_info: None,
            needs_redraw: true,
        }
    }

    pub fn spinner(&self) -> char {
        SPINNER[self.spinner_idx % SPINNER.len()]
    }

    pub fn displayed_transcript(&self) -> &Transcript {
        self.active_subagent
            .as_deref()
            .and_then(|id| self.subagents.iter().find(|view| view.id == id))
            .map(|view| &view.transcript)
            .unwrap_or(&self.transcript)
    }

    fn displayed_transcript_mut(&mut self) -> &mut Transcript {
        if let Some(index) = self
            .active_subagent
            .as_deref()
            .and_then(|id| self.subagents.iter().position(|view| view.id == id))
        {
            return &mut self.subagents[index].transcript;
        }
        &mut self.transcript
    }

    pub fn tick(&mut self) {
        if self.state != RunState::Idle
            || self.transcript.streaming()
            || self
                .subagents
                .iter()
                .any(|view| view.running || view.transcript.streaming())
        {
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
            self.ambient_tip_idx = (self.ambient_tip_idx + 1) % crate::locale::AMBIENT_TIP_COUNT;
            if self.tip.is_none() {
                self.needs_redraw = true;
            }
        }
        // disarm expired chords
        if let Some(chord) = self.ctrl_c_armed {
            if chord.started.elapsed() > CTRL_C_QUIT_WINDOW {
                self.ctrl_c_armed = None;
            }
        }
    }

    pub fn show_tip(&mut self, text: impl Into<String>) {
        self.tip = Some((text.into(), Instant::now()));
        self.needs_redraw = true;
    }

    fn apply_palette_rpc(&mut self, params: &serde_json::Value) {
        match crate::theme::parse_palette_notification(params) {
            Ok(Some(n)) => self.merge_palette(n.pack, n.activate),
            Ok(None) => {}
            Err(err) => self.show_tip(format!("palette ignored: {err}")),
        }
        self.needs_redraw = true;
    }

    fn remove_palette_rpc(&mut self, params: &serde_json::Value) {
        if params.get("protocol").and_then(serde_json::Value::as_u64) != Some(0) {
            return;
        }
        let Some(id) = params.get("id").and_then(serde_json::Value::as_str) else {
            return;
        };
        if id == "default" {
            return;
        }
        if self.active_palette_id == id {
            self.activate_palette("default");
        }
        self.palettes.retain(|palette| palette.id != id);
        self.needs_redraw = true;
    }

    fn merge_palette(&mut self, pack: crate::theme::PalettePack, activate: bool) {
        let id = pack.id.clone();
        let loaded = pack.loaded;
        if let Some(existing) = self.palettes.iter_mut().find(|p| p.id == id) {
            *existing = pack;
        } else {
            self.palettes.push(pack);
        }
        if !loaded && self.active_palette_id == id {
            self.activate_palette("default");
        } else if activate && loaded {
            self.activate_palette(&id);
        } else if loaded && self.active_palette_id == id {
            self.sync_theme_from_active();
        }
    }

    fn activate_palette(&mut self, id: &str) {
        if !self.palettes.iter().any(|p| p.id == id) {
            return;
        }
        self.active_palette_id = id.to_string();
        self.sync_theme_from_active();
        self.show_tip(format!(
            "theme: {} {}",
            self.active_palette_id,
            self.theme.mode.as_str()
        ));
    }

    fn select_palette(&mut self, id: &str, ctl: &Controller) {
        let Some(palette) = self.palettes.iter().find(|palette| palette.id == id) else {
            return;
        };
        if palette.loaded {
            self.activate_palette(id);
        } else {
            self.show_tip(format!("loading theme plugin for {id}…"));
        }
        ctl.send(Cmd::PluginThemeSelected {
            agent_id: self.session_id.clone(),
            id: id.into(),
        });
    }

    fn sync_theme_from_active(&mut self) {
        let mode = self.theme.mode;
        if let Some(pack) = self
            .palettes
            .iter()
            .find(|p| p.id == self.active_palette_id)
        {
            self.theme = pack.theme(mode);
        }
    }

    fn apply_theme_arg(&mut self, arg: &str, ctl: &Controller) {
        match arg {
            "" => self.open_theme_picker(),
            id => {
                if self.palettes.iter().any(|p| p.id == id) {
                    self.select_palette(id, ctl);
                } else {
                    self.show_tip(format!("unknown palette: {id}"));
                    self.transcript
                        .push_notice(NoticeLevel::Warn, format!("unknown palette `{id}`"));
                }
            }
        }
    }

    fn open_theme_picker(&mut self) {
        let items = self
            .palettes
            .iter()
            .map(|pack| PickerItem {
                id: pack.id.clone(),
                label: pack.label.clone(),
                meta: if pack.id == self.active_palette_id {
                    format!("{} · active", pack.source)
                } else if pack.loaded {
                    format!("{} · ready", pack.source)
                } else {
                    format!("{} · stopped", pack.source)
                },
                provider: None,
            })
            .collect::<Vec<_>>();
        let sel = items
            .iter()
            .position(|item| item.id == self.active_palette_id)
            .unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Theme,
            title: self
                .locale
                .tr(
                    " theme · enter apply · esc close · ctrl+t dark/light ",
                    " 主题 · enter 应用 · esc 关闭 · ctrl+t 切换明暗 ",
                )
                .into(),
            sel,
            items,
        });
    }

    fn open_ui_plugin_picker(&mut self) {
        let items = self
            .ui_plugins
            .iter()
            .map(|plugin| PickerItem {
                id: plugin.id.clone(),
                label: plugin.label.clone(),
                meta: format!("{} · {}", plugin.source, plugin.status),
                provider: None,
            })
            .collect();
        let sel = self
            .ui_plugins
            .iter()
            .position(|plugin| plugin.status == "active")
            .unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::UiPlugin,
            title: self
                .locale
                .tr(
                    " UI Plugins · enter apply · esc close ",
                    " UI 插件 · enter 应用 · esc 关闭 ",
                )
                .into(),
            sel,
            items,
        });
    }

    fn open_static_plugin_picker(&mut self) {
        let items = self
            .static_plugins
            .iter()
            .map(|plugin| PickerItem {
                id: plugin.entry_id.clone(),
                label: plugin.module_name.clone(),
                meta: format!(
                    "static · {} · {}",
                    if plugin.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    plugin.fiber_phase.as_deref().unwrap_or("inactive"),
                ),
                provider: None,
            })
            .collect();
        self.picker = Some(Picker {
            kind: PickerKind::StaticPlugin,
            title: self
                .locale
                .tr(
                    " Host plugins · static · read only · esc close ",
                    " Host 插件 · 静态 · 只读 · esc 关闭 ",
                )
                .into(),
            sel: 0,
            items,
        });
    }

    fn open_cordis_plugin_picker(&mut self) {
        let items = self
            .cordis_plugins
            .iter()
            .map(|plugin| PickerItem {
                id: plugin.id.clone(),
                label: plugin.name.clone(),
                meta: match plugin.status.as_str() {
                    "awaiting-approval" => "dynamic · awaiting approval · enter review".into(),
                    "running" | "waiting" => "dynamic · running · enter stop".into(),
                    "starting-host" | "client-pending" => "dynamic · starting".into(),
                    "failed" => "dynamic · failed · enter retry".into(),
                    _ => "dynamic · stopped · enter restore".into(),
                },
                provider: None,
            })
            .collect();
        self.picker = Some(Picker {
            kind: PickerKind::CordisPlugin,
            title: self
                .locale
                .tr(
                    " Cordis plugins · dynamic · enter manage · esc close ",
                    " Cordis 插件 · 动态 · enter 管理 · esc 关闭 ",
                )
                .into(),
            sel: 0,
            items,
        });
    }

    fn open_cordis_approval_picker(&mut self, request_id: String) {
        self.picker = Some(Picker {
            kind: PickerKind::CordisApproval,
            title: self
                .locale
                .tr(
                    " plugin approval · enter decide ",
                    " 插件审批 · enter 决定 ",
                )
                .into(),
            sel: 0,
            items: vec![
                PickerItem {
                    id: "allow-version".into(),
                    label: self.locale.tr("Allow this version", "允许当前版本").into(),
                    meta: String::new(),
                    provider: Some(request_id.clone()),
                },
                PickerItem {
                    id: "allow-future".into(),
                    label: self
                        .locale
                        .tr("Allow future versions", "允许后续版本")
                        .into(),
                    meta: String::new(),
                    provider: Some(request_id.clone()),
                },
                PickerItem {
                    id: "reject".into(),
                    label: self.locale.tr("Reject", "拒绝").into(),
                    meta: String::new(),
                    provider: Some(request_id),
                },
            ],
        });
    }

    fn plugin_command_active(&self, name: &str) -> bool {
        self.plugin_commands
            .iter()
            .any(|command| command.name == name)
    }

    pub fn slash_matches(&self) -> Vec<SlashEntry> {
        if !self.input.buf.starts_with('/') {
            return Vec::new();
        }
        if let Some((name, arg)) = self.input.buf[1..].split_once(' ') {
            return self.slash_argument_matches(name, arg);
        }
        let prefix = &self.input.buf[1..];
        let mut out: Vec<SlashEntry> = SLASH_COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(prefix))
            .map(|c| SlashEntry {
                name: c.name.to_string(),
                usage: c.usage.to_string(),
                desc: self.locale.command_desc(c.name, c.desc).to_string(),
                skill: false,
                plugin: false,
                completion: None,
            })
            .collect();
        for command in &self.plugin_commands {
            if command.name.starts_with(prefix)
                && !SLASH_COMMANDS.iter().any(|c| c.name == command.name)
            {
                out.push(SlashEntry {
                    name: command.name.clone(),
                    usage: command
                        .input
                        .as_ref()
                        .map(|input| format!("/{} [{}]", command.name, input.hint))
                        .unwrap_or_else(|| format!("/{}", command.name)),
                    desc: command.description.clone(),
                    skill: false,
                    plugin: true,
                    completion: None,
                });
            }
        }
        // Host skills share the '/' namespace. Builtins win first, then an
        // active client command, because the latter never enters a prompt.
        for s in &self.skills {
            if s.name.eq_ignore_ascii_case("logout") {
                continue;
            }
            if s.name.starts_with(prefix)
                && !SLASH_COMMANDS.iter().any(|c| c.name == s.name)
                && !self.plugin_command_active(&s.name)
            {
                out.push(SlashEntry {
                    name: s.name.clone(),
                    usage: s
                        .input_hint
                        .as_ref()
                        .map(|hint| format!("/{} {}", s.name, hint))
                        .unwrap_or_else(|| format!("/{}", s.name)),
                    desc: s.description.clone(),
                    skill: !s.client_command,
                    plugin: s.client_command,
                    completion: None,
                });
            }
        }
        // An exact command must win over a longer name sharing its prefix.
        out.sort_by_key(|entry| entry.name != prefix);
        out
    }

    fn slash_argument_matches(&self, name: &str, arg: &str) -> Vec<SlashEntry> {
        let prefix = arg.trim_start();
        if prefix.contains(char::is_whitespace) {
            return Vec::new();
        }

        if !SLASH_COMMANDS.iter().any(|command| command.name == name) {
            if let Some(command) = self
                .plugin_commands
                .iter()
                .find(|command| command.name == name)
            {
                return command
                    .input
                    .as_ref()
                    .into_iter()
                    .flat_map(|input| input.options.iter())
                    .filter(|option| option.value.starts_with(prefix))
                    .map(|option| SlashEntry {
                        name: name.to_string(),
                        usage: option.label.clone().unwrap_or_else(|| option.value.clone()),
                        desc: option.description.clone().unwrap_or_default(),
                        skill: false,
                        plugin: true,
                        completion: Some(format!("/{name} {}", option.value)),
                    })
                    .collect();
            }
        }

        self.builtin_argument_options(name)
            .into_iter()
            .filter(|(value, _, _)| value.starts_with(prefix))
            .map(|(value, label, desc)| SlashEntry {
                name: name.to_string(),
                usage: label,
                desc,
                skill: false,
                plugin: false,
                completion: Some(format!("/{name} {value}")),
            })
            .collect()
    }

    fn builtin_argument_options(&self, name: &str) -> Vec<(String, String, String)> {
        let plain =
            |value: &str, desc: &str| (value.to_string(), value.to_string(), desc.to_string());
        match name {
            "model" => {
                if !self.last_models.is_empty() {
                    return self
                        .last_models
                        .iter()
                        .map(|model| (model.id.clone(), model.name.clone(), model.provider.clone()))
                        .collect();
                }
                let mut ids = host_catalog_models().unwrap_or_else(|| {
                    MODEL_PRESETS
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect()
                });
                if !ids.iter().any(|id| id == &self.cfg.model) {
                    ids.insert(0, self.cfg.model.clone());
                }
                ids.into_iter()
                    .map(|id| (id.clone(), id, String::new()))
                    .collect()
            }
            "agent" if !self.last_presets.is_empty() => self
                .last_presets
                .iter()
                .map(|preset| {
                    (
                        preset.id.clone(),
                        preset.name.clone(),
                        preset.description.clone(),
                    )
                })
                .collect(),
            "agent" => AGENT_MODES
                .iter()
                .map(|(id, label, desc)| {
                    ((*id).to_string(), (*label).to_string(), (*desc).to_string())
                })
                .collect(),
            "effort" if !self.effort_choices.is_empty() => self
                .effort_choices
                .iter()
                .map(|effort| (effort.clone(), effort.clone(), String::new()))
                .collect(),
            "effort" => vec![
                plain("off", "disable extended reasoning"),
                plain("high", "high reasoning effort"),
                plain("max", "maximum reasoning effort"),
            ],
            "permission" if !self.permission_choices.is_empty() => self
                .permission_choices
                .iter()
                .map(|preset| {
                    (
                        preset.id.clone(),
                        preset.name.clone(),
                        preset.description.clone(),
                    )
                })
                .collect(),
            "permission" => PERMISSION_PRESETS
                .iter()
                .map(|(id, desc)| plain(id, desc))
                .collect(),
            "plan" => vec![
                plain("on", "enable plan mode"),
                plain("off", "disable plan mode"),
            ],
            "theme" => self
                .palettes
                .iter()
                .map(|palette| {
                    (
                        palette.id.clone(),
                        palette.label.clone(),
                        if palette.loaded {
                            "theme plugin".to_string()
                        } else {
                            "theme plugin · stopped".to_string()
                        },
                    )
                })
                .collect(),
            "ui" => self
                .ui_plugins
                .iter()
                .map(|plugin| {
                    (
                        plugin.id.clone(),
                        plugin.label.clone(),
                        format!("{} · {}", plugin.source, plugin.status),
                    )
                })
                .collect(),
            "auth" => self
                .auth
                .methods
                .iter()
                .map(|method| {
                    (
                        method.id.clone(),
                        method.name.clone().unwrap_or_else(|| method.id.clone()),
                        method.description.clone().unwrap_or_default(),
                    )
                })
                .collect(),
            "lang" => vec![plain("zh", "中文"), plain("en", "English")],
            "liang" => vec![plain("on", "show pet"), plain("off", "hide pet")],
            _ => Vec::new(),
        }
    }

    pub fn handle(&mut self, ev: AppEvent, ctl: &Controller) {
        let modes_before = self.modes.clone();
        self.handle_inner(ev, ctl);
        // Persist mode-fact changes (chips survive restarts — the cache is
        // the landing state's source of truth until the host reports).
        if self.modes != modes_before {
            self.save_modes_cache();
        }
        // A turn ran on the picked model → the stream is the truth again.
        if self.selected_model.is_some()
            && self.selected_model.as_deref() == self.transcript.last_model.as_deref()
        {
            self.selected_model = None;
        }
    }

    fn handle_inner(&mut self, ev: AppEvent, ctl: &Controller) {
        match ev {
            AppEvent::Terminate => {
                self.quit = true;
            }
            AppEvent::Term(term) => self.handle_term(term, ctl),
            AppEvent::Ui(ui) => self.apply_ui(ui),
            AppEvent::Rpc { method, params } => {
                if method == crate::cordis::UI_UPDATE {
                    match serde_json::from_value::<UiPluginCatalog>(params) {
                        Ok(catalog) if catalog.protocol == 0 => self.ui_plugins = catalog.plugins,
                        Ok(_) => {}
                        Err(err) => self.show_tip(format!("UI Plugin catalog ignored: {err}")),
                    }
                    self.needs_redraw = true;
                    return;
                }
                if method == crate::cordis::APPROVALS_UPDATE {
                    match serde_json::from_value::<CordisApprovalsSnapshot>(params) {
                        Ok(snapshot) if snapshot.protocol == 0 => {
                            self.pending_cordis_approvals = snapshot.approvals;
                        }
                        Ok(_) => {}
                        Err(err) => self.show_tip(format!("plugin approvals ignored: {err}")),
                    }
                    self.needs_redraw = true;
                    return;
                }
                if method == crate::cordis::THEME_UPDATE {
                    self.apply_palette_rpc(&params);
                    return;
                }
                if method == crate::cordis::THEME_REMOVE {
                    self.remove_palette_rpc(&params);
                    return;
                }
                if method == crate::cordis::COMMANDS_UPDATE {
                    match serde_json::from_value::<PluginCommandCatalog>(params) {
                        Ok(catalog) if catalog.protocol == 0 => {
                            self.plugin_commands = catalog.commands;
                        }
                        Ok(_) => {}
                        Err(err) => self.show_tip(format!("commands ignored: {err}")),
                    }
                    self.needs_redraw = true;
                    return;
                }
                if method == crate::cordis::OVERLAY_UPDATE {
                    match serde_json::from_value::<PluginOverlaySnapshot>(params) {
                        Ok(snapshot) if snapshot.protocol == 0 => match snapshot.overlay {
                            Some(PluginOverlay::Select(mut select)) => {
                                if let Some(sel) = select_initial_index(&select) {
                                    select.sel = sel;
                                    self.slider_overlay = None;
                                    self.view_overlay = None;
                                    self.select_overlay = Some(select);
                                } else {
                                    self.show_tip("overlay ignored: invalid select");
                                    self.slider_overlay = None;
                                    self.select_overlay = None;
                                    self.view_overlay = None;
                                }
                            }
                            Some(PluginOverlay::Slider(mut slider))
                                if !slider.id.is_empty()
                                    && slider.min.is_finite()
                                    && slider.max.is_finite()
                                    && slider.step.is_finite()
                                    && slider.value.is_finite()
                                    && slider.min < slider.max
                                    && slider.step > 0.0
                                    && (!slider.snap_to_marks || !slider.marks.is_empty())
                                    && slider.marks.iter().all(|mark| {
                                        mark.value.is_finite()
                                            && mark.value >= slider.min
                                            && mark.value <= slider.max
                                            && !mark.label.is_empty()
                                    }) =>
                            {
                                slider.value = slider.value.clamp(slider.min, slider.max);
                                self.select_overlay = None;
                                self.view_overlay = None;
                                self.slider_overlay = Some(slider);
                            }
                            Some(PluginOverlay::Slider(_)) => {
                                self.show_tip("overlay ignored: invalid slider");
                                self.slider_overlay = None;
                                self.select_overlay = None;
                                self.view_overlay = None;
                            }
                            Some(PluginOverlay::View(mut view))
                                if !view.id.is_empty()
                                    && !view.title.is_empty()
                                    && crate::slots::validate_node_tree(&view.nodes).is_ok() =>
                            {
                                view.notify_plugin = true;
                                self.slider_overlay = None;
                                self.select_overlay = None;
                                self.view_overlay = Some(view);
                            }
                            Some(PluginOverlay::View(_)) => {
                                self.show_tip("overlay ignored: invalid view");
                                self.slider_overlay = None;
                                self.select_overlay = None;
                                self.view_overlay = None;
                            }
                            None => {
                                self.slider_overlay = None;
                                self.select_overlay = None;
                                self.view_overlay = None;
                            }
                        },
                        Ok(_) => {}
                        Err(err) => self.show_tip(format!("overlay ignored: {err}")),
                    }
                    self.needs_redraw = true;
                    return;
                }
                if method == crate::cordis::SLOTS_UPDATE {
                    match crate::slots::parse_snapshot(&params) {
                        Ok(Some(snapshot)) => {
                            let stale = self.slot_snapshots.get(&snapshot.slot).is_some_and(|current| {
                                matches!((snapshot.rev, current.rev), (Some(next), Some(previous)) if next <= previous)
                            });
                            if !stale {
                                self.slot_snapshots.insert(snapshot.slot.clone(), snapshot);
                            }
                        }
                        Ok(None) => {}
                        Err(err) => self.show_tip(format!("slot ignored: {err}")),
                    }
                    self.needs_redraw = true;
                    return;
                }
                for ui in parse_notification(&method, &params) {
                    self.apply_ui(ui);
                }
                self.needs_redraw = true;
            }
            AppEvent::RuntimeStderr(_line) => {
                // kept in proto's tail buffer for diagnostics; stay quiet here
            }
            AppEvent::RuntimeExited(code) => {
                self.prompt_pending = false;
                self.queued = 0;
                self.queued_cells.clear();
                self.pending_steer_cells.clear();
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
                    CtlEvent::Starting { .. } => {
                        self.state = RunState::Starting;
                        self.run_started = Some(Instant::now());
                        self.state_note = "starting runtime".into();
                    }
                    CtlEvent::Ready { server } => {
                        self.server_info = Some(server.clone());
                        if !self.prompt_pending {
                            self.state = RunState::Idle;
                            self.run_started = None;
                            self.state_note.clear();
                        }
                    }
                    CtlEvent::PromptQueued { .. } => {
                        let started_queued_prompt = !self.prompt_pending && self.queued > 0;
                        self.prompt_pending = false;
                        if self.state == RunState::Starting {
                            self.state = RunState::Running;
                        }
                        self.state_note.clear();
                        if started_queued_prompt {
                            self.queued = self.queued.saturating_sub(1);
                            if let Some(cells) = self.queued_cells.pop_front() {
                                self.transcript.mark_prompt_delivered(&cells);
                            }
                        }
                    }
                    CtlEvent::SteerSettled {
                        message_id,
                        deferred,
                    } => {
                        if let Some(cells) = self.pending_steer_cells.remove(&message_id) {
                            if deferred {
                                self.transcript.mark_prompt_queued(&cells);
                                self.queued += 1;
                                self.queued_cells.push_back(cells);
                                self.show_tip(
                                    "agent deferred Send Now — queued after the active turn",
                                );
                            }
                        }
                    }
                    CtlEvent::Error(err) => {
                        self.prompt_pending = false;
                        self.state = RunState::Idle;
                        self.run_started = None;
                        self.transcript.push_notice(NoticeLevel::Error, err);
                    }
                    CtlEvent::CancelRequested => {
                        self.state_note = "cancelling".into();
                        self.transcript.cancel_open_work();
                    }
                    CtlEvent::Interrupted => {
                        self.prompt_pending = false;
                        self.state = RunState::Idle;
                        self.run_started = None;
                        self.state_note.clear();
                        self.transcript.cancel_open_work();
                        self.transcript
                            .push_notice(NoticeLevel::Warn, "interrupted — turn cancelled".into());
                    }
                    CtlEvent::Skills { skills } => {
                        self.skills = skills;
                    }
                    CtlEvent::StaticPlugins { plugins } => {
                        self.static_plugins = plugins;
                        self.open_static_plugin_picker();
                    }
                    CtlEvent::CordisPlugins { plugins } => {
                        self.cordis_plugins = plugins;
                        self.open_cordis_plugin_picker();
                    }
                    CtlEvent::Catalog { models, presets } => {
                        if !presets.is_empty() {
                            self.last_presets = presets.clone();
                        }
                        if !models.is_empty() {
                            self.last_models = models.clone();
                        }
                        let mode_current = self.current_mode();
                        if let Some(picker) = &mut self.picker {
                            match picker.kind {
                                PickerKind::Model if !models.is_empty() => {
                                    let current = self.cfg.model.clone();
                                    let current_provider = self.cfg.provider.clone();
                                    picker.items = models
                                        .into_iter()
                                        .map(|m| PickerItem {
                                            id: m.id.clone(),
                                            label: m.id,
                                            meta: format!(
                                                "{} · {}{}",
                                                m.provider,
                                                m.name,
                                                if m.vision { " · vision" } else { "" }
                                            ),
                                            provider: Some(m.provider),
                                        })
                                        .collect();
                                    picker.sel = picker
                                        .items
                                        .iter()
                                        .position(|i| {
                                            i.id == current
                                                && i.provider.as_deref()
                                                    == Some(current_provider.as_str())
                                        })
                                        .unwrap_or(0);
                                }
                                PickerKind::Mode if !presets.is_empty() => {
                                    picker.items = presets
                                        .into_iter()
                                        .map(|p| PickerItem {
                                            id: p.id.clone(),
                                            label: p.name,
                                            meta: if p.broken {
                                                format!("⚠ broken · {}", p.description)
                                            } else {
                                                p.description
                                            },
                                            provider: None,
                                        })
                                        .collect();
                                    picker.sel = picker
                                        .items
                                        .iter()
                                        .position(|i| i.id == mode_current)
                                        .unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                    }
                    CtlEvent::SessionModes { modes, current } => {
                        if !modes.is_empty() {
                            self.permission_choices = modes.clone();
                        }
                        if let Some(id) = current {
                            self.modes.permission = Some(id);
                        }
                        let reported = self.modes.permission.clone();
                        let current_id = self.current_permission().to_string();
                        let choices = self.permission_choices.clone();
                        if let Some(picker) = &mut self.picker {
                            if matches!(picker.kind, PickerKind::Permission) && !choices.is_empty()
                            {
                                picker.items = permission_picker_items(
                                    &choices,
                                    reported.as_deref(),
                                    &current_id,
                                );
                                picker.sel = picker
                                    .items
                                    .iter()
                                    .position(|i| i.id == current_id)
                                    .unwrap_or(0);
                            }
                        }
                    }
                    CtlEvent::Efforts { efforts, default } => {
                        if !efforts.is_empty() {
                            self.effort_choices = efforts.clone();
                        }
                        self.open_effort_picker(efforts, default);
                    }
                    CtlEvent::PresetSet { preset } => {
                        let label = self.agent_label(&preset);
                        self.modes.agent_preset = Some(preset.clone());
                        self.transcript.push_notice(
                            NoticeLevel::Info,
                            format!(
                                "⚙ agent → {} · composes on this session's first prompt",
                                label
                            ),
                        );
                    }
                    CtlEvent::TuiOpDone(desc) => {
                        self.transcript.push_notice(NoticeLevel::Info, desc);
                    }
                    CtlEvent::TuiOpFailed(desc) => {
                        self.transcript.push_notice(NoticeLevel::Warn, desc);
                    }
                    CtlEvent::Auth(snap) => {
                        let retrying_prompt =
                            self.state == RunState::Running || self.prompt_pending;
                        if let Some((level, text)) = snap.notice() {
                            self.transcript.push_notice(level, text);
                        }
                        if snap.status == crate::acp_auth::AuthStatus::Configured {
                            if matches!(
                                self.picker.as_ref().map(|p| p.kind),
                                Some(PickerKind::Auth)
                            ) {
                                self.picker = None;
                            }
                        }
                        if snap.status == crate::acp_auth::AuthStatus::NeedsAuth {
                            self.prompt_pending = retrying_prompt;
                            self.state = RunState::Idle;
                            self.run_started = None;
                            self.state_note.clear();
                        }
                        self.auth = snap;
                    }
                    CtlEvent::OpenAuth => {
                        self.open_auth_surface(ctl);
                    }
                    CtlEvent::AgentCaps { load_session } => {
                        self.load_session = load_session;
                    }
                    CtlEvent::SessionBound { session_id, notice } => {
                        if self.session_id != session_id {
                            self.reset_subagent_views();
                        }
                        self.session_id = session_id.clone();
                        self.transcript.set_root_session(session_id);
                        self.session_bound = true;
                        if let Some(notice) = notice {
                            self.transcript.push_notice(NoticeLevel::Info, notice);
                        }
                        ctl.send(Cmd::FetchSkills);
                    }
                    CtlEvent::SessionList { sessions, prefix } => {
                        self.on_acp_session_list(sessions, prefix, ctl);
                    }
                    CtlEvent::SessionListUnavailable { prefix, error } => {
                        self.on_acp_session_list_unavailable(prefix, error, ctl);
                    }
                }
                self.needs_redraw = true;
            }
            AppEvent::PermissionAsk {
                title,
                options,
                reply,
            } => {
                self.open_permission_ask(title, options, reply);
            }
            AppEvent::ElicitationAsk { form, reply } => {
                self.elicitation_ask = Some(ElicitationAskOverlay {
                    form: crate::elicitation::ElicitationFormState::new(form),
                    scroll: 0,
                    reply: Some(reply),
                });
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

    /// Fold one decoded protocol fact into both client chrome and transcript.
    /// Direct ACP facts and JSON-RPC notifications must take the same path.
    fn apply_ui(&mut self, ui: crate::events::UiEvent) {
        use crate::events::UiEvent as E;

        if let E::SubagentStarted { parent, child } = &ui {
            if !self.subagents.iter().any(|view| view.id == *child) {
                self.subagents.push(SubagentView {
                    id: child.clone(),
                    parent: parent.clone(),
                    label: format!("subagent {}", self.subagents.len() + 1),
                    running: true,
                    transcript: Transcript::new(child.clone()),
                });
            }
            if parent == &self.session_id {
                self.transcript.apply(ui);
            } else if let Some(view) = self.subagents.iter_mut().find(|view| view.id == *parent) {
                view.transcript.apply(ui);
            }
            self.needs_redraw = true;
            return;
        }

        if let E::SubagentFinished { child } = &ui {
            let parent = self
                .subagents
                .iter_mut()
                .find(|view| view.id == *child)
                .map(|view| {
                    view.running = false;
                    view.parent.clone()
                });
            if parent.as_deref() == Some(self.session_id.as_str()) {
                self.transcript.apply(ui);
            } else if let Some(parent) = parent {
                if let Some(view) = self.subagents.iter_mut().find(|view| view.id == parent) {
                    view.transcript.apply(ui);
                }
            }
            self.needs_redraw = true;
            return;
        }

        if let Some(session) = ui_session(&ui) {
            if session != self.session_id {
                if let Some(view) = self.subagents.iter_mut().find(|view| view.id == session) {
                    view.transcript.apply(ui);
                    self.needs_redraw = true;
                    return;
                }
            }
        }

        let mut apply_to_transcript = true;
        match &ui {
            E::PlanMode { session, active } if *session == self.session_id => {
                if self.modes.plan == *active {
                    apply_to_transcript = false;
                } else {
                    self.modes.plan = *active;
                }
            }
            E::SandboxMode { session, mode } if *session == self.session_id => {
                self.modes.sandbox = Some(mode.clone());
            }
            E::ApprovalPolicy { session, policy } if *session == self.session_id => {
                self.modes.approval = Some(policy.clone());
            }
            E::PermissionPreset { session, preset } if *session == self.session_id => {
                self.modes.permission = Some(preset.clone());
            }
            E::AgentPreset { session, preset } if *session == self.session_id => {
                self.modes.agent_preset = Some(preset.clone());
                apply_to_transcript = false;
            }
            E::SessionTitle { session, title } if *session == self.session_id => {
                self.session_title = Some(title.clone());
            }
            _ => {}
        }

        if let E::SessionStatus { session, running } = &ui {
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
                    self.prompt_pending = false;
                    self.run_started = None;
                    self.state_note.clear();
                }
            }
        }
        if apply_to_transcript {
            self.transcript.apply(ui);
        }
        self.needs_redraw = true;
    }

    fn handle_term(&mut self, ev: Event, ctl: &Controller) {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                // CG rescue first: a bare arrow/⌫ with ⌘/⌥ physically held
                // gets its modifier restored (macOS terminals drop them).
                self.handle_key(crate::input::rescue_key(key), ctl)
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse, ctl),
            Event::Resize(..) => self.needs_redraw = true,
            Event::Paste(text) => {
                if let Some(keys) = decode_leaked_csi_u_keys(&text) {
                    for key in keys {
                        if key.kind != KeyEventKind::Release {
                            self.handle_key(crate::input::rescue_key(key), ctl);
                        }
                    }
                    return;
                }
                if let Some(ask) = &mut self.elicitation_ask {
                    ask.form.paste(&text);
                    self.needs_redraw = true;
                    return;
                }
                self.input.insert_str(&text.replace('\n', " "));
                self.reconcile_attachments();
                self.needs_redraw = true;
            }
            _ => {}
        }
    }

    /// grok-build mouse semantics, scaled down: wheel scrolls; left-drag
    /// selects with a live highlight (auto-scrolling at the pane edges) and
    /// copies on release; double-click selects & copies a word. Shift+drag
    /// bypasses capture in most terminals → native selection still works.
    fn handle_mouse(&mut self, mouse: MouseEvent, ctl: &Controller) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.elicitation_ask.is_some() {
                    self.elicitation_scroll_by(-3);
                } else if self.view_overlay.is_some() {
                    self.view_scroll_by(-3);
                } else {
                    self.mouse_scroll(3, mouse.column, mouse.row);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.elicitation_ask.is_some() {
                    self.elicitation_scroll_by(3);
                } else if self.view_overlay.is_some() {
                    self.view_scroll_by(3);
                } else {
                    self.mouse_scroll(-3, mouse.column, mouse.row);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.needs_redraw = true;
                if let Some(action) = self
                    .slot_actions
                    .iter()
                    .find(|(rect, _)| {
                        mouse.column >= rect.x
                            && mouse.column < rect.right()
                            && mouse.row >= rect.y
                            && mouse.row < rect.bottom()
                    })
                    .map(|(_, action)| action.clone())
                {
                    self.sel = None;
                    self.selecting = false;
                    self.last_click = None;
                    match action {
                        crate::slots::TuiAction::Command { name, args } => {
                            ctl.send(Cmd::InvokePluginCommand { name, args });
                        }
                    }
                    return;
                }
                // Clicking a tool block toggles its expand/collapse instead of
                // starting a text selection.
                if let Some(ci) = self.tool_at(mouse.column, mouse.row) {
                    self.sel = None;
                    self.selecting = false;
                    self.last_click = None;
                    self.toggle_tool(ci);
                    return;
                }
                let Some(p) = self.chat_hit(mouse.column, mouse.row) else {
                    // Click outside the chat pane dismisses the highlight.
                    self.sel = None;
                    self.selecting = false;
                    return;
                };
                let double = self.last_click.take().is_some_and(|(at, x, y)| {
                    at.elapsed() < DOUBLE_CLICK_WINDOW
                        && x.abs_diff(mouse.column) <= 1
                        && y == mouse.row
                });
                self.last_click = Some((Instant::now(), mouse.column, mouse.row));
                if double {
                    self.select_word_at(p);
                } else {
                    self.sel = Some(Selection { anchor: p, head: p });
                    self.selecting = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.selecting => {
                // Edge auto-scroll (grok: compute_autoscroll): dragging
                // past the pane keeps scrolling while events arrive.
                let a = self.chat_view.area;
                if mouse.row < a.y {
                    self.scroll_by(2);
                } else if mouse.row >= a.y.saturating_add(a.height) {
                    self.scroll_by(-2);
                }
                let head = self.chat_clamp(mouse.column, mouse.row);
                if let Some(sel) = &mut self.sel {
                    sel.head = head;
                }
                self.needs_redraw = true;
            }
            MouseEventKind::Up(MouseButton::Left) if self.selecting => {
                self.selecting = false;
                self.finish_selection();
            }
            MouseEventKind::Moved => {
                // grok-style hover: track which inline chip the pointer is
                // over; redraw only on changes (mouse moves are a firehose).
                let hover = self.chip_at(mouse.column, mouse.row);
                if hover != self.hover_att {
                    self.hover_att = hover;
                    self.needs_redraw = true;
                }
            }
            _ => {}
        }
    }

    /// Char-index spans of live `[image n]` tokens in the draft, sorted by
    /// position: `(start, end_exclusive, attachment idx)`.
    pub fn token_spans(&self) -> Vec<(usize, usize, usize)> {
        let buf = &self.input.buf;
        let mut spans = Vec::new();
        for (idx, att) in self.pending_images.iter().enumerate() {
            if let Some(byte) = buf.find(&att.token) {
                let start = buf[..byte].chars().count();
                spans.push((start, start + att.token.chars().count(), idx));
            }
        }
        spans.sort_unstable();
        spans
    }

    /// Cut the whole token when `cursor` deletes into one (backward: the
    /// char left of the cursor; forward: the char at it). Returns whether
    /// a token was cut.
    fn delete_token_at(&mut self, cursor: usize, backward: bool) -> bool {
        let probe = if backward {
            let Some(p) = cursor.checked_sub(1) else {
                return false;
            };
            p
        } else {
            cursor
        };
        let Some(&(start, end, idx)) = self
            .token_spans()
            .iter()
            .find(|(s, e, _)| probe >= *s && probe < *e)
        else {
            return false;
        };
        let b0 = byte_of(&self.input.buf, start);
        let b1 = byte_of(&self.input.buf, end);
        self.input.buf.replace_range(b0..b1, "");
        self.input.cursor = start;
        self.input.reset_vertical_goal();
        if let Some(att) = self.pending_images.remove(idx) {
            self.show_tip(format!("removed {}", att.name));
        }
        true
    }

    /// Drop attachments whose token no longer survives in the draft text.
    fn reconcile_attachments(&mut self) {
        if self.pending_images.reconcile(&self.input.buf) > 0 {
            self.hover_att = None;
            self.needs_redraw = true;
        }
    }

    /// The chip to preview: mouse hover wins, else the chip the text
    /// cursor sits in or immediately after (“光标在附近”).
    pub fn preview_att(&self) -> Option<usize> {
        if let Some(idx) = self.hover_att {
            return Some(idx);
        }
        let c = self.input.cursor;
        self.token_spans()
            .iter()
            .find(|(s, e, _)| c >= *s && c <= *e)
            .map(|&(_, _, idx)| idx)
    }

    /// Hit-test a screen cell against the inline chips drawn this frame.
    fn chip_at(&self, col: u16, row: u16) -> Option<usize> {
        self.att_chips
            .iter()
            .find(|(r, _)| {
                col >= r.x
                    && col < r.x.saturating_add(r.width)
                    && row >= r.y
                    && row < r.y.saturating_add(r.height)
            })
            .map(|(_, idx)| *idx)
    }

    /// Hit-test a screen cell against the chat pane; `None` outside it.
    fn chat_hit(&self, col: u16, row: u16) -> Option<SelPoint> {
        let a = self.chat_view.area;
        if self.chat_view.lines.is_empty()
            || col < a.x
            || col >= a.x.saturating_add(a.width)
            || row < a.y
            || row >= a.y.saturating_add(a.height)
        {
            return None;
        }
        Some(SelPoint {
            line: (self.chat_view.top + (row - a.y) as usize).min(self.chat_view.lines.len() - 1),
            col: (col - a.x) as usize,
        })
    }

    /// Like `chat_hit`, but clamps to the pane so drags outside it still
    /// extend the selection to the nearest edge.
    fn chat_clamp(&self, col: u16, row: u16) -> SelPoint {
        let a = self.chat_view.area;
        let col = col.clamp(a.x, a.x.saturating_add(a.width.saturating_sub(1)));
        let row = row.clamp(a.y, a.y.saturating_add(a.height.saturating_sub(1)));
        self.chat_hit(col, row)
            .unwrap_or(SelPoint { line: 0, col: 0 })
    }

    /// The transcript cell that owns the line under a screen cell, if any.
    fn tool_at(&self, col: u16, row: u16) -> Option<usize> {
        let p = self.chat_hit(col, row)?;
        self.chat_view.owners.get(p.line).copied().flatten()
    }

    /// Mouse wheel always scrolls the conversation, including over tool cards.
    fn mouse_scroll(&mut self, delta: i64, _col: u16, _row: u16) {
        self.scroll_by(delta);
    }

    /// Scroll the open plugin view overlay (wheel and keyboard share this
    /// path). The renderer clamps to the actual content height, so a large
    /// value (End) reliably reaches the bottom.
    fn view_scroll_by(&mut self, delta: i64) {
        if let Some(view) = self.view_overlay.as_mut() {
            if delta < 0 {
                view.scroll = view.scroll.saturating_sub(delta.unsigned_abs() as usize);
            } else {
                view.scroll = view.scroll.saturating_add(delta as usize);
            }
            self.needs_redraw = true;
        }
    }

    /// Scroll the markdown description pane of the open ACP elicitation form
    /// (wheel and keyboard share this path). The renderer clamps to the
    /// visible pane height, so `usize::MAX` (End) reaches the last row.
    fn elicitation_scroll_by(&mut self, delta: i64) {
        if let Some(ask) = self.elicitation_ask.as_mut() {
            if delta < 0 {
                ask.scroll = ask.scroll.saturating_sub(delta.unsigned_abs() as usize);
            } else {
                ask.scroll = ask.scroll.saturating_add(delta as usize);
            }
            self.needs_redraw = true;
        }
    }

    /// Toggle a tool between its collapsed viewport and full expansion.
    fn toggle_tool(&mut self, ci: usize) {
        let label = {
            let Some(cell) = self.displayed_transcript_mut().cells.get_mut(ci) else {
                return;
            };
            cell.expanded = !cell.expanded;
            if cell.expanded {
                "expanded"
            } else {
                "collapsed"
            }
        };
        self.show_tip(format!("{label} tool output · click toggles"));
        self.needs_redraw = true;
    }

    /// grok `finish_text_drag`: reconstruct the dragged text and copy it —
    /// the highlight persists only when something actually reached the
    /// clipboard path. A plain click (caret) just clears the highlight.
    fn finish_selection(&mut self) {
        self.needs_redraw = true;
        let Some(sel) = self.sel else { return };
        if sel.is_caret() {
            self.sel = None;
            return;
        }
        let text = self.selection_text(sel);
        if text.trim().is_empty() {
            self.sel = None;
            return;
        }
        self.copy_text(&text);
    }

    fn copy_text(&mut self, text: &str) {
        let chars = text.chars().count();
        if crate::clipboard::copy(text) {
            self.show_tip(format!("✓ copied {chars} chars — esc clears the highlight"));
        } else {
            self.show_tip("copy failed — hold shift and drag for the terminal's native selection");
        }
    }

    /// Extract the selected text from the layout snapshot: cell-range slices
    /// per line, trailing whitespace trimmed, joined with newlines.
    pub fn selection_text(&self, sel: Selection) -> String {
        let lines = &self.chat_view.lines;
        if lines.is_empty() {
            return String::new();
        }
        let (s, e) = sel.ordered();
        let last = lines.len() - 1;
        let (sl, el) = (s.line.min(last), e.line.min(last));
        let mut out = Vec::with_capacity(el - sl + 1);
        for (li, text) in lines.iter().enumerate().take(el + 1).skip(sl) {
            let c0 = if li == sl { s.col } else { 0 };
            let c1 = if li == el { e.col + 1 } else { usize::MAX };
            out.push(slice_by_cells(text, c0, c1).trim_end().to_string());
        }
        out.join("\n")
    }

    /// Double-click: select the whitespace-delimited word under the pointer
    /// and copy it right away (grok's word select & copy).
    fn select_word_at(&mut self, p: SelPoint) {
        let Some(line) = self.chat_view.lines.get(p.line) else {
            return;
        };
        let Some((col, width, word)) = word_span(line, p.col) else {
            self.sel = None;
            return;
        };
        self.sel = Some(Selection {
            anchor: SelPoint { line: p.line, col },
            head: SelPoint {
                line: p.line,
                col: col + width - 1,
            },
        });
        self.selecting = false;
        let word = word.clone();
        self.copy_text(&word);
    }

    /// Where the per-workspace mode cache lives (beside the session logs).
    fn modes_cache_path(cfg: &RuntimeConfig) -> std::path::PathBuf {
        std::path::Path::new(&cfg.session_root).join("dsh-tui-modes.json")
    }

    /// Last-known mode facts for this workspace; `plan` never carries over
    /// (it is a per-session switch).
    fn load_modes_cache(cfg: &RuntimeConfig) -> Option<Modes> {
        let text = std::fs::read_to_string(Self::modes_cache_path(cfg)).ok()?;
        let root: serde_json::Value = serde_json::from_str(&text).ok()?;
        let entry = root.get("workspaces")?.get(&cfg.workspace)?;
        let mut modes: Modes = serde_json::from_value(entry.clone()).ok()?;
        modes.plan = false;
        Some(modes)
    }

    /// Merge this workspace's mode facts into the cache file; failures are
    /// silent (the cache is a convenience, never a requirement).
    fn save_modes_cache(&self) {
        let path = Self::modes_cache_path(&self.cfg);
        let mut root: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if !root.is_object() {
            root = serde_json::json!({});
        }
        let Ok(entry) = serde_json::to_value(&self.modes) else {
            return;
        };
        root["workspaces"][&self.cfg.workspace] = entry;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, root.to_string());
    }

    fn locale_settings_path(cfg: &RuntimeConfig) -> std::path::PathBuf {
        settings_path(&cfg.session_root)
    }

    fn load_settings(cfg: &RuntimeConfig) -> UiSettings {
        let current = Self::locale_settings_path(cfg);
        if let Some(settings) = std::fs::read_to_string(&current)
            .ok()
            .and_then(|text| serde_json::from_str::<UiSettings>(&text).ok())
        {
            return settings;
        }
        if current.exists() {
            return UiSettings::default();
        }
        let legacy = legacy_settings_path(&cfg.session_root);
        let Some((text, settings)) = std::fs::read_to_string(legacy).ok().and_then(|text| {
            serde_json::from_str::<UiSettings>(&text)
                .ok()
                .map(|settings| (text, settings))
        }) else {
            return UiSettings::default();
        };
        if let Some(dir) = current.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if !current.exists() {
            let _ = std::fs::write(current, text);
        }
        settings
    }

    fn save_settings(&self) {
        let path = Self::locale_settings_path(&self.cfg);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut current: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .filter(|value: &serde_json::Value| value.is_object())
            .unwrap_or_else(|| serde_json::json!({}));
        current["language"] = serde_json::json!(self.locale);
        if let Ok(text) = serde_json::to_string_pretty(&current) {
            let _ = std::fs::write(path, text);
        }
    }

    fn set_locale(&mut self, arg: &str) {
        let next = if arg.trim().is_empty() {
            self.locale.alternate()
        } else if let Some(locale) = Locale::parse(arg) {
            locale
        } else {
            self.show_tip(
                self.locale
                    .tr("usage: /lang [zh|en]", "用法：/lang [zh|en]"),
            );
            return;
        };
        self.locale = next;
        self.save_settings();
        self.show_tip(match next {
            Locale::En => "Language switched to English",
            Locale::Zh => "界面语言已切换为中文",
        });
        self.needs_redraw = true;
    }

    pub fn scroll_by(&mut self, delta: i64) {
        let cur = self.scroll_up as i64;
        self.scroll_up = (cur + delta).max(0) as usize; // clamped to content in ui::draw
        self.needs_redraw = true;
    }

    fn handle_view_key(&mut self, key: KeyEvent, ctl: &Controller) {
        // Scroll keys work regardless of modifier bits: terminals that
        // report arrows with modifiers (kitty keyboard protocol and friends)
        // must still scroll the view. The view has no other arrow bindings.
        match key.code {
            KeyCode::Up => {
                self.view_scroll_by(-1);
                return;
            }
            KeyCode::Down => {
                self.view_scroll_by(1);
                return;
            }
            KeyCode::PageUp => {
                self.view_scroll_by(-5);
                return;
            }
            KeyCode::PageDown => {
                self.view_scroll_by(5);
                return;
            }
            KeyCode::Home => {
                if let Some(view) = self.view_overlay.as_mut() {
                    view.scroll = 0;
                    self.needs_redraw = true;
                }
                return;
            }
            KeyCode::End => {
                // Render clamps to the content height, so `usize::MAX` is
                // reliably the bottom of the view.
                if let Some(view) = self.view_overlay.as_mut() {
                    view.scroll = usize::MAX;
                    self.needs_redraw = true;
                }
                return;
            }
            _ => {}
        }
        if key.modifiers != KeyModifiers::NONE {
            return;
        }
        let event = match key.code {
            KeyCode::Esc => Some("cancel"),
            KeyCode::Enter => Some("submit"),
            _ => None,
        };
        if let Some(event) = event {
            if let Some(view) = self.view_overlay.take() {
                if view.notify_plugin {
                    ctl.send(Cmd::PluginOverlayEvent {
                        id: view.id,
                        event: event.into(),
                        value: None,
                    });
                }
            }
        }
    }

    fn handle_slider_key(&mut self, key: KeyEvent, ctl: &Controller) {
        if key.modifiers != KeyModifiers::NONE {
            return;
        }
        if key.code == KeyCode::Enter {
            if let Some(mut slider) = self.slider_overlay.take() {
                if slider.snap_to_marks {
                    if let Some(mark) = slider.marks.iter().min_by(|left, right| {
                        (left.value - slider.value)
                            .abs()
                            .total_cmp(&(right.value - slider.value).abs())
                    }) {
                        slider.value = mark.value;
                    }
                }
                ctl.send(Cmd::PluginOverlayEvent {
                    id: slider.id,
                    event: "submit".into(),
                    value: Some(serde_json::json!(slider.value)),
                });
            }
            return;
        }
        if key.code == KeyCode::Esc {
            if let Some(slider) = self.slider_overlay.take() {
                ctl.send(Cmd::PluginOverlayEvent {
                    id: slider.id,
                    event: "cancel".into(),
                    value: Some(serde_json::json!(slider.value)),
                });
            }
            return;
        }
        let direction = match key.code {
            KeyCode::Left => -1.0,
            KeyCode::Right => 1.0,
            _ => return,
        };
        let Some(slider) = self.slider_overlay.as_mut() else {
            return;
        };
        let next = (slider.value + direction * slider.step).clamp(slider.min, slider.max);
        if next == slider.value {
            return;
        }
        slider.value = next;
        ctl.send(Cmd::PluginOverlayEvent {
            id: slider.id.clone(),
            event: "change".into(),
            value: Some(serde_json::json!(slider.value)),
        });
    }

    fn handle_select_key(&mut self, key: KeyEvent, ctl: &Controller) {
        if key.modifiers != KeyModifiers::NONE {
            return;
        }
        if key.code == KeyCode::Enter {
            if let Some(select) = self.select_overlay.take() {
                let value = select.options[select.sel].value.clone();
                ctl.send(Cmd::PluginOverlayEvent {
                    id: select.id,
                    event: "submit".into(),
                    value: Some(serde_json::json!(value)),
                });
            }
            return;
        }
        if key.code == KeyCode::Esc {
            if let Some(select) = self.select_overlay.take() {
                let value = select.options[select.sel].value.clone();
                ctl.send(Cmd::PluginOverlayEvent {
                    id: select.id,
                    event: "cancel".into(),
                    value: Some(serde_json::json!(value)),
                });
            }
            return;
        }
        let Some(select) = self.select_overlay.as_mut() else {
            return;
        };
        let previous = select.sel;
        match key.code {
            KeyCode::Up => {
                select.sel = select.sel.saturating_sub(1);
            }
            KeyCode::Down => {
                select.sel = (select.sel + 1).min(select.options.len() - 1);
            }
            KeyCode::Home => select.sel = 0,
            KeyCode::End => select.sel = select.options.len() - 1,
            _ => return,
        }
        if select.sel == previous {
            return;
        }
        select.value = select.options[select.sel].value.clone();
        ctl.send(Cmd::PluginOverlayEvent {
            id: select.id.clone(),
            event: "change".into(),
            value: Some(serde_json::json!(select.value)),
        });
    }

    fn handle_key(&mut self, key: KeyEvent, ctl: &Controller) {
        self.needs_redraw = true;
        // DSH_TUI_KEYDEBUG=1: surface exactly what the terminal delivered
        // (after CG rescue) in the tip row — kills keybinding mysteries.
        if self.key_debug {
            self.show_tip(format!("key: {:?} + {:?}", key.modifiers, key.code));
        }

        if key.modifiers == KeyModifiers::ALT && !self.pending_cordis_approvals.is_empty() {
            let decision = match key.code {
                KeyCode::Char('1') => Some("allow-version"),
                KeyCode::Char('2') => Some("allow-future"),
                KeyCode::Char('3') => Some("reject"),
                _ => None,
            };
            if let Some(decision) = decision {
                ctl.send(Cmd::RespondCordisApproval {
                    request_id: self.pending_cordis_approvals[0].request_id.clone(),
                    decision: decision.into(),
                });
                return;
            }
        }

        // Standard ACP form elicitation is the top-most modal.
        if self.elicitation_ask.is_some() {
            self.handle_elicitation_key(key);
            return;
        }

        // ACP tool permission sits above session pickers (Backchat ask panel).
        if self.permission_ask.is_some() {
            self.handle_permission_ask_key(key);
            return;
        }

        if self.view_overlay.is_some() {
            self.handle_view_key(key, ctl);
            return;
        }

        if self.select_overlay.is_some() {
            self.handle_select_key(key, ctl);
            return;
        }

        if self.slider_overlay.is_some() {
            self.handle_slider_key(key, ctl);
            return;
        }

        // --- model picker overlay steals input first (grok modal semantics)
        if self.picker.is_some() {
            self.handle_picker_key(key, ctl);
            return;
        }

        if self.active_subagent.is_some() {
            if key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::NONE {
                self.handle_esc(ctl);
                return;
            }
            if key.code == KeyCode::Down && key.modifiers == KeyModifiers::NONE {
                self.open_subagent_switcher();
                return;
            }
            let ctx = crate::input::KeyCtx { input_empty: true };
            if let Some(action) = crate::input::classify(&key, ctx) {
                match action {
                    Action::Esc
                    | Action::Quit
                    | Action::ToggleTheme
                    | Action::ScrollHalfUp
                    | Action::ScrollHalfDown
                    | Action::PageUp
                    | Action::PageDown
                    | Action::JumpTop
                    | Action::JumpTail => self.dispatch(action, ctl),
                    _ => {}
                }
            }
            return;
        }

        if key.code == KeyCode::Down
            && key.modifiers == KeyModifiers::NONE
            && self.active_subagent.is_none()
            && self.input.is_empty()
            && self.input.hist_pos.is_none()
            && !self.subagents.is_empty()
        {
            self.open_subagent_switcher();
            return;
        }

        // The slash menu owns vertical arrows while it is visible. Ordinary
        // non-empty drafts use them for visual-line cursor motion below.
        if key.modifiers == KeyModifiers::NONE && self.input.buf.starts_with('/') {
            let n = self.slash_matches().len();
            if n > 0 {
                match key.code {
                    KeyCode::Up => self.slash_sel = self.slash_sel.checked_sub(1).unwrap_or(n - 1),
                    KeyCode::Down => self.slash_sel = (self.slash_sel + 1) % n,
                    _ => {}
                }
                if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                    return;
                }
            }
        }

        let ctx = crate::input::KeyCtx {
            input_empty: self.input.is_empty(),
        };
        if let Some(action) = crate::input::classify(&key, ctx) {
            self.dispatch(action, ctl);
        }
        // Any edit may have cut an [image n] token — the tray follows the
        // text (grok's lexicon-scan model).
        self.reconcile_attachments();
    }

    /// Apply one classified [`Action`] — the only place key semantics touch
    /// app state, so `input::keymap` stays a pure table.
    fn dispatch(&mut self, action: Action, ctl: &Controller) {
        match action {
            Action::Insert(ch) => {
                self.input.insert(ch);
                self.slash_sel = 0;
            }
            Action::Newline => self.input.insert('\n'),
            Action::Enter => {
                let menu = self.slash_matches();
                if !menu.is_empty() {
                    let entry = menu[self.slash_sel.min(menu.len() - 1)].clone();
                    self.accept_slash(&entry, ctl);
                } else {
                    self.submit(ctl);
                }
            }
            Action::TabComplete => {
                let menu = self.slash_matches();
                if !menu.is_empty() {
                    let entry = &menu[self.slash_sel.min(menu.len() - 1)];
                    self.input.set(
                        entry
                            .completion
                            .clone()
                            .unwrap_or_else(|| format!("/{} ", entry.name)),
                    );
                }
            }
            Action::Esc => self.handle_esc(ctl),
            Action::CtrlC => self.handle_ctrl_c(ctl),
            Action::Quit => self.quit = true,
            Action::ClearScrollback => {
                self.transcript.clear();
                self.sel = None;
                self.transcript
                    .push_notice(NoticeLevel::Info, "scrollback cleared".into());
            }
            Action::ToggleTheme => {
                self.theme = self.theme.toggled();
                self.show_tip(format!(
                    "theme: {} {}",
                    self.active_palette_id,
                    self.theme.mode.as_str()
                ));
            }
            Action::ToggleExpandAll => {
                self.transcript.expand_all = !self.transcript.expand_all;
                self.show_tip(if self.transcript.expand_all {
                    "expanded all thoughts and tool results"
                } else {
                    "collapsed all thoughts and tool results"
                });
            }
            Action::SendNow => self.send_now(ctl),
            Action::AttachClipboard => self.clip_image("", ctl),
            Action::ModelPicker => self.open_model_picker(ctl),
            Action::CycleAgent => self.cycle_agent(ctl),
            Action::ShowKeys => self.push_keys(),
            Action::CyclePermission => self.cycle_permission(ctl),
            Action::HistoryPrev => self.history_prev(),
            Action::HistoryNext => self.history_next(),
            Action::ScrollHalfUp => self.scroll_by(10),
            Action::ScrollHalfDown => self.scroll_by(-10),
            Action::PageUp => self.scroll_by(20),
            Action::PageDown => self.scroll_by(-20),
            Action::JumpTop => self.scroll_up = usize::MAX,
            Action::JumpTail => self.scroll_up = 0,
            Action::CursorLeft => {
                self.input.cursor = self.input.cursor.saturating_sub(1);
                self.input.reset_vertical_goal();
            }
            Action::CursorRight => {
                self.input.cursor = (self.input.cursor + 1).min(self.input.len_chars());
                self.input.reset_vertical_goal();
            }
            Action::CursorUp => self.input.move_vertical(self.composer_wrap_width, -1),
            Action::CursorDown => self.input.move_vertical(self.composer_wrap_width, 1),
            Action::WordLeft => {
                self.input.cursor = self.input.prev_word();
                self.input.reset_vertical_goal();
            }
            Action::WordRight => {
                self.input.cursor = self.input.next_word();
                self.input.reset_vertical_goal();
            }
            Action::LineStart => {
                self.input
                    .move_to_visual_line_start(self.composer_wrap_width);
            }
            Action::LineEnd => {
                self.input.move_to_visual_line_end(self.composer_wrap_width);
            }
            Action::Backspace => {
                // Deleting into an inline chip cuts the whole [image n]
                // token (and un-stages that image) instead of one bracket.
                if !self.delete_token_at(self.input.cursor, true) {
                    self.input.backspace();
                }
            }
            Action::DeleteForward => {
                if !self.delete_token_at(self.input.cursor, false) {
                    self.input.delete_forward();
                }
            }
            Action::DeleteWordBack => self.input.delete_word_back(),
            Action::KillToEnd => self.input.kill_to_end(),
            Action::KillToStart => self.input.kill_to_start(),
        }
    }

    fn handle_permission_ask_key(&mut self, key: KeyEvent) {
        let n = self
            .permission_ask
            .as_ref()
            .map(|ask| ask.options.len().max(1))
            .unwrap_or(1);
        match key.code {
            KeyCode::Esc => self.finish_permission_ask(PermissionAskReply::Cancelled),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.finish_permission_ask(PermissionAskReply::Cancelled);
            }
            KeyCode::Up => {
                if let Some(ask) = &mut self.permission_ask {
                    ask.sel = ask.sel.checked_sub(1).unwrap_or(n - 1);
                }
            }
            KeyCode::Down => {
                if let Some(ask) = &mut self.permission_ask {
                    ask.sel = (ask.sel + 1) % n;
                }
            }
            KeyCode::Enter => {
                let id = self
                    .permission_ask
                    .as_ref()
                    .and_then(|ask| ask.options.get(ask.sel).map(|o| o.option_id.clone()));
                self.finish_permission_ask(match id {
                    Some(id) => PermissionAskReply::Selected(id),
                    None => PermissionAskReply::Cancelled,
                });
            }
            _ => {}
        }
    }

    fn handle_elicitation_key(&mut self, key: KeyEvent) {
        let Some(ask) = &mut self.elicitation_ask else {
            return;
        };
        // The description pane scrolls without touching the form: PageUp /
        // PageDown always, Home / End only while the field is not a text
        // editor (typed answers keep their cursor keys). The render clamps
        // to the visible pane, so End reliably reaches the last row.
        let editing_text = ask.form.current_is_text();
        match key.code {
            KeyCode::PageUp => {
                ask.scroll = ask.scroll.saturating_sub(5);
                return;
            }
            KeyCode::PageDown => {
                ask.scroll = ask.scroll.saturating_add(5);
                return;
            }
            KeyCode::Home if !editing_text => {
                ask.scroll = 0;
                return;
            }
            KeyCode::End if !editing_text => {
                ask.scroll = usize::MAX;
                return;
            }
            _ => {}
        }
        let before = ask.form.index;
        let reply = ask.form.handle_key(key);
        if ask.form.index != before {
            // A new field owns a fresh pane; don't carry the old offset.
            ask.scroll = 0;
        }
        if let Some(reply) = reply {
            self.finish_elicitation(reply);
        }
    }

    fn finish_elicitation(&mut self, reply: crate::elicitation::ElicitationReply) {
        if let Some(mut ask) = self.elicitation_ask.take() {
            if let Some(tx) = ask.reply.take() {
                let _ = tx.send(reply);
            }
        }
        self.needs_redraw = true;
    }

    fn open_permission_ask(
        &mut self,
        title: String,
        options: Vec<PermissionAskOption>,
        reply: tokio::sync::oneshot::Sender<PermissionAskReply>,
    ) {
        let sel = permission_ask_default_sel(&options);
        self.permission_ask = Some(PermissionAskOverlay {
            title,
            sel,
            options,
            reply: Some(reply),
        });
        self.needs_redraw = true;
    }

    fn finish_permission_ask(&mut self, reply: PermissionAskReply) {
        if let Some(mut ask) = self.permission_ask.take() {
            if let Some(tx) = ask.reply.take() {
                let _ = tx.send(reply);
            }
        }
        self.needs_redraw = true;
    }

    fn handle_picker_key(&mut self, key: KeyEvent, ctl: &Controller) {
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::Theme)
            && matches!(
                crate::input::classify(
                    &key,
                    crate::input::KeyCtx {
                        input_empty: self.input.buf.is_empty(),
                    },
                ),
                Some(Action::ToggleTheme)
            )
        {
            self.dispatch(Action::ToggleTheme, ctl);
            return;
        }
        let Some(picker) = &mut self.picker else {
            return;
        };
        let n = picker.items.len().max(1);
        // Page keys jump a screenful of the open popup (rows recorded by the
        // draw pass); they never wrap, unlike ↑/↓.
        let page = self.picker_page_rows.max(1);
        match key.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Up => picker.sel = picker.sel.checked_sub(1).unwrap_or(n - 1),
            KeyCode::Down => picker.sel = (picker.sel + 1) % n,
            KeyCode::PageUp => picker.sel = picker.sel.saturating_sub(page),
            KeyCode::PageDown => picker.sel = picker.sel.saturating_add(page).min(n - 1),
            KeyCode::Home => picker.sel = 0,
            KeyCode::End => picker.sel = n - 1,
            KeyCode::Enter => {
                let Some(item) = picker.items.get(picker.sel).cloned() else {
                    self.picker = None;
                    return;
                };
                let kind = picker.kind;
                self.picker = None;
                match kind {
                    PickerKind::Model => self.select_model(item, ctl),
                    PickerKind::Mode => self.set_mode(item.id, ctl),
                    PickerKind::Theme => self.select_palette(&item.id, ctl),
                    PickerKind::UiPlugin => {
                        ctl.send(Cmd::PluginUiSelected {
                            agent_id: self.session_id.clone(),
                            id: item.id,
                        });
                    }
                    PickerKind::Permission => self.set_permission(item.id, ctl),
                    PickerKind::Session => {
                        if self.resume_via_acp {
                            self.load_acp_session(&item.id, ctl);
                        } else {
                            self.resume_session(&item.id, ctl);
                        }
                    }
                    PickerKind::Subagent => {
                        self.active_subagent = if item.id == self.session_id {
                            None
                        } else if self.subagents.iter().any(|view| view.id == item.id) {
                            Some(item.id)
                        } else {
                            None
                        };
                        self.scroll_up = 0;
                        self.sel = None;
                    }
                    PickerKind::Effort => {
                        let effort = item.id;
                        self.modes.effort = Some(effort.clone());
                        ctl.send(Cmd::SelectModel {
                            session_id: self.session_id.clone(),
                            provider: None,
                            model: None,
                            effort: Some(effort.clone()),
                        });
                        self.transcript
                            .push_notice(NoticeLevel::Info, format!("reasoning effort → {effort}"));
                    }
                    PickerKind::Auth => self.start_auth(&item.id, ctl),
                    PickerKind::StaticPlugin => {}
                    PickerKind::CordisPlugin => {
                        let action = self
                            .cordis_plugins
                            .iter()
                            .find(|plugin| plugin.id == item.id)
                            .map(|plugin| {
                                (
                                    plugin.id.clone(),
                                    plugin.status.clone(),
                                    plugin.approval_request_id.clone(),
                                )
                            });
                        if let Some((_plugin_id, _, Some(request_id))) = action.as_ref() {
                            self.open_cordis_approval_picker(request_id.clone());
                        } else if let Some((plugin_id, status, _)) = action {
                            if matches!(status.as_str(), "starting-host" | "client-pending") {
                                self.show_tip(format!("plugin {plugin_id} is already starting"));
                                return;
                            }
                            let enabled = !matches!(status.as_str(), "running" | "waiting");
                            ctl.send(Cmd::SetCordisPluginEnabled {
                                agent_id: self.session_id.clone(),
                                plugin_id: plugin_id.clone(),
                                enabled,
                            });
                            self.show_tip(if enabled {
                                format!("restoring plugin {plugin_id}…")
                            } else {
                                format!("stopping plugin {plugin_id}…")
                            });
                        }
                    }
                    PickerKind::CordisApproval => {
                        if let Some(request_id) = item.provider {
                            ctl.send(Cmd::RespondCordisApproval {
                                request_id,
                                decision: item.id,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn open_subagent_switcher(&mut self) {
        let mut items = vec![PickerItem {
            id: self.session_id.clone(),
            label: self.locale.tr("main", "主会话").into(),
            meta: self.locale.tr("current session", "当前会话").into(),
            provider: None,
        }];
        items.extend(self.subagents.iter().map(|view| PickerItem {
            id: view.id.clone(),
            label: view.label.clone(),
            meta: if view.running {
                self.locale.tr("running", "运行中").into()
            } else {
                self.locale.tr("finished", "已完成").into()
            },
            provider: None,
        }));
        let current = self.active_subagent.as_deref().unwrap_or(&self.session_id);
        let current_index = items
            .iter()
            .position(|item| item.id == current)
            .unwrap_or(0);
        let sel = (current_index + 1) % items.len();
        self.picker = Some(Picker {
            kind: PickerKind::Subagent,
            title: self
                .locale
                .tr(
                    " agents · ↑/↓ select · enter open · esc close ",
                    " Agent · ↑/↓ 选择 · enter 打开 · esc 关闭 ",
                )
                .into(),
            sel,
            items,
        });
    }

    fn open_model_picker(&mut self, ctl: &Controller) {
        // Ask the ACP agent for its real catalog; seed the picker
        // with fallback presets / env snapshot meanwhile.
        ctl.send(Cmd::FetchCatalog);
        let mut items: Vec<PickerItem> = host_catalog_models()
            .unwrap_or_else(|| MODEL_PRESETS.iter().map(|s| s.to_string()).collect())
            .into_iter()
            .map(|id| PickerItem {
                id: id.clone(),
                label: id,
                meta: String::new(),
                provider: None,
            })
            .collect();
        if !items.iter().any(|i| i.id == self.cfg.model) {
            items.insert(
                0,
                PickerItem {
                    id: self.cfg.model.clone(),
                    label: self.cfg.model.clone(),
                    meta: String::new(),
                    provider: None,
                },
            );
        }
        let sel = items
            .iter()
            .position(|i| i.id == self.cfg.model)
            .unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Model,
            title: self
                .locale
                .tr(
                    " model · enter select · esc close ",
                    " 模型 · enter 选择 · esc 关闭 ",
                )
                .into(),
            sel,
            items,
        });
    }

    fn open_effort_picker(&mut self, efforts: Vec<String>, default: Option<String>) {
        let mut items: Vec<PickerItem> = efforts
            .into_iter()
            .map(|e| {
                let is_default = default.as_deref() == Some(e.as_str());
                PickerItem {
                    id: e.clone(),
                    label: e,
                    meta: if is_default {
                        self.locale.tr("default", "默认").into()
                    } else {
                        String::new()
                    },
                    provider: None,
                }
            })
            .collect();
        if items.is_empty() {
            items = ["off", "high", "max"]
                .iter()
                .map(|e| PickerItem {
                    id: e.to_string(),
                    label: e.to_string(),
                    meta: String::new(),
                    provider: None,
                })
                .collect();
        }
        self.picker = Some(Picker {
            kind: PickerKind::Effort,
            title: self
                .locale
                .tr(
                    " reasoning effort · enter select · esc close ",
                    " 推理强度 · enter 选择 · esc 关闭 ",
                )
                .into(),
            sel: 0,
            items,
        });
    }

    /// `/resume`: list this workspace's durable sessions in a picker
    /// (grok-build's session picker). Live ACP prefers `session/list`.
    fn open_resume_picker(&mut self, ctl: &Controller) {
        if !self.demo && self.load_session {
            ctl.send(Cmd::ListSessions { prefix: None });
            self.show_tip("listing ACP sessions…");
            return;
        }
        if !self.demo {
            self.show_tip("agent did not advertise loadSession — listing local JSONL");
        }
        self.open_local_resume_picker();
    }

    fn open_local_resume_picker(&mut self) {
        self.resume_via_acp = false;
        let sessions = crate::sessions::list_sessions(
            &self.cfg.session_root,
            &self.cfg.workspace,
            &self.session_id,
        );
        if sessions.is_empty() {
            self.transcript.push_notice(
                NoticeLevel::Info,
                "no durable sessions for this workspace yet — finish a turn and /resume finds it"
                    .into(),
            );
            return;
        }
        let items: Vec<PickerItem> = sessions
            .iter()
            .map(|s| session_picker_row(&s.id, s.title.as_deref(), Some(s), None))
            .collect();
        self.resume_candidates = sessions;
        self.picker = Some(Picker {
            kind: PickerKind::Session,
            title: self
                .locale
                .tr(
                    " resume session · {n} sessions · enter select · esc close ",
                    " 恢复会话 · {n} 个会话 · enter 选择 · esc 关闭 ",
                )
                .replace("{n}", &items.len().to_string())
                .into(),
            sel: 0,
            items,
        });
    }

    /// Resume a durable session: replay its JSONL into the scrollback and
    /// point the next prompt at the same id — the runtime (or host dsh)
    /// keeps appending to the same log.
    fn resume_session(&mut self, id_or_prefix: &str, ctl: &Controller) {
        if self.resume_candidates.is_empty() {
            self.resume_candidates = crate::sessions::list_sessions(
                &self.cfg.session_root,
                &self.cfg.workspace,
                &self.session_id,
            );
        }
        let matches: Vec<crate::sessions::SessionSummary> = self
            .resume_candidates
            .iter()
            .filter(|s| s.id.starts_with(id_or_prefix))
            .cloned()
            .collect();
        let session = match matches.as_slice() {
            [one] => one.clone(),
            [] => {
                self.transcript.push_notice(
                    NoticeLevel::Warn,
                    format!("no session matches “{id_or_prefix}” — /resume lists them"),
                );
                return;
            }
            many => match many.iter().find(|s| s.id == id_or_prefix) {
                Some(one) => one.clone(),
                None => {
                    self.transcript.push_notice(
                        NoticeLevel::Warn,
                        format!(
                            "“{id_or_prefix}” is ambiguous ({} matches) — /resume lists them",
                            many.len()
                        ),
                    );
                    return;
                }
            },
        };
        let events = match crate::sessions::read_session_events(&session.file) {
            Ok(events) => events,
            Err(err) => {
                self.transcript.push_notice(
                    NoticeLevel::Warn,
                    format!("cannot read {}: {err:#}", session.file.display()),
                );
                return;
            }
        };

        self.session_id = session.id.clone();
        self.reset_subagent_views();
        self.transcript.clear();
        self.transcript.set_root_session(session.id.clone());
        // Replay folds the session's real mode facts over the cached ones.
        self.modes = Self::load_modes_cache(&self.cfg).unwrap_or_default();
        // The resumed stream's own model is the truth for the chip.
        self.selected_model = None;
        self.show_banner = false;
        self.queued = 0;
        self.queued_cells.clear();
        self.pending_steer_cells.clear();
        self.prompt_pending = false;
        self.sel = None;
        let mut replayed = 0usize;
        for ev in &events {
            if ev.get("type").and_then(serde_json::Value::as_str) == Some("session") {
                continue; // header line
            }
            // Live, the TUI echoes user prompts locally and the event parser
            // skips them; on replay the log is the only source, so push here.
            if let Some(text) = crate::sessions::user_text(ev) {
                self.transcript.push_user(text, false);
                replayed += 1;
                continue;
            }
            self.handle(
                AppEvent::Rpc {
                    method: "session.event".into(),
                    params: serde_json::json!({ "sessionId": session.id, "event": ev }),
                },
                ctl,
            );
            replayed += 1;
        }
        self.state = RunState::Idle;
        self.run_started = None;
        self.scroll_up = 0;
        self.resume_candidates = Vec::new();
        self.transcript.push_notice(
            NoticeLevel::Info,
            format!(
                "⟲ resumed {} · {} turn{} · {replayed} events replayed — the next prompt continues it",
                session.id,
                session.turns,
                if session.turns == 1 { "" } else { "s" },
            ),
        );
        self.needs_redraw = true;
    }

    fn reset_session_ui(&mut self) {
        self.reset_subagent_views();
        self.transcript.clear();
        self.modes = Self::load_modes_cache(&self.cfg).unwrap_or_default();
        self.selected_model = None;
        self.session_title = None;
        self.show_banner = false;
        self.queued = 0;
        self.queued_cells.clear();
        self.pending_steer_cells.clear();
        self.prompt_pending = false;
        self.sel = None;
        self.state = RunState::Idle;
        self.run_started = None;
        self.scroll_up = 0;
        self.resume_candidates = Vec::new();
        self.resume_via_acp = false;
        self.session_bound = self.demo;
    }

    fn reset_subagent_views(&mut self) {
        self.subagents.clear();
        self.active_subagent = None;
        if matches!(
            self.picker.as_ref().map(|picker| picker.kind),
            Some(PickerKind::Subagent)
        ) {
            self.picker = None;
        }
    }

    fn load_acp_session(&mut self, id: &str, ctl: &Controller) {
        self.reset_session_ui();
        self.session_id = id.to_string();
        self.transcript.set_root_session(id.to_string());
        ctl.send(Cmd::LoadSession {
            session_id: id.to_string(),
        });
        self.show_tip(format!("session/load {id} …"));
        self.needs_redraw = true;
    }

    fn on_acp_session_list(
        &mut self,
        sessions: Vec<SessionListItem>,
        prefix: Option<String>,
        ctl: &Controller,
    ) {
        let skip = self.session_id.clone();
        let sessions: Vec<SessionListItem> =
            sessions.into_iter().filter(|s| s.id != skip).collect();
        if let Some(prefix) = prefix.as_deref().filter(|p| !p.is_empty()) {
            match unique_session_list_match(&sessions, prefix) {
                Ok(id) => {
                    self.load_acp_session(&id, ctl);
                    return;
                }
                Err(msg) => {
                    self.transcript.push_notice(NoticeLevel::Warn, msg);
                    if sessions.is_empty() {
                        return;
                    }
                }
            }
        }
        if sessions.is_empty() {
            self.transcript.push_notice(
                NoticeLevel::Info,
                "no ACP sessions from session/list — finish a turn and /resume finds it".into(),
            );
            return;
        }
        // Local JSONL summaries (turns, age, prompt preview) for the same
        // workspace; the local log is the only source for those fields.
        let local_by_id: std::collections::HashMap<String, crate::sessions::SessionSummary> =
            crate::sessions::list_sessions(
                &self.cfg.session_root,
                &self.cfg.workspace,
                &self.session_id,
            )
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        let items: Vec<PickerItem> = sessions
            .iter()
            .map(|s| {
                let local = local_by_id.get(&s.id);
                let title = s
                    .title
                    .clone()
                    .filter(|t| !t.is_empty())
                    .or_else(|| local.and_then(|l| l.title.clone()));
                session_picker_row(&s.id, title.as_deref(), local, s.updated_at.as_deref())
            })
            .collect();
        self.resume_via_acp = true;
        self.picker = Some(Picker {
            kind: PickerKind::Session,
            title: self
                .locale
                .tr(
                    " resume session · {n} sessions · enter select · esc close ",
                    " 恢复会话 · {n} 个会话 · enter 选择 · esc 关闭 ",
                )
                .replace("{n}", &items.len().to_string())
                .into(),
            sel: 0,
            items,
        });
    }

    fn on_acp_session_list_unavailable(
        &mut self,
        prefix: Option<String>,
        error: String,
        ctl: &Controller,
    ) {
        self.show_tip(format!(
            "session/list unavailable ({error}) — listing local JSONL"
        ));
        if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
            if self.load_session {
                self.load_acp_session(&prefix, ctl);
                return;
            }
            self.resume_session(&prefix, ctl);
            return;
        }
        self.open_local_resume_picker();
        if self.load_session {
            self.resume_via_acp = true;
        }
    }

    fn open_mode_picker(&mut self, ctl: &Controller) {
        ctl.send(Cmd::FetchCatalog);
        let items: Vec<PickerItem> = if self.demo {
            AGENT_MODES
                .iter()
                .map(|(id, name, desc)| PickerItem {
                    id: id.to_string(),
                    label: name.to_string(),
                    meta: desc.to_string(),
                    provider: None,
                })
                .collect()
        } else {
            self.last_presets
                .iter()
                .map(|p| PickerItem {
                    id: p.id.clone(),
                    label: p.name.clone(),
                    meta: if p.broken {
                        format!("⚠ broken · {}", p.description)
                    } else {
                        p.description.clone()
                    },
                    provider: None,
                })
                .collect()
        };
        let current = self.current_mode();
        let sel = items.iter().position(|i| i.id == current).unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Mode,
            title: self
                .locale
                .tr(
                    " agent · enter select · esc close ",
                    " Agent · enter 选择 · esc 关闭 ",
                )
                .into(),
            sel,
            items,
        });
    }

    /// The effective composition id: folded `agent-preset/selected` / ACP
    /// `config_option_update`, else the demo stock default.
    pub fn current_mode(&self) -> String {
        self.modes.agent_preset.clone().unwrap_or_else(|| {
            if self.demo {
                "standard".into()
            } else {
                self.last_presets
                    .first()
                    .map(|p| p.id.clone())
                    .unwrap_or_default()
            }
        })
    }

    /// Resolve a protocol preset id through the latest ACP catalog. Demo mode
    /// uses its local stock catalog; unknown ids remain readable as-is.
    pub fn agent_label(&self, id: &str) -> String {
        self.last_presets
            .iter()
            .find(|preset| preset.id == id)
            .map(|preset| preset.name.clone())
            .or_else(|| {
                self.demo
                    .then(|| {
                        AGENT_MODES
                            .iter()
                            .find(|(preset_id, _, _)| *preset_id == id)
                            .map(|(_, name, _)| (*name).to_string())
                    })
                    .flatten()
            })
            .unwrap_or_else(|| id.to_string())
    }

    /// Pick the agent preset composed on this session's first prompt. The
    /// host locks it once the session agent exists (`/new` for a fresh one).
    fn set_mode(&mut self, preset: String, ctl: &Controller) {
        let label = self.agent_label(&preset);
        if self.modes.agent_preset.as_deref() == Some(preset.as_str()) {
            self.show_tip(format!("agent already {label}"));
            return;
        }
        ctl.send(Cmd::SetPreset {
            session_id: self.session_id.clone(),
            preset: preset.clone(),
        });
        // Preset scopes can mount their own skill registries.
        ctl.send(Cmd::FetchSkills);
        self.show_tip(format!("agent → {label} …"));
    }

    /// Ctrl+Shift+A cycles the advertised agent presets directly. `/agent`
    /// keeps the picker for explicit selection; the shortcut mirrors
    /// Shift+Tab's one-keystroke permission switching.
    fn cycle_agent(&mut self, ctl: &Controller) {
        let current = self.current_mode();
        let choices: Vec<&str> = if self.demo {
            AGENT_MODES.iter().map(|(id, _, _)| *id).collect()
        } else {
            self.last_presets
                .iter()
                .map(|preset| preset.id.as_str())
                .collect()
        };
        let Some(next) = choices
            .iter()
            .position(|id| *id == current)
            .map(|index| choices[(index + 1) % choices.len()])
            .or_else(|| choices.first().copied())
        else {
            ctl.send(Cmd::FetchCatalog);
            self.show_tip("agent presets unavailable");
            return;
        };
        self.set_mode(next.to_string(), ctl);
    }

    fn select_model(&mut self, item: PickerItem, ctl: &Controller) {
        let model = item.id;
        let provider = item.provider;
        let provider_changed = provider
            .as_deref()
            .is_some_and(|candidate| candidate != self.cfg.provider);
        if model != self.cfg.model || provider_changed {
            self.cfg.model = model.clone();
            self.selected_model = Some(model.clone());
            if let Some(p) = &provider {
                self.cfg.provider = p.clone();
            }
            ctl.send(Cmd::SelectModel {
                session_id: self.session_id.clone(),
                provider,
                model: Some(model.clone()),
                effort: None,
            });
        }
        // Stage 2: offer efforts for the chosen model.
        ctl.send(Cmd::FetchEfforts {
            provider: self.cfg.provider.clone(),
            model: self.cfg.model.clone(),
        });
    }

    fn set_model(&mut self, model: String, ctl: &Controller) {
        if model == self.cfg.model {
            return;
        }
        self.cfg.model = model.clone();
        self.selected_model = Some(model.clone());
        ctl.send(Cmd::SelectModel {
            session_id: self.session_id.clone(),
            provider: None,
            model: Some(model),
            effort: None,
        });
    }

    /// grok: Shift+Tab cycles the permission preset.
    fn cycle_permission(&mut self, ctl: &Controller) {
        let current = self.current_permission().to_string();
        let next = if self.permission_choices.len() >= 2 {
            let idx = self
                .permission_choices
                .iter()
                .position(|p| p.id == current)
                .unwrap_or(0);
            self.permission_choices[(idx + 1) % self.permission_choices.len()]
                .id
                .clone()
        } else {
            let idx = PERMISSION_PRESETS
                .iter()
                .position(|(p, _)| *p == current)
                .unwrap_or(0);
            PERMISSION_PRESETS[(idx + 1) % PERMISSION_PRESETS.len()]
                .0
                .to_string()
        };
        self.set_permission(next, ctl);
    }

    /// The effective permission preset: the folded `permission/preset` fact,
    /// or the harness default (workspace-write) before the session reports.
    pub fn current_permission(&self) -> &str {
        self.modes
            .permission
            .as_deref()
            .unwrap_or("workspace-write")
    }

    /// Ask the host to switch this session's permission preset; the durable
    /// `permission/preset` event echoes back and folds the ⛨ chip. Before
    /// the first prompt the host stages the switch and applies it when the
    /// session is created.
    fn set_permission(&mut self, preset: String, ctl: &Controller) {
        if self.modes.permission.as_deref() == Some(preset.as_str()) {
            self.show_tip(format!("permission already {preset}"));
            return;
        }
        ctl.send(Cmd::SetPermission {
            session_id: self.session_id.clone(),
            preset: preset.clone(),
        });
        self.show_tip(format!("permission → {preset} …"));
    }

    /// `/permission` — the two stock presets with their meaning, the current
    /// one preselected (picker twin of the blind shift+tab cycle).
    fn open_permission_picker(&mut self) {
        let reported = self.modes.permission.clone();
        let current = self.current_permission().to_string();
        let items = if self.permission_choices.is_empty() {
            PERMISSION_PRESETS
                .iter()
                .map(|(id, desc)| {
                    let mark = if reported.as_deref() == Some(*id) {
                        " · current"
                    } else if reported.is_none() && *id == current {
                        " · default"
                    } else {
                        ""
                    };
                    PickerItem {
                        id: id.to_string(),
                        label: permission_label(id),
                        meta: format!("{desc}{mark}"),
                        provider: None,
                    }
                })
                .collect()
        } else {
            permission_picker_items(&self.permission_choices, reported.as_deref(), &current)
        };
        let sel = items.iter().position(|i| i.id == current).unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Permission,
            title: self
                .locale
                .tr(
                    " permission · enter apply · esc close ",
                    " 权限 · enter 应用 · esc 关闭 ",
                )
                .into(),
            sel,
            items,
        });
    }

    fn handle_esc(&mut self, ctl: &Controller) {
        if self.elicitation_ask.is_some() {
            self.finish_elicitation(crate::elicitation::ElicitationReply::Cancelled);
            return;
        }
        if self.permission_ask.is_some() {
            self.finish_permission_ask(PermissionAskReply::Cancelled);
            return;
        }
        if self.picker.is_some() {
            self.picker = None;
            return;
        }
        if self.active_subagent.take().is_some() {
            self.scroll_up = 0;
            self.sel = None;
            self.needs_redraw = true;
            return;
        }
        // A lingering copy highlight is dismissed first (idle only — while
        // running, esc keeps its interrupt meaning and clears it in passing).
        if self.sel.take().is_some() && matches!(self.state, RunState::Idle) {
            self.needs_redraw = true;
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
                    ctl.send(Cmd::Interrupt {
                        session_id: self.session_id.clone(),
                    });
                    self.state_note = "cancelling".into();
                } else {
                    self.show_tip("demo turn — it finishes on its own");
                }
            }
            RunState::Idle => {
                // Esc clears the draft — inline [image n] chips live in it,
                // so staged images go with it (reconcile below).
                if !self.input.is_empty() {
                    self.input.history.push(self.input.buf.clone());
                    self.input.clear();
                    self.reconcile_attachments();
                    self.show_tip("draft cleared — ↑ recalls it");
                    return;
                }
                self.show_tip("esc — idle · a running turn is interrupted with esc");
            }
        }
    }

    fn handle_ctrl_c(&mut self, _ctl: &Controller) {
        if !self.input.is_empty() {
            // Clearing the draft never counts as the first press of the
            // double-Ctrl+C quit chord.
            self.ctrl_c_armed = None;
            self.input.history.push(self.input.buf.clone());
            self.input.clear();
            self.reconcile_attachments();
            self.show_tip("draft cleared — ↑ recalls it");
            return;
        }
        let required = 2;
        let mut chord = self.ctrl_c_armed.take().unwrap_or(CtrlCQuitChord {
            started: Instant::now(),
            presses: 0,
            required,
        });
        chord.presses += 1;
        if chord.presses >= chord.required {
            self.quit = true;
            return;
        }
        let remaining = chord.required - chord.presses;
        self.ctrl_c_armed = Some(chord);
        self.show_tip(if remaining == 1 {
            "press ctrl+c again to exit".into()
        } else {
            format!("press ctrl+c {remaining} more times to exit while the agent is running")
        });
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

    fn accept_slash(&mut self, entry: &SlashEntry, ctl: &Controller) {
        if let Some(completion) = &entry.completion {
            self.input.set(completion.clone());
        }
        if entry.plugin {
            let line = self.input.buf.clone();
            let rest = line
                .strip_prefix('/')
                .and_then(|s| s.strip_prefix(entry.name.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();
            self.input.history.push(line);
            self.input.clear();
            self.slash_sel = 0;
            ctl.send(Cmd::InvokePluginCommand {
                name: entry.name.clone(),
                args: rest,
            });
            return;
        }
        if entry.skill {
            // Web-UI semantics: picking a skill lands the literal "/name "
            // in the composer; enter on the completed line ships it as an
            // ordinary prompt and the host injects the skill body.
            let full = format!("/{}", entry.name);
            let line = self.input.buf.trim().to_string();
            if line == full || line.starts_with(&format!("{full} ")) {
                self.submit(ctl);
            } else {
                self.input.set(format!("{full} "));
                self.slash_sel = 0;
            }
            return;
        }
        let line = self.input.buf.clone();
        let rest = line
            .strip_prefix('/')
            .and_then(|s| s.strip_prefix(entry.name.as_str()))
            .unwrap_or("")
            .trim()
            .to_string();
        self.input.clear();
        self.slash_sel = 0;
        self.run_slash(&entry.name, &rest, ctl);
    }

    pub fn run_slash(&mut self, name: &str, arg: &str, ctl: &Controller) {
        match name {
            "help" => self.push_help(),
            "keys" => self.push_keys(),
            "lang" => self.set_locale(arg),
            "clear" => {
                self.transcript.clear();
                self.sel = None;
                self.transcript.push_notice(
                    NoticeLevel::Info,
                    self.locale.tr("scrollback cleared", "滚动区已清空").into(),
                );
            }
            "quit" => self.quit = true,
            "liang" => {
                self.pet_visible = match arg {
                    "on" | "show" => true,
                    "off" | "hide" => false,
                    _ => !self.pet_visible,
                };
                let msg = if self.pet_visible {
                    "🤫 小难梁已召唤 — 安静，他在想 AGI · /liang 收回"
                } else {
                    "小难梁去隆基市场买卡了 — /liang 再次召唤"
                };
                self.show_tip(msg);
            }
            "theme" => self.apply_theme_arg(arg, ctl),
            "ui" => {
                if arg.is_empty() {
                    self.open_ui_plugin_picker();
                } else if self.ui_plugins.iter().any(|plugin| plugin.id == arg) {
                    ctl.send(Cmd::PluginUiSelected {
                        agent_id: self.session_id.clone(),
                        id: arg.to_string(),
                    });
                } else {
                    self.show_tip(format!("unknown UI Plugin: {arg}"));
                }
            }
            "plugins" => {
                ctl.send(Cmd::FetchStaticPlugins);
                self.show_tip(self.locale.tr(
                    "reading static plugins from Host…",
                    "正在从 Host 读取静态插件…",
                ));
            }
            "cordis-plugins" => {
                ctl.send(Cmd::FetchCordisPlugins {
                    agent_id: self.session_id.clone(),
                });
                self.show_tip(self.locale.tr(
                    "reading dynamic Cordis plugins from Host…",
                    "正在从 Host 读取动态 Cordis 插件…",
                ));
            }
            "model" => {
                if arg.is_empty() {
                    self.open_model_picker(ctl);
                } else {
                    self.set_model(arg.to_string(), ctl);
                }
            }
            "agent" => {
                if arg.is_empty() {
                    self.open_mode_picker(ctl);
                } else {
                    self.set_mode(arg.to_string(), ctl);
                }
            }
            "new" => {
                if self.demo {
                    let id = if arg.is_empty() {
                        format!("dsh-{}", timestamp())
                    } else {
                        arg.to_string()
                    };
                    self.reset_subagent_views();
                    self.session_id = id.clone();
                    self.transcript.set_root_session(id.clone());
                    self.modes = Self::load_modes_cache(&self.cfg).unwrap_or_default();
                    self.session_title = None;
                    ctl.send(Cmd::FetchSkills);
                    self.transcript.push_notice(
                        NoticeLevel::Info,
                        format!("new session · {id} — /agent picks its agent preset"),
                    );
                } else {
                    self.reset_session_ui();
                    ctl.send(Cmd::NewSession);
                    self.show_tip("session/new …");
                }
            }
            "session" => self.push_session_info(),
            "status" => self.push_status_info(),
            "auth" => self.start_auth(arg, ctl),
            "resume" => {
                if arg.is_empty() {
                    self.open_resume_picker(ctl);
                } else if !self.demo && self.load_session {
                    ctl.send(Cmd::ListSessions {
                        prefix: Some(arg.to_string()),
                    });
                    self.show_tip("listing ACP sessions…");
                } else {
                    if !self.demo {
                        self.show_tip(
                            "agent did not advertise loadSession — replaying local JSONL",
                        );
                    }
                    self.resume_session(arg, ctl);
                }
            }
            "effort" => {
                if arg.is_empty() {
                    ctl.send(Cmd::FetchEfforts {
                        provider: self.cfg.provider.clone(),
                        model: self.cfg.model.clone(),
                    });
                } else {
                    ctl.send(Cmd::SelectModel {
                        session_id: self.session_id.clone(),
                        provider: None,
                        model: None,
                        effort: Some(arg.to_string()),
                    });
                    self.modes.effort = Some(arg.to_string());
                }
            }
            "permission" => {
                if arg.is_empty() {
                    self.open_permission_picker();
                } else if let Some(preset) = normalize_permission(arg) {
                    self.set_permission(preset.to_string(), ctl);
                } else {
                    // Not a stock spelling — pass through for custom preset
                    // tables; the host lists what it knows on a miss.
                    self.set_permission(arg.to_string(), ctl);
                }
            }
            "plan" => {
                if let Some(action) = self
                    .skills
                    .iter()
                    .find(|command| command.name == "plan")
                    .and_then(|command| command.config_action.clone())
                {
                    let value = match arg {
                        "" if self.modes.plan => action.reset_value.clone(),
                        "" | "on" => Some(action.value.clone()),
                        "off" => action.reset_value.clone(),
                        _ => None,
                    };
                    if let Some(value) = value {
                        ctl.send(Cmd::SetConfigOption {
                            config_id: action.config_id,
                            value,
                        });
                        return;
                    }
                }
                // Agents without a declared config action keep the ordinary
                // ACP slash-prompt transport; `/plan message` also belongs to
                // the command handler rather than the config switch.
                let text = if arg.is_empty() {
                    "/plan".to_string()
                } else {
                    format!("/plan {arg}")
                };
                self.send_agent_text(text, ctl);
            }
            "image" => self.send_image(arg, ctl),
            "clip" => self.clip_image(arg, ctl),
            other => {
                self.transcript.push_notice(
                    NoticeLevel::Warn,
                    if self.locale == Locale::Zh {
                        format!("未知命令 /{other} — 使用 /help 查看命令")
                    } else {
                        format!("unknown command /{other} — /help lists commands")
                    },
                );
            }
        }
    }

    /// Take a pending Terminal Auth launch so the main loop can leave the TUI.
    pub fn take_terminal_auth(&mut self) -> Option<crate::acp_auth::TerminalAuthLaunch> {
        self.pending_terminal_auth.take()
    }

    fn open_auth_surface(&mut self, ctl: &Controller) {
        if self.demo {
            return;
        }
        if self.pending_terminal_auth.is_some() {
            return;
        }
        if matches!(self.picker.as_ref().map(|p| p.kind), Some(PickerKind::Auth)) {
            return;
        }
        self.state = RunState::Idle;
        self.run_started = None;
        self.state_note.clear();
        self.show_tip("sign-in needed — /auth to retry");
        self.start_auth("", ctl);
    }

    fn open_auth_picker(&mut self) {
        let items: Vec<PickerItem> = self
            .auth
            .methods
            .iter()
            .map(|method| PickerItem {
                id: method.id.clone(),
                label: method.name.clone().unwrap_or_else(|| method.id.clone()),
                meta: method.type_name.clone(),
                provider: None,
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let current = self.auth.method_id.clone();
        let sel = current
            .as_ref()
            .and_then(|id| items.iter().position(|item| item.id == *id))
            .unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Auth,
            title: self
                .locale
                .tr(
                    " sign in · enter select · esc close ",
                    " 登录 · enter 选择 · esc 关闭 ",
                )
                .into(),
            sel,
            items,
        });
    }

    fn start_auth(&mut self, arg: &str, ctl: &Controller) {
        use crate::acp_auth::{
            authenticate_meta_from_method, select_auth_method, values_from_auth_arg,
        };
        if self.demo {
            self.transcript
                .push_notice(NoticeLevel::Info, "demo has no ACP authenticate".into());
            return;
        }
        if self.auth.methods.is_empty() {
            self.transcript.push_notice(
                NoticeLevel::Warn,
                self.auth
                    .message
                    .clone()
                    .unwrap_or_else(|| "this agent did not advertise auth methods".into()),
            );
            return;
        }
        let arg = arg.trim();
        if arg.is_empty() && self.auth.methods.len() > 1 {
            self.open_auth_picker();
            return;
        }
        let (method_id, rest) = {
            let mut parts = arg.splitn(2, char::is_whitespace);
            let first = parts.next().unwrap_or("");
            let rest = parts.next().unwrap_or("").trim();
            if self.auth.methods.iter().any(|m| m.id == first) {
                (first.to_string(), rest.to_string())
            } else {
                (
                    self.auth
                        .method_id
                        .clone()
                        .unwrap_or_else(|| self.auth.methods[0].id.clone()),
                    arg.to_string(),
                )
            }
        };
        let Some(method) = select_auth_method(&self.auth.methods, Some(&method_id)).cloned() else {
            self.transcript.push_notice(
                NoticeLevel::Warn,
                format!("ACP auth method is unavailable or not supported: {method_id}"),
            );
            return;
        };
        if method.is_env_prompt() {
            let vars = method
                .vars
                .iter()
                .map(|v| v.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.transcript.push_notice(
                NoticeLevel::Warn,
                if vars.is_empty() {
                    format!(
                        "ACP auth method {} requires credential variables and cannot be started as a sign-in flow.",
                        method.id
                    )
                } else {
                    format!(
                        "ACP auth method {} requires credential variables ({vars}) and cannot be started as a sign-in flow.",
                        method.id
                    )
                },
            );
            return;
        }
        let values = values_from_auth_arg(&method, &rest);
        if method.form && authenticate_meta_from_method(&method, &values).is_none() {
            self.show_tip("usage: /auth <api-key> · gateway: /auth <base-url> <api-key>");
            return;
        }
        if values.is_empty() {
            if let Some(launch) = method.terminal_launch.clone() {
                self.pending_terminal_auth = Some(launch);
                self.transcript.push_notice(
                    NoticeLevel::Info,
                    format!(
                        "leaving the TUI for {} — return here when it finishes",
                        method.name.as_deref().unwrap_or(&method.id)
                    ),
                );
                return;
            }
        }
        ctl.send(Cmd::Authenticate {
            method_id: method.id,
            values,
        });
    }

    fn push_help(&mut self) {
        if self.locale == Locale::Zh {
            let text = "\
## help

- enter · 发送；当前轮次运行时将后续消息排队
- ctrl+x · 立即 steer 当前轮次
- esc · 中断（保留草稿）；空闲时清除草稿
- ctrl+c · 有草稿先清除；无草稿时连按 2 次退出（不中断）
- shift+tab · 轮换权限预设 · /permission 打开选择器
- ctrl+p · 打开模型选择器，然后选择推理强度
- ctrl+shift+a · 直接轮换 Agent 预设
- /agent · 切换 ACP 广告的 Agent 预设
- /lang · 切换界面语言：/lang zh 或 /lang en
- /auth · ACP 登录；多种方式时打开选择器
- /effort · 推理强度 · /permission 权限预设 · /plan 计划模式
- /resume · 恢复持久会话并继续写入原日志
- /image · 暂存本地图片：/image ./pic.png [说明]
- /clip · 暂存剪贴板图片；ctrl+v 同样可用
- !cmd · 在会话级本地 shell 中运行命令，不经过 Agent；初始目录为 workspace，cd/环境变量跨命令保留
- /<skill> · Agent 命令会进入 / 菜单，选择后由 Host 注入技能正文
- ctrl+o · 展开思考和工具输出 · ctrl+l · 清屏
- 编辑 · readline 组合键 + ⌘/⌥ 方向键 · 完整映射见 /keys
- 点击工具 · 展开/折叠 · 滚轮滚动对话
- pgup/pgdn · 翻页 · end 回到最新消息
- 鼠标拖动 · 选择并复制文本 · 双击复制单词

每轮会显示：流式思考与回答、工具调用与结果、注入上下文、Subagent 生命周期、
token 用量（含缓存命中）以及轮次结束原因。";
            self.transcript.push_markdown(text.to_string());
            return;
        }
        let text = "\
## help

- enter · send · queues a follow-up while a turn runs
- ctrl+x · steer the active turn immediately
- esc · interrupt (draft survives) · clears the draft when idle
- ctrl+c · clear a draft; 2× quits with no draft (never interrupts)
- shift+tab · cycle permission (workspace-write ⇄ full access) · /permission opens the preset picker
- ctrl+p · model picker (host catalog) → effort picker
- ctrl+shift+a · cycle agent preset directly
- /agent · switch the agent preset advertised over ACP
- /auth · ACP sign-in (picker when several methods; else Terminal Auth or authenticate _meta)
- /effort · reasoning effort · /permission preset · /plan host plan mode
- /resume · pick up a durable session — transcript replays, log continues
- /image · stage a local image — /image ./pic.png [caption]
- /clip · stage the clipboard image — /clip [caption] · ctrl+v also works
- !cmd · run in the session's local shell (not the agent); starts in the workspace, keeps cd/env across commands
- /<skill> · agent commands join the / menu — enter ships it and the host injects the skill body
- ctrl+o · expand thoughts + tool output · ctrl+l · clear
- editing · readline chords + ⌘/⌥ arrows (ctrl+arrows elsewhere) · full map in /keys
- click tool · expand/collapse that tool · wheel scrolls the conversation
- pgup/pgdn · scroll · mouse wheel works · end follows the tail
- mouse drag · select text — copied on release · 2×click copies a word

Per turn: streamed reasoning, answer, tool calls with results, injected
context, subagent lifecycles, token usage (incl. cache hits), end reason.";
        self.transcript.push_markdown(text.to_string());
    }

    fn push_keys(&mut self) {
        let text = crate::input::keymap::keys_markdown(
            self.locale == Locale::Zh,
            cfg!(target_os = "macos"),
        );
        self.view_overlay = Some(ViewOverlay {
            id: "builtin.keys".into(),
            title: self.locale.tr("Keyboard shortcuts", "快捷键").to_string(),
            nodes: vec![crate::slots::TuiNode::Markdown {
                id: "keys".into(),
                text,
                streaming: false,
            }],
            scroll: 0,
            notify_plugin: false,
        });
    }

    fn push_session_info(&mut self) {
        let creds = if self.demo {
            "demo mode (no API calls)".to_string()
        } else if self.auth.status == crate::acp_auth::AuthStatus::Configured {
            match self
                .auth
                .method_name
                .as_deref()
                .or(self.auth.method_id.as_deref())
            {
                Some(name) => format!("ACP authenticate · {name}"),
                None => "ACP authenticate · configured".into(),
            }
        } else if self.auth.status == crate::acp_auth::AuthStatus::NeedsAuth {
            match self
                .auth
                .method_name
                .as_deref()
                .or(self.auth.method_id.as_deref())
            {
                Some(name) => format!("sign-in needed · {name} · /auth"),
                None => "sign-in needed · /auth".into(),
            }
        } else if self.cfg.has_credentials() {
            match self.cfg.credential_source() {
                Some(src) => format!("api key present · {src}"),
                None => "api key present".to_string(),
            }
        } else {
            "DEEPSEEK_API_KEY not set".to_string()
        };
        let u = self.transcript.usage;
        let total = u.input + u.output + u.cached + u.reasoning;
        let s = self.transcript.stats;
        let llm_millis = s.turn_millis.saturating_sub(s.tool_millis);
        // The same facts the Client-side `acpSessionStats` service folds —
        // rendered here from the transcript's own accumulator.
        let effort_line = self
            .modes
            .effort
            .as_deref()
            .map(|effort| format!("\n- effort · {effort}"))
            .unwrap_or_default();
        let mut text = format!(
            "## session\n\n\
             - session · {}{}\n\
             - provider · {} / {}{}\n\
             - agent · {}{}\n\
             - workspace · {}\n\
             - session root · {}\n\
             - runtime · {}\n\
             - server · {}\n\
             - credentials · {}\n\
             - tokens · ↑{} ↓{} (cached {} · reasoning {}) · Σ {}\n\
             - turns · {} · steps · {}\n\
             - LLM · {} · tool · {}",
            self.session_id,
            self.session_title
                .as_deref()
                .map(|t| format!(" · {t}"))
                .unwrap_or_default(),
            self.cfg.provider,
            self.cfg.model,
            effort_line,
            self.agent_label(&self.current_mode()),
            if self.modes.agent_preset.is_none() {
                " (default)"
            } else {
                ""
            },
            self.cfg.workspace,
            self.cfg.session_root,
            self.cfg.bin,
            self.server_info.as_deref().unwrap_or("not started"),
            creds,
            fmt_tokens(u.input),
            fmt_tokens(u.output),
            fmt_tokens(u.cached),
            fmt_tokens(u.reasoning),
            fmt_tokens(total),
            s.turns,
            s.steps,
            fmt_duration(llm_millis),
            fmt_duration(s.tool_millis),
        );
        if s.ttft_count > 0 {
            text.push_str(&format!(
                "\n- TTFT avg · {}",
                fmt_duration(s.ttft_total_millis / s.ttft_count as u64)
            ));
        }
        if llm_millis > 0 && u.output > 0 {
            text.push_str(&format!(
                "\n- rate · {:.1} tok/s",
                u.output as f64 / (llm_millis as f64 / 1000.0)
            ));
        }
        self.transcript.push_markdown(text);
    }

    /// Compact status fallback: the run state plus painter-owned ACP facts.
    ///
    /// The live `/status` is a Client Plugin command (`status-view`): it opens
    /// the semantic overlay and takes every token/turn/step/timing figure from
    /// the Client-side `acpSessionStats.current()` — the same snapshot
    /// `stats-view` renders in the composer dock. This arm only serves runs
    /// without a Client tree (demo, standalone painter) and deliberately
    /// reads no `Transcript.usage`/`stats` accumulator, so the two surfaces
    /// can never drift apart.
    fn push_status_info(&mut self) {
        let state = match self.state {
            RunState::Idle => self.locale.tr("idle", "空闲").to_string(),
            RunState::Starting => self.locale.tr("starting", "启动中").to_string(),
            RunState::Running => self.locale.tr("running", "工作中").to_string(),
        };
        let perm = self
            .modes
            .permission
            .clone()
            .or_else(|| self.modes.sandbox.clone())
            .unwrap_or_else(|| self.current_permission().to_string());
        let perm_label = if self.locale == Locale::Zh {
            match perm.as_str() {
                "read-only" => "只读".to_string(),
                "workspace-write" => "工作区可写".to_string(),
                "danger-full-access" => "完全访问".to_string(),
                _ => permission_label(&perm),
            }
        } else {
            permission_label(&perm)
        };
        let effort_line = self
            .modes
            .effort
            .as_deref()
            .map(|effort| format!("\n- effort · {effort}"))
            .unwrap_or_default();
        // ACP facts: connection, authenticate state, session binding, and
        // the server banner when the runtime has reported it.
        let acp = if self.demo {
            "demo".to_string()
        } else if self.attached {
            "attached".to_string()
        } else {
            "not attached".to_string()
        };
        let auth_line = match self.auth.status {
            crate::acp_auth::AuthStatus::Configured => {
                let method = self
                    .auth
                    .method_name
                    .as_deref()
                    .or(self.auth.method_id.as_deref());
                Some(format!(
                    "configured{}",
                    method.map(|m| format!(" · {m}")).unwrap_or_default()
                ))
            }
            crate::acp_auth::AuthStatus::NeedsAuth => {
                let method = self
                    .auth
                    .method_name
                    .as_deref()
                    .or(self.auth.method_id.as_deref());
                Some(format!(
                    "needs sign-in{}",
                    method.map(|m| format!(" · {m}")).unwrap_or_default()
                ))
            }
            _ => None,
        };
        let mut text = format!("## status\n\n- state · {state}\n- acp · {acp}\n");
        if let Some(auth) = auth_line {
            text.push_str(&format!("- auth · {auth}\n"));
        }
        text.push_str(&if self.session_bound {
            format!("- session · {}\n", self.session_id)
        } else {
            "- session · unbound\n".to_string()
        });
        if let Some(server) = &self.server_info {
            text.push_str(&format!("- server · {server}\n"));
        }
        text.push_str(&format!(
            "- model · {}{}\n\
             - agent · {}{}\n\
             - permission · {}\n\
             - plan · {}",
            self.cfg.model,
            effort_line,
            self.agent_label(&self.current_mode()),
            if self.modes.agent_preset.is_none() {
                " (default)"
            } else {
                ""
            },
            perm_label,
            if self.modes.plan { "on" } else { "off" },
        ));
        self.transcript.push_markdown(text);
    }
}

/// Compact token count: `1234` → `1.2K`, `1_500_000` → `1.5M`.
fn fmt_tokens(value: u64) -> String {
    if value < 1000 {
        value.to_string()
    } else if value < 1_000_000 {
        format!("{:.1}K", value as f64 / 1000.0)
    } else {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    }
}

/// Compact duration: `1500ms` → `1.5s`, `135_000ms` → `2m15s`.
fn fmt_duration(ms: u64) -> String {
    if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m{}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

impl App {
    fn submit(&mut self, ctl: &Controller) {
        let text = self.input.buf.trim().to_string();
        // Client namespaces don't take images — keep the chips editable
        // instead of silently dropping them.
        if !self.pending_images.is_empty() && (text.starts_with('/') || text.starts_with('!')) {
            self.show_tip(
                "send or delete the [image] chips first — /commands and !shell don't take images",
            );
            return;
        }
        if let Some(cmdline) = text.strip_prefix('/') {
            let mut parts = cmdline.splitn(2, ' ');
            let name = parts.next().unwrap_or("").to_string();
            let arg = parts.next().unwrap_or("").trim().to_string();
            // Host skills share the '/' namespace (builtins win a name): a
            // skill line ships as an ordinary prompt — the host's pre-step
            // boundary recognizes the leading /name and injects the body.
            let builtin = SLASH_COMMANDS.iter().any(|c| c.name == name);
            let acp_client_command = self
                .skills
                .iter()
                .any(|command| command.name == name && command.client_command);
            if !builtin && (self.plugin_command_active(&name) || acp_client_command) {
                self.input.history.push(text);
                self.input.clear();
                ctl.send(Cmd::InvokePluginCommand { name, args: arg });
                return;
            }
            if !builtin && self.skills.iter().any(|s| s.name == name) {
                self.input.history.push(text.clone());
                self.input.clear();
                self.send_agent_text(text, ctl);
                return;
            }
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

        // Inline [image n] chips ride along with the prompt text (or send
        // alone): chip order = block order (图文交替).
        if !self.pending_images.is_empty() {
            self.input.history.push(self.input.buf.clone());
            let staged = self.take_staged_blocks();
            self.input.clear();
            self.send_staged(staged, ctl);
            return;
        }

        if text.is_empty() {
            return;
        }
        self.input.history.push(text.clone());
        self.input.clear();
        self.send_agent_text(text, ctl);
    }

    /// Send raw text as an agent prompt (shared by submit and command
    /// passthroughs like /plan).
    fn send_agent_text(&mut self, text: String, ctl: &Controller) {
        self.show_banner = false;
        let running = self.state == RunState::Running || self.prompt_pending || self.queued > 0;
        let cell = self.transcript.cells.len();
        self.transcript.push_user(text.clone(), running);
        if running {
            self.queued += 1;
            self.queued_cells.push_back(vec![cell]);
            self.show_tip("queued — lands after this turn · ctrl+x would send now");
        } else {
            self.prompt_pending = true;
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
            self.state_note = if self.demo {
                String::new()
            } else {
                "contacting runtime".into()
            };
        }
        self.scroll_up = 0;
        ctl.send(Cmd::Prompt {
            session_id: self.session_id.clone(),
            text,
        });
    }

    /// `/image <path> [caption]` — stage a local raster in the composer; it is
    /// sent on the next Enter (caption becomes the prompt text).
    fn send_image(&mut self, arg: &str, _ctl: &Controller) {
        let (path, caption) = match arg.split_once(char::is_whitespace) {
            Some((p, rest)) => (p, rest.trim().to_string()),
            None => (arg, String::new()),
        };
        if path.is_empty() {
            self.show_tip("/image needs a path — /image ./pic.png [caption]");
            return;
        }
        let Some(media_type) = media_type_for(path) else {
            self.show_tip("unsupported image — use .png .jpg .jpeg .webp .gif");
            return;
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => {
                self.show_tip(format!("cannot read {path}: {err}"));
                return;
            }
        };
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let stored_path = std::fs::canonicalize(path)
            .or_else(|_| std::path::absolute(path))
            .unwrap_or_else(|_| std::path::PathBuf::from(path))
            .to_string_lossy()
            .into_owned();
        self.stage_image(name, stored_path, media_type.to_string(), bytes, caption);
    }

    /// `/clip [caption]` — stage the clipboard image in the composer.
    fn clip_image(&mut self, caption: &str, _ctl: &Controller) {
        match read_clipboard_image() {
            Some((bytes, media_type)) => self.stage_image(
                "clipboard.png".into(),
                "clipboard".into(),
                media_type.to_string(),
                bytes,
                caption.to_string(),
            ),
            None => self.show_tip("clipboard has no image, or this platform isn't supported"),
        }
    }

    /// Stage an image as an inline `[image N]` chip at the cursor; up to
    /// [`crate::attachments::MAX_STAGED`] ride the next Enter with the text.
    fn stage_image(
        &mut self,
        name: String,
        path: String,
        media_type: String,
        data: Vec<u8>,
        caption: String,
    ) {
        let token = match self.pending_images.add(name, path, media_type, data) {
            Ok(att) => att.token.clone(),
            Err(full) => {
                self.show_tip(full);
                return;
            }
        };
        if !caption.is_empty() {
            self.input.set(caption);
            self.input.insert(' ');
        } else if self.input.cursor > 0
            && !self
                .input
                .buf
                .chars()
                .nth(self.input.cursor - 1)
                .is_none_or(char::is_whitespace)
        {
            self.input.insert(' ');
        }
        self.input.insert_str(&token);
        self.show_tip("image staged — ⌫ deletes its chip · hover it to preview");
        self.needs_redraw = true;
    }

    /// Drain the tray and split the draft on chip spans, in reading order.
    fn take_staged_blocks(&mut self) -> Vec<StagedBlock> {
        self.reconcile_attachments();
        let buf = self.input.buf.clone();
        split_draft_into_staged_blocks(&buf, self.pending_images.drain())
    }

    fn next_prompt_id(&mut self) -> u64 {
        let id = self.next_prompt_id;
        self.next_prompt_id = self.next_prompt_id.wrapping_add(1).max(1);
        id
    }

    /// Echo staged blocks in the transcript (text and image thumbnails in
    /// draft order) and send one prompt whose ACP blocks match that order.
    fn emit_staged_prompt(
        &mut self,
        staged: Vec<StagedBlock>,
        queued: bool,
        steer_message_id: Option<u64>,
        ctl: &Controller,
    ) {
        self.show_banner = false;
        let first_cell = self.transcript.cells.len();
        for block in &staged {
            match block {
                StagedBlock::Text(text) => self.transcript.push_user(text.clone(), queued),
                StagedBlock::Image(att) => self.transcript.push_image(
                    att.name.clone(),
                    String::new(),
                    att.path.clone(),
                    att.data.clone(),
                    queued,
                ),
            }
        }
        if queued {
            self.queued_cells
                .push_back((first_cell..self.transcript.cells.len()).collect());
        }
        if let Some(message_id) = steer_message_id {
            self.pending_steer_cells.insert(
                message_id,
                (first_cell..self.transcript.cells.len()).collect(),
            );
        }
        self.scroll_up = 0;
        let blocks = prompt_blocks_from_staged(staged);
        ctl.send(if let Some(message_id) = steer_message_id {
            Cmd::SteerImages {
                session_id: self.session_id.clone(),
                message_id,
                blocks,
            }
        } else {
            Cmd::PromptImages {
                session_id: self.session_id.clone(),
                blocks,
            }
        });
    }

    /// Submit path for the staged tray: set run state / queue bookkeeping,
    /// then emit the interleaved prompt.
    fn send_staged(&mut self, staged: Vec<StagedBlock>, ctl: &Controller) {
        if staged.is_empty() {
            return;
        }
        let n = staged
            .iter()
            .filter(|b| matches!(b, StagedBlock::Image(_)))
            .count();
        let running = self.state == RunState::Running || self.prompt_pending || self.queued > 0;
        if running {
            self.queued += 1;
            self.show_tip(if n <= 1 {
                "image queued — lands after this turn".to_string()
            } else {
                format!("{n} images queued — land after this turn")
            });
        } else {
            self.prompt_pending = true;
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
            self.state_note = if n <= 1 {
                "sending image".into()
            } else {
                format!("sending {n} images")
            };
        }
        self.emit_staged_prompt(staged, running, None, ctl);
    }

    /// Send-now is ACP steering: issue another prompt immediately while the
    /// current turn remains active. Esc is the only cancellation path.
    fn send_now(&mut self, ctl: &Controller) {
        let raw = self.input.buf.trim().to_string();
        if !self.pending_images.is_empty() && (raw.starts_with('/') || raw.starts_with('!')) {
            self.show_tip(
                "send or delete the [image] chips first — /commands and !shell don't take images",
            );
            return;
        }
        let staged = if self.pending_images.is_empty() {
            if raw.is_empty() {
                return;
            }
            vec![StagedBlock::Text(raw)]
        } else {
            self.take_staged_blocks()
        };
        if staged.is_empty() {
            return;
        }
        let running = self.state == RunState::Running || self.prompt_pending || self.queued > 0;
        self.input.history.push(self.input.buf.clone());
        self.input.clear();
        self.show_banner = false;
        let queued = false;
        let has_images = staged.iter().any(|b| matches!(b, StagedBlock::Image(_)));
        if has_images {
            let n = staged
                .iter()
                .filter(|b| matches!(b, StagedBlock::Image(_)))
                .count();
            if running {
                self.show_tip("steered with image — lands at the next agent step");
            } else {
                self.prompt_pending = true;
                self.state = RunState::Starting;
                self.run_started = Some(Instant::now());
                self.state_note = if n == 1 {
                    "sending image".into()
                } else {
                    format!("sending {n} images")
                };
            }
            let steer_message_id = running.then(|| self.next_prompt_id());
            self.emit_staged_prompt(staged, queued, steer_message_id, ctl);
        } else {
            let text = match staged.into_iter().next() {
                Some(StagedBlock::Text(t)) => t,
                _ => return,
            };
            let cell = self.transcript.cells.len();
            self.transcript.push_user(text.clone(), queued);
            if running {
                self.show_tip("steered — lands at the next agent step");
            } else {
                self.prompt_pending = true;
                self.state = RunState::Starting;
                self.run_started = Some(Instant::now());
            }
            self.scroll_up = 0;
            ctl.send(if running {
                let message_id = self.next_prompt_id();
                self.pending_steer_cells.insert(message_id, vec![cell]);
                Cmd::Steer {
                    session_id: self.session_id.clone(),
                    message_id,
                    text,
                }
            } else {
                Cmd::Prompt {
                    session_id: self.session_id.clone(),
                    text,
                }
            });
        }
    }

    /// Submit a prompt programmatically (used by DSH_TUI_AUTOPROMPT).
    pub fn auto_prompt(&mut self, text: &str, ctl: &Controller) {
        self.show_banner = false;
        self.input.set(text.to_string());
        self.submit(ctl);
    }

    fn run_local_shell(&mut self, cmd: String) {
        self.shell_seq += 1;
        let id = self.shell_seq;
        let cell = self.transcript.push_shell(cmd.clone());
        self.shell_pending.push((id, cell));
        let request = ShellRequest { id, command: cmd };
        let worker = self.shell_worker.get_or_insert_with(|| {
            ShellWorker::spawn(self.cfg.workspace.clone(), self.bus_tx.clone())
        });
        if let Err(request) = worker.send(request) {
            let worker = ShellWorker::spawn(self.cfg.workspace.clone(), self.bus_tx.clone());
            if let Err(request) = worker.send(request) {
                let _ = self.bus_tx.send(AppEvent::ShellDone {
                    id: request.id,
                    code: None,
                    output: "failed to start shell worker".into(),
                });
            }
            self.shell_worker = Some(worker);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/app__persistent_shell_tests.rs"]
mod persistent_shell_tests;


pub fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs:x}")
}

/// Model ids from the host catalog snapshot: either inline JSON in
/// `DSH_TUI_MODELS` or a JSON file at `DSH_TUI_MODELS_FILE` (written by the
/// dsh plugin shim and refreshed on llm registry changes). Accepts
/// `["model-id", ...]` or `[{"id": "...", ...}, ...]`.
pub fn host_catalog_models() -> Option<Vec<String>> {
    let raw = match std::env::var("DSH_TUI_MODELS") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            let path = std::env::var("DSH_TUI_MODELS_FILE").ok()?;
            std::fs::read_to_string(path).ok()?
        }
    };
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let arr = value.as_array()?;
    let mut out = Vec::new();
    for item in arr {
        match item {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Object(_) => {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    out.push(id.to_string());
                }
            }
            _ => {}
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Slice `s` by display-cell range `[c0, c1)`: a char is included when its
/// cell span overlaps the range (so a double-width char straddling the
/// boundary is kept — matching what the highlight visually covers).
pub(crate) fn slice_by_cells(s: &str, c0: usize, c1: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0).max(1);
        if w + cw > c0 && w < c1 {
            out.push(ch);
        }
        w += cw;
        if w >= c1 {
            break;
        }
    }
    out
}

/// The whitespace-delimited word covering display column `col` of `line`:
/// `(start_col, cell_width, word)`. `None` on whitespace or past the end.
pub(crate) fn word_span(line: &str, col: usize) -> Option<(usize, usize, String)> {
    let cw = |ch: char| ch.width().unwrap_or(0).max(1);
    let chars: Vec<char> = line.chars().collect();
    let mut w = 0usize;
    let mut hit = None;
    for (i, ch) in chars.iter().enumerate() {
        if col < w + cw(*ch) {
            hit = Some(i);
            break;
        }
        w += cw(*ch);
    }
    let i = hit?;
    if chars[i].is_whitespace() {
        return None;
    }
    let (mut a, mut b) = (i, i);
    while a > 0 && !chars[a - 1].is_whitespace() {
        a -= 1;
    }
    while b + 1 < chars.len() && !chars[b + 1].is_whitespace() {
        b += 1;
    }
    let start_col: usize = chars[..a].iter().copied().map(cw).sum();
    let width: usize = chars[a..=b].iter().copied().map(cw).sum();
    Some((start_col, width, chars[a..=b].iter().collect()))
}

#[cfg(test)]
#[path = "../tests/unit/app__resume_tests.rs"]
mod resume_tests;


#[cfg(test)]
#[path = "../tests/unit/app__selection_tests.rs"]
mod selection_tests;


#[cfg(test)]
#[path = "../tests/unit/app__mode_tests.rs"]
mod mode_tests;


#[cfg(test)]
#[path = "../tests/unit/app__palette_tests.rs"]
mod palette_tests;


#[cfg(test)]
#[path = "../tests/unit/app__right_slot_tests.rs"]
mod right_slot_tests;

