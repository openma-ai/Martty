//! Built-in TUI localization.
//!
//! ACP and plugin payloads remain authored by their owner. This module only
//! localizes client-owned chrome and built-in commands.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    #[default]
    En,
    Zh,
}

impl Locale {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "en" | "en-us" | "en_us" => Some(Self::En),
            "zh" | "zh-cn" | "zh_cn" | "cn" => Some(Self::Zh),
            _ => None,
        }
    }

    pub fn alternate(self) -> Self {
        match self {
            Self::En => Self::Zh,
            Self::Zh => Self::En,
        }
    }

    pub fn tr(self, en: &'static str, zh: &'static str) -> &'static str {
        match self {
            Self::En => en,
            Self::Zh => zh,
        }
    }

    pub fn command_desc(self, name: &str, fallback: &'static str) -> &'static str {
        if self == Self::En {
            return fallback;
        }
        match name {
            "help" => "显示帮助和使用提示",
            "keys" => "查看键盘快捷键",
            "new" => "开始一个新会话",
            "resume" => "恢复当前工作区的持久会话",
            "clear" => "清空对话滚动区",
            "model" => "通过 ACP 实时切换模型",
            "agent" => "切换 Agent 预设 · option+a",
            "effort" => "设置当前会话的推理强度",
            "permission" => "选择权限预设 · shift+tab 轮换",
            "plan" => "切换 Host 计划模式",
            "image" => "发送本地图片（png/jpeg/webp/gif）",
            "clip" => "附加剪贴板图片（macOS/Linux）",
            "theme" => "切换明暗模式或主题包",
            "plugins" => "停用或恢复动态插件",
            "session" => "显示会话和运行时信息",
            "auth" => "ACP 登录（Backchat authenticate）",
            "lang" => "切换界面语言",
            "logo" => "让鲸鱼重新出现",
            "liang" => "召唤小难梁 — 🤫 空闲 · ⌨︎ 工作中",
            "quit" => "退出 dsh-tui",
            _ => fallback,
        }
    }

    pub fn ambient_tip(self, index: usize) -> &'static str {
        const EN: [&str; 8] = [
            "esc interrupts a running turn — your draft survives",
            "type ! to run a command in the session's persistent local shell",
            "enter queues a follow-up; ctrl+x steers the active turn now",
            "click a tool to expand it · wheel always scrolls the conversation",
            "the footer under the composer shows token usage + cache hit rate",
            "answers render markdown: headings, code, links, and images",
            "/agent or option+a switches the agent preset",
            "/new starts a fresh session · /theme switches packs · ctrl+t toggles dark/light",
        ];
        const ZH: [&str; 8] = [
            "esc 可中断当前轮次，草稿会保留",
            "输入 ! 可直接运行本地命令，不经过 Agent",
            "enter 会排队后续消息；ctrl+x 立即 steer 当前轮次",
            "点击工具可展开 · 滚轮始终滚动对话",
            "输入框下方显示 token 用量和缓存命中率",
            "回答支持 Markdown：标题、代码、链接和图片",
            "/agent 或 option+a 可切换 Agent 预设",
            "/new 新建会话 · /theme 切换主题包 · ctrl+t 切换明暗模式",
        ];
        match self {
            Self::En => EN[index % EN.len()],
            Self::Zh => ZH[index % ZH.len()],
        }
    }
}

pub const AMBIENT_TIP_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocaleSettings {
    pub language: Locale,
}
