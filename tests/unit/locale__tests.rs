use crate::locale::Locale;

#[test]
fn zh_command_desc_covers_every_builtin_and_plugin_command() {
    let zh = Locale::Zh;
    // Builtin chrome commands advertised in the slash menu.
    assert_eq!(zh.command_desc("vim", ""), "切换 vim 模式编辑（默认关闭）");
    assert_eq!(zh.command_desc("ui", ""), "切换 UI 插件");
    assert_eq!(zh.command_desc("plugins", ""), "查看 Host 插件状态（只读）");
    assert_eq!(
        zh.command_desc("cordis-plugins", ""),
        "查看或管理动态 Cordis 插件"
    );
    // Built-in Client Plugin commands keep their authored text in English.
    assert_eq!(zh.command_desc("unknown", "fallback"), "fallback");
    assert_eq!(Locale::En.command_desc("plugins", "fallback"), "fallback");

    // Built-in Client Plugin commands (`tuiCommands`) localize at render time.
    assert_eq!(
        zh.plugin_command_desc("agents", "Toggle the Agent panel (on/off)"),
        "切换 Agent 面板（on/off）"
    );
    assert_eq!(
        zh.plugin_command_desc("status", "Session run state and key stats"),
        "显示会话运行状态与关键统计"
    );
    assert_eq!(
        zh.plugin_command_desc("plan-view", "Open the current ACP plan"),
        "打开当前 ACP 计划"
    );
    assert_eq!(
        zh.plugin_command_desc(
            "harness",
            "Switch Harness now and start a new session"
        ),
        "立即切换 Harness 并启动新会话"
    );
    // Unknown plugin commands keep their authored description.
    assert_eq!(
        zh.plugin_command_desc("mystery", "Some plugin text"),
        "Some plugin text"
    );
    assert_eq!(
        Locale::En.plugin_command_desc("agents", "Toggle the Agent panel (on/off)"),
        "Toggle the Agent panel (on/off)"
    );
}
