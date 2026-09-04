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
    /// UI language for the validation errors the form renders. The App sets
    /// it right after construction; it defaults to the builtin `En`.
    pub locale: crate::locale::Locale,
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
            locale: crate::locale::Locale::default(),
        }
    }

    /// Whether the focused field owns a text editor (typed answer or an
    /// inline `Other` input). Text fields keep their cursor keys.
    pub fn current_is_text(&self) -> bool {
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
        use crate::locale::Locale::En;
        let tr = |en: &'static str, zh: &'static str| {
            if self.locale == En {
                en
            } else {
                zh
            }
        };
        let Some(state) = self.fields.get(self.index) else {
            return Err(tr("form has no current field", "表单没有当前字段").into());
        };
        match &state.field.kind {
            ElicitationFieldKind::Text { .. } => {
                if state.field.required && state.input.buf.trim().is_empty() {
                    return Err(tr(
                        "Enter a value or use Tab to skip an optional field.",
                        "输入内容，可选字段可用 Tab 跳过。",
                    )
                    .into());
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
                    .map_err(|_| tr("Enter a whole number.", "请输入整数。").to_string())?;
                if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
                    return Err(tr(
                        "The number is outside the allowed range.",
                        "数字超出允许范围。",
                    )
                    .into());
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
                    .map_err(|_| tr("Enter a number.", "请输入数字。").to_string())?;
                if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
                    return Err(tr(
                        "The number is outside the allowed range.",
                        "数字超出允许范围。",
                    )
                    .into());
                }
            }
            ElicitationFieldKind::Single { options, .. } => {
                let Some(option) = options.get(state.cursor) else {
                    return Err(tr("Choose an option.", "请选择一个选项。").into());
                };
                if option.custom && state.input.buf.trim().is_empty() {
                    return Err(tr(
                        "Type the Other answer, then press Enter.",
                        "输入「其他」答案后按 Enter。",
                    )
                    .into());
                }
            }
            ElicitationFieldKind::Multi {
                options, min, max, ..
            } => {
                let count = state.selected.iter().filter(|selected| **selected).count();
                let minimum = (*min).max(usize::from(state.field.required));
                if count < minimum {
                    return Err(self.locale.trf(
                        "Select at least {} option(s).",
                        "至少选择 {} 个选项。",
                        &[minimum.to_string()],
                    ));
                }
                if max.is_some_and(|max| count > max) {
                    return Err(self.locale.trf(
                        "Select at most {} option(s).",
                        "最多选择 {} 个选项。",
                        &[max.unwrap_or(0).to_string()],
                    ));
                }
                if options
                    .iter()
                    .enumerate()
                    .any(|(index, option)| option.custom && state.selected[index])
                    && state.input.buf.trim().is_empty()
                {
                    return Err(tr(
                        "Type the Other answer, then press Enter.",
                        "输入「其他」答案后按 Enter。",
                    )
                    .into());
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
            self.error = Some(
                self.locale
                    .tr("This field is required.", "此字段为必填项。")
                    .into(),
            );
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
            || (key.code == KeyCode::Char('c')
                && key
                    .modifiers
                    .contains(KeyModifiers::CONTROL)
                && !key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT))
        {
            // Only plain ctrl+c cancels the form; ctrl+shift+c (copy
            // selection) and other chorded variants must not wipe input.
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
                        // An agent may legally send a multi-select with zero
                        // options; there is nothing to toggle then.
                        if let Some(selected) = state.selected.get_mut(state.cursor) {
                            *selected = !*selected;
                        }
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
#[path = "../tests/unit/elicitation__tests.rs"]
mod tests;
