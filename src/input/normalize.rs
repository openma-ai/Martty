//! Rescue key events whose modifiers the terminal dropped.
//!
//! macOS terminals rarely deliver ⌘ (and often ⌥) to the application:
//! they swallow the chord, remap it to readline control codes, or send
//! the bare key. The keymap handles the control-code translations; this
//! module covers the third case for navigation and deletion chords.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModifierState;

fn probe() -> ModifierState {
    #[cfg(target_os = "macos")]
    {
        super::macos_mod::snapshot()
    }
    #[cfg(not(target_os = "macos"))]
    {
        ModifierState::default()
    }
}

/// Upgrade `key` with physically-held state when the terminal sent no
/// modifiers. Arbitrary text, Enter, Esc, and paste stay untouched.
pub fn rescue_key(key: KeyEvent) -> KeyEvent {
    let held = probe();
    rescue_with(key, held)
}

/// Pure core of [`rescue_key`] (probe injected for tests).
fn rescue_with(key: KeyEvent, held: ModifierState) -> KeyEvent {
    if !key.modifiers.is_empty() {
        return key; // the terminal spoke; believe it
    }
    if !matches!(
        key.code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete
    ) {
        return key;
    }
    let mut out = key;
    // ⌘ wins per macOS convention (⌘⌫ line-kill is the stronger action;
    // almost no one holds ⌘⌥ simultaneously) — same rule as grok-build.
    if held.command {
        out.modifiers |= KeyModifiers::SUPER;
    } else if held.option {
        out.modifiers |= KeyModifiers::ALT;
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/input__normalize__tests.rs"]
mod tests;
