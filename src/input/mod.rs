//! Input layer, decoupled from the app (grok-build's architecture):
//!
//! * [`macos_mod`] — CoreGraphics side-channel reading the *physical*
//!   modifier keys (macOS only, no permissions needed).
//! * [`normalize`] — rescues key events whose modifiers the terminal
//!   dropped: bare `Left` + a physically held ⌘ becomes `SUPER+Left`.
//! * [`keymap`] — one pure `KeyEvent → Action` table, unit-testable,
//!   with the OS conventions (⌘/⌥ on macOS, ctrl+arrows elsewhere).
//! * [`editor`] — the single-line form editor (elicitation fields):
//!   buffer, cursor, history, word motions, kill commands.
//! * [`composer`] — the main composer editor on `ratatui-textarea`:
//!   buffer + cursor + undo in the widget, prompt history and the
//!   screen↔buffer layout mirror beside it.
//!
//! `app` only dispatches [`keymap::Action`]s; nothing in here touches
//! `App`, the transcript, or the controller.

pub mod composer;
pub mod editor;
pub mod keymap;
pub mod vim;
#[cfg(target_os = "macos")]
mod macos_mod;
pub mod normalize;

pub use keymap::{classify, Action, KeyCtx};
pub use vim::{VimMode, VimState};
pub use normalize::rescue_key;

/// Physically-held modifier keys at one instant. All-false where the OS
/// has no probe (non-macOS) — rescue then never fires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModifierState {
    pub command: bool,
    pub option: bool,
}
