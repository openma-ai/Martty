# 迁移计划

目标：保持现有 TUI 体验，同时把扩展能力收敛到独立的 Cordis client 树；
ACP 负责 agent 控制面，私有 compositor 通道负责 Node → Rust 绘制数据。

## 已完成 — ACP client 控制面

- profile 主路径在 Host Base tree 挂 ACP plugin，并用标准 stdin/stdout 连接独立
  TUI Client 进程；standalone `acp-client` 仍可 spawn/attach 任意 ACP agent。
- Rust 使用官方 `agent-client-protocol` 处理 initialize、认证、session、prompt、
  permission、config option 与 update。
- Agent Cordis 树与 TUI Client Cordis 树位于不同进程，只通过 ACP 通信。
- 双方通过 initialize `_meta.dsh.cordis.protocol = 0` 协商；inspect/run/lifecycle
  使用 `_dsh/cordis/*`，painter 子能力使用 `_dsh/cordis/tui/*`。
- 所有自定义 wire 方法都是下划线前缀的 ACP Extension Request/Notification；
  theme/slots/commands/overlay 被 Node mux 截获，不进入 agent 流。
- `tui-cordis-client-runner` 是 client 树上的独立 sibling；shell 只绑定并转发 transport。

## 已完成 — Theme 插件 POC

- `tui-theme` 是真实 Cordis service 插件，提供 `ctx.tuiTheme`。
- 配色包作为 sibling plugin，`inject: ['tuiTheme']` 后调用 `register`。
- 18 个 token 名封闭；每个 dark/light pack 必须完整且只接受 `#RRGGBB`。
- Node 通过 `_dsh/cordis/tui/theme/update` notification 通知 Rust；插件看不到传输方法。
- `--demo` 保持内置 `default`，`--demo-skin` 挂载并激活 gallery `ember`。

## 已完成 — Creator skill overlay

- TUI 包内部的 Host overlay 在上游 `cordis` preset 的 standing scope 上注册
  `tui-plugin-development`，不复制或修改 Creator composition。
- skill 只对 Creator agent 可见；bundle 卸载后立即移除。
- skill 的装载不依赖 ACP；Client inspect/run 仍只负责运行期 TUI 能力与动态包。

## 已完成 — 根级右栏 slot POC

- `tui-slots` 是真实 Cordis service，声明 root/list `chrome.right`。
- 静态和动态插件都用 `inject` / `register` 贡献任意合法 `TuiNode` 树；
  `panel.update` 即时替换，fiber 卸载即撤销。
- Registry 聚合 contribution、命名空间 node id，并生成带单调 `rev` 的完整 snapshot。
- Node 经 `_dsh/cordis/tui/slots/update` notification 送给 Rust；Rust 校验并在独立右栏渲染。
- 最后一个 contribution 消失时主界面恢复全宽；窄终端只暂时隐藏右栏。
- Client inspect 的 `Slots.list` 让 Creator 能生成 `code.client`；gallery export
  `@openma/deepseek-harness-tui/right-demo` 不默认挂载。

验收：

```sh
make rust-test
cd npm && npm test
cd ../host && npm test
dsh-tui --demo
dsh-tui --demo-skin
```

## 下一步 — 对话槽

复用现有 `tuiSlots` service 和 [tui-node.v0.schema.json](tui-node.v0.schema.json)：

1. 壳声明 `conversation.chat`。
2. 独立插件使用 `tuiSlots.inject(slot, () => tuiSlots.register(...))`。
3. Registry 聚合 contribution，生成带单调 `rev` 的完整 snapshot。
4. Node 经 `_dsh/cordis/tui/slots/update` notification 送给 Rust。
5. Rust 校验 schema，按稳定 node id 保留展开、选择和滚动状态。

首版只允许向对话时间线追加节点，不能替换 composer，也不能把插件节点伪装成
durable ACP session event。ACP `session/update` 仍是 transcript 真源。

## 后续

- `/refresh`：重建 Client 插件 fiber，不杀 agent、不丢草稿。
- `chrome.status` 等壳槽：复用同一 registry/snapshot 协议。
- 可选 JS painter：只替换 renderer，不改变插件 ABI、slot 名或 token 名。

Token 名、node kind 和 slot 名一旦发布只追加；字段语义变化必须提升 protocol。
