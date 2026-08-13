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
    Catalog { models: Vec<CatalogModel> },
    /// Selectable reasoning efforts for the current model.
    Efforts { efforts: Vec<String>, default: Option<String> },
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

/// UI → controller commands.
#[derive(Debug, Clone)]
pub enum Cmd {
    Prompt { session_id: String, text: String },
    Interrupt,
    /// Plugin-mode hot switch (falls back to SetModel semantics standalone).
    SelectModel {
        session_id: String,
        provider: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    },
    FetchCatalog,
    FetchEfforts { provider: String, model: String },
    SetPermission { session_id: String, preset: String },
    Shutdown,
}
