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
}

/// UI → controller commands.
#[derive(Debug, Clone)]
pub enum Cmd {
    Prompt { session_id: String, text: String },
    Interrupt,
    SetModel(String),
    Shutdown,
}
