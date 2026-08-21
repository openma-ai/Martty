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
mod tests {
    use super::*;

    fn bare(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    const CMD: ModifierState = ModifierState {
        command: true,
        option: false,
    };
    const OPT: ModifierState = ModifierState {
        command: false,
        option: true,
    };
    const BOTH: ModifierState = ModifierState {
        command: true,
        option: true,
    };

    #[test]
    fn cmd_left_upgrades_to_super() {
        let out = rescue_with(bare(KeyCode::Left), CMD);
        assert_eq!(out.modifiers, KeyModifiers::SUPER);
        assert_eq!(out.code, KeyCode::Left);
    }

    #[test]
    fn opt_arrow_and_backspace_upgrade_to_alt() {
        for code in [KeyCode::Left, KeyCode::Right, KeyCode::Backspace] {
            assert_eq!(rescue_with(bare(code), OPT).modifiers, KeyModifiers::ALT);
        }
    }

    #[test]
    fn cmd_wins_when_both_held() {
        let out = rescue_with(bare(KeyCode::Backspace), BOTH);
        assert_eq!(out.modifiers, KeyModifiers::SUPER);
    }

    #[test]
    fn text_enter_esc_are_never_rescued() {
        for code in [
            KeyCode::Char('a'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::Char('v'),
        ] {
            let out = rescue_with(bare(code), BOTH);
            assert_eq!(out.modifiers, KeyModifiers::NONE, "{code:?} must stay bare");
        }
    }

    #[test]
    fn delivered_modifiers_are_believed() {
        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(
            rescue_with(key, CMD),
            key,
            "non-empty modifiers pass through"
        );
    }

    #[test]
    fn nothing_held_is_identity() {
        let key = bare(KeyCode::Left);
        assert_eq!(rescue_with(key, ModifierState::default()), key);
    }
}
