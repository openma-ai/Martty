use super::*;

fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, m)
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SUPER: KeyModifiers = KeyModifiers::SUPER;

fn empty() -> KeyCtx {
    KeyCtx {
        input_empty: true,
        history_active: false,
    }
}

fn typing() -> KeyCtx {
    KeyCtx {
        input_empty: false,
        history_active: false,
    }
}

fn browsing_history() -> KeyCtx {
    KeyCtx {
        input_empty: false,
        history_active: true,
    }
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
fn alt_up_opens_the_client_queue_selector() {
    assert_eq!(
        classify(&key(KeyCode::Up, ALT), empty()),
        Some(Action::EditQueuedPrompt)
    );
    assert_eq!(
        classify(&key(KeyCode::Up, ALT), typing()),
        Some(Action::EditQueuedPrompt)
    );
}

#[test]
fn ctrl_a_moves_to_line_start_for_terminals_that_encode_cmd_left_as_readline() {
    assert_eq!(
        classify(&key(KeyCode::Char('a'), CTRL), typing()),
        Some(Action::LineStart)
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
fn ctrl_shift_a_keeps_agent_cycling_off_the_readline_ctrl_a_chord() {
    assert_eq!(
        classify(
            &key(KeyCode::Char('a'), CTRL | KeyModifiers::SHIFT),
            typing()
        ),
        Some(Action::CycleAgent)
    );
}

#[test]
fn agent_cycle_is_ctrl_shift_a_on_every_platform() {
    assert_eq!(
        classify(&key(KeyCode::Char('a'), ALT), typing()),
        None,
        "option+a is no longer a platform-specific alias"
    );
    assert_eq!(
        classify(&key(KeyCode::Char('a'), CTRL), typing()),
        Some(Action::LineStart)
    );
}

#[test]
fn arrows_move_inside_a_draft_and_keep_browsing_active_history() {
    assert_eq!(
        classify(&key(KeyCode::Up, NONE), typing()),
        Some(Action::CursorUp)
    );
    assert_eq!(
        classify(&key(KeyCode::Down, NONE), typing()),
        Some(Action::CursorDown)
    );
    assert_eq!(
        classify(&key(KeyCode::Up, NONE), empty()),
        Some(Action::HistoryPrev)
    );
    assert_eq!(
        classify(&key(KeyCode::Down, NONE), empty()),
        Some(Action::HistoryNext)
    );
    assert_eq!(
        classify(&key(KeyCode::Up, NONE), browsing_history()),
        Some(Action::HistoryPrev)
    );
    assert_eq!(
        classify(&key(KeyCode::Down, NONE), browsing_history()),
        Some(Action::HistoryNext)
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
fn ctrl_k_kills_to_line_end_even_on_an_empty_draft() {
    assert_eq!(
        classify(&key(KeyCode::Char('k'), CTRL), empty()),
        Some(Action::KillToEnd),
        "ctrl+k is free for the text editor on every draft state"
    );
    assert_eq!(
        classify(&key(KeyCode::Char('k'), CTRL), typing()),
        Some(Action::KillToEnd),
        "typing keeps the readline kill-to-end behavior"
    );
    assert_eq!(classify(&key(KeyCode::Char('.'), CTRL), empty()), None);
    assert_eq!(
        classify(&key(KeyCode::Char('k'), NONE), typing()),
        Some(Action::Insert('k'))
    );
}

#[test]
fn unbound_chords_do_not_type_their_base_letter() {
    assert_eq!(classify(&key(KeyCode::Char('z'), ALT), typing()), None);
    assert_eq!(classify(&key(KeyCode::Char('c'), SUPER), typing()), None);
    assert_eq!(classify(&key(KeyCode::Char('r'), CTRL), typing()), None);
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

#[test]
fn every_action_has_exactly_one_documented_row_except_typing_insert() {
    let all = [
        Action::Insert('x'),
        Action::Newline,
        Action::Enter,
        Action::Esc,
        Action::CtrlC,
        Action::Quit,
        Action::ClearScrollback,
        Action::ToggleTheme,
        Action::ToggleExpandAll,
        Action::SendNow,
        Action::EditQueuedPrompt,
        Action::AttachClipboard,
        Action::ModelPicker,
        Action::CycleAgent,
        Action::CyclePermission,
        Action::TabComplete,
        Action::HistoryPrev,
        Action::HistoryNext,
        Action::ScrollHalfUp,
        Action::ScrollHalfDown,
        Action::PageUp,
        Action::PageDown,
        Action::JumpTop,
        Action::JumpTail,
        Action::CursorLeft,
        Action::CursorRight,
        Action::CursorUp,
        Action::CursorDown,
        Action::WordLeft,
        Action::WordRight,
        Action::LineStart,
        Action::LineEnd,
        Action::Backspace,
        Action::DeleteForward,
        Action::DeleteWordBack,
        Action::KillToEnd,
        Action::KillToStart,
        Action::Undo,
        Action::Redo,
        Action::YankPaste,
        Action::KillLine,
        Action::SelectLeft,
        Action::SelectRight,
        Action::SelectUp,
        Action::SelectDown,
        Action::SelectWordLeft,
        Action::SelectWordRight,
        Action::SelectLineStart,
        Action::SelectLineEnd,
        Action::CopySelection,
        Action::CutSelection,
    ];
    let documented: Vec<Action> = KEY_ROWS.iter().map(|row| row.action).collect();
    assert_eq!(
        documented.len(),
        all.len() - 1,
        "exactly one row per action (Insert is plain typing and stays out)"
    );
    for action in all {
        if matches!(action, Action::Insert(_)) {
            continue;
        }
        assert!(
            documented.contains(&action),
            "{action:?} is missing from KEY_ROWS — /keys would not show it"
        );
    }
}

#[test]
fn displayed_chords_resolve_to_their_action() {
    for row in KEY_ROWS {
        for (code, mods, empty) in row.probes {
            let got = classify(
                &KeyEvent::new(*code, *mods),
                KeyCtx {
                    input_empty: *empty,
                    history_active: false,
                },
            );
            assert_eq!(
                got,
                Some(row.action),
                "KEY_ROWS drift: {:?} + {mods:?} must be {:?}, got {got:?}",
                code,
                row.action
            );
        }
    }
}
#[test]
fn keys_markdown_is_well_formed() {
    for zh in [false, true] {
        for mac in [false, true] {
            let md = keys_markdown(zh, mac);
            assert!(!md.is_empty());
            let mut saw_line = false;
            let mut in_group = false;
            for line in md.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("### ") {
                    in_group = true;
                    continue;
                }
                if !in_group || trimmed.is_empty() {
                    continue;
                }
                saw_line = true;
                assert!(
                    trimmed.contains(" · "),
                    "binding line must be `chords · description`: {line:?}\n{md}"
                );
                assert!(!trimmed.contains('|'), "no table cells: {line:?}");
            }
            assert!(saw_line, "no binding lines in:\n{md}");
        }
    }
}

#[test]
fn keys_markdown_renders_within_the_terminal_width() {
    let theme = crate::theme::Theme::dark();
    for width in [80usize, 120] {
        for zh in [false, true] {
            for mac in [false, true] {
                let md = keys_markdown(zh, mac);
                let lines = crate::markdown::render(&md, &theme, width);
                assert!(!lines.is_empty());
                for line in lines {
                    let w = line.width() as usize;
                    assert!(
                        w <= width,
                        "rendered keys line is {w} wide at {width} ({zh}, mac={mac}): {line:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn keys_markdown_uses_the_platform_spellings_and_groups_everything() {
    let mac = keys_markdown(false, true);
    assert!(mac.contains("⌘←"), "macOS chord missing:\n{mac}");
    assert!(
        mac.contains("ctrl+shift+a"),
        "unified agent chord missing on macOS:\n{mac}"
    );
    assert!(!mac.contains("option+a"), "old macOS alias leaked:\n{mac}");
    let linux = keys_markdown(false, false);
    assert!(linux.contains("ctrl+←"), "linux chord missing:\n{linux}");
    assert!(
        !linux.contains("⌘←"),
        "macOS chord leaked to linux:\n{linux}"
    );
    assert!(
        linux.contains("ctrl+shift+a"),
        "linux agent chord missing:\n{linux}"
    );
    for group in [
        KeyGroup::Send,
        KeyGroup::Navigate,
        KeyGroup::Composer,
        KeyGroup::App,
        KeyGroup::Mouse,
    ] {
        assert!(
            linux.contains(&group.title(false)),
            "group {:?} missing:\n{linux}",
            group
        );
    }
    let zh = keys_markdown(true, false);
    // The popup window border names the dialog; the body starts with the
    // intro sentence instead of a repeated `## 快捷键` heading.
    assert!(zh.contains("双用途键"), "zh intro missing:\n{zh}");
    assert!(zh.contains("[空输入时]"), "zh context tag missing:\n{zh}");
    assert!(zh.contains("发送"), "zh send group missing:\n{zh}");
    assert!(
        zh.contains("关闭 / 推荐"),
        "Esc completion hint missing:\n{zh}"
    );
    assert!(
        zh.contains("选择一条排队消息编辑"),
        "queue editor shortcut missing:\n{zh}"
    );
}

#[test]
fn keys_markdown_lists_ctrl_j_and_the_composer_section() {
    let en = crate::input::keymap::keys_markdown(false, false);
    assert!(en.contains("ctrl+j"), "en keys must list ctrl+j");
    assert!(en.contains("shift+enter / ctrl+j"), "newline row shows both chords");
    assert!(en.contains("composer textarea"), "dedicated textarea section");
    assert!(en.contains("ctrl+shift+k"), "kill-line row is present");
    let zh = crate::input::keymap::keys_markdown(true, false);
    assert!(zh.contains("ctrl+j"), "zh keys must list ctrl+j");
    assert!(zh.contains("输入框 · textarea"), "zh section title");
}
