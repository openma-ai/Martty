//! Unified event bus for the single-threaded UI loop.

use serde_json::Value;

/// Everything the app loop can receive.
pub enum AppEvent {
    /// Terminal input.
    Term(crossterm::event::Event),
    /// JSON-RPC notification from the harness runtime.
    Rpc { method: String, params: Value },
    /// One line of runtime stderr (kept for diagnostics).
    RuntimeStderr(String),
    /// Runtime subprocess exited.
    RuntimeExited(Option<i32>),
    /// Controller lifecycle updates.
    Ctl(CtlEvent),
    /// Output of a local `!` shell command.
    ShellDone {
        id: u64,
        code: Option<i32>,
        output: String,
    },
}

/// Controller → UI status updates.
#[derive(Debug, Clone)]
#[allow(dead_code)] // message_id: protocol fidelity; surfaced in debug logs only
pub enum CtlEvent {
    /// Spawning + initializing the runtime.
    Starting { runtime: String },
    /// initialize returned.
    Ready { server: String },
    /// session/prompt accepted into the durable inbox.
    PromptQueued { message_id: String },
    /// A command failed.
    Error(String),
    /// Runtime was killed on purpose.
    Interrupted,
    /// Host model catalog arrived (plugin mode `tui/catalog`).
    Catalog {
        models: Vec<CatalogModel>,
        presets: Vec<CatalogPreset>,
    },
    /// Host skill catalog arrived (plugin mode `tui/skills`).
    Skills { skills: Vec<SkillInfo> },
    /// The host accepted an agent preset for this session (plugin mode
    /// `tui/preset`); it composes on the session's first prompt.
    PresetSet { preset: String },
    /// Selectable reasoning efforts for the current model.
    Efforts {
        efforts: Vec<String>,
        default: Option<String>,
    },
    /// A tui/* control call succeeded.
    TuiOpDone(String),
    /// A tui/* control call failed (or is unsupported on this transport).
    TuiOpFailed(String),
}

#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub vision: bool,
}

/// One agent preset (mode) from the host roster: standard / code / minimal /
/// creator in the stock Web UI, plus any custom presets the profile ships.
#[derive(Debug, Clone)]
pub struct CatalogPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub broken: bool,
}

/// One user-invocable host skill (plugin mode `tui/skills`): typing
/// `/name …` as a prompt makes the host inject the skill body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
}

/// One staged image on its way to the host (base64 payload).
#[derive(Debug, Clone)]
pub struct ImagePart {
    pub data: String,
    pub media_type: String,
    pub name: String,
}

/// UI → controller commands.
#[derive(Debug, Clone)]
pub enum Cmd {
    Prompt {
        session_id: String,
        text: String,
    },
    /// Send local raster images (base64 bytes) as image content blocks
    /// riding one prompt (the staged composer tray, drained on Enter).
    PromptImages {
        session_id: String,
        text: String,
        images: Vec<ImagePart>,
    },
    /// Interrupt the active turn. Plugin mode forwards to the host's
    /// `session/interrupt`; standalone kills the runtime subprocess.
    Interrupt {
        session_id: String,
    },
    /// Plugin-mode hot switch (falls back to SetModel semantics standalone).
    SelectModel {
        session_id: String,
        provider: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    },
    FetchCatalog,
    /// Fetch user-invocable host skills for the slash menu.
    FetchSkills,
    FetchEfforts {
        provider: String,
        model: String,
    },
    SetPermission {
        session_id: String,
        preset: String,
    },
    /// Pick the agent preset (mode) composed on the session's first prompt.
    SetPreset {
        session_id: String,
        preset: String,
    },
    Shutdown,
}
