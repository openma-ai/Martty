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

#[test]
fn space_on_an_optionless_multi_select_is_a_noop_not_a_panic() {
    // ACP allows a multi-select property with an empty enum; Space toggles
    // selected[cursor], which must not index a zero-length vector.
    let mut form = ElicitationFormState::new(ElicitationForm {
        message: "Pick things".into(),
        fields: vec![ElicitationField {
            name: "choices".into(),
            custom_name: None,
            title: "Choices".into(),
            description: None,
            required: false,
            kind: ElicitationFieldKind::Multi {
                options: vec![],
                default: vec![],
                min: 0,
                max: None,
            },
        }],
    });

    assert_eq!(form.fields[0].selected.len(), 0);
    assert_eq!(
        form.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        None,
        "space with zero options must not panic"
    );
    let reply = form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        reply,
        Some(ElicitationReply::Accepted(BTreeMap::from([(
            "choices".into(),
            ElicitationValue::StringArray(vec![])
        )])))
    );
}
