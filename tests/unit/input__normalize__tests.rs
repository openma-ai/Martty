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
