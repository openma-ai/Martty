use super::*;

#[test]
fn empty_permission_options_cancel() {
    assert_eq!(
        permission_ask_empty_outcome(&[]),
        Some(PermissionAskReply::Cancelled)
    );
    assert_eq!(
        permission_ask_empty_outcome(&[PermissionAskOption {
            option_id: "allow".into(),
            kind: "allow_once".into(),
            name: "Allow once".into(),
        }]),
        None
    );
}

#[test]
fn permission_ask_defaults_to_allow_once() {
    let options = [
        PermissionAskOption {
            option_id: "reject".into(),
            kind: "reject_once".into(),
            name: "Reject".into(),
        },
        PermissionAskOption {
            option_id: "allow".into(),
            kind: "allow_once".into(),
            name: "Allow once".into(),
        },
    ];
    assert_eq!(permission_ask_default_sel(&options), 1);
    assert_eq!(permission_ask_default_sel(&options[..1]), 0);
}
