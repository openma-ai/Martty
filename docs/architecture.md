# 架构

ACP client 是一个 Cordis 插件：提供 `ctx.acpClient`，讲 ACP，**不包、不 import 任何 harness**。TUI 是挂在这棵 **client 树** 上的壳（Rust 占 TTY）。配色等 TUI 插件 `inject` 的是 client 侧服务，不是 `dsh-tui` 包名，也不是 dsh-acp。

主路径 `dsh --profile tui` 启动两个进程：Host 进程的 Base Cordis 树直接挂完整 ACP plugin；Host runner 再启动独立的 TUI Client 进程。两者只通过 TUI Client 的标准 stdin/stdout 讲 ACP，不共享 Cordis service、plugin id、`inject` 或 fiber。Client 进程的 fd 3/4 只继承用户 TTY，再映射为 Rust painter 的 stdin/stdout；Host↔Client ACP 不走 fd 3/4。

独立入口 `dsh-tui` 保留通用 ACP client 语义：它的 Client 树可以 spawn `dsh-acp`、`dsh --profile acp` 或其它 ACP agent，也可以接调用方提供的 stream。

## ACP client：根插件或附加插件

同一份 `apply`，两种组合位置：

| | 独立根插件 | 附加在已有树上 |
|---|---|---|
| 位置 | client 组合的第一行，进程根 | 别人的 client 树里一行 `insert` |
| 提供 | `ctx.acpClient`（以及它 boot 的 client 服务） | 同样 `ctx.acpClient` |
| Agent | `config.agent.command` spawn（默认可 `dsh-acp`）或 `config.stream` 接已有标准管道 | profile 主路径由 Host runner 把 `config.stream` 接到跨进程 stdio；独立入口可 spawn |
| 不做什么 | 不 `ctx.plugin` 进 dsh-base | 不与 ACP Host 插件同进程、也不把 ACP 改成进程内 stream |

接到 dsh 上只是 agent 配置，例如 `{ command: "dsh-acp" }` 或 `{ command: "dsh", args: ["--profile", "acp"] }`。换成别的 ACP agent 只改这条命令。

## 协商后的 DSH Cordis ACP 扩展

Server 树（profile 主路径里的 Base + ACP plugin，或 standalone dsh-acp）和 Client 树（TUI/皮）**位于不同进程且不共享 Cordis**。协调只用 ACP：

| 方向 | 走 ACP | 不走 |
|---|---|---|
| Agent → UI | `session/update`、`available_commands_update`、`config_option_update`、permission；协商后可发 `_dsh/cordis/inspect/*`、`run/*`、`plugins/*` notification | 整棵插件图、`inject` 列表、fiber、`ctx.*` |
| UI → Agent | `session/prompt`、`cancel`、`set_session_config_option`、`authenticate`；协商后可发 `_dsh/cordis/inspect/*`、`run/*`、`plugin/*`、`plugins/*` request/notification | client 插件去 `register` 对端服务 |

Server 插件要让任意 client 看见：投影成标准 ACP（命令、config、update）。Client 插件（换皮、槽）只改本树。TUI 作为 Cordis **Client** 把可发现的能力目录（`Theme.listTokens`、`Slots.list`）序列化到 `_dsh/cordis/*` 扩展，和网页同步 inspect manifest 是同一类工作。双方必须先在 initialize capability 的 `_meta.dsh.cordis.protocol` 上达成版本 0；普通 ACP agent 没有该 capability 时，TUI 不发送也不消费这些扩展。

```text
Host 进程：Base Cordis + ACP plugin
  tui-client-plugin-registry 收集已安装 package 的绝对 Client entry
        │ TUI Client 子进程的标准 stdin/stdout（ACP）
        │ runner 另以 JSON 环境快照传递 Client entry 目录（不传 Cordis 对象）
Client 进程：独立 Cordis root
  第三方插件   sibling insert + inject ['tuiTheme' / 'tuiPresets' / 'tuiSlots']
  Client runner inject ['tuiTheme', 'tuiPresets', 'tuiSlots', ...]，执行 dsh-tool-cordis 的 code.client
  TUI 壳       注入 acpClient 与 Client services，只占 TTY
  plan-view    注入 acpSessionPlan + tuiSlots + tuiCommands + tuiOverlay
  stats-view   注入 acpSessionStats + tuiSlots
  status-view  注入 acpSessionStatus + acpSessionStats + tuiCommands +
               tuiOverlay，注册 /status 命令
  tui-presets  提供 tuiPresets，注册持久化 /ui 选择与组合生命周期
  martty-preset default UI Plugin，组合 welcome.hero + welcome.info
  deepseek-logo 注入 tuiPresets + tuiSlots，注册 deepseek UI Plugin；
               组合 DeepSeek welcome.hero + 动态 welcome.info
  acp-session-status 提供 acpSessionStatus（连接/服务端/认证/会话/
               模型/effort/权限/plan/agent 等运行状态事实）
  tui-slots    提供 tuiSlots，声明 welcome.hero / welcome.info / chrome.right /
               conversation.input.dock / conversation.composer.dock
  tui-theme    提供 tuiTheme
  tui-local-plugins 合并已安装 package entries 与 $MARTTY_HOME/plugins，
               恢复 UI Plugin，并按 /theme owner 启停 Theme Plugin
  acp-client   attach Host stdio，提供 acpClient
        │ Client 进程 fd 3/4 仅传 TTY
  Rust painter stdin/stdout 占 TTY；自己的 fd 3/4 是 compositor mux

standalone：Client 进程也可改为 spawn 任意 ACP agent 的标准 stdio
```

profile 只需安装 `@openma/deepseek-harness-tui`。dsh 仍按正常 profile 规则提供
Base；TUI bundle 从自己的运行时依赖挂 ACP plugin 与 Creator Host overlay，并挂
Host runner。runner 不再 spawn `dsh-acp`，只启动独立 TUI Client 进程并连接标准
ACP stdio。Client root 由该进程自行创建，Host bundle rows 不等于 Client fibers。

## Creator skill overlay

Creator 的 skill 属于 agent Host，不属于 TUI Client 树。TUI 主包内部的
`creator-overlay` 入口叠到 Host Base tree：它注入
`agentPresets` 与 `skills`，通过 `standingKeyFor('cordis')` 取得上游 Creator 的
standing scope key，再用相同 key 挂一个自己的 fiber 并注册
`tui-plugin-development`。因此 skill 只进入 `cordis` 的 scope layer；其它 preset
和全局目录看不到，卸载时也只撤销这一层贡献。

overlay 通过 profile Loader 取得 Host 正在使用的 `dsh-scope` 模块实例。这样
`link:` 热部署包即使有自己的开发依赖，也不会产生第二套私有 scope symbol，把
本应属于 Creator 的 skill 误注册到全局层。

这不是 Creator fork，也不修改其 `agent.cordis.yml`。上游 preset 升级后重启
profile 即会得到新组合，overlay 重新叠在新 standing scope 上。skill 的发现与
注册不走 ACP；TUI Client 能力查询和动态 `code.client` 执行仍走已经协商的
Client inspect/run 通道。

## 分层

```text
第三方插件    sibling insert + inject ['tuiTheme'] → 配色 register
              sibling insert + inject ['tuiPresets'] → UI 组合 register
              sibling insert + inject ['tuiSlots'] → slot register
tui-theme     配色表 → Theme registry
tui-presets   多个 UI contribution → 持久化 /ui 单选组合
tui-slots     TuiNode 树 → welcome.hero / welcome.info / chrome.right / input dock / composer dock registry
plan-view     标准 ACP Plan 投影 → input dock 摘要 + /plan-view overlay
stats-view    标准 ACP usage/timing 投影 → composer dock 统计行
status-view   acpSessionStatus + acpSessionStats → /status markdown overlay
martty-preset default UI Plugin → Martty Hero + 原生动态信息区
deepseek-logo deepseek UI Plugin → DeepSeek Hero + 原生动态信息区
acp-session-status 标准 ACP 运行状态投影：连接/服务端/认证/会话/模型/
              effort/权限/plan/agent —— 不累计 token 或耗时（那是
              acpSessionStats 的职责）
Client runner Client inspect + code.client 挂载/停止
TUI 壳        消费 Theme、slot 与通用 overlay 快照树
acp-client    session/new · authenticate · prompt · cancel · config · commands
              提供 acpSessionConfig（观察标准快照；set 仍由 Rust 发标准 ACP）
              提供 acpSessionPlan（观察标准 plan update；内置 Plan 插件只是消费者）
              提供 acpSessionStats 与通用、随 effect 回收的 ACP observer registry
              `_dsh/cordis/*` 是 initialize 协商后的 Cordis 扩展
              `_dsh/cordis/tui/*` 是 mux 隔离的 painter 子域
Rust 画布     输入、keymap、现有 widget 读 Theme、kitty、剪贴板
```

| 层 | 职责 | 不负责 |
|---|---|---|
| 配色插件 | 为封闭 token 名提供 dark/light 色值 | 改布局、改 token 名、占 TTY、依赖某个 harness |
| acp-client | ACP 会话控制；作根或作 insert | 组合 dsh、当 UI 内模 |
| tui-theme / tui-slots | 提供 `tuiTheme` / `tuiSlots` registry | 占 TTY、依赖 agent |
| Client runner | 向 `dsh-tool-cordis` 发布 TUI Client 能力并挂载 `code.client` | 占 TTY、运行 Web React 插件 |
| TUI 壳 | TTY、输入、compositor；消费插件提供的结构化 slot/overlay snapshot | 拥有 Plan/统计等业务投影，或解释 harness `SessionEvent` |
| Rust | 把 Theme 画进现有 widget | 在 JS 里解释颜色 |

本体只有内置 `default`。动态 Theme Plugin 用 `tuiTheme.register` 声明 palette；
`/theme` 是特殊的单选 Plugin 开关，启动目标 Plugin 并停止当前 Theme Plugin，因而
palette、command、overlay、slot 与 RPC 随同一个 Fiber 一起上下线。`dsh-tui --demo`
保持 `default`；`--demo-skin` 仍是静态 gallery 演示路径。

## 控制面与绘制面

控制面走 ACP 已有方法（含 `authenticate`，对标 Backchat 的 probe / 登录面板：表单走 `_meta`，Terminal Auth 占 TTY）。Client `initialize` 声明 `fs.readTextFile` / `fs.writeTextFile` 与 `terminal`（createTerminal，有别于 `auth.terminal`）；会话中途 `auth_required` 打开 client `/auth`，不要让用户去敲 `/login`。Transcript 来自 `session/update`。配色是 **client 树** 上的 `tuiTheme.register`；送到 Rust 画布的是 `_dsh/cordis/tui/theme/update` notification，不是第三方插件 API。`_meta.dsh.cordis` 只协商能力，chrome 数据必须走扩展 notification。

## 主题

Token **名**封闭，见 [plugins.md](plugins.md)。内置 `default` 的色值仍是冷蓝灰。配色包可以（也必须）为全部 token 提供 `#RRGGBB`。对话节点以后若开放，节点上仍然只写 token 名，不写 RGB。

`ctrl+t` 切当前主题的 dark/light。`/theme` 切整个 Theme Plugin。kitty 宠物是 RGBA
精灵，不随 token 重上色；启动锁屏的 `MAR` 读海洋渐变，`TTY` 读终端前景黑/白。
UI Plugin 可以组合多个结构性 UI contribution，但不等同于 Theme。`default`（Martty）与
`deepseek` 都同时装配 `welcome.hero` 和 `welcome.info`：前者是居中的
`logo + hint` 品牌区，后者是左下的版本、模型、workspace、session、凭据、访问说明
与帮助区。两套 preset 当前复用同一个原生动态 info renderer，但该区域可以独立被
插件替换。旧 DeepSeek Harness 鲸鱼仍复用原版响应式 primitive。Rust 负责内部几何、
水平居中与整个欢迎块的垂直居中；`/ui deepseek` / `/ui default` 切换并持久化到
`$MARTTY_HOME/settings.json`。

Creator 的 `cordis_define/run` Package 属于 Session 进程内预览。显式保存后，Client
源码以 `$MARTTY_HOME/plugins/<artifact-id>/plugin.json` 为磁盘真源；Client 启动时
重新发现。第三方 package 则继续由 dsh profile 安装，其 Host registrar 只向
`tuiClientPlugins` 登记 `{ id, kind, entry }`，runner 把这个可序列化快照交给独立
Client import。两类来源进入同一个 lifecycle manager；同 id 时安装 package 优先。
新磁盘 artifact 使用 `kind: "ui"`；旧 `ui-preset` 与内部 `uiPreset` key 仅为兼容。
UI Plugin 只组合结构性 UI contribution，不拥有 Theme；保存的 `uiPreset`、`theme`
和 dark/light 相互独立。

`MARTTY_HOME` 依次取显式环境变量、`$DSH_HOME/.martty`、`~/.martty`。默认 Session
根是 `$MARTTY_HOME/sessions`。旧 settings、Creator artifacts 与 Session 根只作为
非破坏迁移/发现来源保留。

## 明确不做

- 把 Web 的 `dsh-client-ui-*` React 组件跑在终端里。
- 开放 ACP 方法表给第三方。
- 把 acp-client 和 acp-bridge 放进同一进程抢 stdout。
- TUI npm 只允许在 `@deepseek-ai/*` 里依赖 `@deepseek-ai/cordis`，不直接依赖 `dsh-*`；ACP 与 Creator 是明确的运行时传递依赖，但不进入 Client bundle rows。
- 插件发 kitty、设 raw mode、读整屏 size 做绝对定位。
- 用往时间线加一条消息来冒充「换了 TUI」。
