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
    EditQueuedPrompt,
    AttachClipboard,
    ModelPicker,
    CycleAgent,
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
    CursorUp,
    CursorDown,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Backspace,
    DeleteForward,
    DeleteWordBack,
    KillToEnd,
    KillToStart,
    /// Undo the last draft edit (the textarea keeps a 50-step history).
    Undo,
    /// Redo the last undone draft edit.
    Redo,
    /// Paste the yank buffer (last kill) at the cursor.
    YankPaste,
    /// Delete the whole current line (ctrl+shift+k).
    KillLine,
    /// Start/extend a text selection toward the left (shift+←).
    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectWordLeft,
    SelectWordRight,
    SelectLineStart,
    SelectLineEnd,
    /// Copy the keyboard selection to the system clipboard (+ yank).
    CopySelection,
    /// Cut the keyboard selection (clipboard + yank + delete).
    CutSelection,
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
        // ctrl+enter / ⌘⏎ steer the active turn (send-now); legacy
        // terminals collapse ctrl+enter to plain enter, which degrades to
        // a normal send/queue. shift+enter stays a draft newline.
        KeyCode::Enter if sup => SendNow,
        KeyCode::Enter if ctrl => SendNow,
        // ctrl+j is the readline newline byte (issue #55) — reliable in
        // every terminal, unlike ctrl+enter.
        KeyCode::Char('j') if ctrl => Newline,
        KeyCode::Enter if shift => Newline,
        KeyCode::Enter => Enter,
        KeyCode::Esc => Esc,

        // --- app chords -------------------------------------------------
        // ctrl+shift+c copies the keyboard selection (plain ctrl+c clears
        // the draft / quits; the editor's own yank copy rides along).
        // ctrl+x cuts it — steer moved to ctrl+enter.
        KeyCode::Char('c') if ctrl && shift => CopySelection,
        KeyCode::Char('c') if ctrl => CtrlC,
        KeyCode::Char('x') if ctrl => CutSelection,
        KeyCode::Char('q') if ctrl => Quit,
        KeyCode::Char('l') if ctrl => ClearScrollback,
        KeyCode::Char('t') if ctrl => ToggleTheme,
        // ^o (not ^e): ghostty/iterm send ^e for ⌘→, and "explode the
        // transcript" on ⌘→ was evil.
        KeyCode::Char('o') if ctrl => ToggleExpandAll,
        KeyCode::Up if alt => EditQueuedPrompt,
        KeyCode::Char('v') if ctrl => AttachClipboard,
        // ctrl+p (not ctrl+m): terminals send ctrl+m as the Enter byte.
        KeyCode::Char('p') if ctrl => ModelPicker,
        // Keep the agent shortcut on the modified chord. Legacy terminals
        // may collapse it to ^a, which safely degrades to line-start.
        KeyCode::Char('a') if ctrl && shift => CycleAgent,
        // ^a = start of line — also what macOS terminals commonly send for
        // ⌘← when their natural-editing mode consumes the Command modifier.
        KeyCode::Char('a') if ctrl => LineStart,
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
        // ctrl+shift+k kills the whole line; plain ctrl+k keeps the
        // readline kill-to-line-end behavior.
        KeyCode::Char('k') if ctrl && shift => KillLine,
        KeyCode::Char('k') if ctrl => KillToEnd,
        KeyCode::Char('w') if ctrl => DeleteWordBack,
        // ^e = end of line — also what ghostty/iterm send for ⌘→.
        KeyCode::Char('e') if ctrl && shift => SelectLineEnd,
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
        // Shift + move extends the text selection (the editor cancels it
        // on plain moves, edits and clicks).
        KeyCode::Left if sup && shift => SelectLineStart,
        KeyCode::Right if sup && shift => SelectLineEnd,
        KeyCode::Left if shift && (alt || ctrl) => SelectWordLeft,
        KeyCode::Right if shift && (alt || ctrl) => SelectWordRight,
        KeyCode::Up if shift && !sup && !alt && !ctrl && !ctx.input_empty => SelectUp,
        KeyCode::Down if shift && !sup && !alt && !ctrl && !ctx.input_empty => SelectDown,
        KeyCode::Home if shift => SelectLineStart,
        KeyCode::End if shift => SelectLineEnd,
        KeyCode::Left if shift && !sup && !alt && !ctrl => SelectLeft,
        KeyCode::Right if shift && !sup && !alt && !ctrl => SelectRight,
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
        KeyCode::Up if ctx.input_empty => HistoryPrev,
        KeyCode::Down if ctx.input_empty => HistoryNext,
        KeyCode::Up => CursorUp,
        KeyCode::Down => CursorDown,
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

        // --- undo / redo / yank --------------------------------------------
        // ctrl+z everywhere, ⌘z on macOS (natural-editing terminals send
        // ^z for ⌘z, which the ctrl arm catches); redo on ctrl+shift+z and
        // ⌘⇧z. ctrl+y pastes the yank buffer (last kill), readline-style.
        KeyCode::Char('z') if sup && shift => Redo,
        KeyCode::Char('z') if sup => Undo,
        KeyCode::Char('z') if ctrl && shift => Redo,
        KeyCode::Char('z') if ctrl => Undo,
        KeyCode::Char('y') if ctrl => YankPaste,

        // --- text ----------------------------------------------------------
        // Unbound ⌃/⌥/⌘ chords are ignored instead of inserting their base
        // letter (⌥← used to type a literal "b" into the draft).
        KeyCode::Char(ch) if !ctrl && !alt && !sup => Insert(ch),
        _ => return None,
    };
    Some(action)
}

// ---------------------------------------------------------------------------
// `/keys` documentation table — the single source of truth for `push_keys`.
//
// Every row is backed by probes: concrete `(KeyCode, KeyModifiers, ctx)` that
// `classify` must resolve back to the row's Action, so the printed shortcuts
// cannot drift from the real keymap (the tests below enforce that).
// ---------------------------------------------------------------------------

use Action::*;

/// When a binding applies. Context rows are dual-use keys whose meaning
/// changes when the draft is empty (scrolling vs. editing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtxNote {
    Always,
    EmptyPrompt,
    Typing,
}

impl CtxNote {
    pub fn tag(self, zh: bool) -> &'static str {
        match (self, zh) {
            (CtxNote::Always, _) => "",
            (CtxNote::EmptyPrompt, false) => "[empty]",
            (CtxNote::EmptyPrompt, true) => "[空输入时]",
            (CtxNote::Typing, false) => "[typing]",
            (CtxNote::Typing, true) => "[输入时]",
        }
    }
}

/// The `/keys` groups, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyGroup {
    Send,
    Navigate,
    /// The composer textarea's own editing keys (motions, kill, undo,
    /// selection, yank) — one dedicated section in `/keys`.
    Composer,
    App,
    Mouse,
}

impl KeyGroup {
    fn title(self, zh: bool) -> &'static str {
        match (self, zh) {
            (KeyGroup::Send, false) => "send · interrupt",
            (KeyGroup::Send, true) => "发送 · 中断",
            (KeyGroup::Navigate, false) => "scroll · navigate",
            (KeyGroup::Navigate, true) => "滚动 · 导航",
            (KeyGroup::Composer, false) => "composer textarea",
            (KeyGroup::Composer, true) => "输入框 · textarea",
            (KeyGroup::App, false) => "app shortcuts",
            (KeyGroup::App, true) => "应用快捷键",
            (KeyGroup::Mouse, false) => "mouse",
            (KeyGroup::Mouse, true) => "鼠标",
        }
    }
}

/// One documented keybinding.
pub struct KeyRow {
    /// Documentation anchor; consumed by the anti-drift tests (the renderer
    /// only needs the display fields, so non-test builds see this unused).
    #[allow(dead_code)]
    pub action: Action,
    pub group: KeyGroup,
    /// Display spellings; the platform set is picked at render time.
    pub chords_mac: &'static [&'static str],
    pub chords_other: &'static [&'static str],
    pub ctx: CtxNote,
    pub desc_en: &'static str,
    pub desc_zh: &'static str,
    /// One concrete event per displayed chord that `classify` must map back
    /// to `action` (anti-drift probes: `(code, modifiers, input_empty)`).
    #[allow(dead_code)]
    pub probes: &'static [(KeyCode, KeyModifiers, bool)],
}

impl KeyRow {
    fn chords(&self, mac: bool) -> &'static [&'static str] {
        if mac {
            self.chords_mac
        } else {
            self.chords_other
        }
    }
}

/// Mouse affordances live in the app event path, not `classify`, so they
/// carry no probes — they are documented here only.
pub struct MouseRow {
    pub chords: &'static [&'static str],
    pub desc_en: &'static str,
    pub desc_zh: &'static str,
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const SUPER: KeyModifiers = KeyModifiers::SUPER;
/// `CONTROL | SHIFT` — bitflags has no const `BitOr`, so spell the bits.
const CTRL_SHIFT: KeyModifiers = KeyModifiers::from_bits_retain(0b0000_0011);
/// `SUPER | SHIFT` (bit 3 | bit 0).
const SUPER_SHIFT: KeyModifiers = KeyModifiers::from_bits_retain(0b0000_1001);
/// `SHIFT | ALT` (bit 0 | bit 2).
const SHIFT_ALT: KeyModifiers = KeyModifiers::from_bits_retain(0b0000_0101);
/// `SHIFT | CTRL` (bit 0 | bit 1).
const SHIFT_CTRL: KeyModifiers = KeyModifiers::from_bits_retain(0b0000_0011);

const fn p(code: KeyCode, m: KeyModifiers, empty: bool) -> (KeyCode, KeyModifiers, bool) {
    (code, m, empty)
}

pub const KEY_ROWS: &[KeyRow] = &[
    // --- send · interrupt -------------------------------------------------
    KeyRow {
        action: Enter,
        group: KeyGroup::Send,
        chords_mac: &["enter"],
        chords_other: &["enter"],
        ctx: CtxNote::Always,
        desc_en: "send · empty Enter sends the Queue head now",
        desc_zh: "发送；空输入时立即发送 Queue 队首",
        probes: &[p(KeyCode::Enter, NONE, false)],
    },
    KeyRow {
        action: Newline,
        group: KeyGroup::Send,
        chords_mac: &["shift+enter", "ctrl+j"],
        chords_other: &["shift+enter", "ctrl+j"],
        ctx: CtxNote::Always,
        desc_en: "newline in the draft",
        desc_zh: "草稿内换行",
        probes: &[
            p(KeyCode::Enter, SHIFT, false),
            p(KeyCode::Char('j'), CTRL, false),
        ],
    },
    KeyRow {
        action: SendNow,
        group: KeyGroup::Send,
        chords_mac: &["ctrl+enter", "⌘⏎"],
        chords_other: &["ctrl+enter"],
        ctx: CtxNote::Always,
        desc_en: "send now — steers the active turn",
        desc_zh: "立即发送（steer 当前轮次）",
        probes: &[
            p(KeyCode::Enter, CTRL, false),
            p(KeyCode::Enter, SUPER, false),
        ],
    },
    KeyRow {
        action: EditQueuedPrompt,
        group: KeyGroup::Send,
        chords_mac: &["⌥↑"],
        chords_other: &["alt+↑"],
        ctx: CtxNote::Always,
        desc_en: "choose a queued prompt to edit",
        desc_zh: "选择一条排队消息编辑",
        probes: &[p(KeyCode::Up, ALT, true)],
    },
    KeyRow {
        action: Esc,
        group: KeyGroup::Send,
        chords_mac: &["esc"],
        chords_other: &["esc"],
        ctx: CtxNote::Always,
        desc_en: "close / suggestions · cancel edit · interrupt",
        desc_zh: "关闭 / 推荐 · 取消编辑 · 中断",
        probes: &[p(KeyCode::Esc, NONE, false)],
    },
    KeyRow {
        action: CtrlC,
        group: KeyGroup::Send,
        chords_mac: &["ctrl+c"],
        chords_other: &["ctrl+c"],
        ctx: CtxNote::Always,
        desc_en: "clear draft · 2× quits with no draft",
        desc_zh: "清草稿；无草稿时连按 2 次退出",
        probes: &[p(KeyCode::Char('c'), CTRL, false)],
    },
    KeyRow {
        action: Quit,
        group: KeyGroup::Send,
        chords_mac: &["ctrl+q"],
        chords_other: &["ctrl+q"],
        ctx: CtxNote::Always,
        desc_en: "quit dsh-tui",
        desc_zh: "退出 dsh-tui",
        probes: &[p(KeyCode::Char('q'), CTRL, false)],
    },
    // --- scroll · navigate ------------------------------------------------
    KeyRow {
        action: HistoryPrev,
        group: KeyGroup::Navigate,
        chords_mac: &["↑"],
        chords_other: &["↑"],
        ctx: CtxNote::EmptyPrompt,
        desc_en: "previous prompt",
        desc_zh: "上一条历史",
        probes: &[p(KeyCode::Up, NONE, true)],
    },
    KeyRow {
        action: HistoryNext,
        group: KeyGroup::Navigate,
        chords_mac: &["↓"],
        chords_other: &["↓"],
        ctx: CtxNote::EmptyPrompt,
        desc_en: "next prompt",
        desc_zh: "下一条历史",
        probes: &[p(KeyCode::Down, NONE, true)],
    },
    KeyRow {
        action: JumpTop,
        group: KeyGroup::Navigate,
        chords_mac: &["home", "⌘↑"],
        chords_other: &["home"],
        ctx: CtxNote::EmptyPrompt,
        desc_en: "transcript top",
        desc_zh: "对话顶部",
        probes: &[p(KeyCode::Home, NONE, true), p(KeyCode::Up, SUPER, false)],
    },
    KeyRow {
        action: JumpTail,
        group: KeyGroup::Navigate,
        chords_mac: &["end", "⌘↓"],
        chords_other: &["end"],
        ctx: CtxNote::EmptyPrompt,
        desc_en: "transcript tail",
        desc_zh: "对话底部",
        probes: &[p(KeyCode::End, NONE, true), p(KeyCode::Down, SUPER, false)],
    },
    KeyRow {
        action: ScrollHalfUp,
        group: KeyGroup::Navigate,
        chords_mac: &["ctrl+u"],
        chords_other: &["ctrl+u"],
        ctx: CtxNote::EmptyPrompt,
        desc_en: "scroll up half a page",
        desc_zh: "上翻半页",
        probes: &[p(KeyCode::Char('u'), CTRL, true)],
    },
    KeyRow {
        action: ScrollHalfDown,
        group: KeyGroup::Navigate,
        chords_mac: &["ctrl+d"],
        chords_other: &["ctrl+d"],
        ctx: CtxNote::EmptyPrompt,
        desc_en: "scroll down half a page",
        desc_zh: "下翻半页",
        probes: &[p(KeyCode::Char('d'), CTRL, true)],
    },
    KeyRow {
        action: PageUp,
        group: KeyGroup::Navigate,
        chords_mac: &["pgup"],
        chords_other: &["pgup"],
        ctx: CtxNote::Always,
        desc_en: "page up",
        desc_zh: "上一页",
        probes: &[p(KeyCode::PageUp, NONE, false)],
    },
    KeyRow {
        action: PageDown,
        group: KeyGroup::Navigate,
        chords_mac: &["pgdn"],
        chords_other: &["pgdn"],
        ctx: CtxNote::Always,
        desc_en: "page down",
        desc_zh: "下一页",
        probes: &[p(KeyCode::PageDown, NONE, false)],
    },
    // --- composer textarea -------------------------------------------------
    KeyRow {
        action: LineStart,
        group: KeyGroup::Composer,
        chords_mac: &["home", "⌘←", "ctrl+a"],
        chords_other: &["home", "ctrl+a"],
        ctx: CtxNote::Typing,
        desc_en: "line start",
        desc_zh: "行首",
        probes: &[
            p(KeyCode::Home, NONE, false),
            p(KeyCode::Left, SUPER, false),
            p(KeyCode::Char('a'), CTRL, false),
        ],
    },
    KeyRow {
        action: LineEnd,
        group: KeyGroup::Composer,
        chords_mac: &["end", "⌘→", "ctrl+e"],
        chords_other: &["end", "ctrl+e"],
        ctx: CtxNote::Typing,
        desc_en: "line end",
        desc_zh: "行尾",
        probes: &[
            p(KeyCode::End, NONE, false),
            p(KeyCode::Right, SUPER, false),
            p(KeyCode::Char('e'), CTRL, false),
        ],
    },
    KeyRow {
        action: CursorLeft,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+b"],
        chords_other: &["ctrl+b"],
        ctx: CtxNote::Typing,
        desc_en: "cursor left",
        desc_zh: "光标左移",
        probes: &[p(KeyCode::Char('b'), CTRL, false)],
    },
    KeyRow {
        action: CursorRight,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+f"],
        chords_other: &["ctrl+f"],
        ctx: CtxNote::Typing,
        desc_en: "cursor right",
        desc_zh: "光标右移",
        probes: &[p(KeyCode::Char('f'), CTRL, false)],
    },
    KeyRow {
        action: SelectLeft,
        group: KeyGroup::Composer,
        chords_mac: &["shift+←"],
        chords_other: &["shift+←"],
        ctx: CtxNote::Typing,
        desc_en: "select left",
        desc_zh: "向左选择",
        probes: &[p(KeyCode::Left, SHIFT, false)],
    },
    KeyRow {
        action: SelectRight,
        group: KeyGroup::Composer,
        chords_mac: &["shift+→"],
        chords_other: &["shift+→"],
        ctx: CtxNote::Typing,
        desc_en: "select right",
        desc_zh: "向右选择",
        probes: &[p(KeyCode::Right, SHIFT, false)],
    },
    KeyRow {
        action: SelectUp,
        group: KeyGroup::Composer,
        chords_mac: &["shift+↑"],
        chords_other: &["shift+↑"],
        ctx: CtxNote::Typing,
        desc_en: "select up a screen line",
        desc_zh: "向上选择一行",
        probes: &[p(KeyCode::Up, SHIFT, false)],
    },
    KeyRow {
        action: SelectDown,
        group: KeyGroup::Composer,
        chords_mac: &["shift+↓"],
        chords_other: &["shift+↓"],
        ctx: CtxNote::Typing,
        desc_en: "select down a screen line",
        desc_zh: "向下选择一行",
        probes: &[p(KeyCode::Down, SHIFT, false)],
    },
    KeyRow {
        action: SelectWordLeft,
        group: KeyGroup::Composer,
        chords_mac: &["⌥⇧←"],
        chords_other: &["ctrl+shift+←"],
        ctx: CtxNote::Typing,
        desc_en: "select word backward",
        desc_zh: "向前选一个词",
        probes: &[
            p(KeyCode::Left, SHIFT_ALT, false),
            p(KeyCode::Left, SHIFT_CTRL, false),
        ],
    },
    KeyRow {
        action: SelectWordRight,
        group: KeyGroup::Composer,
        chords_mac: &["⌥⇧→"],
        chords_other: &["ctrl+shift+→"],
        ctx: CtxNote::Typing,
        desc_en: "select word forward",
        desc_zh: "向后选一个词",
        probes: &[
            p(KeyCode::Right, SHIFT_ALT, false),
            p(KeyCode::Right, SHIFT_CTRL, false),
        ],
    },
    KeyRow {
        action: SelectLineStart,
        group: KeyGroup::Composer,
        chords_mac: &["shift+home", "⌘⇧←"],
        chords_other: &["shift+home"],
        ctx: CtxNote::Typing,
        desc_en: "select to line start",
        desc_zh: "选择到行首",
        probes: &[
            p(KeyCode::Home, SHIFT, false),
            p(KeyCode::Left, SUPER_SHIFT, false),
        ],
    },
    KeyRow {
        action: SelectLineEnd,
        group: KeyGroup::Composer,
        chords_mac: &["shift+end", "⌘⇧→", "ctrl+shift+e"],
        chords_other: &["shift+end", "ctrl+shift+e"],
        ctx: CtxNote::Typing,
        desc_en: "select to line end",
        desc_zh: "选择到行尾",
        probes: &[
            p(KeyCode::End, SHIFT, false),
            p(KeyCode::Right, SUPER_SHIFT, false),
            p(KeyCode::Char('e'), CTRL_SHIFT, false),
        ],
    },
    KeyRow {
        action: CopySelection,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+shift+c"],
        chords_other: &["ctrl+shift+c"],
        ctx: CtxNote::Typing,
        desc_en: "copy the selection",
        desc_zh: "复制选中文本",
        probes: &[p(KeyCode::Char('c'), CTRL_SHIFT, false)],
    },
    KeyRow {
        action: CutSelection,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+x"],
        chords_other: &["ctrl+x"],
        ctx: CtxNote::Typing,
        desc_en: "cut the selection",
        desc_zh: "剪切选中文本",
        probes: &[p(KeyCode::Char('x'), CTRL, false)],
    },
    KeyRow {
        action: CursorUp,
        group: KeyGroup::Composer,
        chords_mac: &["↑"],
        chords_other: &["↑"],
        ctx: CtxNote::Typing,
        desc_en: "cursor up",
        desc_zh: "光标上移",
        probes: &[p(KeyCode::Up, NONE, false)],
    },
    KeyRow {
        action: CursorDown,
        group: KeyGroup::Composer,
        chords_mac: &["↓"],
        chords_other: &["↓"],
        ctx: CtxNote::Typing,
        desc_en: "cursor down",
        desc_zh: "光标下移",
        probes: &[p(KeyCode::Down, NONE, false)],
    },
    KeyRow {
        action: WordLeft,
        group: KeyGroup::Composer,
        chords_mac: &["⌥←", "esc-b"],
        chords_other: &["ctrl+←"],
        ctx: CtxNote::Typing,
        desc_en: "word back",
        desc_zh: "按词左移",
        probes: &[
            p(KeyCode::Left, ALT, false),
            p(KeyCode::Char('b'), ALT, false),
            p(KeyCode::Left, CTRL, false),
        ],
    },
    KeyRow {
        action: WordRight,
        group: KeyGroup::Composer,
        chords_mac: &["⌥→", "esc-f"],
        chords_other: &["ctrl+→"],
        ctx: CtxNote::Typing,
        desc_en: "word forward",
        desc_zh: "按词右移",
        probes: &[
            p(KeyCode::Right, ALT, false),
            p(KeyCode::Char('f'), ALT, false),
            p(KeyCode::Right, CTRL, false),
        ],
    },
    KeyRow {
        action: KillToStart,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+u", "⌘⌫"],
        chords_other: &["ctrl+u"],
        ctx: CtxNote::Typing,
        desc_en: "kill to line start",
        desc_zh: "删除到行首",
        probes: &[
            p(KeyCode::Char('u'), CTRL, false),
            p(KeyCode::Backspace, SUPER, false),
        ],
    },
    KeyRow {
        action: KillToEnd,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+k"],
        chords_other: &["ctrl+k"],
        ctx: CtxNote::Always,
        desc_en: "kill to line end",
        desc_zh: "删除到行尾",
        probes: &[
            p(KeyCode::Char('k'), CTRL, false),
            p(KeyCode::Char('k'), CTRL, true),
        ],
    },
    KeyRow {
        action: KillLine,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+shift+k"],
        chords_other: &["ctrl+shift+k"],
        ctx: CtxNote::Typing,
        desc_en: "kill the whole line",
        desc_zh: "删除整行",
        probes: &[p(KeyCode::Char('k'), CTRL_SHIFT, false)],
    },
    KeyRow {
        action: DeleteWordBack,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+w", "⌥⌫"],
        chords_other: &["ctrl+w", "ctrl+⌫"],
        ctx: CtxNote::Typing,
        desc_en: "delete word back",
        desc_zh: "删除前一词",
        probes: &[
            p(KeyCode::Char('w'), CTRL, false),
            p(KeyCode::Backspace, ALT, false),
            p(KeyCode::Backspace, CTRL, false),
        ],
    },
    KeyRow {
        action: Undo,
        group: KeyGroup::Composer,
        chords_mac: &["⌘z"],
        chords_other: &["ctrl+z"],
        ctx: CtxNote::Typing,
        desc_en: "undo the last draft edit",
        desc_zh: "撤销上一步草稿编辑",
        probes: &[
            p(KeyCode::Char('z'), CTRL, false),
            p(KeyCode::Char('z'), SUPER, false),
        ],
    },
    KeyRow {
        action: Redo,
        group: KeyGroup::Composer,
        chords_mac: &["⌘⇧z"],
        chords_other: &["ctrl+shift+z"],
        ctx: CtxNote::Typing,
        desc_en: "redo the last undone edit",
        desc_zh: "重做上一步撤销",
        probes: &[
            p(KeyCode::Char('z'), CTRL_SHIFT, false),
            p(KeyCode::Char('z'), SUPER_SHIFT, false),
        ],
    },
    KeyRow {
        action: YankPaste,
        group: KeyGroup::Composer,
        chords_mac: &["ctrl+y"],
        chords_other: &["ctrl+y"],
        ctx: CtxNote::Typing,
        desc_en: "paste the last killed text",
        desc_zh: "粘贴最近一次删除的文本",
        probes: &[p(KeyCode::Char('y'), CTRL, false)],
    },
    KeyRow {
        action: DeleteForward,
        group: KeyGroup::Composer,
        chords_mac: &["delete", "ctrl+d"],
        chords_other: &["delete", "ctrl+d"],
        ctx: CtxNote::Typing,
        desc_en: "delete forward",
        desc_zh: "删除光标后字符",
        probes: &[
            p(KeyCode::Delete, NONE, false),
            p(KeyCode::Char('d'), CTRL, false),
        ],
    },
    KeyRow {
        action: Backspace,
        group: KeyGroup::Composer,
        chords_mac: &["⌫"],
        chords_other: &["⌫"],
        ctx: CtxNote::Typing,
        desc_en: "backspace",
        desc_zh: "退格",
        probes: &[p(KeyCode::Backspace, NONE, false)],
    },
    // --- app shortcuts ----------------------------------------------------
    KeyRow {
        action: CycleAgent,
        group: KeyGroup::App,
        chords_mac: &["ctrl+shift+a"],
        chords_other: &["ctrl+shift+a"],
        ctx: CtxNote::Always,
        desc_en: "cycle agent preset",
        desc_zh: "切换 Agent 预设",
        probes: &[p(KeyCode::Char('a'), CTRL_SHIFT, false)],
    },
    KeyRow {
        action: CyclePermission,
        group: KeyGroup::App,
        chords_mac: &["shift+tab"],
        chords_other: &["shift+tab"],
        ctx: CtxNote::Always,
        desc_en: "cycle permission preset (/permission)",
        desc_zh: "轮换权限预设（/permission）",
        probes: &[p(KeyCode::BackTab, NONE, false)],
    },
    KeyRow {
        action: ModelPicker,
        group: KeyGroup::App,
        chords_mac: &["ctrl+p"],
        chords_other: &["ctrl+p"],
        ctx: CtxNote::Always,
        desc_en: "model picker → effort picker",
        desc_zh: "模型选择器 → 推理强度",
        probes: &[p(KeyCode::Char('p'), CTRL, false)],
    },
    KeyRow {
        action: ToggleTheme,
        group: KeyGroup::App,
        chords_mac: &["ctrl+t"],
        chords_other: &["ctrl+t"],
        ctx: CtxNote::Always,
        desc_en: "toggle dark/light",
        desc_zh: "切换明暗主题",
        probes: &[p(KeyCode::Char('t'), CTRL, false)],
    },
    KeyRow {
        action: ToggleExpandAll,
        group: KeyGroup::App,
        chords_mac: &["ctrl+o"],
        chords_other: &["ctrl+o"],
        ctx: CtxNote::Always,
        desc_en: "expand/collapse thoughts + tool output",
        desc_zh: "展开/折叠思考与工具输出",
        probes: &[p(KeyCode::Char('o'), CTRL, false)],
    },
    KeyRow {
        action: ClearScrollback,
        group: KeyGroup::App,
        chords_mac: &["ctrl+l"],
        chords_other: &["ctrl+l"],
        ctx: CtxNote::Always,
        desc_en: "clear scrollback",
        desc_zh: "清屏",
        probes: &[p(KeyCode::Char('l'), CTRL, false)],
    },
    KeyRow {
        action: TabComplete,
        group: KeyGroup::App,
        chords_mac: &["tab"],
        chords_other: &["tab"],
        ctx: CtxNote::Always,
        desc_en: "complete /commands",
        desc_zh: "补全 / 命令",
        probes: &[p(KeyCode::Tab, NONE, false)],
    },
    KeyRow {
        action: AttachClipboard,
        group: KeyGroup::App,
        chords_mac: &["ctrl+v"],
        chords_other: &["ctrl+v"],
        ctx: CtxNote::Always,
        desc_en: "attach the clipboard image",
        desc_zh: "粘贴剪贴板图片",
        probes: &[p(KeyCode::Char('v'), CTRL, false)],
    },
];

pub const MOUSE_ROWS: &[MouseRow] = &[
    MouseRow {
        chords: &["click"],
        desc_en: "expand/collapse a tool · wheel scrolls",
        desc_zh: "点击展开/折叠工具 · 滚轮滚动",
    },
    MouseRow {
        chords: &["drag"],
        desc_en: "select text · copies on release",
        desc_zh: "拖动选择文本，松开即复制",
    },
    MouseRow {
        chords: &["2×click"],
        desc_en: "copy a word",
        desc_zh: "双击复制单词",
    },
    MouseRow {
        chords: &["shift+drag"],
        desc_en: "terminal-native selection",
        desc_zh: "终端原生选择",
    },
];

/// Render the `/keys` map as a single-column markdown list: one `### group`
/// heading, then one `- chords [ctx] · description` item per binding. The
/// transcript renders it through the markdown pipeline like `/help`.
pub fn keys_markdown(zh: bool, mac: bool) -> String {
    let mut out = String::new();
    out.push_str(if zh {
        "## 快捷键\n\n双用途键：`[空输入时]` 滚动，`[输入时]` 编辑。\n\n"
    } else {
        "## keys — shortcut map\n\nDual-use keys: `[empty]` scrolls, `[typing]` edits.\n\n"
    });
    let mut last_group: Option<KeyGroup> = None;
    for row in KEY_ROWS {
        if last_group != Some(row.group) {
            if last_group.is_some() {
                out.push('\n');
            }
            out.push_str(&format!("### {}\n\n", row.group.title(zh)));
            last_group = Some(row.group);
        }
        let ctx = row.ctx.tag(zh);
        let desc = if zh { row.desc_zh } else { row.desc_en };
        let chords = row.chords(mac).join(" / ");
        // One list item per binding: `chords [ctx] · description`.
        let ctx_suffix = if ctx.is_empty() {
            String::new()
        } else {
            format!(" {ctx}")
        };
        out.push_str(&format!("- {chords}{ctx_suffix} · {desc}\n"));
    }
    out.push('\n');
    out.push_str(&format!("### {}\n\n", KeyGroup::Mouse.title(zh)));
    for row in MOUSE_ROWS {
        out.push_str(&format!(
            "- {} · {}\n",
            row.chords.join(" / "),
            if zh { row.desc_zh } else { row.desc_en }
        ));
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/input__keymap__tests.rs"]
mod tests;
