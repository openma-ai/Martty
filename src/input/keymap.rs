//! The one `KeyEvent → Action` table (grok-build style): a pure function
//! with no `App` access, so every binding is visible and testable in one
//! place. OS conventions live here — ⌘/⌥ chords for macOS (delivered by
//! kitty-protocol terminals or rescued via the CG probe), ctrl+arrows
//! for linux/windows, and the readline control codes that macOS
//! terminals (ghostty · iterm "natural editing") send for ⌘-chords.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What the pressed key should do. `app` dispatches these; overlays
/// (model picker) and the slash menu intercept before classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Insert(char),
    Newline,
    Enter,
    Esc,
    CtrlC,
    Quit,
    ClearScrollback,
    ToggleTheme,
    ToggleExpandAll,
    SendNow,
    AttachClipboard,
    ModelPicker,
    CycleAgent,
    /// Show the shortcut list (`/keys`).
    ShowKeys,
    CyclePermission,
    TabComplete,
    HistoryPrev,
    HistoryNext,
    ScrollHalfUp,
    ScrollHalfDown,
    PageUp,
    PageDown,
    JumpTop,
    JumpTail,
    CursorLeft,
    CursorRight,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Backspace,
    DeleteForward,
    DeleteWordBack,
    KillToEnd,
    KillToStart,
}

/// The composer facts the mapping depends on (dual-use keys: `home`/`^u`
/// scroll when the draft is empty, edit when it isn't).
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyCtx {
    pub input_empty: bool,
}

/// Classify one key event. `None` = unbound (ignored — unrecognized
/// ⌃/⌥/⌘ chords must not type their base letter).
pub fn classify(key: &KeyEvent, ctx: KeyCtx) -> Option<Action> {
    use Action::*;
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    // ⌘: SUPER via kitty protocol / CG rescue; META from legacy encodings.
    let sup = m.intersects(KeyModifiers::SUPER | KeyModifiers::META);

    let action = match key.code {
        KeyCode::Enter if shift => Newline,
        KeyCode::Enter => Enter,
        KeyCode::Esc => Esc,

        // --- app chords -------------------------------------------------
        KeyCode::Char('c') if ctrl => CtrlC,
        KeyCode::Char('q') if ctrl => Quit,
        KeyCode::Char('l') if ctrl => ClearScrollback,
        KeyCode::Char('t') if ctrl => ToggleTheme,
        // ^o (not ^e): ghostty/iterm send ^e for ⌘→, and "explode the
        // transcript" on ⌘→ was evil.
        KeyCode::Char('o') if ctrl => ToggleExpandAll,
        KeyCode::Char('x') if ctrl => SendNow,
        KeyCode::Char('v') if ctrl => AttachClipboard,
        // ctrl+p (not ctrl+m): terminals send ctrl+m as the Enter byte.
        KeyCode::Char('p') if ctrl => ModelPicker,
        KeyCode::Char('a') if ctrl => CycleAgent,
        // ctrl+. → the shortcut list; needs a terminal that can encode
        // ctrl+punctuation (kitty protocol) — /keys always works.
        KeyCode::Char('.') if ctrl => ShowKeys,
        KeyCode::BackTab => CyclePermission,
        // kitty-protocol terminals may report shift+tab instead of BackTab.
        KeyCode::Tab if shift => CyclePermission,
        KeyCode::Tab => TabComplete,

        // --- readline / dual-use control codes ---------------------------
        // ^u/^d: kill/delete while typing, vim half-page on an empty draft.
        // (⌘⌫ reaches us as ^u in ghostty/iterm "natural editing".)
        KeyCode::Char('u') if ctrl && !ctx.input_empty => KillToStart,
        KeyCode::Char('d') if ctrl && !ctx.input_empty => DeleteForward,
        KeyCode::Char('u') if ctrl => ScrollHalfUp,
        KeyCode::Char('d') if ctrl => ScrollHalfDown,
        KeyCode::Char('k') if ctrl => KillToEnd,
        KeyCode::Char('w') if ctrl => DeleteWordBack,
        // ^e = end of line — also what ghostty/iterm send for ⌘→.
        KeyCode::Char('e') if ctrl => LineEnd,
        KeyCode::Char('b') if ctrl => CursorLeft,
        KeyCode::Char('f') if ctrl => CursorRight,
        // esc-b/esc-f: ⌥←/⌥→ in most macOS terminals (option-as-meta).
        KeyCode::Char('b') if alt => WordLeft,
        KeyCode::Char('f') if alt => WordRight,

        // --- scrolling ----------------------------------------------------
        KeyCode::PageUp => PageUp,
        KeyCode::PageDown => PageDown,

        // --- navigation (order: ⌘ → word-hop modifiers → plain) -----------
        KeyCode::Home if ctx.input_empty => JumpTop,
        KeyCode::End if ctx.input_empty => JumpTail,
        KeyCode::Home => LineStart,
        KeyCode::End => LineEnd,
        // ⌘-arrows: line ends horizontally, transcript top/tail vertically.
        KeyCode::Left if sup => LineStart,
        KeyCode::Right if sup => LineEnd,
        KeyCode::Up if sup => JumpTop,
        KeyCode::Down if sup => JumpTail,
        // Word hops: ⌥-arrows on macOS · ctrl-arrows on linux/windows.
        KeyCode::Left if alt || ctrl => WordLeft,
        KeyCode::Right if alt || ctrl => WordRight,
        KeyCode::Up => HistoryPrev,
        KeyCode::Down => HistoryNext,
        KeyCode::Left => CursorLeft,
        KeyCode::Right => CursorRight,

        // --- deletion ------------------------------------------------------
        // ⌘⌫ kills to line start; ⌥⌫ (macOS) / ctrl+⌫ (linux/windows)
        // delete a word; ^w works everywhere. Deleting over an inline
        // [image n] chip is handled by the dispatcher (whole-token cut).
        KeyCode::Backspace if sup => KillToStart,
        KeyCode::Backspace if alt || ctrl => DeleteWordBack,
        KeyCode::Backspace => Backspace,
        KeyCode::Delete => DeleteForward,

        // --- text ----------------------------------------------------------
        // Unbound ⌃/⌥/⌘ chords are ignored instead of inserting their base
        // letter (⌥← used to type a literal "b" into the draft).
        KeyCode::Char(ch) if !ctrl && !alt && !sup => Insert(ch),
        _ => return None,
    };
    Some(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const CTRL: KeyModifiers = KeyModifiers::CONTROL;
    const ALT: KeyModifiers = KeyModifiers::ALT;
    const SUPER: KeyModifiers = KeyModifiers::SUPER;

    fn empty() -> KeyCtx {
        KeyCtx { input_empty: true }
    }

    fn typing() -> KeyCtx {
        KeyCtx { input_empty: false }
    }

    #[test]
    fn cmd_arrows_are_line_and_transcript_motions() {
        assert_eq!(
            classify(&key(KeyCode::Left, SUPER), typing()),
            Some(Action::LineStart)
        );
        assert_eq!(
            classify(&key(KeyCode::Right, SUPER), typing()),
            Some(Action::LineEnd)
        );
        assert_eq!(
            classify(&key(KeyCode::Up, SUPER), typing()),
            Some(Action::JumpTop)
        );
        assert_eq!(
            classify(&key(KeyCode::Down, SUPER), typing()),
            Some(Action::JumpTail)
        );
        assert_eq!(
            classify(&key(KeyCode::Backspace, SUPER), typing()),
            Some(Action::KillToStart)
        );
    }

    #[test]
    fn word_hops_accept_both_conventions() {
        // ⌥ (macOS) and ctrl (linux/windows) both hop words.
        for m in [ALT, CTRL] {
            assert_eq!(
                classify(&key(KeyCode::Left, m), typing()),
                Some(Action::WordLeft)
            );
            assert_eq!(
                classify(&key(KeyCode::Right, m), typing()),
                Some(Action::WordRight)
            );
            assert_eq!(
                classify(&key(KeyCode::Backspace, m), typing()),
                Some(Action::DeleteWordBack)
            );
        }
        // esc-b/esc-f fallback.
        assert_eq!(
            classify(&key(KeyCode::Char('b'), ALT), typing()),
            Some(Action::WordLeft)
        );
        assert_eq!(
            classify(&key(KeyCode::Char('f'), ALT), typing()),
            Some(Action::WordRight)
        );
    }

    #[test]
    fn ctrl_a_cycles_agents_while_other_editing_codes_keep_their_meaning() {
        assert_eq!(
            classify(&key(KeyCode::Char('a'), CTRL), typing()),
            Some(Action::CycleAgent)
        );
        assert_eq!(
            classify(&key(KeyCode::Char('e'), CTRL), typing()),
            Some(Action::LineEnd)
        );
        assert_eq!(
            classify(&key(KeyCode::Char('u'), CTRL), typing()),
            Some(Action::KillToStart)
        );
    }

    #[test]
    fn expand_all_lives_on_ctrl_o_not_ctrl_e() {
        assert_eq!(
            classify(&key(KeyCode::Char('o'), CTRL), empty()),
            Some(Action::ToggleExpandAll)
        );
        assert_eq!(
            classify(&key(KeyCode::Char('e'), CTRL), empty()),
            Some(Action::LineEnd)
        );
    }

    #[test]
    fn dual_use_keys_scroll_only_on_empty_draft() {
        assert_eq!(
            classify(&key(KeyCode::Char('u'), CTRL), empty()),
            Some(Action::ScrollHalfUp)
        );
        assert_eq!(
            classify(&key(KeyCode::Char('d'), CTRL), empty()),
            Some(Action::ScrollHalfDown)
        );
        assert_eq!(
            classify(&key(KeyCode::Home, NONE), empty()),
            Some(Action::JumpTop)
        );
        assert_eq!(
            classify(&key(KeyCode::Home, NONE), typing()),
            Some(Action::LineStart)
        );
        assert_eq!(
            classify(&key(KeyCode::End, NONE), typing()),
            Some(Action::LineEnd)
        );
    }

    #[test]
    fn ctrl_dot_shows_the_shortcut_list() {
        assert_eq!(
            classify(&key(KeyCode::Char('.'), CTRL), empty()),
            Some(Action::ShowKeys)
        );
        assert_eq!(
            classify(&key(KeyCode::Char('.'), NONE), typing()),
            Some(Action::Insert('.'))
        );
    }

    #[test]
    fn unbound_chords_do_not_type_their_base_letter() {
        assert_eq!(classify(&key(KeyCode::Char('z'), ALT), typing()), None);
        assert_eq!(classify(&key(KeyCode::Char('c'), SUPER), typing()), None);
        assert_eq!(classify(&key(KeyCode::Char('g'), CTRL), typing()), None);
        assert_eq!(
            classify(&key(KeyCode::Char('z'), NONE), typing()),
            Some(Action::Insert('z'))
        );
    }

    #[test]
    fn shift_enter_is_newline_and_shift_tab_cycles_permission() {
        assert_eq!(
            classify(&key(KeyCode::Enter, KeyModifiers::SHIFT), typing()),
            Some(Action::Newline)
        );
        assert_eq!(
            classify(&key(KeyCode::BackTab, NONE), empty()),
            Some(Action::CyclePermission)
        );
        assert_eq!(
            classify(&key(KeyCode::Tab, KeyModifiers::SHIFT), empty()),
            Some(Action::CyclePermission)
        );
    }
}
