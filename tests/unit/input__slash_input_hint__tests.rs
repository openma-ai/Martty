//! Slash command argument hints: resolving the ghost for the typed command
//! and applying it as real text.

use crate::bus::{CommandConfigAction, SkillInfo};
use crate::input::slash_input_hint::{
    apply_slash_input_hint, resolve_slash_input_hint, resolve_slash_input_hint_in,
    SlashHintCommand, SlashInputHint,
};

fn skill(name: &str, hint: Option<&str>) -> SkillInfo {
    SkillInfo {
        name: name.into(),
        description: format!("{name} description"),
        input_hint: hint.map(str::to_string),
        config_action: None,
        client_command: false,
    }
}

fn review() -> SkillInfo {
    skill("review", Some("optional review instructions"))
}

fn review_branch() -> SkillInfo {
    skill("review-branch", Some("branch name"))
}

fn goal() -> SkillInfo {
    skill("goal", Some("[<objective>|clear|pause|resume]"))
}

fn adapt() -> SkillInfo {
    skill("adapt", Some("[target] [context (mobile, tablet, print...)]"))
}

fn compact() -> SkillInfo {
    skill("compact", None)
}

fn plan() -> SkillInfo {
    let mut command = skill("plan", Some("mode"));
    command.config_action = Some(CommandConfigAction {
        config_id: "collaboration_mode".into(),
        value: "plan".into(),
        reset_value: Some("default".into()),
    });
    command
}

fn commands() -> Vec<SkillInfo> {
    vec![
        review(),
        review_branch(),
        goal(),
        adapt(),
        compact(),
        plan(),
    ]
}

// --- resolving ------------------------------------------------------------

#[test]
fn resolves_the_hint_for_a_bare_command() {
    assert_eq!(
        resolve_slash_input_hint("/review", &commands(), false),
        Some(SlashInputHint {
            command_name: "review".into(),
            hint: "optional review instructions".into(),
            prefix: "/review".into(),
        })
    );
}

#[test]
fn resolves_the_hint_for_a_bare_command_with_a_trailing_space() {
    assert_eq!(
        resolve_slash_input_hint("/review ", &commands(), false),
        resolve_slash_input_hint("/review", &commands(), false)
    );
    assert_eq!(
        resolve_slash_input_hint("/review\t", &commands(), false),
        resolve_slash_input_hint("/review", &commands(), false)
    );
}

#[test]
fn resolves_hints_from_any_harness_that_publishes_one() {
    // Codex and Claude spell their hints differently; the composer reads
    // whatever the agent declared.
    let commands = commands();
    assert_eq!(
        resolve_slash_input_hint("/review-branch", &commands, false)
            .expect("hint")
            .hint,
        "branch name"
    );
    assert_eq!(
        resolve_slash_input_hint("/goal", &commands, false)
            .expect("hint")
            .hint,
        "[<objective>|clear|pause|resume]"
    );
    assert_eq!(
        resolve_slash_input_hint("/adapt", &commands, false)
            .expect("hint")
            .hint,
        "[target] [context (mobile, tablet, print...)]"
    );
}

#[test]
fn has_no_hint_once_the_user_started_the_argument() {
    let commands = commands();
    assert!(resolve_slash_input_hint("/review focus on tests", &commands, false).is_none());
    assert!(resolve_slash_input_hint("/review x", &commands, false).is_none());
}

#[test]
fn has_no_hint_for_a_command_that_declares_none() {
    let commands = commands();
    assert!(resolve_slash_input_hint("/compact", &commands, false).is_none());
    // A whitespace-only hint is not a hint.
    let blank = vec![skill("compact", Some("  "))];
    assert!(resolve_slash_input_hint("/compact", &blank, false).is_none());
}

#[test]
fn has_no_hint_for_an_unknown_command_or_plain_text() {
    let commands = commands();
    assert!(resolve_slash_input_hint("/unknown", &commands, false).is_none());
    assert!(resolve_slash_input_hint("review", &commands, false).is_none());
    assert!(resolve_slash_input_hint("", &commands, false).is_none());
    assert!(resolve_slash_input_hint("/", &commands, false).is_none());
    // A multi-line draft is not a bare command.
    assert!(resolve_slash_input_hint("/review\nmore", &commands, false).is_none());
}

#[test]
fn matches_the_catalogue_spelling_exactly() {
    let commands = commands();
    assert!(resolve_slash_input_hint("/Review", &commands, false).is_none());
    assert_eq!(
        resolve_slash_input_hint("/review-branch", &commands, false)
            .expect("hint")
            .command_name,
        "review-branch"
    );
}

#[test]
fn never_offers_the_hint_of_a_session_state_switch() {
    // `/plan` is a local config switch: it is complete without an argument.
    let commands = vec![plan()];
    assert!(resolve_slash_input_hint("/plan", &commands, false).is_none());
}

#[test]
fn is_empty_once_the_ghost_has_been_dismissed() {
    assert!(resolve_slash_input_hint("/review", &commands(), true).is_none());
}

#[test]
fn resolves_over_plugin_style_catalogue_entries() {
    let catalogue = [SlashHintCommand {
        name: "deploy",
        hint: Some("environment"),
        config_action: false,
    }];
    let hint = resolve_slash_input_hint_in("/deploy", &catalogue, false).expect("hint");
    assert_eq!(hint.hint, "environment");
    assert_eq!(hint.prefix, "/deploy");
    assert!(resolve_slash_input_hint_in("/deploy ", &catalogue, false).is_some());
    assert!(resolve_slash_input_hint_in("/deploy now", &catalogue, false).is_none());
}

// --- applying -------------------------------------------------------------

#[test]
fn completes_the_command_with_exactly_one_space() {
    let applied = apply_slash_input_hint("/review", "optional review instructions");
    assert_eq!(applied.text, "/review optional review instructions");
    assert_eq!(applied.selection_start, 8);
    assert_eq!(applied.selection_end, 36);
}

#[test]
fn collapses_the_space_the_user_already_typed() {
    assert_eq!(
        apply_slash_input_hint("/review ", "optional review instructions"),
        apply_slash_input_hint("/review", "optional review instructions")
    );
    assert_eq!(
        apply_slash_input_hint("/review   ", "optional review instructions"),
        apply_slash_input_hint("/review", "optional review instructions")
    );
}

#[test]
fn selects_the_inserted_hint_so_the_user_can_type_over_it() {
    let applied = apply_slash_input_hint("/review", "optional review instructions");
    let chars: Vec<char> = applied.text.chars().collect();
    assert_eq!(
        chars[applied.selection_start..applied.selection_end]
            .iter()
            .collect::<String>(),
        "optional review instructions"
    );
}

#[test]
fn is_the_shared_apply_path_for_tab_and_right_click() {
    // Both composer affordances must produce the same insert + selection.
    let via_tab = apply_slash_input_hint("/review ", "optional review instructions");
    let via_right_click = apply_slash_input_hint("/review ", "optional review instructions");
    assert_eq!(via_tab, via_right_click);
    assert_eq!(via_tab.text, "/review optional review instructions");
}

// --- ghost text -----------------------------------------------------------

#[test]
fn ghost_text_keeps_a_single_space_after_the_command() {
    let hint = resolve_slash_input_hint("/review", &commands(), false).expect("hint");
    assert_eq!(hint.ghost_text("/review"), " optional review instructions");
    assert_eq!(hint.ghost_text("/review "), "optional review instructions");
    assert_eq!(hint.ghost_text("/review\t"), "optional review instructions");
}
