//! Optional vim modal editing for the composer (`/vim`), implemented as a
//! pure state machine in front of the normal keymap — no library changes.
//!
//! * **Normal** mode: plain keys become commands (h/j/k/l, w/b/e, dd, u,
//!   p, gg/G, i/a/A/o/O, …). Ctrl/⌘ chords always fall through to the app
//!   keymap (steer, undo, redo, …), so nothing global breaks.
//! * **Insert** mode: identical to the readline editing — `esc` returns to
//!   normal instead of clearing the draft.
//! * Off by default; `/vim` toggles it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::composer::ComposerEditor;

/// The composer editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    /// Vim editing is off — the plain keymap handles everything.
    #[default]
    Off,
    /// Normal (command) mode: plain keys are commands.
    Normal,
    /// Insert mode: plain keys type text, exactly like readline.
    Insert,
}

/// Half-entered two-key commands (`dd`, `gg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    D,
    G,
}

/// The vim state machine. Lives in `App`; plain-key handling is delegated
/// here before the normal keymap runs.
#[derive(Debug, Default)]
pub struct VimState {
    pub mode: VimMode,
    pending: Option<Pending>,
}

impl VimState {
    /// Turn vim editing on or off. Switching on lands in insert mode so
    /// the current draft keeps behaving as before until `esc`.
    pub fn set(&mut self, on: bool) {
        self.mode = if on { VimMode::Insert } else { VimMode::Off };
        self.pending = None;
    }

    pub fn is_active(&self) -> bool {
        self.mode != VimMode::Off
    }

    /// Handle one key while vim is active. Returns `true` when the key was
    /// consumed (must not reach the app keymap).
    pub fn handle_key(&mut self, key: &KeyEvent, editor: &mut ComposerEditor) -> bool {
        match self.mode {
            VimMode::Off => false,
            VimMode::Insert => {
                if key.code == KeyCode::Esc {
                    self.mode = VimMode::Normal;
                    self.pending = None;
                    true
                } else {
                    false
                }
            }
            VimMode::Normal => self.normal_key(key, editor),
        }
    }

    fn normal_key(&mut self, key: &KeyEvent, editor: &mut ComposerEditor) -> bool {
        // Ctrl/Alt/⌘ chords belong to the app keymap (steer, undo, …).
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::META)
        {
            return false;
        }
        let KeyCode::Char(ch) = key.code else {
            // Enter / arrows / backspace keep their app behavior in
            // normal mode (they are handy even in vim).
            return false;
        };

        // Any plain key clears the half-entered command unless it
        // completes it.
        let pending = self.pending.take();
        match (pending, ch) {
            (Some(Pending::D), 'd') => editor.kill_line(),
            (Some(Pending::G), 'g') => editor.jump_top(),
            // Two-key prefixes.
            (_, 'd') => self.pending = Some(Pending::D),
            (_, 'g') => self.pending = Some(Pending::G),
            // Motions.
            (_, 'h') => editor.move_left(),
            (_, 'l') => editor.move_right(),
            (_, 'j') => editor.move_down(),
            (_, 'k') => editor.move_up(),
            (_, 'w') => editor.word_right(),
            (_, 'b') => editor.word_left(),
            (_, 'e') => editor.word_end(),
            (_, '0') => editor.line_head(),
            (_, '$') => editor.line_tail(),
            (_, 'G') => editor.jump_bottom(),
            // Editing.
            (_, 'x') => editor.delete_forward(),
            (_, 'X') => editor.backspace(),
            (_, 'u') => {
                editor.undo();
            }
            (_, 'p') => {
                editor.paste_yank();
            }
            // Entering insert mode.
            (_, 'i') => self.mode = VimMode::Insert,
            (_, 'a') => {
                editor.move_right();
                self.mode = VimMode::Insert;
            }
            (_, 'I') => {
                editor.line_head();
                self.mode = VimMode::Insert;
            }
            (_, 'A') => {
                editor.line_tail();
                self.mode = VimMode::Insert;
            }
            (_, 'o') => {
                editor.line_tail();
                editor.insert_newline();
                self.mode = VimMode::Insert;
            }
            (_, 'O') => {
                editor.line_head();
                editor.insert_newline();
                self.mode = VimMode::Insert;
            }
            // Anything else (letters, digits, punctuation) is consumed as
            // an unknown command — never typed into the draft.
            _ => {}
        }
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/input__vim__tests.rs"]
mod tests;
