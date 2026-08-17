//! Standard ACP form elicitation translated into a terminal-friendly model.

use std::collections::{BTreeMap, BTreeSet};

use agent_client_protocol::schema::v1::{
    CreateElicitationRequest, ElicitationMode, ElicitationPropertySchema, MultiSelectItems,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::editor::Input;

#[derive(Debug, Clone, PartialEq)]
pub struct ElicitationForm {
    pub message: String,
    pub fields: Vec<ElicitationField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElicitationField {
    pub name: String,
    pub custom_name: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: ElicitationFieldKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElicitationFieldKind {
    Text {
        default: Option<String>,
    },
    Integer {
        default: Option<i64>,
        min: Option<i64>,
        max: Option<i64>,
    },
    Number {
        default: Option<f64>,
        min: Option<f64>,
        max: Option<f64>,
    },
    Boolean {
        default: Option<bool>,
    },
    Single {
        options: Vec<ElicitationOption>,
        default: Option<String>,
    },
    Multi {
        options: Vec<ElicitationOption>,
        default: Vec<String>,
        min: usize,
        max: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitationOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElicitationValue {
    String(String),
    Integer(i64),
    Number(f64),
    Boolean(bool),
    StringArray(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElicitationReply {
    Accepted(BTreeMap<String, ElicitationValue>),
    Cancelled,
}

pub struct ElicitationFieldState {
    pub field: ElicitationField,
    pub cursor: usize,
    pub selected: Vec<bool>,
    pub input: Input,
    pub editing_custom: bool,
    pub answered: bool,
    pub skipped: bool,
}

pub struct ElicitationFormState {
    pub message: String,
    pub index: usize,
    pub fields: Vec<ElicitationFieldState>,
    pub error: Option<String>,
}

impl ElicitationFormState {
    pub fn new(form: ElicitationForm) -> Self {
        let fields = form
            .fields
            .into_iter()
            .map(|field| {
                let option_count = match &field.kind {
                    ElicitationFieldKind::Single { options, .. }
                    | ElicitationFieldKind::Multi { options, .. } => options.len(),
                    ElicitationFieldKind::Boolean { .. } => 2,
                    _ => 0,
                };
                let mut input = Input::new();
                let cursor = match &field.kind {
                    ElicitationFieldKind::Text { default } => {
                        if let Some(value) = default {
                            input.set(value.clone());
                        }
                        0
                    }
                    ElicitationFieldKind::Integer { default, .. } => {
                        if let Some(value) = default {
                            input.set(value.to_string());
                        }
                        0
                    }
                    ElicitationFieldKind::Number { default, .. } => {
                        if let Some(value) = default {
                            input.set(value.to_string());
                        }
                        0
                    }
                    ElicitationFieldKind::Single { options, default } => default
                        .as_ref()
                        .and_then(|value| options.iter().position(|option| &option.value == value))
                        .unwrap_or(0),
                    ElicitationFieldKind::Boolean {
                        default: Some(false),
                    } => 1,
                    _ => 0,
                };
                let mut selected = vec![false; option_count];
                match &field.kind {
                    ElicitationFieldKind::Multi {
                        options, default, ..
                    } => {
                        for (index, option) in options.iter().enumerate() {
                            selected[index] = default.contains(&option.value);
                        }
                    }
                    ElicitationFieldKind::Boolean { default } => {
                        if let Some(value) = default {
                            selected[usize::from(!value)] = true;
                        }
                    }
                    _ => {}
                }
                ElicitationFieldState {
                    field,
                    cursor,
                    selected,
                    input,
                    editing_custom: false,
                    answered: false,
                    skipped: false,
                }
            })
            .collect();
        Self {
            message: form.message,
            index: 0,
            fields,
            error: None,
        }
    }

    fn current_is_text(&self) -> bool {
        self.fields.get(self.index).is_some_and(|state| {
            matches!(
                state.field.kind,
                ElicitationFieldKind::Text { .. }
                    | ElicitationFieldKind::Integer { .. }
                    | ElicitationFieldKind::Number { .. }
            ) || state.editing_custom
        })
    }

    fn current_custom(&self) -> bool {
        self.fields
            .get(self.index)
            .is_some_and(|state| match &state.field.kind {
                ElicitationFieldKind::Single { options, .. }
                | ElicitationFieldKind::Multi { options, .. } => options
                    .get(state.cursor)
                    .is_some_and(|option| option.custom),
                _ => false,
            })
    }

    fn validate_current(&self) -> Result<(), String> {
        let Some(state) = self.fields.get(self.index) else {
            return Err("form has no current field".into());
        };
        match &state.field.kind {
            ElicitationFieldKind::Text { .. } => {
                if state.field.required && state.input.buf.trim().is_empty() {
                    return Err("Enter a value or use Tab to skip an optional field.".into());
                }
            }
            ElicitationFieldKind::Integer { min, max, .. } => {
                if state.input.buf.trim().is_empty() && !state.field.required {
                    return Ok(());
                }
                let value: i64 = state
                    .input
                    .buf
                    .trim()
                    .parse()
                    .map_err(|_| "Enter a whole number.".to_string())?;
                if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
                    return Err("The number is outside the allowed range.".into());
                }
            }
            ElicitationFieldKind::Number { min, max, .. } => {
                if state.input.buf.trim().is_empty() && !state.field.required {
                    return Ok(());
                }
                let value: f64 = state
                    .input
                    .buf
                    .trim()
                    .parse()
                    .map_err(|_| "Enter a number.".to_string())?;
                if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
                    return Err("The number is outside the allowed range.".into());
                }
            }
            ElicitationFieldKind::Single { options, .. } => {
                let Some(option) = options.get(state.cursor) else {
                    return Err("Choose an option.".into());
                };
                if option.custom && state.input.buf.trim().is_empty() {
                    return Err("Type the Other answer, then press Enter.".into());
                }
            }
            ElicitationFieldKind::Multi {
                options, min, max, ..
            } => {
                let count = state.selected.iter().filter(|selected| **selected).count();
                let minimum = (*min).max(usize::from(state.field.required));
                if count < minimum {
                    return Err(format!("Select at least {minimum} option(s)."));
                }
                if max.is_some_and(|max| count > max) {
                    return Err(format!("Select at most {} option(s).", max.unwrap_or(0)));
                }
                if options
                    .iter()
                    .enumerate()
                    .any(|(index, option)| option.custom && state.selected[index])
                    && state.input.buf.trim().is_empty()
                {
                    return Err("Type the Other answer, then press Enter.".into());
                }
            }
            ElicitationFieldKind::Boolean { .. } => {}
        }
        Ok(())
    }

    fn collect(&self) -> ElicitationReply {
        let mut content = BTreeMap::new();
        for state in &self.fields {
            if !state.answered || state.skipped {
                continue;
            }
            match &state.field.kind {
                ElicitationFieldKind::Text { .. } => {
                    let value = state.input.buf.trim();
                    if !value.is_empty() {
                        content.insert(
                            state.field.name.clone(),
                            ElicitationValue::String(value.into()),
                        );
                    }
                }
                ElicitationFieldKind::Integer { .. } => {
                    if let Ok(value) = state.input.buf.trim().parse::<i64>() {
                        content.insert(state.field.name.clone(), ElicitationValue::Integer(value));
                    }
                }
                ElicitationFieldKind::Number { .. } => {
                    if let Ok(value) = state.input.buf.trim().parse::<f64>() {
                        content.insert(state.field.name.clone(), ElicitationValue::Number(value));
                    }
                }
                ElicitationFieldKind::Boolean { .. } => {
                    content.insert(
                        state.field.name.clone(),
                        ElicitationValue::Boolean(state.selected[0]),
                    );
                }
                ElicitationFieldKind::Single { options, .. } => {
                    if let Some(option) = options.get(state.cursor) {
                        content.insert(
                            state.field.name.clone(),
                            ElicitationValue::String(option.value.clone()),
                        );
                        if option.custom {
                            if let Some(name) = &state.field.custom_name {
                                content.insert(
                                    name.clone(),
                                    ElicitationValue::String(state.input.buf.trim().into()),
                                );
                            }
                        }
                    }
                }
                ElicitationFieldKind::Multi { options, .. } => {
                    let values = options
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| state.selected[*index])
                        .map(|(_, option)| option.value.clone())
                        .collect();
                    content.insert(
                        state.field.name.clone(),
                        ElicitationValue::StringArray(values),
                    );
                    if let Some(name) = &state.field.custom_name {
                        let custom_selected = options
                            .iter()
                            .enumerate()
                            .any(|(index, option)| option.custom && state.selected[index]);
                        if custom_selected {
                            content.insert(
                                name.clone(),
                                ElicitationValue::String(state.input.buf.trim().into()),
                            );
                        }
                    }
                }
            }
        }
        ElicitationReply::Accepted(content)
    }

    fn complete_current(&mut self) -> Option<ElicitationReply> {
        if let Err(error) = self.validate_current() {
            self.error = Some(error);
            return None;
        }
        self.error = None;
        if let Some(state) = self.fields.get_mut(self.index) {
            state.answered = true;
            state.skipped = false;
            state.editing_custom = false;
        }
        if self.index + 1 < self.fields.len() {
            self.index += 1;
            None
        } else {
            Some(self.collect())
        }
    }

    fn skip_current(&mut self) -> Option<ElicitationReply> {
        let Some(state) = self.fields.get_mut(self.index) else {
            return None;
        };
        if state.field.required {
            self.error = Some("This field is required.".into());
            return None;
        }
        state.skipped = true;
        state.answered = true;
        state.editing_custom = false;
        self.error = None;
        if self.index + 1 < self.fields.len() {
            self.index += 1;
            None
        } else {
            Some(self.collect())
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ElicitationReply> {
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return Some(ElicitationReply::Cancelled);
        }
        if key.code == KeyCode::BackTab {
            self.index = self.index.saturating_sub(1);
            self.error = None;
            return None;
        }
        if key.code == KeyCode::Tab {
            return self.skip_current();
        }

        if self.current_is_text() {
            let state = &mut self.fields[self.index];
            match key.code {
                KeyCode::Enter => return self.complete_current(),
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
                {
                    state.input.insert(ch)
                }
                KeyCode::Backspace => state.input.backspace(),
                KeyCode::Delete => state.input.delete_forward(),
                KeyCode::Left => state.input.cursor = state.input.cursor.saturating_sub(1),
                KeyCode::Right => {
                    state.input.cursor = (state.input.cursor + 1).min(state.input.len_chars())
                }
                KeyCode::Home => state.input.cursor = 0,
                KeyCode::End => state.input.cursor = state.input.len_chars(),
                _ => {}
            }
            self.error = None;
            return None;
        }

        let option_count = match &self.fields[self.index].field.kind {
            ElicitationFieldKind::Single { options, .. }
            | ElicitationFieldKind::Multi { options, .. } => options.len(),
            ElicitationFieldKind::Boolean { .. } => 2,
            _ => 0,
        };
        match key.code {
            KeyCode::Up if option_count > 0 => {
                let state = &mut self.fields[self.index];
                state.cursor = state.cursor.checked_sub(1).unwrap_or(option_count - 1);
            }
            KeyCode::Down if option_count > 0 => {
                let state = &mut self.fields[self.index];
                state.cursor = (state.cursor + 1) % option_count;
            }
            KeyCode::Char(' ') => {
                let state = &mut self.fields[self.index];
                match state.field.kind {
                    ElicitationFieldKind::Multi { .. } => {
                        state.selected[state.cursor] = !state.selected[state.cursor];
                    }
                    ElicitationFieldKind::Boolean { .. } => {
                        state.selected.fill(false);
                        state.selected[state.cursor] = true;
                    }
                    _ => {}
                }
            }
            KeyCode::Char(ch)
                if self.current_custom()
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
            {
                let state = &mut self.fields[self.index];
                if matches!(state.field.kind, ElicitationFieldKind::Multi { .. }) {
                    state.selected[state.cursor] = true;
                }
                state.editing_custom = true;
                state.input.insert(ch);
            }
            KeyCode::Enter => {
                if self.current_custom() {
                    let state = &mut self.fields[self.index];
                    if matches!(state.field.kind, ElicitationFieldKind::Multi { .. }) {
                        state.selected[state.cursor] = true;
                    }
                    if state.input.buf.trim().is_empty() {
                        state.editing_custom = true;
                        self.error = None;
                        return None;
                    }
                }
                if matches!(
                    self.fields[self.index].field.kind,
                    ElicitationFieldKind::Boolean { .. }
                ) {
                    let state = &mut self.fields[self.index];
                    state.selected.fill(false);
                    state.selected[state.cursor] = true;
                }
                return self.complete_current();
            }
            _ => {}
        }
        self.error = None;
        None
    }

    pub fn paste(&mut self, text: &str) {
        let custom = self.current_custom();
        if self.current_is_text() || custom {
            let state = &mut self.fields[self.index];
            if custom && matches!(state.field.kind, ElicitationFieldKind::Multi { .. }) {
                state.selected[state.cursor] = true;
            }
            state.editing_custom |= custom;
            state.input.insert_str(&text.replace('\n', " "));
            self.error = None;
        }
    }
}

fn string_options(
    schema: &agent_client_protocol::schema::v1::StringPropertySchema,
) -> Vec<ElicitationOption> {
    if let Some(options) = &schema.one_of {
        return options
            .iter()
            .map(|option| ElicitationOption {
                value: option.value.clone(),
                label: option.title.clone(),
                description: option.description.clone(),
                custom: false,
            })
            .collect();
    }
    schema
        .enum_values
        .as_ref()
        .map(|values| {
            values
                .iter()
                .map(|value| ElicitationOption {
                    value: value.clone(),
                    label: value.clone(),
                    description: None,
                    custom: false,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn multi_options(items: &MultiSelectItems) -> Result<Vec<ElicitationOption>, String> {
    match items {
        MultiSelectItems::String(items) => Ok(items
            .values
            .iter()
            .map(|value| ElicitationOption {
                value: value.clone(),
                label: value.clone(),
                description: None,
                custom: false,
            })
            .collect()),
        MultiSelectItems::Titled(items) => Ok(items
            .options
            .iter()
            .map(|option| ElicitationOption {
                value: option.value.clone(),
                label: option.title.clone(),
                description: option.description.clone(),
                custom: false,
            })
            .collect()),
        MultiSelectItems::Other(_) => Err("unsupported elicitation multi-select item type".into()),
        _ => Err("unsupported elicitation multi-select item type".into()),
    }
}

pub fn form_from_request(request: &CreateElicitationRequest) -> Result<ElicitationForm, String> {
    let ElicitationMode::Form(mode) = &request.mode else {
        return Err("only form elicitation is supported".into());
    };
    let required: BTreeSet<&str> = mode
        .requested_schema
        .required
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();

    // DSH exposes “Other” as a standard enum value plus a sibling free-text
    // property. Fold those two schema properties into one terminal control.
    let mut paired_custom = BTreeMap::<String, (String, String)>::new();
    for (name, property) in &mode.requested_schema.properties {
        let options = match property {
            ElicitationPropertySchema::String(schema) => string_options(schema),
            ElicitationPropertySchema::Array(schema) => multi_options(&schema.items)?,
            _ => Vec::new(),
        };
        let Some(other) = options.iter().find(|option| option.label == "Other") else {
            continue;
        };
        let custom_name = format!("{name}_custom");
        let plain_custom = matches!(
            mode.requested_schema.properties.get(&custom_name),
            Some(ElicitationPropertySchema::String(schema))
                if schema.one_of.is_none() && schema.enum_values.is_none()
        );
        if plain_custom {
            paired_custom.insert(name.clone(), (custom_name, other.value.clone()));
        }
    }
    let hidden: BTreeSet<&str> = paired_custom
        .values()
        .map(|(name, _)| name.as_str())
        .collect();

    let mut fields = Vec::new();
    for (name, property) in &mode.requested_schema.properties {
        if hidden.contains(name.as_str()) {
            continue;
        }
        let paired = paired_custom.get(name);
        let (title, description, kind) = match property {
            ElicitationPropertySchema::String(schema) => {
                let mut options = string_options(schema);
                if options.is_empty() {
                    (
                        schema.title.clone().unwrap_or_else(|| name.clone()),
                        schema.description.clone(),
                        ElicitationFieldKind::Text {
                            default: schema.default.clone(),
                        },
                    )
                } else {
                    if let Some((_, custom_value)) = paired {
                        for option in &mut options {
                            option.custom = option.value == *custom_value;
                        }
                    }
                    (
                        schema.title.clone().unwrap_or_else(|| name.clone()),
                        schema.description.clone(),
                        ElicitationFieldKind::Single {
                            options,
                            default: schema.default.clone(),
                        },
                    )
                }
            }
            ElicitationPropertySchema::Array(schema) => {
                let mut options = multi_options(&schema.items)?;
                if let Some((_, custom_value)) = paired {
                    for option in &mut options {
                        option.custom = option.value == *custom_value;
                    }
                }
                (
                    schema.title.clone().unwrap_or_else(|| name.clone()),
                    schema.description.clone(),
                    ElicitationFieldKind::Multi {
                        options,
                        default: schema.default.clone().unwrap_or_default(),
                        min: schema.min_items.unwrap_or(0) as usize,
                        max: schema.max_items.map(|value| value as usize),
                    },
                )
            }
            ElicitationPropertySchema::Integer(schema) => (
                schema.title.clone().unwrap_or_else(|| name.clone()),
                schema.description.clone(),
                ElicitationFieldKind::Integer {
                    default: schema.default,
                    min: schema.minimum,
                    max: schema.maximum,
                },
            ),
            ElicitationPropertySchema::Number(schema) => (
                schema.title.clone().unwrap_or_else(|| name.clone()),
                schema.description.clone(),
                ElicitationFieldKind::Number {
                    default: schema.default,
                    min: schema.minimum,
                    max: schema.maximum,
                },
            ),
            ElicitationPropertySchema::Boolean(schema) => (
                schema.title.clone().unwrap_or_else(|| name.clone()),
                schema.description.clone(),
                ElicitationFieldKind::Boolean {
                    default: schema.default,
                },
            ),
            ElicitationPropertySchema::Other(_) => {
                return Err(format!("unsupported elicitation property: {name}"));
            }
            _ => return Err(format!("unsupported elicitation property: {name}")),
        };
        fields.push(ElicitationField {
            name: name.clone(),
            custom_name: paired.map(|(custom_name, _)| custom_name.clone()),
            title,
            description,
            required: required.contains(name.as_str()),
            kind,
        });
    }
    if fields.is_empty() {
        return Err("elicitation form has no supported fields".into());
    }
    Ok(ElicitationForm {
        message: request.message.clone(),
        fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;

    #[test]
    fn dsh_question_schema_becomes_one_field_with_inline_other_input() {
        let request: CreateElicitationRequest = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "s1",
            "message": "The agent needs your input.",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "question_0": {
                        "type": "string",
                        "title": "Target",
                        "description": "Where should this run?",
                        "oneOf": [
                            { "const": "option_0", "title": "Local" },
                            { "const": "option_1", "title": "Remote" },
                            { "const": "custom_0", "title": "Other" }
                        ]
                    },
                    "question_0_custom": {
                        "type": "string",
                        "title": "Other",
                        "description": "Type a custom answer."
                    }
                },
                "required": ["question_0"]
            }
        }))
        .expect("standard ACP form");

        let form = form_from_request(&request).expect("supported form");

        assert_eq!(form.message, "The agent needs your input.");
        assert_eq!(
            form.fields.len(),
            1,
            "Other is part of the choice, not a second field"
        );
        assert_eq!(form.fields[0].name, "question_0");
        assert_eq!(
            form.fields[0].custom_name.as_deref(),
            Some("question_0_custom")
        );
        assert!(form.fields[0].required);
        let ElicitationFieldKind::Single { options, .. } = &form.fields[0].kind else {
            panic!("expected single select")
        };
        assert_eq!(
            options
                .iter()
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>(),
            ["Local", "Remote", "Other"]
        );
        assert!(options[2].custom);
    }

    fn single_form() -> ElicitationForm {
        ElicitationForm {
            message: "Choose".into(),
            fields: vec![ElicitationField {
                name: "question_0".into(),
                custom_name: None,
                title: "Target".into(),
                description: Some("Where?".into()),
                required: true,
                kind: ElicitationFieldKind::Single {
                    options: vec![
                        ElicitationOption {
                            value: "local".into(),
                            label: "Local".into(),
                            description: None,
                            custom: false,
                        },
                        ElicitationOption {
                            value: "remote".into(),
                            label: "Remote".into(),
                            description: None,
                            custom: false,
                        },
                    ],
                    default: None,
                },
            }],
        }
    }

    fn boolean_form(default: Option<bool>) -> ElicitationForm {
        ElicitationForm {
            message: "Confirm".into(),
            fields: vec![ElicitationField {
                name: "confirmed".into(),
                custom_name: None,
                title: "Continue?".into(),
                description: None,
                required: true,
                kind: ElicitationFieldKind::Boolean { default },
            }],
        }
    }

    #[test]
    fn single_select_down_then_enter_accepts_the_selected_value() {
        let mut form = ElicitationFormState::new(single_form());

        assert_eq!(
            form.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            None
        );
        let reply = form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            reply,
            Some(ElicitationReply::Accepted(BTreeMap::from([(
                "question_0".into(),
                ElicitationValue::String("remote".into())
            ),])))
        );
    }

    #[test]
    fn escape_cancels_the_whole_form() {
        let mut form = ElicitationFormState::new(single_form());

        assert_eq!(
            form.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(ElicitationReply::Cancelled),
        );
    }

    #[test]
    fn boolean_default_can_be_changed_with_the_same_radio_keys_as_choices() {
        let mut form = ElicitationFormState::new(boolean_form(Some(false)));

        assert_eq!(form.fields[0].cursor, 1, "the false default focuses No");
        assert_eq!(
            form.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            None
        );
        let reply = form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            reply,
            Some(ElicitationReply::Accepted(BTreeMap::from([(
                "confirmed".into(),
                ElicitationValue::Boolean(true)
            ),])))
        );
    }

    #[test]
    fn other_choice_collects_inline_text_and_returns_both_schema_properties() {
        let mut spec = single_form();
        spec.fields[0].custom_name = Some("question_0_custom".into());
        let ElicitationFieldKind::Single { options, .. } = &mut spec.fields[0].kind else {
            unreachable!()
        };
        options.push(ElicitationOption {
            value: "other".into(),
            label: "Other".into(),
            description: None,
            custom: true,
        });
        let mut form = ElicitationFormState::new(spec);
        form.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        form.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        assert!(form.fields[0].editing_custom);
        for ch in "cluster-a".chars() {
            form.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        let reply = form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            reply,
            Some(ElicitationReply::Accepted(BTreeMap::from([
                (
                    "question_0".into(),
                    ElicitationValue::String("other".into())
                ),
                (
                    "question_0_custom".into(),
                    ElicitationValue::String("cluster-a".into())
                ),
            ])))
        );
    }
}
