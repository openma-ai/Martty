# TUI 文档

| 文档 | 内容 |
|---|---|
| [architecture.md](architecture.md) · [EN](architecture.en.md) | 分层、TTY 所有权、控制面与绘制面 |
| [plugins.md](plugins.md) · [EN](plugins.en.md) | 第三方插件可调用的 API；没有列出的能力宿主不提供 |
| [migration.md](migration.md) · [EN](migration.en.md) | Client 插件能力的迁移状态 |
| [tui-palette.v0.schema.json](tui-palette.v0.schema.json) | `tuiTheme.register` protocol `0` |
| [fixtures/demo-skin.v0.json](fixtures/demo-skin.v0.json) | 阶段 1 gallery 配色 `ember` |
| [tui-node.v0.schema.json](tui-node.v0.schema.json) | `tuiSlots` 节点协议；`chrome.right` 已开放 |
| [fixtures/demo-surface.v0.json](fixtures/demo-surface.v0.json) | TuiNode gallery 快照 |

实现必须服从 [plugins.md](plugins.md) 与已开放的 schema。换 compositor（Rust 或 JS）不得改插件字节。
