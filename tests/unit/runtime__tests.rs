use super::*;

#[test]
fn launch_model_defaults_to_deepseek_flash() {
    assert_eq!(
        resolve_launch_model(None, None, None, "deepseek-official"),
        "deepseek-flash"
    );
}

#[test]
fn official_deepseek_routes_rewrite_retired_flash_ids() {
    for provider in ["", "deepseek", "deepseek-official"] {
        assert_eq!(
            canonical_model_id(provider, "deepseek-v4-flash"),
            "deepseek-flash",
            "{provider}"
        );
        assert_eq!(
            canonical_model_id(provider, "deepseek-v4-flash-vision-exp"),
            "deepseek-flash",
            "{provider}"
        );
    }
    assert_eq!(
        canonical_model_id("deepseek-official", "deepseek-official/deepseek-v4-flash"),
        "deepseek-official/deepseek-flash"
    );
}

#[test]
fn third_party_routes_keep_ids_their_catalog_still_lists() {
    assert_eq!(
        canonical_model_id("opencode", "deepseek-v4-flash"),
        "deepseek-v4-flash"
    );
    assert_eq!(
        canonical_model_id("opencode-go", "deepseek-v4-flash-vision-exp"),
        "deepseek-v4-flash-vision-exp"
    );
    assert_eq!(canonical_model_id("opencode", "kimi-k2.5"), "kimi-k2.5");
    assert_eq!(
        resolve_launch_model(None, None, Some("deepseek-v4-flash"), "opencode"),
        "deepseek-v4-flash"
    );
}

#[test]
fn moonshot_routes_rewrite_dropped_kimi_k2_ids() {
    for provider in ["moonshotai", "moonshotai-cn"] {
        for id in [
            "kimi-k2.5",
            "kimi-k2-0711-preview",
            "kimi-k2-0905-preview",
            "kimi-k2-thinking",
            "kimi-k2-thinking-turbo",
            "kimi-k2-turbo-preview",
        ] {
            assert_eq!(canonical_model_id(provider, id), "kimi-k2.6", "{provider} {id}");
        }
    }
    assert_eq!(
        canonical_model_id("moonshotai", "moonshotai/kimi-k2.5"),
        "moonshotai/kimi-k2.6"
    );
    assert_eq!(canonical_model_id("moonshotai", "kimi-k2.6"), "kimi-k2.6");
    assert_eq!(
        canonical_model_id("deepseek-official", "kimi-k2.5"),
        "kimi-k2.5"
    );
}

#[test]
fn explicit_and_env_models_outrank_a_saved_id() {
    assert_eq!(
        resolve_launch_model(
            Some("deepseek-v4-pro"),
            Some("deepseek-v4-flash"),
            Some("kimi-k2.5"),
            "deepseek-official",
        ),
        "deepseek-v4-pro"
    );
    assert_eq!(
        resolve_launch_model(None, Some("deepseek-v4-flash"), Some("deepseek-v4-pro"), "deepseek"),
        "deepseek-flash"
    );
}
