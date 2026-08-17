//! Rescue key events whose modifiers the terminal dropped.
//!
//! macOS terminals rarely deliver ⌘ (and often ⌥) to the application:
//! they swallow the chord, remap it to readline control codes, or send
//! the bare key. The keymap handles the control-code translations; this
//! module covers the third case: navigation/deletion chords, plus the one
//! Option+A app chord that macOS may precompose as Unicode.

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
    let rescued = rescue_with(key, held);
    #[cfg(target_os = "macos")]
    if rescued == key && key.modifiers.is_empty() && matches!(key.code, KeyCode::Char('\u{00e5}')) {
        // macOS US input materializes Option+A as `å`. Some terminals only
        // deliver that character after CoreGraphics already reports key-up,
        // so the physical-state probe cannot win the race. Since Option+A is
        // an application chord, consume its canonical terminal encoding.
        return KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT);
    }
    rescued
}

/// Pure core of [`rescue_key`] (probe injected for tests).
fn rescue_with(key: KeyEvent, held: ModifierState) -> KeyEvent {
    if !key.modifiers.is_empty() {
        return key; // the terminal spoke; believe it
    }
    // macOS US input turns Option+A into U+00E5 before many terminals report
    // it. Recover from physical key state rather than matching that Unicode
    // output, so keyboard layouts and composed text are not globally stolen.
    if held.option && held.a_key && matches!(key.code, KeyCode::Char(_)) {
        let mut out = key;
        out.code = KeyCode::Char('a');
        out.modifiers = KeyModifiers::ALT;
        return out;
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
        a_key: false,
    };
    const OPT: ModifierState = ModifierState {
        command: false,
        option: true,
        a_key: false,
    };
    const BOTH: ModifierState = ModifierState {
        command: true,
        option: true,
        a_key: false,
    };
    const OPT_A: ModifierState = ModifierState {
        command: false,
        option: true,
        a_key: true,
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
    fn option_a_unicode_output_is_restored_to_the_agent_shortcut() {
        let out = rescue_with(bare(KeyCode::Char('å')), OPT_A);
        assert_eq!(out.code, KeyCode::Char('a'));
        assert_eq!(out.modifiers, KeyModifiers::ALT);
    }

    #[test]
    fn unicode_text_is_not_stolen_without_the_physical_a_key() {
        let key = bare(KeyCode::Char('å'));
        assert_eq!(rescue_with(key, OPT), key);
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
