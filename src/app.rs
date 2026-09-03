//! App state and input handling — the grok-build interaction homage.
//!
//! Enter sends (or queues mid-turn, client-side); Ctrl+X steers the active
//! turn immediately; Esc cancels a running turn with the draft preserved, and
//! Esc owns interrupt; Ctrl+C clears a draft, then needs two empty presses to quit;
//! `!` runs a command in the session's local shell; `/` opens the slash menu; Up recalls
//! history on an empty prompt.

use std::collections::{HashMap, HashSet, VecDeque};
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
use crate::input::{Action, VimMode};
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
/// How often the composer cap re-checks the workspace git branch (tick
/// cadence). Catches checkouts done by the agent or in another terminal;
/// `!git checkout` in the session shell refreshes immediately instead.
const GIT_CHECK_INTERVAL: Duration = Duration::from_secs(5);

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
        name: "agent",
        usage: "/agent [id]",
        desc: "switch agent preset · ctrl+shift+a",
    },
    SlashCommand {
        name: "auth",
        usage: "/auth [method|api-key]",
        desc: "ACP sign-in (Backchat authenticate)",
    },
    SlashCommand {
        name: "clear",
        usage: "/clear",
        desc: "clear the scrollback",
    },
    SlashCommand {
        name: "clip",
        usage: "/clip [text]",
        desc: "attach the clipboard image (macOS/Linux)",
    },
    SlashCommand {
        name: "close",
        usage: "/close",
        desc: "close the current session tab (last tab cannot close)",
    },
    SlashCommand {
        name: "cordis-plugins",
        usage: "/cordis-plugins",
        desc: "review or manage dynamic Cordis plugins",
    },
    SlashCommand {
        name: "effort",
        usage: "/effort [off|high|max]",
        desc: "reasoning effort for this session",
    },
    SlashCommand {
        name: "help",
        usage: "/help",
        desc: "show help and tips",
    },
    SlashCommand {
        name: "image",
        usage: "/image <path> [text]",
        desc: "send a local image (png/jpeg/webp/gif)",
    },
    SlashCommand {
        name: "keys",
        usage: "/keys",
        desc: "keyboard shortcuts",
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
        name: "model",
        usage: "/model [id]",
        desc: "switch model · live over ACP",
    },
    SlashCommand {
        name: "new",
        usage: "/new [id]",
        desc: "start a fresh session",
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
        name: "plugins",
        usage: "/plugins",
        desc: "show Host plugin status (read-only)",
    },
    SlashCommand {
        name: "quit",
        usage: "/quit",
        desc: "exit martty",
    },
    SlashCommand {
        name: "resume",
        usage: "/resume [n|id]",
        desc: "list the n most recent sessions (default 50) · /resume <id> resumes it",
    },
    SlashCommand {
        name: "session",
        usage: "/session [view|prev|next]",
        desc: "show session info · prev/next switch session tab",
    },
    SlashCommand {
        name: "theme",
        usage: "/theme [id|toggle]",
        desc: "switch Theme Plugin or toggle dark/light",
    },
    SlashCommand {
        name: "ui",
        usage: "/ui [id]",
        desc: "switch UI Plugin",
    },
    SlashCommand {
        name: "vim",
        usage: "/vim [on|off]",
        desc: "toggle vim modal editing (default off)",
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

pub use crate::input::composer::ComposerEditor;

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

/// Composer drag-selection (same gesture as the chat pane): both endpoints
/// are cells in the input text-area coordinates — `(row, col)` where the
/// drag began and where the pointer is now. The covered char range is
/// derived with [`App::input_selection_range`], which treats both endpoint
/// cells as inclusive so either drag direction selects exactly the cells
/// the pointer crossed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputSel {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
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
    /// Absolute top line while the user explores scrollback. `None` follows
    /// the streaming tail; a scroll gesture is resolved into a fresh anchor
    /// by the next draw.
    pub(crate) manual_top: Option<usize>,
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
    Auth,
    CordisPlugin,
    CordisApproval,
    AgentHistory,
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
    /// Visual group for argument candidates. The group label is rendered on
    /// its first option without adding a selectable separator row.
    pub section: Option<String>,
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
    /// First row of the popup's viewport. Unlike `sel` (which keys and the
    /// wheel move freely), the window only scrolls when the selection
    /// leaves it — the draw pass adjusts this by the minimum needed, so a
    /// static window lets ↑/↓ and wheel sweep the highlight row by row
    /// instead of re-pinning the window edge under it every frame.
    pub offset: usize,
}

/// The `/plugins` static inventory as a two-level tree (provider → plugin),
/// rendered by `ui::draw_plugin_tree` with the tui-tree-widget crate.
/// Items are rebuilt from `static_plugins` on every draw; the TreeState keeps
/// the selection and the opened provider branches stable across frames.
pub struct PluginTree {
    pub title: String,
    pub state: tui_tree_widget::TreeState<String>,
}

/// The provider bucket for one loader entry: the npm scope when the module
/// name carries one, otherwise a stable `core` bucket. This is the first
/// level of the `/plugins` tree.
pub fn plugin_provider(module: &str) -> String {
    module
        .split_once('/')
        .map(|(scope, _)| scope)
        .unwrap_or("core")
        .to_string()
}

/// The plugin name shown under its provider: the module name without the
/// npm scope (`@deepseek-ai/dsh-agent` → `dsh-agent`).
pub fn plugin_short_name(module: &str) -> String {
    module
        .split_once('/')
        .map(|(_, name)| name)
        .unwrap_or(module)
        .to_string()
}

pub struct SubagentView {
    pub id: String,
    pub parent: String,
    pub label: String,
    pub running: bool,
    pub failed: bool,
    pub transcript: Transcript,
}

/// One non-live session's full state while another tab is viewed (issue
/// #94). The App's own fields always describe the viewed session; a tab
/// switch moves them into a slot and loads the target slot in their place.
///
/// Everything the frame paints between the session tab strip and the
/// composer stats dock is session state: the transcript and its scroll
/// position, the composer draft + staged images + `@file` browser, the
/// queue shelf, subagents, ACP asks, painter popups — a tab switch must
/// never show another session's chrome on this session's tab.
pub struct SessionSlot {
    pub id: String,
    pub title: Option<String>,
    pub transcript: Transcript,
    /// Composer draft (text, cursor, per-tab recall history) rides with
    /// its session like the queue does — the composer area is bound to
    /// the viewed tab.
    pub input: ComposerEditor,
    /// Images staged in the draft as `[image n]` chips (draft-bound).
    pub pending_images: crate::attachments::Staged,
    /// Composer well pinned to the amplified height (issue #92).
    pub input_expanded: bool,
    /// Open `@file` browser + its Esc-dismissed `@` token tag. Both are
    /// draft-bound: the menu filters the draft's token, so it follows the
    /// draft to its tab.
    pub file_menu: Option<crate::file_ref::FileMenu>,
    pub file_menu_dismissed: Option<String>,
    /// Chat scroll offset in lines above the bottom (0 = follow).
    pub scroll_up: usize,
    /// Model explicitly picked on this session (`/model`); wins over
    /// `transcript.last_model` in the chip until a turn realizes it.
    pub selected_model: Option<String>,
    /// Welcome banner: shown until this session sends its first prompt.
    pub show_banner: bool,
    /// Meta-row chrome while the session runs: state note text and the
    /// elapsed-timer anchor.
    pub state_note: String,
    pub run_started: Option<Instant>,
    /// Authoritative per-session running bit (folded from `SessionStatus`
    /// and turn lifecycle events).
    pub running: bool,
    /// Finished while parked → tab badge until the user views the tab.
    pub completed_unseen: bool,
    pub prompt_queue: VecDeque<ClientQueuedPrompt>,
    pub prompt_pending: bool,
    pub modes: Modes,
    pub session_bound: bool,
    pub pending_steer_cells: HashMap<u64, PendingSteer>,
    pub subagents: Vec<SubagentView>,
    pub current_subagents: HashSet<String>,
    pub next_subagent_starts_batch: bool,
    pub active_subagent: Option<String>,
    pub agent_selection: Option<String>,
    /// A session-bound ACP ask (permission or elicitation) that arrived
    /// while this session was out of view — or that was on screen when the
    /// user switched away. Asks follow their session: only the live tab
    /// renders and answers them, switching never cancels one, and the tab
    /// strip marks a tab with a pending ask.
    pub permission_ask: Option<PermissionAskOverlay>,
    pub elicitation_ask: Option<ElicitationAskOverlay>,
    /// Painter-owned info popup (`/help`, `/keys`, `/session`, painter
    /// `/status` — `notify_plugin == false`) parked with its session: it
    /// leaves the screen on a switch and resurfaces, scroll and all, when
    /// the user returns. Compositor-owned plugin views never park — the
    /// tab-click path cancels them instead (see `cancel_plugin_overlays`).
    pub view_overlay: Option<ViewOverlay>,
    /// `/plugins` / `/cordis-plugins` inventory tree (selection state
    /// included) parked with its session like the info popups.
    pub plugin_tree: Option<PluginTree>,
}

impl SessionSlot {
    /// A fresh, empty session tab. `bound` is true only when the id is
    /// already a real session id (demo, local JSONL resume).
    fn fresh(id: String, bound: bool) -> Self {
        SessionSlot {
            transcript: Transcript::new(id.clone()),
            id,
            title: None,
            input: ComposerEditor::new(),
            pending_images: crate::attachments::Staged::default(),
            input_expanded: false,
            file_menu: None,
            file_menu_dismissed: None,
            scroll_up: 0,
            selected_model: None,
            // A fresh tab has not prompted yet — the welcome banner paints
            // until its first send (resume paths force it off).
            show_banner: true,
            state_note: String::new(),
            run_started: None,
            running: false,
            completed_unseen: false,
            prompt_queue: VecDeque::new(),
            prompt_pending: false,
            modes: Modes::default(),
            session_bound: bound,
            pending_steer_cells: HashMap::new(),
            subagents: Vec::new(),
            current_subagents: HashSet::new(),
            next_subagent_starts_batch: true,
            active_subagent: None,
            agent_selection: None,
            permission_ask: None,
            elicitation_ask: None,
            view_overlay: None,
            plugin_tree: None,
        }
    }
}

/// One FIFO entry for a tab awaiting its `SessionBound` (see the App
/// field `awaiting_binds`).
struct AwaitingBind {
    id: String,
    open: bool,
}

/// One row of the session tab strip (native chrome, `ui::draw_session_tabs`).
pub struct SessionTab {
    pub label: String,
    pub running: bool,
    pub completed_unseen: bool,
    /// A session-bound ACP ask (permission/elicitation) is pending on this
    /// tab — the agent is waiting for an answer only this tab can give.
    pub ask_pending: bool,
    pub current: bool,
}

pub(crate) const AGENT_HISTORY_ID: &str = "__martty_internal__:agent-history";

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
/// facts the Web UI chips read). Only client preferences such as Agent preset
/// and effort survive across sessions; permission facts must come from the
/// current Host session.
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
    current_subagents: HashSet<String>,
    next_subagent_starts_batch: bool,
    pub active_subagent: Option<String>,
    pub(crate) agent_selection: Option<String>,
    pub input: ComposerEditor,
    /// Optional vim modal editing (`/vim`); off by default.
    pub vim: crate::input::VimState,
    /// Display-cell width of the composer text well from the latest frame.
    pub(crate) composer_wrap_width: usize,
    /// Screen rect of the composer input well from the latest frame — mouse
    /// hit-testing (recorded by `ui::draw_input`).
    pub(crate) input_area: ratatui::layout::Rect,
    /// First text row shown inside the well (the viewport scroll from
    /// `ui::draw_input`).
    pub(crate) input_top: usize,
    /// Absolute screen cell of the soft caret from the latest frame (the
    /// reversed block `ui::draw` paints in the composer well or the
    /// elicitation field). `main` repositions the *hidden* hardware cursor
    /// onto this cell after every draw, so IME candidate popups anchor at
    /// the caret instead of wherever the frame diff left the cursor;
    /// `None` when no caret was painted this frame.
    pub caret_cell: Option<(u16, u16)>,
    /// Ordered char-index range selected in the composer: drag-select and
    /// copy highlight, `None` when no input selection is shown.
    pub(crate) input_sel: Option<InputSel>,
    /// A left-button drag inside the composer well is in progress.
    input_selecting: bool,
    /// Mouse pointer cell from the latest `Moved` event. The composer
    /// expand button is hover-revealed only while the pointer sits on it
    /// (issue #92 — mouse-only affordance, no key binding).
    pub(crate) mouse_pos: Option<(u16, u16)>,
    /// Composer well enlarged by the mouse-only expand button (issue #92):
    /// `true` pins the well to the amplified height, `false` restores the
    /// draft-following auto height.
    pub(crate) input_expanded: bool,
    /// Screen rect of the mouse-only expand/collapse button from the
    /// latest frame — it sits on the composer card's top-right border.
    /// Recorded every frame, even before the pointer finds it.
    pub(crate) expand_btn: Option<ratatui::layout::Rect>,
    /// True while the pointer rests on the expand button; only then does
    /// the frame painter show it (`needs_redraw` on change).
    pub(crate) hover_expand_btn: bool,
    pub state: RunState,
    pub state_note: String,
    /// Welcome banner (whale + wordmark) — shown until the first real prompt.
    pub show_banner: bool,
    /// Pixel-art Liang at the composer's right edge (`/liang` toggles him).
    /// Off by default — `/liang on` summons him.
    pub pet_visible: bool,
    /// True when the terminal speaks the kitty graphics protocol: image
    /// thumbnails and the background layer emit real pixels (set by `main`).
    pub pet_pixels: bool,
    /// The pet sprite the frame just drew around: cell box + working flag.
    /// Filled by `ui::draw` (the layout math lives there — one source of
    /// truth), reconciled against the terminal by `main` after the frame.
    pub pet_want: Option<(ratatui::layout::Rect, bool)>,
    /// Current git branch of the workspace, shown after the project path in
    /// the composer cap when git is available and the terminal is wide
    /// enough. Seeded at startup, then re-checked on a throttled tick and
    /// right after each session shell command (`None` otherwise).
    pub git_branch: Option<String>,
    /// Last time the workspace git branch was re-checked (tick throttle).
    git_check_at: Instant,
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
    /// `/resume` picker rows came from ACP `session/list` (pick → resume/load).
    resume_via_acp: bool,
    /// Agent advertised `loadSession`.
    load_session: bool,
    /// Agent advertised `sessionCapabilities.list` (or legacy `loadSession`).
    list_session: bool,
    /// Agent advertised `sessionCapabilities.resume`.
    resume_session_cap: bool,
    /// Last ACP `session_info_update` title.
    session_title: Option<String>,
    pub slash_sel: usize,
    /// Open `@file` browser menu (grammar + ratatui-explorer in `file_ref`).
    pub file_menu: Option<crate::file_ref::FileMenu>,
    /// Esc-dismissed `@` token tag (see `file_ref::token_tag`): an
    /// unchanged token must not reopen the browser. Survives menu
    /// rebuilds, so it lives on the App, not in `file_menu`.
    pub file_menu_dismissed: Option<String>,
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
    /// Provider-grouped static plugin inventory (`/plugins`): a tui-tree-widget
    /// popup rebuilt from `static_plugins` every draw, selection/open state
    /// kept by the widget's own TreeState.
    pub plugin_tree: Option<PluginTree>,
    /// Rows the open plugin tree actually shows (`h - border`), recorded by
    /// `ui::draw_plugin_tree` — the page size for PageUp/PageDown.
    pub plugin_tree_page_rows: usize,
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
    pub(crate) static_plugins: Vec<crate::bus::StaticPluginItem>,
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
    /// Parked sessions — every session except the viewed one (issue #94).
    /// Conceptual tab order is this list with the live session spliced in
    /// at `current`; the App's own fields above always mirror the live tab.
    parked: Vec<SessionSlot>,
    /// Tab index of the live session (`0..=parked.len()`).
    current: usize,
    /// One FIFO entry for a tab awaiting its `SessionBound`: `/new`
    /// placeholders and `session/load` targets. The bind lands on the tab
    /// that asked, even if the user switched away while it resolved.
    /// Closing the tab before the bind resolves keeps the entry (its FIFO
    /// position still owns the in-flight request) but marks it dead — the
    /// bind is discarded when it arrives instead of rebinding some other
    /// tab.
    awaiting_binds: VecDeque<AwaitingBind>,
    /// Tab strip hit-test rects, recorded by `ui::draw_session_tabs`.
    pub(crate) tab_rects: Vec<(ratatui::layout::Rect, usize)>,
    /// First tab index rendered in the session tab strip (the strip's
    /// scroll window). Mouse clicks on the window's edge tabs nudge it by
    /// one so the neighboring tab appears — a mouse can walk through every
    /// session tab without ever hitting a dead end. The draw pass keeps the
    /// window sane (never past the tail, head-anchored when there is no
    /// overflow, live tab always in view).
    pub(crate) tab_strip_offset: usize,
    pub cfg: RuntimeConfig,
    /// Model explicitly picked this session (`/model`); wins over
    /// `transcript.last_model` in the chip until a turn realizes it.
    pub selected_model: Option<String>,
    pub demo: bool,
    /// A live ACP agent owns runtime, credentials, and its advertised catalog.
    pub attached: bool,
    /// A real `session/new`, `session/resume`, or `session/load` supplied this id.
    /// Cached session options stay hidden until this becomes true.
    pub session_bound: bool,
    /// The unrequested startup/reconnect bind has been seen. Before it,
    /// any `SessionBound` is the acp-side `session/new` this client did not
    /// ask for — so when one arrives while tabs are awaiting their own
    /// binds it must land on the parked tab that is not awaiting anything,
    /// not on the FIFO head (issue #94 startup race). Seeded bound
    /// sessions (demo, tests) have no pending unrequested bind.
    startup_bound: bool,
    /// ACP initialize / authenticate status (live ACP only).
    pub auth: crate::acp_auth::AuthSnapshot,
    /// Leave the TUI and run this agent login, then `authenticate`.
    pending_terminal_auth: Option<crate::acp_auth::TerminalAuthLaunch>,
    pub quit: bool,
    /// Escape suppresses slash recommendations for the current draft without
    /// deleting it. Text edits or an explicit Tab completion reopen them.
    slash_completion_dismissed: bool,
    pub queued: usize,
    /// Follow-ups not yet sent over ACP. The Agent only sees the front item
    /// after the active turn settles.
    prompt_queue: VecDeque<ClientQueuedPrompt>,
    queue_selection: Option<usize>,
    queue_edit: Option<QueueEditState>,
    /// Send Now bubbles awaiting the concurrent ACP request result.
    pending_steer_cells: HashMap<u64, PendingSteer>,
    next_prompt_id: u64,
    /// A first prompt was handed to the controller but has not reached the
    /// ACP request task yet. Runtime startup alone does not make a turn busy.
    prompt_pending: bool,
    shell_seq: u64,
    shell_pending: Vec<(u64, String, usize)>, // (id, session id, cell idx)
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
        | UiEvent::ToolCallPreparing { session }
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
        | UiEvent::ReasoningEffort { session, .. }
        | UiEvent::ApprovalAsked { session, .. }
        | UiEvent::ApprovalDecided { session, .. } => Some(session),
        UiEvent::SubagentStarted { .. }
        | UiEvent::SubagentFinished { .. }
        | UiEvent::Palette { .. } => None,
    }
}

/// Register (or revive) the view for a started subagent in `views`.
fn upsert_subagent_view(
    views: &mut Vec<SubagentView>,
    parent: &str,
    child: &str,
    locale: Locale,
) {
    if let Some(view) = views.iter_mut().find(|view| view.id == child) {
        view.running = true;
        view.failed = false;
    } else {
        let mut transcript = Transcript::new(child.to_string());
        transcript.locale = locale;
        views.push(SubagentView {
            id: child.to_string(),
            parent: parent.to_string(),
            label: locale.trf("subagent {}", "子代理 {}", &[(views.len() + 1).to_string()]),
            running: true,
            failed: false,
            transcript,
        });
    }
}

/// Draft pieces after stripping `[image n]` chips, still in reading order.
#[derive(Clone)]
enum StagedBlock {
    Text(String),
    Image(crate::attachments::Attachment),
}

pub(crate) struct ClientQueuedPrompt {
    id: u64,
    blocks: Vec<StagedBlock>,
}

struct QueueEditState {
    prompt_id: u64,
    delete_confirm: bool,
}

pub(crate) struct QueuePreview {
    pub(crate) id: u64,
    pub(crate) ordinal: usize,
    pub(crate) summary: String,
    pub(crate) selected: bool,
    pub(crate) editing: bool,
}

pub(crate) struct PendingSteer {
    cells: Vec<usize>,
    blocks: Vec<StagedBlock>,
    /// Queue-head retries keep their original FIFO position when deferred.
    requeue_front: bool,
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
        theme: Option<Theme>,
        cfg: RuntimeConfig,
        session_id: String,
        demo: bool,
        attached: bool,
        bus_tx: Sender<AppEvent>,
    ) -> Self {
        let palettes = vec![crate::theme::PalettePack::builtin_default()];
        let settings = Self::load_settings(&cfg);
        // Explicit `--theme` on the CLI wins; otherwise the persisted
        // light/dark mode; otherwise the builtin default (dark).
        let mode = theme
            .as_ref()
            .map(|t| t.mode)
            .or_else(|| settings.theme_mode.as_deref().and_then(crate::theme::Mode::parse))
            .unwrap_or(crate::theme::Mode::Dark);
        let theme = palettes[0].theme(mode);
        let locale = settings.language;
        let mut app = App {
            theme,
            locale,
            palettes,
            active_palette_id: "default".into(),
            ui_preset: settings.ui_preset,
            slot_snapshots: HashMap::new(),
            transcript: Transcript::new(session_id.clone()),
            subagents: Vec::new(),
            current_subagents: HashSet::new(),
            next_subagent_starts_batch: true,
            active_subagent: None,
            agent_selection: None,
            input: ComposerEditor::new(),
            vim: crate::input::VimState::default(),
            composer_wrap_width: 80,
            input_area: ratatui::layout::Rect::default(),
            input_top: 0,
            caret_cell: None,
            input_sel: None,
            input_selecting: false,
            mouse_pos: None,
            input_expanded: false,
            expand_btn: None,
            hover_expand_btn: false,
            state: RunState::Idle,
            state_note: String::new(),
            show_banner: true,
            pet_visible: false,
            pet_pixels: false,
            pet_want: None,
            git_branch: None,
            git_check_at: Instant::now(),
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
            list_session: false,
            resume_session_cap: false,
            session_title: None,
            slash_sel: 0,
            file_menu: None,
            file_menu_dismissed: None,
            picker: None,
            picker_page_rows: 0,
            permission_ask: None,
            elicitation_ask: None,
            slider_overlay: None,
            select_overlay: None,
            view_overlay: None,
            plugin_tree: None,
            plugin_tree_page_rows: 0,
            pending_images: crate::attachments::Staged::default(),
            att_chips: Vec::new(),
            slot_actions: Vec::new(),
            att_thumbs: Vec::new(),
            hover_att: None,
            modes: Modes::default(),
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
            parked: Vec::new(),
            current: 0,
            awaiting_binds: VecDeque::new(),
            tab_rects: Vec::new(),
            tab_strip_offset: 0,
            cfg,
            selected_model: None,
            demo,
            attached,
            session_bound: demo,
            // Seeded-bound sessions (demo, tests, local resume) have no
            // unrequested bind coming; a live ACP start does.
            startup_bound: demo,
            auth: crate::acp_auth::AuthSnapshot::none(),
            pending_terminal_auth: None,
            quit: false,
            slash_completion_dismissed: false,
            queued: 0,
            prompt_queue: VecDeque::new(),
            queue_selection: None,
            queue_edit: None,
            pending_steer_cells: HashMap::new(),
            next_prompt_id: 1,
            prompt_pending: false,
            shell_seq: 0,
            shell_pending: Vec::new(),
            shell_worker: None,
            bus_tx,
            server_info: None,
            needs_redraw: true,
        };
        app.transcript.locale = locale;
        app
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

    pub fn displayed_transcript_mut(&mut self) -> &mut Transcript {
        if let Some(index) = self
            .active_subagent
            .as_deref()
            .and_then(|id| self.subagents.iter().position(|view| view.id == id))
        {
            return &mut self.subagents[index].transcript;
        }
        &mut self.transcript
    }

    /// Move the live session's per-session state out of the App fields into
    /// a parking slot, leaving cheap placeholders behind. Always paired with
    /// [`Self::put_live_slot`] before anything reads the fields again.
    fn take_live_slot(&mut self) -> SessionSlot {
        let mut placeholder = Transcript::new(String::new());
        placeholder.locale = self.locale;
        SessionSlot {
            id: std::mem::take(&mut self.session_id),
            title: self.session_title.take(),
            transcript: std::mem::replace(&mut self.transcript, placeholder),
            // Composer + chat chrome bound to the tab (see `put_live_slot`).
            input: std::mem::take(&mut self.input),
            pending_images: std::mem::take(&mut self.pending_images),
            input_expanded: std::mem::take(&mut self.input_expanded),
            file_menu: self.file_menu.take(),
            file_menu_dismissed: self.file_menu_dismissed.take(),
            scroll_up: std::mem::take(&mut self.scroll_up),
            selected_model: std::mem::take(&mut self.selected_model),
            show_banner: std::mem::take(&mut self.show_banner),
            state_note: std::mem::take(&mut self.state_note),
            run_started: self.run_started.take(),
            running: self.state != RunState::Idle || self.prompt_pending,
            completed_unseen: false,
            prompt_queue: std::mem::take(&mut self.prompt_queue),
            prompt_pending: std::mem::take(&mut self.prompt_pending),
            modes: std::mem::take(&mut self.modes),
            session_bound: std::mem::take(&mut self.session_bound),
            pending_steer_cells: std::mem::take(&mut self.pending_steer_cells),
            subagents: std::mem::take(&mut self.subagents),
            current_subagents: std::mem::take(&mut self.current_subagents),
            next_subagent_starts_batch: std::mem::replace(&mut self.next_subagent_starts_batch, true),
            active_subagent: self.active_subagent.take(),
            agent_selection: self.agent_selection.take(),
            permission_ask: self.permission_ask.take(),
            elicitation_ask: self.elicitation_ask.take(),
            // Painter info popups ride along with their session; plugin
            // views are filtered out (a compositor overlay must never be
            // resurrected stale on another tab — the tab-click path
            // cancels it before the switch, anything left here is dropped).
            view_overlay: self.view_overlay.take().filter(|view| !view.notify_plugin),
            plugin_tree: self.plugin_tree.take(),
        }
    }

    /// Load a slot's state into the App fields — the inverse of
    /// [`Self::take_live_slot`]. RunState is recomputed from the slot's
    /// authoritative `running` bit; an in-flight turn keeps its elapsed
    /// timer and state note.
    fn put_live_slot(&mut self, slot: SessionSlot) {
        self.session_id = slot.id;
        self.session_title = slot.title;
        self.transcript = slot.transcript;
        self.queued = slot.prompt_queue.len();
        self.prompt_queue = slot.prompt_queue;
        self.prompt_pending = slot.prompt_pending;
        self.modes = slot.modes;
        self.session_bound = slot.session_bound;
        self.pending_steer_cells = slot.pending_steer_cells;
        self.subagents = slot.subagents;
        self.current_subagents = slot.current_subagents;
        self.next_subagent_starts_batch = slot.next_subagent_starts_batch;
        self.active_subagent = slot.active_subagent;
        self.agent_selection = slot.agent_selection;
        // A session-bound ask rides along with its session: an ask that was
        // open when the user left this tab (or that arrived while parked)
        // resurfaces here, untouched, ready to answer or Esc away.
        self.permission_ask = slot.permission_ask;
        self.elicitation_ask = slot.elicitation_ask;
        // Painter popups and the /plugins tree resurface the same way.
        self.view_overlay = slot.view_overlay;
        self.plugin_tree = slot.plugin_tree;
        // Composer + chat chrome bound to the tab: draft, staged images,
        // @file browser, scroll, banner, model pick, running note.
        self.input = slot.input;
        self.pending_images = slot.pending_images;
        self.input_expanded = slot.input_expanded;
        self.file_menu = slot.file_menu;
        self.file_menu_dismissed = slot.file_menu_dismissed;
        self.scroll_up = slot.scroll_up;
        self.selected_model = slot.selected_model;
        self.show_banner = slot.show_banner;
        let running = slot.running || slot.prompt_pending;
        self.state = if running {
            RunState::Running
        } else {
            RunState::Idle
        };
        // Restore the meta-row chrome as left: an in-flight turn keeps its
        // elapsed timer and note; a session that settled while parked
        // shows the idle meta row.
        self.run_started = if running { slot.run_started } else { None };
        self.state_note = if running {
            slot.state_note
        } else {
            String::new()
        };
    }

    /// Per-view caches that must not leak across a tab switch. Draft,
    /// staged images, scroll and model pick come back from the slot via
    /// [`Self::put_live_slot`]; everything here is transient interaction
    /// state that must never resurface on the incoming tab.
    fn after_switch(&mut self) {
        self.chat_view = ChatView::default();
        self.sel = None;
        self.selecting = false;
        self.last_click = None;
        self.input_sel = None;
        self.input_selecting = false;
        self.picker = None;
        self.queue_selection = None;
        self.queue_edit = None;
        self.vim.reset_pending();
        self.needs_redraw = true;
    }

    /// Switch the view to tab `tab` (conceptual index, live spliced in at
    /// `current`): park the live state into its slot, load the target.
    pub fn switch_to_session(&mut self, tab: usize) {
        if tab == self.current || tab > self.parked.len() {
            return;
        }
        let pidx = if tab < self.current { tab } else { tab - 1 };
        let live = self.take_live_slot();
        let mut target = self.parked.remove(pidx);
        target.completed_unseen = false;
        // The parked copy takes the live tab's conceptual position.
        let park_at = if self.current < tab {
            self.current
        } else {
            self.current - 1
        };
        self.parked.insert(park_at, live);
        self.current = tab;
        self.put_live_slot(target);
        self.after_switch();
    }

    /// Leave the viewed session for another tab — the mouse click and the
    /// `/session prev|next` commands take the same path: transient
    /// interactions are dismissed and compositor-owned overlays are
    /// released with their cancel events (the plugin owns a
    /// single-overlay slot; Esc would send the same). Painter popups and
    /// ACP asks park with their session instead and resurface on return.
    pub fn switch_view_to_tab(&mut self, tab: usize, ctl: &Controller) {
        self.sel = None;
        self.selecting = false;
        self.last_click = None;
        self.input_sel = None;
        self.input_selecting = false;
        if tab != self.current {
            self.cancel_plugin_overlays(ctl);
        }
        self.switch_to_session(tab);
    }

    /// Park the live session and open a fresh tab for `id` at the end.
    /// `bound` is true only when the id is already a real session id.
    fn open_new_session(&mut self, id: String, bound: bool) {
        let live = self.take_live_slot();
        self.parked.insert(self.current, live);
        self.current = self.parked.len();
        let mut slot = SessionSlot::fresh(id, bound);
        slot.transcript.locale = self.locale;
        self.put_live_slot(slot);
        self.after_switch();
    }

    /// Conceptual tab index of the session with this id (live or parked).
    fn tab_index_of(&self, id: &str) -> Option<usize> {
        if self.session_id == id {
            return Some(self.current);
        }
        self.parked.iter().position(|slot| slot.id == id).map(|pidx| {
            if pidx < self.current {
                pidx
            } else {
                pidx + 1
            }
        })
    }

    /// Tab strip model: every session in tab order with the live tab
    /// spliced in at `current`. Native status chrome fed by session status
    /// facts (same source as `state_line`, never the palette/slot systems).
    pub fn session_tabs(&self) -> Vec<SessionTab> {
        let mut tabs: Vec<SessionTab> = self
            .parked
            .iter()
            .map(|slot| SessionTab {
                label: slot
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| short_id(&slot.id)),
                running: slot.running || slot.prompt_pending,
                completed_unseen: slot.completed_unseen,
                ask_pending: slot.permission_ask.is_some() || slot.elicitation_ask.is_some(),
                current: false,
            })
            .collect();
        tabs.insert(
            self.current,
            SessionTab {
                label: self
                    .session_title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| short_id(&self.session_id)),
                running: self.state != RunState::Idle || self.prompt_pending,
                completed_unseen: false,
                ask_pending: self.permission_ask.is_some() || self.elicitation_ask.is_some(),
                current: true,
            },
        );
        tabs
    }

    /// Number of session tabs (parked + live).
    pub fn session_tab_count(&self) -> usize {
        self.parked.len() + 1
    }

    /// Tab index of the live session — the position where `session_tabs`
    /// splices it into the parked list. The painter uses it to keep the
    /// strip's scroll window anchored on the tab being viewed.
    pub(crate) fn live_tab_index(&self) -> usize {
        self.current
    }

    /// Hit-test a screen cell against the tab rects recorded by the painter.
    fn tab_at(&self, col: u16, row: u16) -> Option<usize> {
        self.tab_rects
            .iter()
            .find(|(rect, _)| {
                col >= rect.x && col < rect.right() && row >= rect.y && row < rect.bottom()
            })
            .map(|(_, idx)| *idx)
    }

    /// Was this session mid-turn (live RunState, or a parked slot's running
    /// bit)? Read just before folding a `SessionStatus` event.
    fn session_was_running(&self, session: &str) -> bool {
        if session == self.session_id {
            matches!(self.state, RunState::Running)
        } else {
            self.parked
                .iter()
                .any(|slot| slot.id == session && slot.running)
        }
    }

    /// Fold an event addressed to a parked session into its slot: the same
    /// mode/status facts the live path folds, plus the running → idle edge
    /// that raises the tab's completion badge. Live UI state is untouched.
    fn apply_to_slot(slot: &mut SessionSlot, ui: crate::events::UiEvent) {
        use crate::events::UiEvent as E;
        let mut apply_to_transcript = true;
        match &ui {
            E::PlanMode { active, .. } => {
                if slot.modes.plan == *active {
                    apply_to_transcript = false;
                } else {
                    slot.modes.plan = *active;
                }
            }
            E::SandboxMode { mode, .. } => slot.modes.sandbox = Some(mode.clone()),
            E::ApprovalPolicy { policy, .. } => slot.modes.approval = Some(policy.clone()),
            E::PermissionPreset { preset, .. } => slot.modes.permission = Some(preset.clone()),
            E::AgentPreset { preset, .. } => {
                slot.modes.agent_preset = Some(preset.clone());
                apply_to_transcript = false;
            }
            E::ReasoningEffort { effort, .. } => {
                slot.modes.effort = Some(effort.clone());
                apply_to_transcript = false;
            }
            E::SessionTitle { title, .. } => slot.title = Some(title.clone()),
            _ => {}
        }
        match &ui {
            E::SessionStatus { running, .. } => {
                if slot.running && !running {
                    slot.completed_unseen = true;
                }
                slot.running = *running;
                if !running {
                    slot.prompt_pending = false;
                }
            }
            E::TurnStart { .. } => {
                // A new turn on this session starts a fresh subagent batch
                // (mirror of the live path in `apply_ui`).
                slot.next_subagent_starts_batch = true;
                slot.running = true;
            }
            E::TurnEnd { .. } => {
                if slot.running {
                    slot.completed_unseen = true;
                }
                slot.prompt_pending = false;
            }
            _ => {}
        }
        if apply_to_transcript {
            slot.transcript.apply(ui);
        }
    }

    /// Dispatch the head of one session's queue — the live session takes
    /// the existing path; a parked session echoes into its own transcript
    /// and sends addressed by its own id (acp.rs fans in per session).
    fn dispatch_session_queue(&mut self, session: &str, ctl: &Controller) {
        if session == self.session_id {
            self.dispatch_next_queued(ctl);
            return;
        }
        let Some(slot) = self.parked.iter_mut().find(|slot| slot.id == session) else {
            return;
        };
        let Some(prompt) = slot.prompt_queue.pop_front() else {
            return;
        };
        for block in &prompt.blocks {
            match block {
                StagedBlock::Text(text) => slot.transcript.push_user(text.clone(), false),
                StagedBlock::Image(att) => slot.transcript.push_image(
                    att.name.clone(),
                    String::new(),
                    att.path.clone(),
                    att.data.clone(),
                    false,
                ),
            }
        }
        slot.prompt_pending = true;
        slot.running = true;
        slot.run_started = Some(Instant::now());
        slot.state_note = self.locale.tr("sending queued followup", "正在发送排队消息").into();
        slot.scroll_up = 0;
        match prompt.blocks.as_slice() {
            [StagedBlock::Text(text)] => ctl.send(Cmd::Prompt {
                session_id: session.to_string(),
                text: text.clone(),
            }),
            _ => ctl.send(Cmd::PromptImages {
                session_id: session.to_string(),
                blocks: prompt_blocks_from_staged(prompt.blocks),
            }),
        }
    }

    /// Settle one Send Now (steer) outcome. The steer's session may have
    /// been parked or even closed while the agent decided, so the pending
    /// entry is looked up on the viewed tab first and then in every parked
    /// slot (`next_prompt_id` is a single counter, so ids are unique app-
    /// wide). A deferred steer is requeued into its own session's FIFO and
    /// its echo bubble hidden there.
    fn settle_steer(&mut self, message_id: u64, deferred: bool) {
        if let Some(pending) = self.pending_steer_cells.remove(&message_id) {
            if deferred {
                self.transcript.hide_cells(&pending.cells);
                let queued = ClientQueuedPrompt {
                    id: message_id,
                    blocks: pending.blocks,
                };
                if pending.requeue_front {
                    self.prompt_queue.push_front(queued);
                } else {
                    self.prompt_queue.push_back(queued);
                }
                self.queued = self.prompt_queue.len();
                self.show_tip(self.locale.tr(
                    "agent deferred Send Now — queued after the active turn",
                    "Agent 暂缓了立即发送 —— 已排在当前轮次之后",
                ));
            }
            return;
        }
        // The owning tab is not the one in view — settle inside its slot.
        for slot in &mut self.parked {
            if let Some(pending) = slot.pending_steer_cells.remove(&message_id) {
                if deferred {
                    slot.transcript.hide_cells(&pending.cells);
                    let queued = ClientQueuedPrompt {
                        id: message_id,
                        blocks: pending.blocks,
                    };
                    if pending.requeue_front {
                        slot.prompt_queue.push_front(queued);
                    } else {
                        slot.prompt_queue.push_back(queued);
                    }
                }
                return;
            }
        }
        // The session was closed before the settle: nothing to do.
    }

    /// Re-detect the workspace git branch for the composer cap label. One
    /// in-process read of `.git/HEAD` (no subprocess), at most every
    /// `GIT_CHECK_INTERVAL` — unless forced right after a session shell
    /// command, when the user may have just run `!git checkout …`.
    fn refresh_git_branch(&mut self, force: bool) {
        if !force && self.git_check_at.elapsed() < GIT_CHECK_INTERVAL {
            return;
        }
        self.git_check_at = Instant::now();
        let branch = crate::ui::head_branch(&self.cfg.workspace);
        if branch != self.git_branch {
            self.git_branch = branch;
            self.needs_redraw = true;
        }
    }

    pub fn tick(&mut self) {
        // The cap's ":branch" label tracks mid-session checkouts (agent
        // `bash` tool, another terminal) on a throttled cadence.
        self.refresh_git_branch(false);
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
            Err(err) => self.show_tip(self.locale.trf(
                "palette ignored: {}",
                "已忽略主题包：{}",
                &[err.to_string()],
            )),
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
        self.show_tip(self.locale.trf(
            "theme: {} {}",
            "主题：{} {}",
            &[
                self.active_palette_id.clone(),
                self.theme.mode.as_str().to_string(),
            ],
        ));
    }

    fn select_palette(&mut self, id: &str, ctl: &Controller) {
        let Some(palette) = self.palettes.iter().find(|palette| palette.id == id) else {
            return;
        };
        if palette.loaded {
            self.activate_palette(id);
        } else {
            self.show_tip(self.locale.trf(
                "loading theme plugin for {}…",
                "正在加载主题插件 {}…",
                &[id.to_string()],
            ));
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
            "toggle" => self.toggle_theme_mode(),
            id => {
                if self.palettes.iter().any(|p| p.id == id) {
                    self.select_palette(id, ctl);
                } else {
                    self.show_tip(self.locale.trf(
                        "unknown palette: {}",
                        "未知主题包：{}",
                        &[id.to_string()],
                    ));
                    self.transcript.push_notice(
                        NoticeLevel::Warn,
                        self.locale
                            .trf("unknown palette `{}`", "未知主题包 `{}`", &[id.to_string()]),
                    );
                }
            }
        }
    }

    fn toggle_theme_mode(&mut self) {
        self.theme = self.theme.toggled();
        self.save_settings();
        self.show_tip(self.locale.trf(
            "theme: {} {}",
            "主题：{} {}",
            &[
                self.active_palette_id.clone(),
                self.theme.mode.as_str().to_string(),
            ],
        ));
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
            offset: 0,
            kind: PickerKind::Theme,
            title: self
                .locale
                .tr(
                    " theme · enter apply · esc close · /theme toggle · ctrl+t ",
                    " 主题 · enter 应用 · esc 关闭 · /theme toggle · ctrl+t ",
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
            offset: 0,
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

    fn open_plugin_tree(&mut self) {
        let mut state = tui_tree_widget::TreeState::default();
        // Open every provider branch by default; the selection starts on the
        // first provider row. Identifiers are paths: [provider] for a branch
        // and [provider, entryId] for one plugin leaf.
        let mut first_provider: Option<String> = None;
        for plugin in &self.static_plugins {
            let provider = crate::app::plugin_provider(&plugin.module_name);
            state.open(vec![provider.clone()]);
            if first_provider.is_none() {
                first_provider = Some(provider);
            }
        }
        if let Some(provider) = first_provider {
            state.select(vec![provider]);
        }
        self.plugin_tree = Some(PluginTree {
            title: self
                .locale
                .tr(
                    " Host plugins · static · read only · ↑↓ ←→ navigate · esc close ",
                    " Host 插件 · 静态 · 只读 · ↑↓ ←→ 导航 · esc 关闭 ",
                )
                .into(),
            state,
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
            offset: 0,
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
            offset: 0,
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

    pub(crate) fn slash_completion_open(&self) -> bool {
        !self.slash_completion_dismissed && !self.slash_matches().is_empty()
    }

    pub(crate) fn queue_editing(&self) -> bool {
        self.queue_edit.is_some()
    }

    pub(crate) fn queue_selecting(&self) -> bool {
        self.queue_selection.is_some()
    }

    pub(crate) fn queue_delete_confirming(&self) -> bool {
        self.queue_edit
            .as_ref()
            .is_some_and(|edit| edit.delete_confirm)
    }

    pub(crate) fn queue_previews(&self, limit: usize) -> Vec<QueuePreview> {
        let editing_id = self.queue_edit.as_ref().map(|edit| edit.prompt_id);
        let focused = self.queue_selection.or_else(|| {
            editing_id.and_then(|id| self.prompt_queue.iter().position(|prompt| prompt.id == id))
        });
        let start = if limit == 0 || self.prompt_queue.len() <= limit {
            0
        } else {
            focused
                .unwrap_or(0)
                .saturating_sub(limit - 1)
                .min(self.prompt_queue.len() - limit)
        };
        self.prompt_queue
            .iter()
            .enumerate()
            .skip(start)
            .take(if limit == 0 {
                self.prompt_queue.len()
            } else {
                limit
            })
            .map(|(index, prompt)| {
                let mut parts = Vec::new();
                for block in &prompt.blocks {
                    match block {
                        StagedBlock::Text(text) => {
                            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                            if !text.is_empty() {
                                parts.push(text);
                            }
                        }
                        StagedBlock::Image(image) => parts.push(format!("▣ {}", image.name)),
                    }
                }
                QueuePreview {
                    id: prompt.id,
                    ordinal: index + 1,
                    summary: if parts.is_empty() {
                        "empty prompt".into()
                    } else {
                        parts.join(" ")
                    },
                    selected: self.queue_selection == Some(index),
                    editing: editing_id == Some(prompt.id),
                }
            })
            .collect()
    }

    pub fn slash_matches(&self) -> Vec<SlashEntry> {
        let first = &self.input.lines()[0];
        if !first.starts_with('/') {
            return Vec::new();
        }
        if let Some((name, arg)) = first[1..].split_once(' ') {
            return self.slash_argument_matches(name, arg);
        }
        let prefix = &first[1..];
        let mut out: Vec<SlashEntry> = SLASH_COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(prefix))
            .map(|c| SlashEntry {
                name: c.name.to_string(),
                usage: c.usage.to_string(),
                desc: self.locale.command_desc(c.name, c.desc).to_string(),
                skill: false,
                plugin: false,
                section: None,
                completion: None,
            })
            .collect();
        let mut plugins: Vec<_> = self
            .plugin_commands
            .iter()
            .filter(|command| {
                command.name.starts_with(prefix)
                    && !SLASH_COMMANDS.iter().any(|c| c.name == command.name)
            })
            .collect();
        plugins.sort_by(|a, b| a.name.cmp(&b.name));
        for command in plugins {
            out.push(SlashEntry {
                name: command.name.clone(),
                usage: command
                    .input
                    .as_ref()
                    .map(|input| format!("/{} [{}]", command.name, input.hint))
                    .unwrap_or_else(|| format!("/{}", command.name)),
                desc: self
                    .locale
                    .plugin_command_desc(&command.name, &command.description)
                    .to_string(),
                skill: false,
                plugin: true,
                section: None,
                completion: None,
            });
        }
        // Host skills share the '/' namespace. Builtins win first, then an
        // active client command, because the latter never enters a prompt.
        // Each group stays alphabetical so the menu reads in name order.
        let mut skills: Vec<_> = self
            .skills
            .iter()
            .filter(|s| {
                !s.name.eq_ignore_ascii_case("logout")
                    && s.name.starts_with(prefix)
                    && !SLASH_COMMANDS.iter().any(|c| c.name == s.name)
                    && !self.plugin_command_active(&s.name)
            })
            .collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        for s in skills {
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
                section: None,
                completion: None,
            });
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
                        section: None,
                        completion: Some(format!("/{name} {}", option.value)),
                    })
                    .collect();
            }
        }

        self.builtin_argument_options(name)
            .into_iter()
            .filter(|(value, _, _)| value.starts_with(prefix))
            .map(|(value, label, desc)| SlashEntry {
                section: match (name, value.as_str()) {
                    ("theme", "toggle") => {
                        Some(self.locale.tr("Appearance", "明暗模式").to_string())
                    }
                    ("theme", _) => Some("Theme Plugins".to_string()),
                    _ => None,
                },
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
            "theme" => {
                let mut options = vec![(
                    "toggle".to_string(),
                    self.locale
                        .tr("Toggle dark / light", "切换 dark / light")
                        .to_string(),
                    self.locale
                        .tr("switch the current theme mode", "切换当前主题的明暗模式")
                        .to_string(),
                )];
                options.extend(self.palettes.iter().map(|palette| {
                    (
                        palette.id.clone(),
                        palette.label.clone(),
                        if palette.loaded {
                            "theme plugin".to_string()
                        } else {
                            "theme plugin · stopped".to_string()
                        },
                    )
                }));
                options
            }
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
            "session" => vec![
                plain("view", "show session + runtime info"),
                plain("prev", "switch to the previous session tab"),
                plain("next", "switch to the next session tab"),
            ],
            _ => Vec::new(),
        }
    }

    pub fn handle(&mut self, ev: AppEvent, ctl: &Controller) {
        // Cheap fingerprints instead of full snapshot clones: chatty agents
        // can stream hundreds of thousands of session updates (issue #94 —
        // a session/load replay storm), and cloning every queue/agents
        // snapshot per event multiplies that into a UI-melting alloc storm.
        let queue_before = (!self.demo).then(|| self.queue_fingerprint());
        let agents_before = (!self.demo).then(|| self.agents_fingerprint());
        self.handle_inner(ev, ctl);
        // A turn ran on the picked model → the stream is the truth again.
        if self.selected_model.is_some()
            && self.selected_model.as_deref() == self.transcript.last_model.as_deref()
        {
            self.selected_model = None;
        }
        if let Some(before) = queue_before {
            if self.queue_fingerprint() != before {
                ctl.send(Cmd::QueueSnapshot {
                    snapshot: self.queue_snapshot(),
                });
            }
        }
        if let Some(before) = agents_before {
            if self.agents_fingerprint() != before {
                ctl.send(Cmd::AgentsSnapshot {
                    snapshot: self.agents_snapshot(),
                });
            }
        }
    }

    /// Identity of everything `queue_snapshot` projects, without the summary
    /// strings: ordered prompt ids (order captures reordering) plus the
    /// selection/edit/confirm flags. A summary edit always passes through an
    /// `editing_id` transition, so text changes are covered too.
    fn queue_fingerprint(&self) -> (Vec<u64>, Option<u64>, Option<u64>, bool) {
        (
            self.prompt_queue.iter().map(|prompt| prompt.id).collect(),
            self.queue_selection
                .and_then(|index| self.prompt_queue.get(index))
                .map(|prompt| prompt.id),
            self.queue_edit.as_ref().map(|edit| edit.prompt_id),
            self.queue_delete_confirming(),
        )
    }

    /// Identity of everything `agents_snapshot` projects, without label
    /// strings: root running bit, per-view id/status/current, the history
    /// count, and the active/selected ids. Labels are assigned at view
    /// creation, so they cannot change without the id set changing.
    fn agents_fingerprint(&self) -> (String, Option<String>, bool, Vec<(String, u8, bool)>, usize) {
        let active_id = match self.active_subagent.as_deref() {
            Some(id) if !self.subagent_in_current_batch(id) => AGENT_HISTORY_ID.into(),
            Some(id) => id.into(),
            None => self.session_id.clone(),
        };
        let root_running = !matches!(self.state, RunState::Idle) || self.prompt_pending;
        let views: Vec<(String, u8, bool)> = self
            .subagents
            .iter()
            .map(|view| {
                let status = if view.running {
                    0
                } else if view.failed {
                    1
                } else {
                    2
                };
                (
                    view.id.clone(),
                    status,
                    self.subagent_in_current_batch(&view.id),
                )
            })
            .collect();
        let history_count = self
            .subagents
            .iter()
            .filter(|view| !self.subagent_in_current_batch(&view.id))
            .count();
        let selected_id = self.agent_selection.as_ref().and_then(|selected| {
            (self.subagents.iter().any(|view| view.id == *selected)
                || *selected == self.session_id
                || (history_count > 0 && selected == AGENT_HISTORY_ID))
                .then(|| selected.clone())
        });
        (active_id, selected_id, root_running, views, history_count)
    }

    fn queue_snapshot(&self) -> crate::bus::QueueSnapshot {
        let editing_id = self.queue_edit.as_ref().map(|edit| edit.prompt_id);
        let selected_id = self
            .queue_selection
            .and_then(|index| self.prompt_queue.get(index))
            .map(|prompt| prompt.id);
        crate::bus::QueueSnapshot {
            count: self.prompt_queue.len(),
            items: self
                .queue_previews(0)
                .into_iter()
                .map(|preview| crate::bus::QueueSnapshotItem {
                    id: preview.id,
                    ordinal: preview.ordinal,
                    summary: preview.summary,
                })
                .collect(),
            selected_id,
            editing_id,
            delete_confirm: self.queue_delete_confirming(),
        }
    }

    fn agents_snapshot(&self) -> crate::bus::AgentsSnapshot {
        let active_id = match self.active_subagent.as_deref() {
            Some(id) if !self.subagent_in_current_batch(id) => AGENT_HISTORY_ID.into(),
            Some(id) => id.into(),
            None => self.session_id.clone(),
        };
        let root_running = !matches!(self.state, RunState::Idle) || self.prompt_pending;
        let mut items = vec![crate::bus::AgentsSnapshotItem {
            id: self.session_id.clone(),
            label: self.locale.tr("main", "主会话").into(),
            kind: "main".into(),
            status: if root_running { "running" } else { "idle" }.into(),
            current: false,
        }];
        items.extend(self.subagents.iter().map(|view| {
            crate::bus::AgentsSnapshotItem {
                id: view.id.clone(),
                label: view.label.clone(),
                kind: "subagent".into(),
                status: if view.running {
                    "running"
                } else if view.failed {
                    "failed"
                } else {
                    "finished"
                }
                .into(),
                current: self.subagent_in_current_batch(&view.id),
            }
        }));
        let history_count = self
            .subagents
            .iter()
            .filter(|view| !self.subagent_in_current_batch(&view.id))
            .count();
        if history_count > 0 {
            items.push(crate::bus::AgentsSnapshotItem {
                id: AGENT_HISTORY_ID.into(),
                label: format!("History ({history_count})"),
                kind: "history".into(),
                status: "idle".into(),
                current: false,
            });
        }
        let selected_id = self.agent_selection.as_ref().and_then(|selected| {
            items
                .iter()
                .any(|item| item.id == *selected)
                .then(|| selected.clone())
        });
        crate::bus::AgentsSnapshot {
            active_id,
            selected_id,
            items,
        }
    }

    pub(crate) fn subagent_in_current_batch(&self, id: &str) -> bool {
        self.current_subagents.is_empty() || self.current_subagents.contains(id)
    }

    fn handle_inner(&mut self, ev: AppEvent, ctl: &Controller) {
        match ev {
            AppEvent::Terminate => {
                self.quit = true;
            }
            AppEvent::Term(term) => self.handle_term(term, ctl),
            AppEvent::Ui(ui) => {
                let idle_session = match &ui {
                    crate::events::UiEvent::SessionStatus {
                        session,
                        running: false,
                    } if self.session_was_running(session) => Some(session.clone()),
                    _ => None,
                };
                self.apply_ui(ui);
                if let Some(session) = idle_session {
                    self.dispatch_session_queue(&session, ctl);
                }
            }
            AppEvent::Rpc { method, params } => {
                if method == crate::cordis::AGENTS_NAVIGATE {
                    let protocol = params.get("protocol").and_then(serde_json::Value::as_u64);
                    let action = params.get("action").and_then(serde_json::Value::as_str);
                    if protocol == Some(crate::cordis::PROTOCOL) {
                        match action {
                            Some("begin") => self.begin_agent_navigation(),
                            Some("previous") => self.move_agent_selection(-1),
                            Some("next") => self.move_agent_selection(1),
                            Some("confirm") => self.confirm_agent_selection(),
                            Some("cancel") => self.cancel_agent_selection(),
                            _ => {}
                        }
                    }
                    self.needs_redraw = true;
                    return;
                }
                if method == crate::cordis::AGENTS_SELECT {
                    let protocol = params.get("protocol").and_then(serde_json::Value::as_u64);
                    let id = params.get("id").and_then(serde_json::Value::as_str);
                    if protocol == Some(crate::cordis::PROTOCOL) {
                        if let Some(id) = id {
                            self.select_agent_transcript(id);
                        }
                    }
                    self.needs_redraw = true;
                    return;
                }
                if method == crate::cordis::UI_UPDATE {
                    match serde_json::from_value::<UiPluginCatalog>(params) {
                        Ok(catalog) if catalog.protocol == 0 => self.ui_plugins = catalog.plugins,
                        Ok(_) => {}
                        Err(err) => self.show_tip(self.locale.trf(
                            "UI Plugin catalog ignored: {}",
                            "已忽略 UI 插件目录：{}",
                            &[err.to_string()],
                        )),
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
                        Err(err) => self.show_tip(self.locale.trf(
                            "plugin approvals ignored: {}",
                            "已忽略插件授权：{}",
                            &[err.to_string()],
                        )),
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
                        Err(err) => self.show_tip(self.locale.trf(
                            "commands ignored: {}",
                            "已忽略命令：{}",
                            &[err.to_string()],
                        )),
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
                                    self.show_tip(self.locale.tr(
                                        "overlay ignored: invalid select",
                                        "已忽略 overlay：无效的 select",
                                    ));
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
                                self.show_tip(self.locale.tr(
                                    "overlay ignored: invalid slider",
                                    "已忽略 overlay：无效的 slider",
                                ));
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
                                self.show_tip(self.locale.tr(
                                    "overlay ignored: invalid view",
                                    "已忽略 overlay：无效的 view",
                                ));
                                self.slider_overlay = None;
                                self.select_overlay = None;
                                self.view_overlay = None;
                            }
                            None => {
                                // The plugin's overlay closed (dispatch ack
                                // or plugin-side close). Only compositor
                                // surfaces go with it: a painter popup that
                                // was parked/restored by a tab switch in the
                                // meantime must survive the ack.
                                self.slider_overlay = None;
                                self.select_overlay = None;
                                if self
                                    .view_overlay
                                    .as_ref()
                                    .is_some_and(|view| view.notify_plugin)
                                {
                                    self.view_overlay = None;
                                }
                            }
                        },
                        Ok(_) => {}
                        Err(err) => self.show_tip(self.locale.trf(
                            "overlay ignored: {}",
                            "已忽略 overlay：{}",
                            &[err.to_string()],
                        )),
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
                        Err(err) => self.show_tip(self.locale.trf(
                            "slot ignored: {}",
                            "已忽略 slot：{}",
                            &[err.to_string()],
                        )),
                    }
                    self.needs_redraw = true;
                    return;
                }
                for ui in parse_notification(&method, &params) {
                    let idle_session = match &ui {
                        crate::events::UiEvent::SessionStatus {
                            session,
                            running: false,
                        } if self.session_was_running(session) => Some(session.clone()),
                        _ => None,
                    };
                    self.apply_ui(ui);
                    if let Some(session) = idle_session {
                        self.dispatch_session_queue(&session, ctl);
                    }
                }
                self.needs_redraw = true;
            }
            AppEvent::RuntimeStderr(_line) => {
                // kept in proto's tail buffer for diagnostics; stay quiet here
            }
            AppEvent::RuntimeExited(code) => {
                // A next prompt restarts the runtime; its fresh startup
                // bind is again an unrequested one (see `startup_bound`).
                self.startup_bound = false;
                self.prompt_pending = false;
                self.queued = 0;
                self.prompt_queue.clear();
                self.queue_selection = None;
                self.queue_edit = None;
                self.pending_steer_cells.clear();
                for slot in &mut self.parked {
                    // The dead runtime owned every session's delivery state,
                    // not just the viewed one.
                    slot.running = false;
                    slot.prompt_pending = false;
                    slot.prompt_queue.clear();
                    slot.pending_steer_cells.clear();
                }
                if self.state != RunState::Idle {
                    self.state = RunState::Idle;
                    self.run_started = None;
                }
                if let Some(c) = code {
                    if c != 0 {
                    self.transcript.push_notice(
                        NoticeLevel::Warn,
                        self.locale.trf(
                            "runtime exited with code {} — next prompt restarts it",
                            "运行时以退出码 {} 结束 —— 下一条提示会重启它",
                            &[c.to_string()],
                        ),
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
                        self.prompt_pending = false;
                        if self.state == RunState::Starting {
                            self.state = RunState::Running;
                        }
                        self.state_note.clear();
                    }
                    CtlEvent::SteerSettled {
                        message_id,
                        deferred,
                    } => {
                        self.settle_steer(message_id, deferred);
                    }
                    CtlEvent::Error(err) => {
                        // Connection-level failure (no session context): the
                        // notice belongs to the viewed tab. Session-scoped
                        // failures arrive as `SessionError` instead.
                        self.prompt_pending = false;
                        self.state = RunState::Idle;
                        self.run_started = None;
                        self.transcript.push_notice(NoticeLevel::Error, err);
                        self.dispatch_next_queued(ctl);
                    }
                    CtlEvent::SessionError {
                        session_id,
                        message,
                    } => {
                        // A failure for one specific session (prompt,
                        // steer, config select, …): the notice lands in that
                        // session's own transcript — a parked tab must not
                        // spill errors onto the viewed one. The session's
                        // delivery state settles (its prompt, if any, is not
                        // coming back) but its queue is left alone: an
                        // unbound/forgotten tab must not burn queued
                        // prompts through repeated rejections.
                        if session_id == self.session_id {
                            self.prompt_pending = false;
                            self.state = RunState::Idle;
                            self.run_started = None;
                            self.transcript
                                .push_notice(NoticeLevel::Error, message);
                        } else if let Some(slot) = self
                            .parked
                            .iter_mut()
                            .find(|slot| slot.id == session_id)
                        {
                            slot.prompt_pending = false;
                            slot.running = false;
                            slot.transcript
                                .push_notice(NoticeLevel::Error, message);
                        }
                    }
                    CtlEvent::BindFailed => {
                        // session/new·resume failed outright. acp completes
                        // bind requests in order, so the FIFO head owns the
                        // failure: drop its entry so a later bind cannot
                        // land on the dead request, and tell the tab that
                        // asked (it stays open and unbound — /close or
                        // retry /new·resume).
                        if let Some(awaiting) = self.awaiting_binds.pop_front() {
                            if awaiting.open {
                                let msg: String = self.locale.tr(
                                    "session bind failed — this tab stays open; /close it or retry /new",
                                    "会话绑定失败 —— 本标签页保持打开;/close 关闭或重试 /new",
                                ).into();
                                if self.session_id == awaiting.id {
                                    self.transcript.push_notice(NoticeLevel::Warn, msg);
                                } else if let Some(slot) = self
                                    .parked
                                    .iter_mut()
                                    .find(|slot| slot.id == awaiting.id)
                                {
                                    slot.transcript.push_notice(NoticeLevel::Warn, msg);
                                }
                            }
                        }
                        self.show_tip(self.locale.tr("session bind failed", "会话绑定失败"));
                        self.needs_redraw = true;
                    }
                    CtlEvent::CancelRequested => {
                        self.state_note = "cancelling".into();
                        self.transcript.cancel_open_work();
                    }
                    CtlEvent::Interrupted { session_id } => {
                        // One connection can run turns for several sessions;
                        // settle whichever session the interruption names —
                        // the viewed one, or a parked slot.
                        if session_id == self.session_id {
                            self.prompt_pending = false;
                            self.state = RunState::Idle;
                            self.run_started = None;
                            self.state_note.clear();
                            self.transcript.cancel_open_work();
                            self.transcript
                                .push_notice(
                                    NoticeLevel::Warn,
                                    self.locale
                                        .tr("interrupted — turn cancelled", "已中断 —— 本轮已取消")
                                        .into(),
                                );
                        } else if let Some(slot) =
                            self.parked.iter_mut().find(|slot| slot.id == session_id)
                        {
                            slot.prompt_pending = false;
                            slot.running = false;
                            slot.transcript.cancel_open_work();
                            slot.transcript
                                .push_notice(
                                    NoticeLevel::Warn,
                                    self.locale
                                        .tr("interrupted — turn cancelled", "已中断 —— 本轮已取消")
                                        .into(),
                                );
                        }
                        self.dispatch_session_queue(&session_id, ctl);
                    }
                    CtlEvent::Skills { skills } => {
                        self.skills = skills;
                    }
                    CtlEvent::StaticPlugins { plugins } => {
                        self.static_plugins = plugins;
                        self.open_plugin_tree();
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
                    CtlEvent::SessionModes {
                        session_id,
                        modes,
                        current,
                    } => {
                        let is_live = match &session_id {
                            Some(sid) => sid == &self.session_id,
                            None => true,
                        };
                        if !modes.is_empty() {
                            self.permission_choices = modes.clone();
                        }
                        if is_live {
                            if let Some(id) = &current {
                                self.modes.permission = Some(id.clone());
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
                        } else if let Some(sid) = session_id.as_deref() {
                            if let Some(slot) = self.parked.iter_mut().find(|s| s.id == sid) {
                                if let Some(id) = current {
                                    slot.modes.permission = Some(id);
                                }
                            }
                        }
                    }
                    CtlEvent::Efforts { efforts, default } => {
                        if !efforts.is_empty() {
                            self.effort_choices = efforts.clone();
                        }
                        self.open_effort_picker(efforts, default);
                    }
                    CtlEvent::PresetSet {
                        session_id,
                        preset,
                    } => {
                        let is_live = session_id.is_empty() || session_id == self.session_id;
                        let label = self.agent_label(&preset);
                        if is_live {
                            self.modes.agent_preset = Some(preset.clone());
                            self.transcript.push_notice(
                                NoticeLevel::Info,
                                self.locale.trf(
                                    "⚙ agent → {} · composes on this session's first prompt",
                                    "⚙ Agent → {} · 在本会话首次输入时生效",
                                    &[label],
                                ),
                            );
                        } else if let Some(slot) =
                            self.parked.iter_mut().find(|s| s.id == session_id)
                        {
                            slot.modes.agent_preset = Some(preset.clone());
                            slot.transcript.push_notice(
                                NoticeLevel::Info,
                                self.locale.trf(
                                    "⚙ agent → {} · composes on this session's first prompt",
                                    "⚙ Agent → {} · 在本会话首次输入时生效",
                                    &[label],
                                ),
                            );
                        }
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
                        if let Some((level, text)) = snap.notice(self.locale) {
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
                    CtlEvent::AgentCaps {
                        load_session,
                        list_session,
                        resume_session,
                    } => {
                        self.load_session = load_session;
                        self.list_session = list_session;
                        self.resume_session_cap = resume_session;
                    }
                    CtlEvent::SessionBound { session_id, notice } => {
                        // The session that just bound, for the queue dispatch
                        // below (prompts submitted while the tab was unbound
                        // are held in its queue — acp.rs rejects unbound ids).
                        let mut just_bound: Option<String> = None;
                        if !self.startup_bound && !self.awaiting_binds.is_empty() {
                            // The unrequested startup/reconnect session/new
                            // resolved while the user's own /new·resume is
                            // still in flight — acp completes the startup
                            // bind before any UI request, so this bind owns
                            // no awaiting entry. Its tab is the parked
                            // session that is unbound and not itself
                            // awaiting anything; never steal the FIFO head
                            // from the request that really owns it.
                            self.startup_bound = true;
                            let owner = self.parked.iter().position(|slot| {
                                !slot.session_bound
                                    && !self.awaiting_binds.iter().any(|e| e.id == slot.id)
                            });
                            match owner {
                                Some(pidx) => {
                                    let slot = &mut self.parked[pidx];
                                    let old_id = slot.id.clone();
                                    slot.id = session_id.clone();
                                    slot.transcript.set_root_session(session_id.clone());
                                    slot.session_bound = true;
                                    if let Some(notice) = notice {
                                        slot.transcript
                                            .push_notice(NoticeLevel::Info, notice);
                                    }
                                    for (_, sid, _) in &mut self.shell_pending {
                                        if sid == &old_id {
                                            *sid = session_id.clone();
                                        }
                                    }
                                    just_bound = Some(slot.id.clone());
                                }
                                None => {
                                    if !self.session_bound
                                        && !self.awaiting_binds.iter().any(|e| e.id == self.session_id)
                                    {
                                        if self.session_id != session_id {
                                            self.reset_subagent_views();
                                        }
                                        let old_id = self.session_id.clone();
                                        self.session_id = session_id.clone();
                                        self.transcript.set_root_session(session_id.clone());
                                        self.session_bound = true;
                                        if let Some(notice) = notice {
                                            self.transcript
                                                .push_notice(NoticeLevel::Info, notice);
                                        }
                                        for (_, sid, _) in &mut self.shell_pending {
                                            if sid == &old_id {
                                                *sid = session_id.clone();
                                            }
                                        }
                                        just_bound = Some(self.session_id.clone());
                                    } else {
                                        // The viewed tab is bound or awaiting its own bind:
                                        // park this startup session as a fresh slot.
                                        let mut slot =
                                            SessionSlot::fresh(session_id.clone(), true);
                                        slot.transcript.locale = self.locale;
                                        if let Some(notice) = notice {
                                            slot.transcript.push_notice(NoticeLevel::Info, notice);
                                        }
                                        just_bound = Some(session_id.clone());
                                        self.parked.push(slot);
                                    }
                                }
                            }
                        } else if let Some(awaiting) = self.awaiting_binds.pop_front() {
                            // The tab that asked for session/new (placeholder
                            // id) or session/resume·load (target id) owns
                            // this bind — the user may have switched away
                            // while it resolved, so rebind by the awaiting
                            // id, not by which tab happens to be live.
                            if !awaiting.open {
                                // The requesting tab was closed (/close)
                                // before the bind resolved. acp already
                                // registered the new session server-side, so
                                // forget it and never hijack the viewed tab.
                                ctl.send(Cmd::ForgetSession {
                                    session_id: session_id.clone(),
                                });
                                self.show_tip(self.locale.trf(
                                    "session {} bound after its tab was closed",
                                    "会话 {} 已绑定，但对应标签页已被关闭",
                                    &[session_id.clone()],
                                ));
                            } else if self.session_id == awaiting.id {
                                let old_id = self.session_id.clone();
                                self.session_id = session_id.clone();
                                self.transcript.set_root_session(session_id.clone());
                                self.session_bound = true;
                                if let Some(notice) = notice {
                                    self.transcript.push_notice(NoticeLevel::Info, notice);
                                }
                                for (_, sid, _) in &mut self.shell_pending {
                                    if sid == &old_id || sid == &awaiting.id {
                                        *sid = session_id.clone();
                                    }
                                }
                                just_bound = Some(self.session_id.clone());
                            } else if let Some(slot) =
                                self.parked.iter_mut().find(|slot| slot.id == awaiting.id)
                            {
                                let old_id = slot.id.clone();
                                slot.id = session_id.clone();
                                slot.transcript.set_root_session(session_id.clone());
                                slot.session_bound = true;
                                if let Some(notice) = notice {
                                    slot.transcript.push_notice(NoticeLevel::Info, notice);
                                }
                                for (_, sid, _) in &mut self.shell_pending {
                                    if sid == &old_id || sid == &awaiting.id {
                                        *sid = session_id.clone();
                                    }
                                }
                                just_bound = Some(slot.id.clone());
                            } else {
                                self.show_tip(self.locale.trf(
                                    "session {} bound after its tab was closed",
                                    "会话 {} 已绑定，但对应标签页已被关闭",
                                    &[session_id.clone()],
                                ));
                            }
                        } else if let Some(slot) =
                            self.parked.iter_mut().find(|slot| slot.id == session_id)
                        {
                            // A session/load for a parked tab resolved.
                            slot.session_bound = true;
                            if let Some(notice) = notice {
                                slot.transcript.push_notice(NoticeLevel::Info, notice);
                            }
                            just_bound = Some(slot.id.clone());
                        } else {
                            // Startup / reconnect: rebind the viewed session.
                            self.startup_bound = true;
                            if self.session_id != session_id {
                                self.reset_subagent_views();
                            }
                            let old_id = self.session_id.clone();
                            self.session_id = session_id.clone();
                            self.transcript.set_root_session(session_id.clone());
                            self.session_bound = true;
                            if let Some(notice) = notice {
                                self.transcript.push_notice(NoticeLevel::Info, notice);
                            }
                            for (_, sid, _) in &mut self.shell_pending {
                                if sid == &old_id {
                                    *sid = session_id.clone();
                                }
                            }
                            just_bound = Some(self.session_id.clone());
                        }
                        if let Some(bound) = just_bound {
                            self.dispatch_session_queue(&bound, ctl);
                        }
                        ctl.send(Cmd::FetchSkills);
                    }
                    CtlEvent::SessionList {
                        sessions,
                        prefix,
                        limit,
                    } => {
                        self.on_acp_session_list(sessions, prefix, limit, ctl);
                    }
                    CtlEvent::SessionListUnavailable {
                        prefix,
                        limit,
                        error,
                    } => {
                        self.on_acp_session_list_unavailable(prefix, limit, error, ctl);
                    }
                }
                self.needs_redraw = true;
            }
            AppEvent::PermissionAsk {
                session_id,
                title,
                options,
                reply,
            } => {
                self.open_permission_ask(&session_id, title, options, reply);
            }
            AppEvent::ElicitationAsk {
                session_id,
                form,
                reply,
            } => {
                self.open_elicitation_ask(session_id.as_deref(), form, reply);
            }
            AppEvent::ShellDone { id, code, output } => {
                if let Some(pos) = self
                    .shell_pending
                    .iter()
                    .position(|(sid, _, _)| *sid == id)
                {
                    let (_, session, cell) = self.shell_pending.remove(pos);
                    // The shell is workspace-wide but its cell lives in the
                    // transcript of the session that ran it — the user may
                    // have switched tabs while the command ran.
                    if session == self.session_id {
                        self.transcript.finish_shell(cell, code, output);
                    } else if let Some(slot) =
                        self.parked.iter_mut().find(|slot| slot.id == session)
                    {
                        slot.transcript.finish_shell(cell, code, output);
                    }
                    self.needs_redraw = true;
                }
                // `!git checkout …` in the session shell moves the branch —
                // refresh the composer cap label right away.
                self.refresh_git_branch(true);
            }
        }
    }

    /// Fold one decoded protocol fact into both client chrome and transcript.
    /// Direct ACP facts and JSON-RPC notifications must take the same path.
    fn apply_ui(&mut self, ui: crate::events::UiEvent) {
        use crate::events::UiEvent as E;

        if let E::TurnStart { session, .. } = &ui {
            if session == &self.session_id {
                self.next_subagent_starts_batch = true;
            }
        }

        if let E::SubagentStarted { parent, child } = &ui {
            if parent != &self.session_id && !self.subagents.iter().any(|view| view.id == *parent)
            {
                // A parked session's subagent tree: register the child in
                // its slot and fold the event into the slot's transcripts.
                for slot in &mut self.parked {
                    if slot.id == *parent || slot.subagents.iter().any(|v| v.id == *parent) {
                        // A fresh turn starts a new batch, exactly like the
                        // live path below — background sessions must keep
                        // their current/history grouping current.
                        if slot.next_subagent_starts_batch {
                            slot.current_subagents.clear();
                            slot.next_subagent_starts_batch = false;
                        }
                        slot.current_subagents.insert(child.clone());
                        upsert_subagent_view(&mut slot.subagents, parent, child, self.locale);
                        if slot.id == *parent {
                            slot.transcript.apply(ui);
                        } else if let Some(view) =
                            slot.subagents.iter_mut().find(|view| view.id == *parent)
                        {
                            view.transcript.apply(ui);
                        }
                        self.needs_redraw = true;
                        return;
                    }
                }
                // Unknown parent: a session this client never opened (or
                // already closed) — drop it, never leak it into live state.
                return;
            }
            if self.next_subagent_starts_batch {
                self.current_subagents.clear();
                self.next_subagent_starts_batch = false;
            }
            self.current_subagents.insert(child.clone());
            upsert_subagent_view(&mut self.subagents, parent, child, self.locale);
            if parent == &self.session_id {
                self.transcript.apply(ui);
            } else if let Some(view) = self.subagents.iter_mut().find(|view| view.id == *parent) {
                view.transcript.apply(ui);
            }
            self.needs_redraw = true;
            return;
        }

        if let E::SubagentFinished { child, failed } = &ui {
            if !self.subagents.iter().any(|view| view.id == *child) {
                // A parked session's subagent: settle it inside its slot.
                for slot in &mut self.parked {
                    if let Some(view) = slot.subagents.iter_mut().find(|view| view.id == *child) {
                        view.running = false;
                        view.failed = *failed;
                        let parent = view.parent.clone();
                        if parent == slot.id {
                            slot.transcript.apply(ui);
                        } else if let Some(pview) =
                            slot.subagents.iter_mut().find(|view| view.id == parent)
                        {
                            pview.transcript.apply(ui);
                        }
                        // Issue #80 parity: a parked session's rail also
                        // auto-closes once every subagent task has ended.
                        if slot.agent_selection.is_some()
                            && !slot.subagents.iter().any(|view| view.running || view.failed)
                        {
                            slot.agent_selection = None;
                        }
                        self.needs_redraw = true;
                        return;
                    }
                }
                // Unknown child — drop (same guard as SubagentStarted).
                return;
            }
            let parent = self
                .subagents
                .iter_mut()
                .find(|view| view.id == *child)
                .map(|view| {
                    view.running = false;
                    view.failed = *failed;
                    view.parent.clone()
                });
            if parent.as_deref() == Some(self.session_id.as_str()) {
                self.transcript.apply(ui);
            } else if let Some(parent) = parent {
                if let Some(view) = self.subagents.iter_mut().find(|view| view.id == parent) {
                    view.transcript.apply(ui);
                }
            }
            // Issue #80: the panel auto-closes once every subagent task has
            // ended. An inline selection left open after the last task would
            // otherwise keep the rail visible forever — the Client plugin
            // only hides the summary while nothing is selected.
            if self.agent_selection.is_some()
                && !self.subagents.iter().any(|view| view.running || view.failed)
            {
                self.agent_selection = None;
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
                for slot in &mut self.parked {
                    if slot.id == session {
                        Self::apply_to_slot(slot, ui);
                        self.needs_redraw = true;
                        return;
                    }
                    if let Some(view) = slot.subagents.iter_mut().find(|view| view.id == session) {
                        view.transcript.apply(ui);
                        self.needs_redraw = true;
                        return;
                    }
                }
                // A session this client never opened (or already closed):
                // drop the event. Foreign-session facts must never fall
                // through into the live transcript (issue #94 leak guard).
                return;
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
            E::ReasoningEffort { session, effort } if *session == self.session_id => {
                self.modes.effort = Some(effort.clone());
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
                self.slash_completion_dismissed = false;
                // The composer is multi-line (soft wrap, ctrl+j), so pasted
                // text keeps its line structure instead of being flattened
                // to spaces (issue #54). `TextArea::insert_str` understands
                // both `\n` and `\r\n`; normalize stray CR-only line
                // endings some terminals (iTerm2 et al.) send.
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                self.input.insert_str(&text);
                self.input_sel = None;
                self.reconcile_attachments();
                self.needs_redraw = true;
            }
            _ => {}
        }
    }

    /// Cancel compositor-owned modals before a session switch, mirroring
    /// the Esc paths of `handle_view_key` / `handle_select_key` /
    /// `handle_slider_key` so the plugin releases its single-overlay slot
    /// (its `current` clears and future overlay opens work again).
    /// Painter-owned views (`notify_plugin == false`, i.e. `/help`, `/keys`,
    /// `/session`, painter `/status`) are left alone — they park with their
    /// session inside the switch and resurface on return.
    fn cancel_plugin_overlays(&mut self, ctl: &Controller) {
        if let Some(view) = self.view_overlay.take() {
            if view.notify_plugin {
                ctl.send(Cmd::PluginOverlayEvent {
                    id: view.id,
                    event: "cancel".into(),
                    value: None,
                });
            } else {
                self.view_overlay = Some(view);
            }
        }
        if let Some(slider) = self.slider_overlay.take() {
            ctl.send(Cmd::PluginOverlayEvent {
                id: slider.id,
                event: "cancel".into(),
                value: Some(serde_json::json!(slider.value)),
            });
        }
        if let Some(select) = self.select_overlay.take() {
            let value = select.options[select.sel].value.clone();
            ctl.send(Cmd::PluginOverlayEvent {
                id: select.id,
                event: "cancel".into(),
                value: Some(serde_json::json!(value)),
            });
        }
    }

    /// grok-build mouse semantics, scaled down: wheel scrolls; left-drag
    /// selects with a live highlight (auto-scrolling at the pane edges) and
    /// copies on release; double-click selects & copies a word. Shift+drag
    /// bypasses capture in most terminals → native selection still works.
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent, ctl: &Controller) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.elicitation_ask.is_some() {
                    self.elicitation_scroll_by(-3);
                } else if self.permission_ask.is_some() {
                    self.permission_ask_scroll_by(-1);
                } else if let Some(tree) = &mut self.plugin_tree {
                    tree.state.scroll_up(3);
                } else if self.view_overlay.is_some() {
                    self.view_scroll_by(-3);
                } else if self.picker.is_some() {
                    self.picker_scroll_by(-1);
                } else {
                    self.mouse_scroll(3, mouse.column, mouse.row);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.elicitation_ask.is_some() {
                    self.elicitation_scroll_by(3);
                } else if self.permission_ask.is_some() {
                    self.permission_ask_scroll_by(1);
                } else if let Some(tree) = &mut self.plugin_tree {
                    tree.state.scroll_down(3);
                } else if self.view_overlay.is_some() {
                    self.view_scroll_by(3);
                } else if self.picker.is_some() {
                    self.picker_scroll_by(1);
                } else {
                    self.mouse_scroll(-3, mouse.column, mouse.row);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.needs_redraw = true;
                // Session tab strip (issue #94): left-click a tab switches.
                if let Some(tab) = self.tab_at(mouse.column, mouse.row) {
                    self.switch_view_to_tab(tab, ctl);
                    // Clicking the strip's outermost visible tab also
                    // nudges the strip so its neighbor appears — repeated
                    // edge clicks walk the mouse through every session
                    // tab, left or right. `switch_view_to_tab` already
                    // released overlays etc.; only the strip moves here.
                    let first = self
                        .tab_rects
                        .iter()
                        .map(|(_, idx)| *idx)
                        .min()
                        .unwrap_or(tab);
                    let last = self
                        .tab_rects
                        .iter()
                        .map(|(_, idx)| *idx)
                        .max()
                        .unwrap_or(tab);
                    if tab == first && tab > 0 {
                        self.tab_strip_offset = self.tab_strip_offset.saturating_sub(1);
                        self.needs_redraw = true;
                    } else if tab == last && tab + 1 < self.session_tab_count() {
                        self.tab_strip_offset = self.tab_strip_offset.saturating_add(1);
                        self.needs_redraw = true;
                    }
                    return;
                }
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
                    self.input_selecting = false;
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
                    self.input_selecting = false;
                    self.toggle_tool(ci);
                    return;
                }
                // The mouse-only expand button (issue #92) wins over caret
                // placement inside the well: clicking toggles the input
                // height instead of moving the caret. It lives inside the
                // well, so it is only live outside child views and the
                // elicitation form (which owns its own field).
                if self.elicitation_ask.is_none()
                    && self.active_subagent.is_none()
                    && self.expand_btn_hit(mouse.column, mouse.row)
                {
                    self.sel = None;
                    self.selecting = false;
                    self.last_click = None;
                    self.input_selecting = false;
                    self.input_sel = None;
                    self.input_expanded = !self.input_expanded;
                    self.needs_redraw = true;
                    return;
                }
                // Click inside the composer well: place the caret at the
                // clicked char and arm an input drag-selection. Like any
                // click outside the chat pane, the chat highlight is
                // dismissed first. The elicitation form owns its own field
                // and child-view chrome replaces the composer, so the
                // hidden caret stays put in both cases.
                if self.elicitation_ask.is_none()
                    && self.active_subagent.is_none()
                    && self.input_hit(mouse.column, mouse.row)
                {
                    self.sel = None;
                    self.selecting = false;
                    self.last_click = None;
                    let cell = self.input_cell_at(mouse.column, mouse.row);
                    let offset = self.input.screen_to_char(self.input_avail(), cell.0, cell.1);
                    self.input.set_cursor_char(offset);
                    self.input_sel = Some(InputSel {
                        anchor: cell,
                        head: cell,
                    });
                    self.input_selecting = true;
                    self.refresh_file_menu();
                    return;
                }
                let Some(p) = self.chat_hit(mouse.column, mouse.row) else {
                    // Click outside the chat pane dismisses the highlight.
                    self.sel = None;
                    self.selecting = false;
                    self.input_sel = None;
                    self.input_selecting = false;
                    return;
                };
                self.input_sel = None;
                self.input_selecting = false;
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
            MouseEventKind::Drag(MouseButton::Left) if self.input_selecting => {
                // The head snaps to the well edges: drags above select to
                // the top visible row, below it to the bottom row.
                let head = self.input_cell_at(mouse.column, mouse.row);
                if let Some(sel) = &mut self.input_sel {
                    sel.head = head;
                }
                self.needs_redraw = true;
            }
            MouseEventKind::Up(MouseButton::Left) if self.selecting => {
                self.selecting = false;
                self.finish_selection();
            }
            MouseEventKind::Up(MouseButton::Left) if self.input_selecting => {
                self.input_selecting = false;
                self.finish_input_selection();
            }
            MouseEventKind::Moved => {
                // grok-style hover: track which inline chip the pointer is
                // over; redraw only on changes (mouse moves are a firehose).
                self.mouse_pos = Some((mouse.column, mouse.row));
                let over_btn = self.expand_btn_hit(mouse.column, mouse.row);
                if over_btn != self.hover_expand_btn {
                    self.hover_expand_btn = over_btn;
                    self.needs_redraw = true;
                }
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
        let buf = self.input.buf();
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
        self.input.delete_char_range(start, end);
        if let Some(att) = self.pending_images.remove(idx) {
            self.show_tip(self.locale.trf("removed {}", "已移除 {}", &[att.name.clone()]));
        }
        true
    }

    /// Drop attachments whose token no longer survives in the draft text.
    fn reconcile_attachments(&mut self) {
        if self.pending_images.reconcile(&self.input.buf()) > 0 {
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
        let c = self.input.cursor_char();
        self.token_spans()
            .iter()
            .find(|(s, e, _)| c >= *s && c <= *e)
            .map(|&(_, _, idx)| idx)
    }

    /// Hit-test the mouse-only composer expand button (issue #92). The
    /// button's rect is recorded by the painter every frame, so the
    /// pointer can discover it while moving across the card's top-right.
    fn expand_btn_hit(&self, col: u16, row: u16) -> bool {
        self.expand_btn.is_some_and(|r| {
            col >= r.x
                && col < r.x.saturating_add(r.width)
                && row >= r.y
                && row < r.y.saturating_add(r.height)
        })
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

    /// Wheel-scroll the open picker — the `/resume` session list, `/model`,
    /// `/mode`, … One notch moves the selection exactly like one ↑/↓ press
    /// (the highlight sweeps through a static window; the window itself only
    /// scrolls once the selection leaves it — see `draw_model_picker`).
    /// Steps clamp at both ends; only the arrows wrap, so an overscrolled
    /// notch never teleports the highlight to the list tail.
    fn picker_scroll_by(&mut self, delta: i64) {
        let Some(picker) = &mut self.picker else { return };
        let last = picker.items.len().saturating_sub(1);
        if delta < 0 {
            picker.sel = picker
                .sel
                .saturating_sub(delta.unsigned_abs() as usize)
                .min(last);
        } else {
            picker.sel = picker.sel.saturating_add(delta as usize).min(last);
        }
        self.needs_redraw = true;
    }

    /// Wheel over an ACP permission ask (which floats above host pickers)
    /// moves its highlight, mirroring the ↑/↓ keys and the modal priority
    /// of `handle_key` — an ask on top of the `/resume` picker must not
    /// scroll the picker underneath it.
    fn permission_ask_scroll_by(&mut self, delta: i64) {
        let Some(ask) = &mut self.permission_ask else { return };
        let last = ask.options.len().saturating_sub(1);
        if delta < 0 {
            ask.sel = ask
                .sel
                .saturating_sub(delta.unsigned_abs() as usize)
                .min(last);
        } else {
            ask.sel = ask.sel.saturating_add(delta as usize).min(last);
        }
        self.needs_redraw = true;
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
        self.show_tip(self.locale.trf(
            "{} tool output · click toggles",
            "{} 工具输出 · 点击切换展开",
            &[label.into()],
        ));
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
            self.show_tip(self.locale.trf(
                "✓ copied {} chars — esc clears the highlight",
                "✓ 已复制 {} 个字符 —— esc 清除高亮",
                &[chars.to_string()],
            ));
        } else {
            self.show_tip(self.locale.tr(
                "copy failed — hold shift and drag for the terminal's native selection",
                "复制失败 —— 按住 shift 拖动可使用终端原生选择",
            ));
        }
    }

    /// True when the screen cell lies inside the composer input well.
    fn input_hit(&self, col: u16, row: u16) -> bool {
        let a = self.input_area;
        a.width > 0
            && a.height > 0
            && col >= a.x
            && col < a.x.saturating_add(a.width)
            && row >= a.y
            && row < a.y.saturating_add(a.height)
    }

    /// Display-cell width of the composer text area, matching
    /// `ui::draw_input` (`area.width - prompt width`).
    fn input_avail(&self) -> usize {
        let a = self.input_area;
        let pw = "❯ "
            .chars()
            .map(|c| c.width().unwrap_or(0).max(1))
            .sum::<usize>();
        a.width.saturating_sub(pw as u16).max(1) as usize
    }

    /// Map a screen cell to a text-area cell `(row, col)` in the same
    /// coordinates as `Input::char_index_at` (prompt offset and viewport
    /// scroll applied), clamped into the visible well.
    fn input_cell_at(&self, col: u16, row: u16) -> (usize, usize) {
        let a = self.input_area;
        let pw = "❯ "
            .chars()
            .map(|c| c.width().unwrap_or(0).max(1))
            .sum::<usize>();
        let rel_row = (row.saturating_sub(a.y) as usize)
            .min(a.height.saturating_sub(1) as usize)
            .saturating_add(self.input_top);
        let rel_col = (col.saturating_sub(a.x.saturating_add(pw as u16)) as usize)
            .min(self.input_avail().saturating_sub(1));
        (rel_row, rel_col)
    }

    /// Ordered char boundaries covered by the composer selection — both
    /// endpoint cells inclusive, so a drag in either direction covers
    /// exactly the cells the pointer crossed. `None` without a selection.
    pub(crate) fn input_selection_range(&mut self) -> Option<(usize, usize)> {
        let sel = self.input_sel?;
        let (s, e) = if sel.anchor <= sel.head {
            (sel.anchor, sel.head)
        } else {
            (sel.head, sel.anchor)
        };
        let avail = self.input_avail();
        let start = self.input.screen_to_char(avail, s.0, s.1);
        let end = self.input.screen_to_char_end(avail, e.0, e.1);
        (start < end).then_some((start, end))
    }

    /// Copy the dragged composer selection; the highlight persists until
    /// the next click or Esc, mirroring the chat pane. A plain click
    /// (caret) just clears the highlight.
    fn finish_input_selection(&mut self) {
        self.needs_redraw = true;
        let Some(sel) = self.input_sel else { return };
        if sel.anchor == sel.head {
            self.input_sel = None;
            return;
        }
        let Some((a, b)) = self.input_selection_range() else {
            self.input_sel = None;
            return;
        };
        let text = self.input.chars_between(a, b);
        if text.trim().is_empty() {
            self.input_sel = None;
            return;
        }
        self.copy_text(&text);
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
        current["themeMode"] = serde_json::json!(self.theme.mode.as_str());
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
        // New notices from every transcript (live, parked, subagent views)
        // render in the switched language; cells already pushed keep theirs.
        self.transcript.locale = next;
        for slot in &mut self.parked {
            slot.transcript.locale = next;
            for view in &mut slot.subagents {
                view.transcript.locale = next;
            }
        }
        for view in &mut self.subagents {
            view.transcript.locale = next;
        }
        self.save_settings();
        self.show_tip(match next {
            Locale::En => "Language switched to English",
            Locale::Zh => "界面语言已切换为中文",
        });
        self.needs_redraw = true;
    }

    pub fn scroll_by(&mut self, delta: i64) {
        // usize::MAX is the JumpTop sentinel — resolve it against the last
        // rendered frame before arithmetic (MAX as i64 wraps to -1, so a
        // wheel flick in the same event burst as JumpTop would teleport the
        // viewport from the top of scrollback to just above the tail).
        let cur = if self.scroll_up == usize::MAX {
            self.chat_view
                .lines
                .len()
                .saturating_sub(self.chat_view.area.height as usize) as i64
        } else {
            self.scroll_up as i64
        };
        // The renderer clamps the relative gesture to the actual content.
        self.scroll_up = (cur + delta).max(0) as usize;
        // Apply this gesture relative to the frame the user actually saw.
        // draw_chat resolves the resulting offset back into an absolute
        // anchor, which later streaming cannot move.
        self.chat_view.manual_top = None;
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
            self.show_tip(self.locale.trf(
                "key: {} + {}",
                "按键：{} + {}",
                &[
                    format!("{:?}", key.modifiers),
                    format!("{:?}", key.code),
                ],
            ));
        }

        // Vim mode intercepts plain keys while it is active; overlays and
        // forms keep their own key handling.
        if self.vim.is_active()
            && self.elicitation_ask.is_none()
            && self.queue_edit.is_none()
            && !self.slash_completion_open()
        {
            if self.vim.handle_key(&key, &mut self.input) {
                self.reconcile_attachments();
                self.refresh_file_menu();
                return;
            }
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

        // --- /plugins tree popup (provider → plugin inventory)
        if self.plugin_tree.is_some() {
            self.handle_plugin_tree_key(key);
            return;
        }

        if self.agent_selection.is_some() {
            if key.modifiers == KeyModifiers::NONE {
                match key.code {
                    KeyCode::Left => self.move_agent_selection(-1),
                    KeyCode::Right => self.move_agent_selection(1),
                    KeyCode::Enter => self.confirm_agent_selection(),
                    KeyCode::Esc => self.cancel_agent_selection(),
                    _ => {}
                }
            }
            return;
        }

        if self.active_subagent.is_some() {
            if key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::NONE {
                self.handle_esc(ctl);
                return;
            }
            if key.code == KeyCode::Down && key.modifiers == KeyModifiers::NONE {
                self.begin_agent_navigation();
                return;
            }
            let ctx = crate::input::KeyCtx {
                input_empty: true,
                history_active: false,
            };
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

        // The @file browser owns its navigation keys while open; everything
        // else falls through to normal editing (which re-syncs the browser
        // through `refresh_file_menu`). Enter settles, Tab drills into a
        // directory, → follows the explorer's enter semantics.
        if let Some(menu) = &mut self.file_menu {
            // ctrl+h toggles hidden files/dirs (the explorer's own binding
            // for ToggleShowHidden); it stays modal while the browser is
            // open, like the navigation keys.
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('h') {
                crate::file_ref::navigate(menu, crate::file_ref::Input::ToggleShowHidden);
                return;
            }
            if key.modifiers == KeyModifiers::NONE {
                match key.code {
                    KeyCode::Up => {
                        crate::file_ref::navigate(menu, crate::file_ref::Input::Up);
                        return;
                    }
                    KeyCode::Down => {
                        crate::file_ref::navigate(menu, crate::file_ref::Input::Down);
                        return;
                    }
                    KeyCode::Home => {
                        crate::file_ref::navigate(menu, crate::file_ref::Input::Home);
                        return;
                    }
                    KeyCode::End => {
                        crate::file_ref::navigate(menu, crate::file_ref::Input::End);
                        return;
                    }
                    KeyCode::PageUp => {
                        crate::file_ref::navigate(menu, crate::file_ref::Input::PageUp);
                        return;
                    }
                    KeyCode::PageDown => {
                        crate::file_ref::navigate(menu, crate::file_ref::Input::PageDown);
                        return;
                    }
                    KeyCode::Left => {
                        crate::file_ref::navigate(menu, crate::file_ref::Input::Left);
                        return;
                    }
                    KeyCode::Right | KeyCode::Tab => {
                        self.file_menu_drill();
                        return;
                    }
                    KeyCode::Enter => {
                        self.file_menu_settle();
                        return;
                    }
                    KeyCode::Esc => {
                        self.dismiss_file_menu();
                        return;
                    }
                    _ => {}
                }
            }
        }

        if let Some(selected) = self.queue_selection {
            let n = self.prompt_queue.len();
            if n == 0 {
                self.queue_selection = None;
                return;
            }
            match (key.code, key.modifiers) {
                (KeyCode::Up, KeyModifiers::NONE) => {
                    self.queue_selection = Some(selected.checked_sub(1).unwrap_or(n - 1));
                }
                (KeyCode::Down, KeyModifiers::NONE) => {
                    self.queue_selection = Some((selected + 1) % n);
                }
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    self.begin_queue_edit_at(selected.min(n - 1));
                }
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.queue_selection = None;
                    self.show_tip(self.locale.tr("queue selection closed", "队列选择已关闭"));
                    if matches!(self.state, RunState::Idle) {
                        self.dispatch_next_queued(ctl);
                    }
                }
                _ => {}
            }
            return;
        }

        if key.code == KeyCode::Char('d')
            && key.modifiers == KeyModifiers::CONTROL
            && self.queue_edit.is_some()
        {
            if let Some(edit) = &mut self.queue_edit {
                edit.delete_confirm = true;
            }
            self.slash_completion_dismissed = true;
            self.show_tip(self.locale.tr(
                "delete queued prompt? · enter confirm · esc back",
                "删除这条排队消息？· enter 确认 · esc 返回",
            ));
            return;
        }

        if key.code == KeyCode::Down
            && key.modifiers == KeyModifiers::NONE
            && self.active_subagent.is_none()
            && self.input.is_empty()
            && self.input.hist_pos.is_none()
            && !self.subagents.is_empty()
        {
            self.begin_agent_navigation();
            return;
        }

        // The slash menu owns vertical arrows while it is visible. Ordinary
        // non-empty drafts use them for visual-line cursor motion below.
        if key.modifiers == KeyModifiers::NONE && self.slash_completion_open() {
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

        // Once Escape dismisses recommendations, the same single-line slash
        // draft participates in history navigation. The editor stash restores
        // it when Down returns past the newest history entry.
        if key.modifiers == KeyModifiers::NONE
            && self.slash_completion_dismissed
            && self.queue_edit.is_none()
        {
            match key.code {
                KeyCode::Up => {
                    self.history_prev_from_draft();
                    return;
                }
                KeyCode::Down => {
                    self.history_next();
                    return;
                }
                _ => {}
            }
        }

        let ctx = crate::input::KeyCtx {
            input_empty: self.input.is_empty(),
            history_active: self.input.hist_pos.is_some(),
        };
        if let Some(action) = crate::input::classify(&key, ctx) {
            self.dispatch(action, ctl);
        }
        // Any edit may have cut an [image n] token — the tray follows the
        // text (grok's lexicon-scan model).
        self.reconcile_attachments();
        // Caret/token changes drive the @file browser: open, navigate the
        // query's directory prefix, or close it.
        self.refresh_file_menu();
    }

    /// Re-scan the composer line at the caret and sync the `@file` browser:
    /// open it on an active token (unless the same token was Esc-dismissed
    /// or vim normal mode is active), re-navigate on query edits, close it
    /// when the token is gone.
    fn refresh_file_menu(&mut self) {
        // Vim normal mode is command editing — no mention browser.
        if self.vim.is_active() && self.vim.mode == VimMode::Normal {
            self.file_menu = None;
            return;
        }
        // The slash menu and the @ menu are mutually exclusive.
        if self.slash_completion_open() {
            self.file_menu = None;
            return;
        }
        let (row, col) = self.input.char_to_rowcol(self.input.cursor_char());
        let Some(line) = self.input.lines().get(row).cloned() else {
            self.file_menu = None;
            return;
        };
        let Some(token) = crate::file_ref::active_at_token(&line, col) else {
            // The token is gone (draft cleared, caret left it): a fresh
            // `@` must be able to reopen the browser, so drop the
            // dismissal tag along with the menu.
            self.file_menu = None;
            self.file_menu_dismissed = None;
            return;
        };
        let tag = crate::file_ref::token_tag(token.quoted, &token.query);
        if let Some(menu) = &mut self.file_menu {
            if menu.row() != row || menu.start() != token.start || menu.end() != token.end {
                menu.retoken(row, &token);
            }
            menu.apply_query(&token.query);
        } else {
            // Esc-dismissed tokens stay closed until their text changes.
            if self.file_menu_dismissed.as_deref() == Some(tag.as_str()) {
                return;
            }
            self.file_menu_dismissed = None;
            if let Some(mut menu) = crate::file_ref::FileMenu::open(
                std::path::Path::new(&self.cfg.workspace),
                row,
                &token,
            ) {
                // The token may already carry a query (dismissed-token
                // reopen): drive the browser to it before showing.
                menu.apply_query(&token.query);
                self.file_menu = Some(menu);
            }
        }
    }

    /// `Enter` on the selected entry: replace the token with `@path` (or
    /// `@dir/` for directories) and close the browser.
    fn file_menu_settle(&mut self) {
        let Some(mention) = self.file_menu.as_ref().and_then(|m| m.current_mention()) else {
            return;
        };
        let (row, start, end) = {
            let menu = self.file_menu.as_ref().expect("checked above");
            (menu.row(), menu.start(), menu.end())
        };
        crate::file_ref::replace_span(&mut self.input, row, start, end, &mention);
        self.file_menu = None;
        self.input_sel = None;
        self.reconcile_attachments();
    }

    /// `Tab` on a directory: rewrite the token to `@dir/` (quoted form
    /// keeps the quote open) and keep the browser inside the directory.
    /// `Tab` on a file settles like `Enter`.
    fn file_menu_drill(&mut self) {
        let Some(menu) = self.file_menu.as_ref() else {
            return;
        };
        if menu.explorer().files().is_empty() {
            return;
        }
        let file = menu.explorer().current().clone();
        if !file.is_dir {
            self.file_menu_settle();
            return;
        }
        let rel = crate::file_ref::relative_path(menu.base(), &file.path);
        let Some(mention) = crate::file_ref::format_file_mention(&rel, true, menu.quoted()) else {
            return;
        };
        let (row, start) = (menu.row(), menu.start());
        let end = start + mention.chars().count();
        crate::file_ref::replace_span(&mut self.input, row, menu.start(), menu.end(), &mention);
        if let Some(menu) = &mut self.file_menu {
            menu.retoken(row, &crate::file_ref::AtToken {
                start,
                end,
                query: format!("{rel}/"),
                quoted: mention.starts_with("@\""),
            });
            menu.apply_query(&format!("{rel}/"));
        }
    }

    /// Esc: close the browser and remember the token so it stays closed
    /// until its text changes.
    fn dismiss_file_menu(&mut self) {
        if let Some(menu) = &self.file_menu {
            self.file_menu_dismissed = Some(crate::file_ref::token_tag(
                menu.quoted(),
                &menu.token_query(),
            ));
        }
        self.file_menu = None;
    }

    /// Apply one classified [`Action`] — the only place key semantics touch
    /// app state, so `input::keymap` stays a pure table.
    fn dispatch(&mut self, action: Action, ctl: &Controller) {
        if matches!(
            action,
            Action::Insert(_)
                | Action::Newline
                | Action::Backspace
                | Action::DeleteForward
                | Action::DeleteWordBack
                | Action::KillToEnd
                | Action::KillToStart
                | Action::KillLine
                | Action::Undo
                | Action::Redo
                | Action::YankPaste
                | Action::SelectLeft
                | Action::SelectRight
                | Action::SelectUp
                | Action::SelectDown
                | Action::SelectWordLeft
                | Action::SelectWordRight
                | Action::SelectLineStart
                | Action::SelectLineEnd
        ) {
            self.slash_completion_dismissed = false;
            // Text edits invalidate the drag-selection highlight.
            self.input_sel = None;
        }
        match action {
            Action::Insert(ch) => {
                self.input.insert_char(ch);
                self.slash_sel = 0;
            }
            Action::Newline => {
                self.input.insert_newline();
            }
            Action::Enter => {
                self.input_sel = None;
                let menu = if self.slash_completion_open() {
                    self.slash_matches()
                } else {
                    Vec::new()
                };
                if !menu.is_empty() {
                    let entry = menu[self.slash_sel.min(menu.len() - 1)].clone();
                    self.accept_slash(&entry, ctl);
                } else if self.queue_edit.is_some() {
                    self.save_queue_edit(ctl);
                } else if self.input.is_empty()
                    && self.pending_images.is_empty()
                    && !self.prompt_queue.is_empty()
                {
                    self.send_queue_head_now(ctl);
                } else {
                    self.submit(ctl);
                }
            }
            Action::TabComplete => {
                self.slash_completion_dismissed = false;
                self.input_sel = None;
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
                    self.transcript.push_notice(
                        NoticeLevel::Info,
                        self.locale.tr("scrollback cleared", "滚动区已清空").into(),
                    );
            }
            Action::ToggleTheme => {
                self.toggle_theme_mode();
            }
            Action::ToggleExpandAll => {
                self.transcript.expand_all = !self.transcript.expand_all;
                self.show_tip(if self.transcript.expand_all {
                    self.locale
                        .tr("expanded all thoughts and tool results", "已展开全部思考与工具输出")
                } else {
                    self.locale
                        .tr("collapsed all thoughts and tool results", "已折叠全部思考与工具输出")
                });
            }
            Action::SendNow => self.send_now(ctl),
            Action::EditQueuedPrompt => self.open_queue_selector(),
            Action::AttachClipboard => self.clip_image("", ctl),
            Action::ModelPicker => self.open_model_picker(ctl),
            Action::CycleAgent => self.cycle_agent(ctl),
            Action::CyclePermission => self.cycle_permission(ctl),
            Action::HistoryPrev => self.history_prev(),
            Action::HistoryNext => self.history_next(),
            Action::ScrollHalfUp => self.scroll_by(10),
            Action::ScrollHalfDown => self.scroll_by(-10),
            Action::PageUp => self.scroll_by(20),
            Action::PageDown => self.scroll_by(-20),
            Action::JumpTop => {
                self.scroll_up = usize::MAX;
                self.chat_view.manual_top = None;
            }
            Action::JumpTail => {
                self.scroll_up = 0;
                self.chat_view.manual_top = None;
            }
            Action::CursorLeft => self.input.move_left(),
            Action::CursorRight => self.input.move_right(),
            Action::CursorUp => {
                let cursor = self.input.cursor_char();
                self.input.move_up();
                if self.queue_edit.is_none() && self.input.cursor_char() == cursor {
                    self.history_prev_from_draft();
                }
            }
            Action::CursorDown => self.input.move_down(),
            Action::WordLeft => self.input.word_left(),
            Action::WordRight => self.input.word_right(),
            Action::LineStart => self.input.line_start(self.composer_wrap_width),
            Action::LineEnd => self.input.line_end(self.composer_wrap_width),
            Action::Backspace => {
                // Deleting into an inline chip cuts the whole [image n]
                // token (and un-stages that image) instead of one bracket.
                if !self.delete_token_at(self.input.cursor_char(), true) {
                    self.input.backspace();
                }
            }
            Action::DeleteForward => {
                if !self.delete_token_at(self.input.cursor_char(), false) {
                    self.input.delete_forward();
                }
            }
            Action::DeleteWordBack => self.input.delete_word_back(),
            Action::KillToEnd => self.input.kill_to_end(self.composer_wrap_width),
            Action::KillToStart => self.input.kill_to_start(self.composer_wrap_width),
            Action::KillLine => self.input.kill_line(),
            Action::Undo => {
                self.input.undo();
            }
            Action::Redo => {
                self.input.redo();
            }
            Action::YankPaste => {
                self.input.paste_yank();
            }
            Action::SelectLeft => self.input.select_left(),
            Action::SelectRight => self.input.select_right(),
            Action::SelectUp => self.input.select_up(),
            Action::SelectDown => self.input.select_down(),
            Action::SelectWordLeft => self.input.select_word_left(),
            Action::SelectWordRight => self.input.select_word_right(),
            Action::SelectLineStart => self.input.select_line_start(),
            Action::SelectLineEnd => self.input.select_line_end(),
            Action::CopySelection => {
                // Keyboard selection first, then the mouse-drag selection.
                if let Some(text) = self.input.selection_text() {
                    if !text.trim().is_empty() {
                        self.input.copy_selection_to_yank();
                        self.copy_text(&text);
                    }
                } else if let Some((a, b)) = self.input_selection_range() {
                    let text = self.input.chars_between(a, b);
                    if !text.trim().is_empty() {
                        self.copy_text(&text);
                    }
                }
            }
            Action::CutSelection => {
                // Keyboard selection first, then the mouse-drag selection.
                if let Some(text) = self.input.selection_text() {
                    if !text.trim().is_empty() {
                        self.input.cut_selection_to_yank();
                        self.copy_text(&text);
                    } else {
                        self.show_tip(self.locale.tr(
                        "nothing to cut — select with shift+arrows",
                        "无可剪切 —— 用 shift+方向键先选中文本",
                    ));
                    }
                } else if let Some((a, b)) = self.input_selection_range() {
                    let text = self.input.chars_between(a, b);
                    if !text.trim().is_empty() {
                        self.input.delete_char_range(a, b);
                        self.input_sel = None;
                        self.copy_text(&text);
                    } else {
                        self.show_tip(self.locale.tr(
                        "nothing to cut — select with shift+arrows",
                        "无可剪切 —— 用 shift+方向键先选中文本",
                    ));
                    }
                } else {
                    self.show_tip(self.locale.tr(
                        "nothing to cut — select with shift+arrows",
                        "无可剪切 —— 用 shift+方向键先选中文本",
                    ));
                }
            }
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

    /// Route an incoming ACP permission ask to the session that owns it:
    /// the live view when the owning tab is on screen, otherwise the
    /// owning parked slot (the ask waits there and its tab is badged until
    /// the user returns). An ask whose session the tab model does not know
    /// falls back to the live view rather than silently cancelling the
    /// agent's tool call. Dropping an unanswered overlay cancels it.
    fn open_permission_ask(
        &mut self,
        session_id: &str,
        title: String,
        options: Vec<PermissionAskOption>,
        reply: tokio::sync::oneshot::Sender<PermissionAskReply>,
    ) {
        // acp_fs's builtin write-outside ask is authored in English; its
        // known strings localize here at render time. Anything else (plugin
        // and host asks) passes through untouched.
        let title = match title
            .strip_prefix("write ")
            .and_then(|rest| rest.strip_suffix(" · outside workspace"))
        {
            Some(path) => self.locale.trf(
                "write {} · outside workspace",
                "写入 {} · 工作区外",
                &[path.into()],
            ),
            None => title,
        };
        let mut options = options;
        for option in &mut options {
            match (option.option_id.as_str(), option.name.as_str()) {
                ("deny", "Deny") => option.name = self.locale.tr("Deny", "拒绝").into(),
                ("allow", "Allow write") => {
                    option.name = self.locale.tr("Allow write", "允许写入").into();
                }
                _ => {}
            }
        }
        let sel = permission_ask_default_sel(&options);
        let overlay = PermissionAskOverlay {
            title,
            sel,
            options,
            reply: Some(reply),
        };
        let belongs_to_live = session_id == self.session_id
            || self.subagents.iter().any(|v| v.id == session_id);
        if belongs_to_live {
            self.permission_ask = Some(overlay);
        } else if let Some(slot) = self
            .parked
            .iter_mut()
            .find(|slot| slot.id == session_id || slot.subagents.iter().any(|v| v.id == session_id))
        {
            // A parked session asked while out of view: keep the ask on its
            // tab — it must not float over the tab on screen.
            slot.permission_ask = Some(overlay);
        } else {
            self.show_tip(self.locale.tr(
                "permission ask from an unknown session — shown here",
                "未知会话的权限请求 —— 在此显示",
            ));
            self.permission_ask = Some(overlay);
        }
        self.needs_redraw = true;
    }

    /// Route an ACP elicitation form like [`Self::open_permission_ask`].
    /// `None` sessions are request-scoped elicitations (auth/config phase)
    /// with no tab to belong to — they surface on the live view.
    fn open_elicitation_ask(
        &mut self,
        session_id: Option<&str>,
        form: crate::elicitation::ElicitationForm,
        reply: tokio::sync::oneshot::Sender<crate::elicitation::ElicitationReply>,
    ) {
        let mut form_state = crate::elicitation::ElicitationFormState::new(form);
        form_state.locale = self.locale;
        let overlay = ElicitationAskOverlay {
            form: form_state,
            scroll: 0,
            reply: Some(reply),
        };
        let for_live = match session_id {
            None => true,
            Some(sid) => sid == self.session_id || self.subagents.iter().any(|v| v.id == sid),
        };
        if for_live {
            self.elicitation_ask = Some(overlay);
        } else if let Some(slot) = self
            .parked
            .iter_mut()
            .find(|slot| {
                session_id.is_some_and(|sid| slot.id == sid || slot.subagents.iter().any(|v| v.id == sid))
            })
        {
            slot.elicitation_ask = Some(overlay);
        } else {
            self.show_tip(self.locale.tr(
                "elicitation from an unknown session — shown here",
                "未知会话的表单请求 —— 在此显示",
            ));
            self.elicitation_ask = Some(overlay);
        }
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
                        input_empty: self.input.is_empty(),
                        history_active: self.input.hist_pos.is_some(),
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
                            self.resume_acp_session(&item.id, ctl);
                        } else {
                            self.resume_session(&item.id, ctl);
                        }
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
                        self.transcript.push_notice(
                            NoticeLevel::Info,
                            self.locale.trf(
                                "reasoning effort → {}",
                                "推理强度 → {}",
                                &[effort.clone()],
                            ),
                        );
                    }
                    PickerKind::Auth => self.start_auth(&item.id, ctl),
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
                                self.show_tip(self.locale.trf(
                                    "plugin {} is already starting",
                                    "插件 {} 已在启动中",
                                    &[plugin_id],
                                ));
                                return;
                            }
                            let enabled = !matches!(status.as_str(), "running" | "waiting");
                            ctl.send(Cmd::SetCordisPluginEnabled {
                                agent_id: self.session_id.clone(),
                                plugin_id: plugin_id.clone(),
                                enabled,
                            });
                            self.show_tip(if enabled {
                                self.locale.trf(
                                    "restoring plugin {}…",
                                    "正在恢复插件 {}…",
                                    &[plugin_id.clone()],
                                )
                            } else {
                                self.locale.trf(
                                    "stopping plugin {}…",
                                    "正在停止插件 {}…",
                                    &[plugin_id.clone()],
                                )
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
                    PickerKind::AgentHistory => self.select_agent_transcript(&item.id),
                }
            }
            _ => {}
        }
    }

    /// Keyboard driving of the `/plugins` tree popup. The tree widget owns
    /// selection and viewport; ←/→ and enter fold/unfold a provider branch.
    fn handle_plugin_tree_key(&mut self, key: KeyEvent) {
        if key.modifiers != KeyModifiers::NONE {
            return;
        }
        if key.code == KeyCode::Esc {
            self.plugin_tree = None;
            return;
        }
        let Some(tree) = &mut self.plugin_tree else {
            return;
        };
        let page = self.plugin_tree_page_rows.max(1);
        match key.code {
            KeyCode::Up => {
                tree.state.key_up();
            }
            KeyCode::Down => {
                tree.state.key_down();
            }
            KeyCode::Left => {
                tree.state.key_left();
            }
            KeyCode::Right | KeyCode::Enter => {
                tree.state.toggle_selected();
            }
            KeyCode::PageUp => {
                tree.state
                    .select_relative(|i| i.map_or(0, |i| i.saturating_sub(page)));
            }
            KeyCode::PageDown => {
                tree.state
                    .select_relative(|i| i.map_or(0, |i| i.saturating_add(page)));
            }
            KeyCode::Home => {
                tree.state.select_first();
            }
            KeyCode::End => {
                tree.state.select_last();
            }
            _ => {}
        }
    }

    fn agent_navigation_ids(&self) -> Vec<String> {
        let mut ids = vec![self.session_id.clone()];
        ids.extend(
            self.subagents
                .iter()
                .filter(|view| self.subagent_in_current_batch(&view.id))
                .map(|view| view.id.clone()),
        );
        if self
            .subagents
            .iter()
            .any(|view| !self.subagent_in_current_batch(&view.id))
        {
            ids.push(AGENT_HISTORY_ID.into());
        }
        ids
    }

    fn begin_agent_navigation(&mut self) {
        if self.subagents.is_empty() {
            return;
        }
        self.agent_selection = Some(match self.active_subagent.as_deref() {
            Some(id) if !self.subagent_in_current_batch(id) => AGENT_HISTORY_ID.into(),
            Some(id) => id.into(),
            None => self.session_id.clone(),
        });
        self.needs_redraw = true;
    }

    fn move_agent_selection(&mut self, delta: isize) {
        let ids = self.agent_navigation_ids();
        if ids.len() < 2 {
            self.agent_selection = None;
            return;
        }
        let current = self
            .agent_selection
            .as_deref()
            .unwrap_or_else(|| self.active_subagent.as_deref().unwrap_or(&self.session_id));
        let index = ids.iter().position(|id| id == current).unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(ids.len() as isize) as usize;
        self.agent_selection = Some(ids[next].clone());
        self.needs_redraw = true;
    }

    fn confirm_agent_selection(&mut self) {
        let Some(id) = self.agent_selection.take() else {
            return;
        };
        self.select_agent_transcript(&id);
    }

    fn cancel_agent_selection(&mut self) {
        self.agent_selection = None;
        self.needs_redraw = true;
    }

    fn select_agent_transcript(&mut self, id: &str) {
        self.agent_selection = None;
        if id == AGENT_HISTORY_ID {
            self.open_agent_history_picker();
            return;
        } else if id == self.session_id {
            self.active_subagent = None;
        } else if self.subagents.iter().any(|view| view.id == id) {
            self.active_subagent = Some(id.to_string());
        } else {
            return;
        }
        self.scroll_up = 0;
        self.sel = None;
    }

    fn open_agent_history_picker(&mut self) {
        let items = self
            .subagents
            .iter()
            .rev()
            .filter(|view| !self.subagent_in_current_batch(&view.id))
            .map(|view| PickerItem {
                id: view.id.clone(),
                label: view.label.clone(),
                meta: if view.running {
                    "running"
                } else if view.failed {
                    "failed"
                } else {
                    "completed"
                }
                .into(),
                provider: None,
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            return;
        }
        self.picker = Some(Picker {
            offset: 0,
            kind: PickerKind::AgentHistory,
            title: "Agent history".into(),
            sel: 0,
            items,
        });
        self.needs_redraw = true;
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
            offset: 0,
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
            offset: 0,
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
    /// (grok-build's session picker). Live ACP prefers `session/list`;
    /// `limit` is the `/resume n` count (most recent first).
    fn open_resume_picker(&mut self, limit: usize, ctl: &Controller) {
        if !self.demo && self.list_session {
            ctl.send(Cmd::ListSessions {
                prefix: None,
                limit,
            });
            self.show_tip(self.locale.tr("listing ACP sessions…", "正在列出 ACP 会话…"));
            return;
        }
        if !self.demo {
            self.show_tip(self.locale.tr(
                "agent did not advertise session/list — listing local JSONL",
                "Agent 未声明 session/list —— 改为列出本地 JSONL",
            ));
        }
        self.open_local_resume_picker(limit);
    }

    fn open_local_resume_picker(&mut self, limit: usize) {
        self.resume_via_acp = false;
        let sessions = crate::sessions::list_sessions(
            &self.cfg.session_root,
            &self.cfg.workspace,
            &self.session_id,
            limit,
        );
        if sessions.is_empty() {
            self.transcript.push_notice(
                NoticeLevel::Info,
                self.locale.tr(
                    "no durable sessions for this workspace yet — finish a turn and /resume finds it",
                    "此工作区还没有持久会话 —— 完成一轮对话后 /resume 即可找回",
                )
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
            offset: 0,
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
                usize::MAX,
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
                    self.locale.trf(
                        "no session matches “{}” — /resume lists them",
                        "没有会话匹配「{}」—— /resume 可列出全部",
                        &[id_or_prefix.into()],
                    ),
                );
                return;
            }
            many => match many.iter().find(|s| s.id == id_or_prefix) {
                Some(one) => one.clone(),
                None => {
                    self.transcript.push_notice(
                        NoticeLevel::Warn,
                        self.locale.trf(
                            "“{}” is ambiguous ({} matches) — /resume lists them",
                            "「{}」有 {} 个匹配，存在歧义 —— /resume 可列出全部",
                            &[id_or_prefix.into(), many.len().to_string()],
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
                    self.locale.trf(
                        "cannot read {}: {}",
                        "无法读取 {}：{}",
                        &[session.file.display().to_string(), format!("{err:#}")],
                    ),
                );
                return;
            }
        };

        if self.session_id != session.id {
            if let Some(tab) = self.tab_index_of(&session.id) {
                // Already open in a tab — switching to it is the resume.
                self.switch_to_session(tab);
                self.resume_candidates = Vec::new();
                self.transcript.push_notice(
                    NoticeLevel::Info,
                    self.locale.trf(
                        "⟲ {} is already open — switched to its tab",
                        "⟲ {} 已在打开的标签页中 —— 已切换过去",
                        &[session.id.clone()],
                    ),
                );
                return;
            }
            // Park the live session and replay into a fresh tab (issue #94).
            self.open_new_session(session.id.clone(), true);
        } else {
            self.reset_subagent_views();
            self.transcript.clear();
            self.transcript.set_root_session(session.id.clone());
            // Replay folds the session's own authoritative mode facts from empty.
            self.modes = Modes::default();
            self.queued = 0;
            self.prompt_queue.clear();
            self.queue_selection = None;
            self.queue_edit = None;
            self.pending_steer_cells.clear();
            self.prompt_pending = false;
            self.sel = None;
        }
        // The resumed stream's own model is the truth for the chip.
        self.selected_model = None;
        self.show_banner = false;
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
            self.locale.trf(
                "⟲ resumed {} · {} turn{} · {} events replayed — the next prompt continues it",
                "⟲ 已恢复 {} · {} 轮对话 · 回放 {} 个事件 —— 下一条消息继续它",
                &[
                    session.id.clone(),
                    session.turns.to_string(),
                    if session.turns == 1 { "" } else { "s" }.to_string(),
                    replayed.to_string(),
                ],
            ),
        );
        self.needs_redraw = true;
    }

    /// The `/new` flow, shared by the slash command and the tab strip `+`
    /// cell: park the live session and open a fresh tab (issue #94).
    fn new_session_flow(&mut self, arg: &str, ctl: &Controller) {
        if self.demo {
            let id = if arg.is_empty() {
                format!("dsh-{}", timestamp())
            } else {
                arg.to_string()
            };
            self.open_new_session(id.clone(), true);
            ctl.send(Cmd::FetchSkills);
            self.transcript.push_notice(
                NoticeLevel::Info,
                self.locale.trf(
                    "new session · {} — /agent picks its agent preset",
                    "新会话 · {} —— /agent 选择它的 Agent 预设",
                    &[id],
                ),
            );
        } else {
            // A local placeholder ids the tab until session/new resolves —
            // the same shape main.rs seeds the startup session with. The
            // real id lands on this tab via `awaiting_binds` at SessionBound.
            let placeholder = format!("dsh-{}", timestamp());
            self.open_new_session(placeholder.clone(), false);
            self.awaiting_binds.push_back(AwaitingBind {
                id: placeholder,
                open: true,
            });
            ctl.send(Cmd::NewSession);
            self.show_tip(self.locale.tr("session/new …", "正在创建会话（session/new）…"));
        }
    }

    /// The `/close` flow (issue #94): stop viewing the current session tab
    /// and drop everything bound to it (transcript, composer draft, queue,
    /// asks, subagents). Local only — ACP has no session/close, so the
    /// server-side session keeps existing; acp.rs forgets its turn state
    /// and queued prompts so nothing more is sent into the void, and the
    /// session's later updates drop at the router. An in-flight turn keeps
    /// running and is dropped when it settles. Never closes the last tab.
    fn close_session_flow(&mut self, ctl: &Controller) {
        let total = self.session_tab_count();
        if total < 2 {
            self.show_tip(self.locale.tr(
                "cannot close the last session — /new opens another first",
                "不能关闭最后一个会话 —— 先用 /new 开一个新的",
            ));
            return;
        }
        let doomed = self.session_id.clone();
        let running = self.state != RunState::Idle || self.prompt_pending;
        // Compositor-owned overlays (plugin view/select/slider) must be
        // released with a cancel event before the tab dies — the same path
        // a tab click takes. Painter popups, asks and the composer draft
        // die with the slot (ask overlays auto-cancel through Drop).
        self.cancel_plugin_overlays(ctl);
        let doomed_slot = self.take_live_slot();
        let discarded = doomed_slot.prompt_queue.len();
        let had_draft = !doomed_slot.input.is_empty()
            || !doomed_slot.pending_images.is_empty();
        let had_ask =
            doomed_slot.permission_ask.is_some() || doomed_slot.elicitation_ask.is_some();
        drop(doomed_slot);

        // The bind this tab may still be awaiting keeps its FIFO position
        // (the in-flight session/new·resume owns it) but is now dead: when
        // it resolves the session is forgotten, never bound onto a
        // neighboring tab.
        if let Some(entry) = self.awaiting_binds.iter_mut().find(|entry| entry.id == doomed) {
            entry.open = false;
        }
        // Forget the session on the ACP side too: drop its turn state and
        // queued prompts (no-op for an unbound placeholder id).
        ctl.send(Cmd::ForgetSession {
            session_id: doomed.clone(),
        });

        // View a neighbor first, keeping the closed tab's conceptual slot:
        // the right neighbor when one exists, else the tab on the left.
        let right = self.current + 1 < total;
        let pidx = if right { self.current } else { self.current - 1 };
        let mut target = self.parked.remove(pidx);
        target.completed_unseen = false;
        self.current = if right {
            self.current
        } else {
            self.current.saturating_sub(1)
        };
        self.put_live_slot(target);
        self.after_switch();
        self.needs_redraw = true;

        let label = short_id(&doomed);
        let mut bits: Vec<String> = Vec::new();
        if running {
            bits.push("its running turn keeps settling".into());
        }
        if discarded > 0 {
            bits.push(format!("{discarded} queued dropped"));
        }
        if had_draft {
            bits.push("draft dropped".into());
        }
        if had_ask {
            bits.push("pending ask cancelled".into());
        }
        if bits.is_empty() {
            self.show_tip(self.locale.trf("closed {}", "已关闭 {}", &[label.clone()]));
        } else {
            self.show_tip(self.locale.trf(
                "closed {} · {}",
                "已关闭 {} · {}",
                &[label.clone(), bits.join(", ")],
            ));
        }
    }

    fn reset_session_ui(&mut self) {
        self.reset_subagent_views();
        self.transcript.clear();
        self.modes = Modes::default();
        self.selected_model = None;
        self.session_title = None;
        self.show_banner = false;
        self.queued = 0;
        self.prompt_queue.clear();
        self.queue_selection = None;
        self.queue_edit = None;
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
        self.current_subagents.clear();
        self.next_subagent_starts_batch = true;
        self.active_subagent = None;
        self.agent_selection = None;
    }

    fn resume_acp_session(&mut self, id: &str, ctl: &Controller) {
        // The welcome banner only ever paints instead of the transcript, so
        // any path that (re)loads real history must dismiss it — the local
        // `resume_session` does the same. Without this, a /resume before the
        // first prompt left the banner covering the whole replayed chat.
        // Dismissed *after* the branch: a fresh tab starts with the banner
        // (composer chrome is tab-bound), and the same-session branch resets
        // its own fields.
        if self.session_id == id {
            // Resuming the viewed session: reset its UI and re-stream.
            self.reset_session_ui();
            self.session_id = id.to_string();
            self.transcript.set_root_session(id.to_string());
        } else if let Some(tab) = self.tab_index_of(id) {
            // Already open in a tab — switching to it is the resume.
            self.switch_to_session(tab);
            return;
        } else {
            // Park the live session; the resumed session binds this fresh
            // tab. With ACP `session/resume` the agent does not replay the
            // transcript; the legacy `session/load` fallback streams it via
            // session/update tagged with this id.
            self.open_new_session(id.to_string(), false);
            self.awaiting_binds.push_back(AwaitingBind {
                id: id.to_string(),
                open: true,
            });
        }
        self.show_banner = false;
        ctl.send(Cmd::ResumeSession {
            session_id: id.to_string(),
        });
        self.show_tip(self.locale.trf("resuming {} …", "正在恢复会话 {} …", &[id.into()]));
        self.needs_redraw = true;
    }

    fn on_acp_session_list(
        &mut self,
        sessions: Vec<SessionListItem>,
        prefix: Option<String>,
        limit: usize,
        ctl: &Controller,
    ) {
        let skip = self.session_id.clone();
        // The `/resume n` cap counts resumable sessions only: the current
        // session is dropped first, then the list truncates to the limit.
        let mut sessions: Vec<SessionListItem> = sessions
            .into_iter()
            .filter(|s| s.id != skip)
            .collect();
        sessions.truncate(limit);
        if let Some(prefix) = prefix.as_deref().filter(|p| !p.is_empty()) {
            match unique_session_list_match(&sessions, prefix) {
                Ok(id) => {
                    self.resume_acp_session(&id, ctl);
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
                self.locale.tr(
                    "no ACP sessions from session/list — finish a turn and /resume finds it",
                    "session/list 没有返回 ACP 会话 —— 完成一轮对话后 /resume 即可找回",
                )
                .into(),
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
                usize::MAX,
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
            offset: 0,
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
        limit: usize,
        error: String,
        ctl: &Controller,
    ) {
        self.show_tip(self.locale.trf(
            "session/list unavailable ({}) — listing local JSONL",
            "session/list 不可用（{}）—— 改为列出本地 JSONL",
            &[error.clone()],
        ));
        if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
            if self.resume_session_cap || self.load_session {
                self.resume_acp_session(&prefix, ctl);
                return;
            }
            self.resume_session(&prefix, ctl);
            return;
        }
        self.open_local_resume_picker(limit);
        if self.resume_session_cap || self.load_session {
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
            offset: 0,
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
            self.show_tip(self.locale.trf("agent already {}", "Agent 已是 {}", &[label.into()]));
            return;
        }
        ctl.send(Cmd::SetPreset {
            session_id: self.session_id.clone(),
            preset: preset.clone(),
        });
        // Preset scopes can mount their own skill registries.
        ctl.send(Cmd::FetchSkills);
        self.show_tip(self.locale.trf("agent → {} …", "Agent → {} …", &[label.into()]));
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
            self.show_tip(self.locale.tr("agent presets unavailable", "Agent 预设不可用"));
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
            self.show_tip(self.locale.trf(
                "permission already {}",
                "权限预设已是 {}",
                &[preset.into()],
            ));
            return;
        }
        ctl.send(Cmd::SetPermission {
            session_id: self.session_id.clone(),
            preset: preset.clone(),
        });
        self.show_tip(self.locale.trf(
            "permission → {} …",
            "权限预设 → {} …",
            &[preset.into()],
        ));
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
            offset: 0,
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
        let had_chat_sel = self.sel.take().is_some();
        let had_input_sel = self.input_sel.take().is_some();
        if (had_chat_sel || had_input_sel) && matches!(self.state, RunState::Idle) {
            self.needs_redraw = true;
            return;
        }
        if self.slash_completion_open() {
            self.slash_completion_dismissed = true;
            return;
        }
        if let Some(edit) = &mut self.queue_edit {
            if edit.delete_confirm {
                edit.delete_confirm = false;
                self.show_tip(self.locale.tr(
                    "delete cancelled · still editing queued prompt",
                    "已取消删除 · 仍在编辑这条排队消息",
                ));
            } else {
                self.queue_edit = None;
                self.input.clear();
                self.input_sel = None;
                self.slash_completion_dismissed = false;
                self.reconcile_attachments();
                self.show_tip(self.locale.tr("queued prompt edit cancelled", "已取消编辑排队消息"));
                if matches!(self.state, RunState::Idle) {
                    self.dispatch_next_queued(ctl);
                }
            }
            return;
        }
        match self.state {
            RunState::Running | RunState::Starting => {
                // grok: Esc cancels immediately; the draft survives.
                if ctl.interrupt_now() {
                    ctl.send(Cmd::Interrupt {
                        session_id: self.session_id.clone(),
                    });
                    self.state_note = self.locale.tr("cancelling", "正在取消").into();
                } else {
                    self.show_tip(self.locale.tr("demo turn — it finishes on its own", "演示轮次 —— 会自动结束"));
                }
            }
            RunState::Idle => {
                // Esc clears the draft — inline [image n] chips live in it,
                // so staged images go with it (reconcile below).
                if !self.input.is_empty() {
                    self.input.history.push(self.input.buf());
                    self.input.clear();
                    self.reconcile_attachments();
                    self.show_tip(self.locale.tr("draft cleared — ↑ recalls it", "草稿已清空 —— ↑ 可找回"));
                    return;
                }
                self.show_tip(self.locale.tr(
                    "esc — idle · a running turn is interrupted with esc",
                    "esc — 空闲 · 运行中的轮次用 esc 中断",
                ));
            }
        }
    }

    fn handle_ctrl_c(&mut self, _ctl: &Controller) {
        if !self.input.is_empty() {
            // Clearing the draft never counts as the first press of the
            // double-Ctrl+C quit chord.
            self.ctrl_c_armed = None;
            self.input.history.push(self.input.buf());
            self.input.clear();
            self.input_sel = None;
            self.reconcile_attachments();
            self.show_tip(self.locale.tr("draft cleared — ↑ recalls it", "草稿已清空 —— ↑ 可找回"));
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
            self.locale
                .tr("press ctrl+c again to exit", "再按一次 ctrl+c 退出")
                .into()
        } else {
            self.locale.trf(
                "press ctrl+c {} more times to exit while the agent is running",
                "Agent 运行中，再按 {} 次 ctrl+c 退出",
                &[remaining.to_string()],
            )
        });
    }

    fn history_prev(&mut self) {
        self.input_sel = None;
        if !self.input.is_empty() && self.input.hist_pos.is_none() {
            return; // grok: history opens from an empty prompt
        }
        if self.input.history.is_empty() {
            return;
        }
        let pos = match self.input.hist_pos {
            None => {
                self.input.stash = self.input.buf();
                self.input.history.len() - 1
            }
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.input.hist_pos = Some(pos);
        self.input.set(self.input.history[pos].clone());
    }

    fn history_prev_from_draft(&mut self) {
        self.input_sel = None;
        if self.input.history.is_empty() {
            return;
        }
        if self.input.hist_pos.is_none() {
            self.input.stash = self.input.buf();
        }
        let pos = match self.input.hist_pos {
            None => self.input.history.len() - 1,
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.input.hist_pos = Some(pos);
        self.input.set(self.input.history[pos].clone());
    }

    fn history_next(&mut self) {
        self.input_sel = None;
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
            let line = self.input.buf();
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
            let line = self.input.buf().trim().to_string();
            if line == full || line.starts_with(&format!("{full} ")) {
                self.submit(ctl);
            } else {
                self.input.set(format!("{full} "));
                self.slash_sel = 0;
            }
            return;
        }
        let line = self.input.buf();
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
            "vim" => {
                let on = match arg {
                    "on" | "1" => true,
                    "off" | "0" => false,
                    _ => !self.vim.is_active(),
                };
                self.vim.set(on);
                self.show_tip(self.locale.tr(
                    "vim mode — i insert · esc normal · /vim off",
                    "vim 模式 — i 插入 · esc 返回 normal · /vim off 关闭",
                ));
            }
            "ui" => {
                if arg.is_empty() {
                    self.open_ui_plugin_picker();
                } else if self.ui_plugins.iter().any(|plugin| plugin.id == arg) {
                    ctl.send(Cmd::PluginUiSelected {
                        agent_id: self.session_id.clone(),
                        id: arg.to_string(),
                    });
                } else {
                    self.show_tip(self.locale.trf(
                    "unknown UI Plugin: {}",
                    "未知 UI 插件：{}",
                    &[arg.into()],
                ));
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
            "new" => self.new_session_flow(arg, ctl),
            "close" => self.close_session_flow(ctl),
            "session" => match arg {
                "view" | "" => self.push_session_info(),
                "prev" => {
                    let count = self.session_tab_count();
                    if count > 1 {
                        let prev =
                            if self.current == 0 { count - 1 } else { self.current - 1 };
                        self.switch_view_to_tab(prev, ctl);
                    }
                }
                "next" => {
                    let count = self.session_tab_count();
                    if count > 1 {
                        let next = (self.current + 1) % count;
                        self.switch_view_to_tab(next, ctl);
                    }
                }
                _ => self.show_tip(self.locale.tr(
                    "usage: /session [view|prev|next]",
                    "用法：/session [view|prev|next]",
                )),
            },
            "status" => self.push_status_info(),
            "auth" => self.start_auth(arg, ctl),
            "resume" => {
                // `/resume [n|id]` — a bare number is how many of the most
                // recent durable sessions to list (default 50); anything
                // else is an id prefix to resume.
                if arg.is_empty() {
                    self.open_resume_picker(crate::sessions::DEFAULT_SESSION_LIST_LIMIT, ctl);
                } else if let Ok(n) = arg.parse::<usize>() {
                    self.open_resume_picker(n.max(1), ctl);
                } else if !self.demo && self.list_session {
                    ctl.send(Cmd::ListSessions {
                        prefix: Some(arg.to_string()),
                        limit: usize::MAX,
                    });
                    self.show_tip(self.locale.tr("listing ACP sessions…", "正在列出 ACP 会话…"));
                } else if !self.demo && (self.resume_session_cap || self.load_session) {
                    self.resume_acp_session(arg, ctl);
                } else {
                    if !self.demo {
                        self.show_tip(self.locale.tr(
                            "agent did not advertise session/resume or loadSession — replaying local JSONL",
                            "Agent 未声明 session/resume 或 loadSession —— 改为回放本地 JSONL",
                        ));
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
                            session_id: self.session_id.clone(),
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
        self.show_tip(self.locale.tr("sign-in needed — /auth to retry", "需要登录 —— /auth 重试"));
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
            offset: 0,
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
                .push_notice(
                    NoticeLevel::Info,
                    self.locale
                        .tr("demo has no ACP authenticate", "演示模式没有 ACP authenticate")
                        .into(),
                );
            return;
        }
        if self.auth.methods.is_empty() {
            self.transcript.push_notice(
                NoticeLevel::Warn,
                self.auth.message.clone().unwrap_or_else(|| {
                    self.locale
                        .tr(
                            "this agent did not advertise auth methods",
                            "此 Agent 未声明认证方式",
                        )
                        .into()
                }),
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
                self.locale.trf(
                    "ACP auth method is unavailable or not supported: {}",
                    "ACP 认证方式不可用或不支持：{}",
                    &[method_id.clone()],
                ),
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
                    self.locale.trf(
                        "ACP auth method {} requires credential variables and cannot be started as a sign-in flow.",
                        "ACP 认证方式 {} 需要凭据变量，无法以登录流程启动。",
                        &[method.id.clone()],
                    )
                } else {
                    self.locale.trf(
                        "ACP auth method {} requires credential variables ({}) and cannot be started as a sign-in flow.",
                        "ACP 认证方式 {} 需要凭据变量（{}），无法以登录流程启动。",
                        &[method.id.clone(), vars],
                    )
                },
            );
            return;
        }
        let values = values_from_auth_arg(&method, &rest);
        if method.form && authenticate_meta_from_method(&method, &values).is_none() {
            self.show_tip(self.locale.tr(
                "usage: /auth <api-key> · gateway: /auth <base-url> <api-key>",
                "用法：/auth <api-key> · 网关：/auth <base-url> <api-key>",
            ));
            return;
        }
        if values.is_empty() {
            if let Some(launch) = method.terminal_launch.clone() {
                self.pending_terminal_auth = Some(launch);
                self.transcript.push_notice(
                    NoticeLevel::Info,
                    self.locale.trf(
                        "leaving the TUI for {} — return here when it finishes",
                        "离开 TUI 前往 {} —— 完成后回到这里",
                        &[method
                            .name
                            .as_deref()
                            .unwrap_or(&method.id)
                            .to_string()],
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
- enter · 发送；空 composer 且 Queue 非空时立即发送队首
- alt+↑ · 选择任意排队消息；↑/↓ 选择，enter 编辑/保存，ctrl+d 删除，esc 退出
- ctrl+enter · 立即 steer 当前轮次
- esc · 优先退出 / 推荐；队列编辑时取消；否则中断（保留草稿），空闲时清草稿
- ctrl+c · 有草稿先清除；无草稿时连按 2 次退出（不中断）
- shift+tab · 轮换权限预设 · /permission 打开选择器
- ctrl+p · 打开模型选择器，然后选择推理强度
- ctrl+shift+a · 直接轮换 Agent 预设
- ↓ · 空输入时展开 Agent 会话导航；←/→ 选择，enter 打开，esc 折叠
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
- 输入框编辑：readline 组合键 + ⌘/⌥ 方向键 + 键盘选区 · 完整映射见 /keys
- 点击工具 · 展开/折叠 · 滚轮滚动对话
- pgup/pgdn · 翻页 · end 回到最新消息
- 鼠标拖动 · 选择并复制文本 · 双击复制单词

每轮会显示：流式思考与回答、工具调用与结果、注入上下文、Subagent 生命周期、
token 用量（含缓存命中）以及轮次结束原因。";
            self.open_text_overlay("builtin.help", self.locale.tr("help", "帮助").to_string(), text.to_string());
            return;
        }
        let text = "\
- enter · send · with an empty composer, sends the Queue head now
- alt+↑ · choose any queued prompt · ↑/↓ select, enter edits/saves, ctrl+d deletes, esc closes
- ctrl+enter · steer the active turn immediately
- esc · first closes / suggestions; cancels queue edits; otherwise interrupts (draft survives) or clears an idle draft
- ctrl+c · clear a draft; 2× quits with no draft (never interrupts)
- shift+tab · cycle permission (workspace-write ⇄ full access) · /permission opens the preset picker
- ctrl+p · model picker (host catalog) → effort picker
- ctrl+shift+a · cycle agent preset directly
- ↓ · expand Agent transcript navigation from an empty prompt · ←/→ choose, enter opens, esc collapses
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
        self.open_text_overlay("builtin.help", self.locale.tr("help", "帮助").to_string(), text.to_string());
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
            "- session · {}{}\n\
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
        self.open_text_overlay(
            "builtin.session",
            self.locale.tr("session", "会话").to_string(),
            text,
        );
    }

    /// Open one builtin info dialog (popup) from a markdown body. `/keys`,
    /// `/help`, `/session` and the painter-side `/status` fallback share this
    /// surface, so the facts never land in the scrollback as transcript cells.
    fn open_text_overlay(&mut self, id: &str, title: String, text: String) {
        self.view_overlay = Some(ViewOverlay {
            id: id.into(),
            title,
            nodes: vec![crate::slots::TuiNode::Markdown {
                id: id.into(),
                text,
                streaming: false,
            }],
            scroll: 0,
            notify_plugin: false,
        });
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
        let mut text = format!("- state · {state}\n- acp · {acp}\n");
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
        // Same popup surface as `/keys`, `/help` and `/session` (issue #53):
        // a status pushed into the transcript would be invisible behind the
        // welcome banner on a fresh start — the chat pane only draws the
        // banner while `show_banner` is set — and would pollute the
        // scrollback with facts that belong to the overlay.
        self.open_text_overlay(
            "builtin.status",
            self.locale.tr("Status", "状态").to_string(),
            text,
        );
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
        // A pending @file browser never survives a send.
        self.file_menu = None;
        let text = self.input.buf().trim().to_string();
        // Client namespaces don't take images — keep the chips editable
        // instead of silently dropping them.
        if !self.pending_images.is_empty() && (text.starts_with('/') || text.starts_with('!')) {
            self.show_tip(
                self.locale.tr(
                    "send or delete the [image] chips first — /commands and !shell don't take images",
                    "先发送或删除 [image] 标记 —— /命令 和 !shell 不接受图片",
                ),
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
            self.input.history.push(self.input.buf());
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
        // An unbound tab (placeholder id awaiting session/new·load) must not
        // send: acp.rs rejects ids it never bound. Hold the prompt in the
        // queue; the SessionBound handler dispatches it with the real id.
        let running = self.state == RunState::Running
            || self.prompt_pending
            || self.queued > 0
            || !self.session_bound;
        if running {
            let id = self.next_prompt_id();
            self.prompt_queue.push_back(ClientQueuedPrompt {
                id,
                blocks: vec![StagedBlock::Text(text.clone())],
            });
            self.queued += 1;
            self.show_tip(self.locale.tr(
                "queued · empty enter sends first · alt+↑ edit",
                "已排队 · 空输入按 enter 发送队首 · alt+↑ 编辑",
            ));
        } else {
            self.transcript.push_user(text.clone(), false);
            self.prompt_pending = true;
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
            self.state_note = if self.demo {
                String::new()
            } else {
                self.locale.tr("contacting runtime", "正在连接运行时").into()
            };
        }
        self.scroll_up = 0;
        if !running {
            ctl.send(Cmd::Prompt {
                session_id: self.session_id.clone(),
                text,
            });
        }
    }

    fn dispatch_next_queued(&mut self, ctl: &Controller) {
        if self.queue_selection.is_some() || self.queue_edit.is_some() {
            self.state_note = self
                .locale
                .tr("queue paused for selection/edit", "队列已暂停 · 请选择或编辑")
                .into();
            self.show_tip(self.locale.tr(
                "queue paused · finish or close the queue editor",
                "队列暂停 · 完成或关闭队列编辑器",
            ));
            return;
        }
        if !self.session_bound {
            // The tab still awaits (or lost) its session bind: an unbound
            // id would be rejected by acp, and every rejection error would
            // re-enter this function and burn the whole queue. The prompt
            // stays queued for the SessionBound handler (issue #94).
            return;
        }
        let Some(prompt) = self.prompt_queue.pop_front() else {
            return;
        };
        self.queued = self.prompt_queue.len();
        self.echo_staged_blocks(&prompt.blocks);
        self.prompt_pending = true;
        self.state = RunState::Starting;
        self.run_started = Some(Instant::now());
        self.state_note = self.locale.tr("sending queued followup", "正在发送排队消息").into();
        self.scroll_up = 0;

        match prompt.blocks.as_slice() {
            [StagedBlock::Text(text)] => ctl.send(Cmd::Prompt {
                session_id: self.session_id.clone(),
                text: text.clone(),
            }),
            _ => ctl.send(Cmd::PromptImages {
                session_id: self.session_id.clone(),
                blocks: prompt_blocks_from_staged(prompt.blocks),
            }),
        }
    }

    /// Empty Enter promotes the FIFO head into the active turn. If the agent
    /// cannot accept the steer, settlement restores the item to the front.
    fn send_queue_head_now(&mut self, ctl: &Controller) {
        if !self.session_bound {
            self.show_tip(self.locale.tr(
                "session still binding — prompt stays queued",
                "会话仍在绑定中 —— 消息保留在队列里",
            ));
            return;
        }
        if !self.input.is_empty()
            || !self.pending_images.is_empty()
            || self.queue_selection.is_some()
            || self.queue_edit.is_some()
        {
            return;
        }
        let Some(prompt) = self.prompt_queue.pop_front() else {
            return;
        };
        self.queued = self.prompt_queue.len();
        self.show_banner = false;
        self.scroll_up = 0;

        let running = self.state == RunState::Running || self.prompt_pending;
        let message_id = prompt.id;
        let blocks = prompt.blocks;
        let cells = self.echo_staged_blocks(&blocks);
        let text = match blocks.as_slice() {
            [StagedBlock::Text(text)] => Some(text.clone()),
            _ => None,
        };

        if running {
            self.pending_steer_cells.insert(
                message_id,
                PendingSteer {
                    cells,
                    blocks: blocks.clone(),
                    requeue_front: true,
                },
            );
            self.show_tip(self.locale.tr(
                "queue head sent now — lands at the next agent step",
                "队首已立即发送 —— 在下一步 Agent 处生效",
            ));
            ctl.send(if let Some(text) = text {
                Cmd::Steer {
                    session_id: self.session_id.clone(),
                    message_id,
                    text,
                }
            } else {
                Cmd::SteerImages {
                    session_id: self.session_id.clone(),
                    message_id,
                    blocks: prompt_blocks_from_staged(blocks),
                }
            });
            return;
        }

        self.prompt_pending = true;
        self.state = RunState::Starting;
        self.run_started = Some(Instant::now());
        self.state_note = self.locale.tr("sending queued followup", "正在发送排队消息").into();
        ctl.send(if let Some(text) = text {
            Cmd::Prompt {
                session_id: self.session_id.clone(),
                text,
            }
        } else {
            Cmd::PromptImages {
                session_id: self.session_id.clone(),
                blocks: prompt_blocks_from_staged(blocks),
            }
        });
    }

    fn open_queue_selector(&mut self) {
        if self.queue_selection.is_some() || self.queue_edit.is_some() {
            return;
        }
        if !self.input.is_empty() {
            self.show_tip(self.locale.tr(
                "send or clear the current draft before editing the queue",
                "编辑队列前请先发送或清空当前草稿",
            ));
            return;
        }
        if self.prompt_queue.is_empty() {
            self.show_tip(self.locale.tr("no queued prompt to edit", "没有可编辑的排队消息"));
            return;
        }
        self.queue_selection = Some(self.prompt_queue.len() - 1);
        self.slash_completion_dismissed = true;
        self.show_tip(self.locale.tr(
            "select queued prompt · ↑/↓ choose · enter edit · esc close",
            "选择排队消息 · ↑/↓ 选择 · enter 编辑 · esc 关闭",
        ));
    }

    fn begin_queue_edit_at(&mut self, index: usize) {
        let Some(prompt) = self.prompt_queue.get(index) else {
            self.queue_selection = None;
            self.show_tip(self.locale.tr(
                "queued prompt already left the queue",
                "这条消息已离开队列",
            ));
            return;
        };
        let prompt_id = prompt.id;
        let blocks = prompt.blocks.clone();
        self.queue_selection = None;
        let mut text = String::new();
        for block in blocks {
            match block {
                StagedBlock::Text(block_text) => text.push_str(&block_text),
                StagedBlock::Image(image) => {
                    let token = image.token.clone();
                    if self.pending_images.restore(image).is_ok() {
                        text.push_str(&token);
                    }
                }
            }
        }
        self.queue_edit = Some(QueueEditState {
            prompt_id,
            delete_confirm: false,
        });
        self.input.set(text);
        self.slash_completion_dismissed = false;
        self.show_tip(self.locale.trf(
            "editing queued prompt {} · enter save · ctrl+d delete · esc cancel",
            "编辑排队消息 {} · enter 保存 · ctrl+d 删除 · esc 取消",
            &[(index + 1).to_string()],
        ));
    }

    fn save_queue_edit(&mut self, ctl: &Controller) {
        let Some(edit) = self.queue_edit.as_ref() else {
            return;
        };
        let prompt_id = edit.prompt_id;
        if edit.delete_confirm {
            self.delete_queue_edit(ctl);
            return;
        }
        let raw = self.input.buf().trim().to_string();
        if raw.is_empty() && self.pending_images.is_empty() {
            self.show_tip(self.locale.tr(
                "queued prompt cannot be empty · ctrl+d deletes it",
                "排队消息不能为空 · ctrl+d 可删除",
            ));
            return;
        }
        let blocks = if self.pending_images.is_empty() {
            vec![StagedBlock::Text(raw)]
        } else {
            self.take_staged_blocks()
        };
        let Some(index) = self
            .prompt_queue
            .iter()
            .position(|prompt| prompt.id == prompt_id)
        else {
            self.queue_edit = None;
            self.input.clear();
            self.show_tip(self.locale.tr(
                "queued prompt already left the queue",
                "这条消息已离开队列",
            ));
            return;
        };
        self.prompt_queue[index].blocks = blocks;
        self.queue_edit = None;
        self.input.clear();
        self.slash_completion_dismissed = false;
        self.reconcile_attachments();
        self.show_tip(self.locale.trf(
            "queued prompt {} updated",
            "排队消息 {} 已更新",
            &[(index + 1).to_string()],
        ));
        if matches!(self.state, RunState::Idle) {
            self.dispatch_next_queued(ctl);
        }
    }

    fn delete_queue_edit(&mut self, ctl: &Controller) {
        let Some(prompt_id) = self.queue_edit.as_ref().map(|edit| edit.prompt_id) else {
            return;
        };
        let Some(index) = self
            .prompt_queue
            .iter()
            .position(|prompt| prompt.id == prompt_id)
        else {
            self.queue_edit = None;
            self.input.clear();
            self.show_tip(self.locale.tr(
                "queued prompt already left the queue",
                "这条消息已离开队列",
            ));
            return;
        };
        self.prompt_queue.remove(index);
        self.queued = self.prompt_queue.len();
        self.queue_edit = None;
        self.input.clear();
        self.slash_completion_dismissed = false;
        self.reconcile_attachments();
        self.show_tip(self.locale.trf(
            "queued prompt {} deleted",
            "排队消息 {} 已删除",
            &[(index + 1).to_string()],
        ));
        if matches!(self.state, RunState::Idle) {
            self.dispatch_next_queued(ctl);
        }
    }

    /// `/image <path> [caption]` — stage a local raster in the composer; it is
    /// sent on the next Enter (caption becomes the prompt text).
    fn send_image(&mut self, arg: &str, _ctl: &Controller) {
        let (path, caption) = match arg.split_once(char::is_whitespace) {
            Some((p, rest)) => (p, rest.trim().to_string()),
            None => (arg, String::new()),
        };
        if path.is_empty() {
            self.show_tip(self.locale.tr(
                "/image needs a path — /image ./pic.png [caption]",
                "/image 需要路径 —— /image ./pic.png [说明]",
            ));
            return;
        }
        let Some(media_type) = media_type_for(path) else {
            self.show_tip(self.locale.tr(
                "unsupported image — use .png .jpg .jpeg .webp .gif",
                "不支持的图片 —— 支持 .png .jpg .jpeg .webp .gif",
            ));
            return;
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => {
                self.show_tip(self.locale.trf(
                    "cannot read {}: {}",
                    "无法读取 {}：{}",
                    &[path.into(), err.to_string()],
                ));
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
            None => self.show_tip(self.locale.tr(
                "clipboard has no image, or this platform isn't supported",
                "剪贴板没有图片，或当前平台不支持",
            )),
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
                self.show_tip(if full == "attachment tray is full — send or remove an [image] chip first" {
                    self.locale
                        .tr(full, "附件栏已满 —— 先发送或移除一个 [image] 标记")
                        .to_string()
                } else {
                    full.to_string()
                });
                return;
            }
        };
        self.input_sel = None;
        if !caption.is_empty() {
            self.input.set(caption);
            self.input.insert_char(' ');
        } else if self.input.cursor_char() > 0
            && !self
                .input
                .buf()
                .chars()
                .nth(self.input.cursor_char() - 1)
                .is_none_or(char::is_whitespace)
        {
            self.input.insert_char(' ');
        }
        self.input.insert_str(&token);
        self.show_tip(self.locale.tr(
            "image staged — ⌫ deletes its chip · hover it to preview",
            "图片已附加 —— ⌫ 删除其标记 · 悬停可预览",
        ));
        self.needs_redraw = true;
    }

    /// Drain the tray and split the draft on chip spans, in reading order.
    fn take_staged_blocks(&mut self) -> Vec<StagedBlock> {
        self.reconcile_attachments();
        let buf = self.input.buf();
        split_draft_into_staged_blocks(&buf, self.pending_images.drain())
    }

    fn next_prompt_id(&mut self) -> u64 {
        let id = self.next_prompt_id;
        self.next_prompt_id = self.next_prompt_id.wrapping_add(1).max(1);
        id
    }

    /// Echo staged blocks in the transcript (text and image thumbnails in
    /// draft order) and send one prompt whose ACP blocks match that order.
    fn echo_staged_blocks(&mut self, staged: &[StagedBlock]) -> Vec<usize> {
        let first_cell = self.transcript.cells.len();
        for block in staged {
            match block {
                StagedBlock::Text(text) => self.transcript.push_user(text.clone(), false),
                StagedBlock::Image(att) => self.transcript.push_image(
                    att.name.clone(),
                    String::new(),
                    att.path.clone(),
                    att.data.clone(),
                    false,
                ),
            }
        }
        (first_cell..self.transcript.cells.len()).collect()
    }

    fn emit_staged_prompt(
        &mut self,
        staged: Vec<StagedBlock>,
        steer_message_id: Option<u64>,
        ctl: &Controller,
    ) {
        self.show_banner = false;
        let cells = self.echo_staged_blocks(&staged);
        if let Some(message_id) = steer_message_id {
            self.pending_steer_cells.insert(
                message_id,
                PendingSteer {
                    cells,
                    blocks: staged.clone(),
                    requeue_front: false,
                },
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
            let id = self.next_prompt_id();
            self.prompt_queue
                .push_back(ClientQueuedPrompt { id, blocks: staged });
            self.queued += 1;
            self.show_tip(if n <= 1 {
                self.locale
                    .tr("image queued · empty enter sends first", "图片已排队 · 空输入按 enter 发送队首")
                    .to_string()
            } else {
                self.locale.trf(
                    "{} images queued · empty enter sends first",
                    "{} 张图片已排队 · 空输入按 enter 发送队首",
                    &[n.to_string()],
                )
            });
            self.scroll_up = 0;
            return;
        } else {
            self.prompt_pending = true;
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
            self.state_note = if n <= 1 {
                self.locale.tr("sending image", "正在发送图片").into()
            } else {
                self.locale
                    .tr_fmt("sending {} images", "正在发送 {} 张图片", n)
            };
        }
        self.emit_staged_prompt(staged, None, ctl);
    }

    /// Send-now is ACP steering: issue another prompt immediately while the
    /// current turn remains active. Esc is the only cancellation path.
    fn send_now(&mut self, ctl: &Controller) {
        let raw = self.input.buf().trim().to_string();
        if !self.pending_images.is_empty() && (raw.starts_with('/') || raw.starts_with('!')) {
            self.show_tip(
                self.locale.tr(
                    "send or delete the [image] chips first — /commands and !shell don't take images",
                    "先发送或删除 [image] 标记 —— /命令 和 !shell 不接受图片",
                ),
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
        self.input.history.push(self.input.buf());
        self.input.clear();
        self.show_banner = false;
        let has_images = staged.iter().any(|b| matches!(b, StagedBlock::Image(_)));
        if has_images {
            let n = staged
                .iter()
                .filter(|b| matches!(b, StagedBlock::Image(_)))
                .count();
            if running {
                self.show_tip(self.locale.tr(
                    "steered with image — lands at the next agent step",
                    "已带图 steer —— 在下一步 Agent 处生效",
                ));
            } else {
                self.prompt_pending = true;
                self.state = RunState::Starting;
                self.run_started = Some(Instant::now());
                self.state_note = if n == 1 {
                    self.locale.tr("sending image", "正在发送图片").into()
                } else {
                    self.locale
                        .tr_fmt("sending {} images", "正在发送 {} 张图片", n)
                };
            }
            let steer_message_id = running.then(|| self.next_prompt_id());
            self.emit_staged_prompt(staged, steer_message_id, ctl);
        } else {
            let text = match staged.into_iter().next() {
                Some(StagedBlock::Text(t)) => t,
                _ => return,
            };
            let cell = self.transcript.cells.len();
            self.transcript.push_user(text.clone(), false);
            if running {
                self.show_tip(self.locale.tr(
                    "steered — lands at the next agent step",
                    "已 steer —— 在下一步 Agent 处生效",
                ));
            } else {
                self.prompt_pending = true;
                self.state = RunState::Starting;
                self.run_started = Some(Instant::now());
            }
            self.scroll_up = 0;
            ctl.send(if running {
                let message_id = self.next_prompt_id();
                self.pending_steer_cells.insert(
                    message_id,
                    PendingSteer {
                        cells: vec![cell],
                        blocks: vec![StagedBlock::Text(text.clone())],
                        requeue_front: false,
                    },
                );
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
        self.input_sel = None;
        self.input.set(text.to_string());
        self.submit(ctl);
    }

    fn run_local_shell(&mut self, cmd: String) {
        self.shell_seq += 1;
        let id = self.shell_seq;
        let cell = self.transcript.push_shell(cmd.clone());
        // Tag the pending cell with its session: the worker is shared and
        // the user may switch tabs before the command settles.
        self.shell_pending.push((id, self.session_id.clone(), cell));
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{micros:x}-{seq:x}")
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
#[path = "../tests/unit/app__session_tabs_tests.rs"]
mod session_tabs_tests;

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

#[cfg(test)]
#[path = "../tests/unit/app__scroll_tests.rs"]
mod scroll_tests;

#[cfg(test)]
#[path = "../tests/unit/app__at_menu_tests.rs"]
mod at_menu_tests;
