//! Slash command argument hints (`AvailableCommand.input.hint`).
//!
//! An ACP agent can declare what it expects after a command name — Codex's
//! `/review` says "optional review instructions", the DSH harness's `/model`
//! says "[model] — blank lists available models". The composer shows such a
//! hint as muted ghost text right after the typed command and turns it into
//! real text on Tab (or on a right-click inside the composer).
//!
//! The helpers here are pure: they take the draft text plus the command
//! catalogue and return what to show and what to insert, so the app layer
//! only has to decide *when* to ask.

use crate::bus::SkillInfo;

/// A slash command's declared argument hint, resolved for the text the user
/// has typed so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashInputHint {
    /// The command name without the leading slash, exactly as the catalogue
    /// spells it (`review`, `review-branch`, `adapt`).
    pub command_name: String,
    /// The declared hint, trimmed. Empty hints are not hints.
    pub hint: String,
    /// The command token as typed, e.g. `/review`.
    pub prefix: String,
}

impl SlashInputHint {
    /// The ghost as it is painted after the typed command: a leading space
    /// only when the draft does not already end with whitespace.
    pub fn ghost_text(&self, draft: &str) -> String {
        if draft.chars().last().is_some_and(char::is_whitespace) {
            self.hint.clone()
        } else {
            format!(" {}", self.hint)
        }
    }
}

/// The result of completing a command with its hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashInputHintApplication {
    /// The new composer text.
    pub text: String,
    /// Char offset of the inserted hint's first character.
    pub selection_start: usize,
    /// Char offset just past the inserted hint.
    pub selection_end: usize,
}

/// One catalogue entry the resolver can match. The ACP-advertised commands
/// (`SkillInfo`) and the client-plugin commands share this shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashHintCommand<'a> {
    /// The command name, without the leading slash.
    pub name: &'a str,
    /// The declared argument hint, if the command has one.
    pub hint: Option<&'a str>,
    /// A session-state switch is complete without an argument.
    pub config_action: bool,
}

/// The typed text has to be the bare command for the hint to apply: `/name`
/// or `/name` plus trailing spaces. Anything else means the user has already
/// started the argument, and a ghost would lie about what is in the box.
///
/// Leading whitespace is not accepted: the slash picker does not open for it
/// either, so a ghost there would have no command to belong to.
fn bare_command(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('/')?;
    // The command token itself carries no whitespace (`\S+`).
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() {
        return None;
    }
    // Only spaces and tabs may trail it — a newline means the draft is a
    // multi-line message, not a bare command.
    if !rest[name_end..].chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }
    Some(name)
}

/// The hint a bare command declares, or `None` when there is none to show.
///
/// Mirrors the composer's pending-argument rule: a command is only waiting
/// for an argument when it declares one, and a session-state switch is
/// complete without one. On top of that the hint itself has to exist — a
/// command without a hint takes free text, it does not suggest one.
///
/// `dismissed` is the composer's own state: Escape hides the ghost, and the
/// caller asks for the resolved hint without it.
pub fn resolve_slash_input_hint_in(
    text: &str,
    commands: &[SlashHintCommand<'_>],
    dismissed: bool,
) -> Option<SlashInputHint> {
    if dismissed {
        return None;
    }
    let command_name = bare_command(text)?;
    // The catalogue owns the spelling: a case-insensitive match would offer a
    // hint for a command the agent never published under that name.
    let command = commands.iter().find(|c| c.name == command_name)?;
    // A config switch is complete without an argument, so it never waits for
    // the hint to be typed.
    if command.config_action {
        return None;
    }
    let hint = command.hint?.trim();
    if hint.is_empty() {
        return None;
    }
    Some(SlashInputHint {
        command_name: command_name.to_string(),
        hint: hint.to_string(),
        prefix: format!("/{command_name}"),
    })
}

/// The hint the agent advertised for the typed command, or `None` when there
/// is nothing to show.
pub fn resolve_slash_input_hint(
    text: &str,
    skills: &[SkillInfo],
    dismissed: bool,
) -> Option<SlashInputHint> {
    let commands: Vec<SlashHintCommand<'_>> = skills
        .iter()
        .map(|skill| SlashHintCommand {
            name: &skill.name,
            hint: skill.input_hint.as_deref(),
            config_action: skill.config_action.is_some(),
        })
        .collect();
    resolve_slash_input_hint_in(text, &commands, dismissed)
}

/// Write the hint into the composer as real text.
///
/// The command keeps exactly one space between its name and the hint, so
/// `/review` and `/review ` both complete to `/review optional review
/// instructions`. The inserted hint is selected: the user can type over it,
/// and Enter sends it as the argument.
pub fn apply_slash_input_hint(text: &str, hint: &str) -> SlashInputHintApplication {
    let prefix = text.trim_end_matches([' ', '\t']);
    let completed = format!("{prefix} {hint}");
    SlashInputHintApplication {
        selection_start: prefix.chars().count() + 1,
        selection_end: completed.chars().count(),
        text: completed,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/input__slash_input_hint__tests.rs"]
mod tests;
