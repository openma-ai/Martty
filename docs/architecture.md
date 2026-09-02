# 架构

## Plugin Package 与视图

Cordis Plugin Package 可以带可选的 `code.host`、`code.client`，因此形成 Host-only、
Client-only 与双向三种运行形态。双向不是两棵树合并：两个 half 分别运行在 Host 与
Martty Client 的 Cordis tree，由同一个 run 管理，并通过 ACP 上的 Package RPC 交换
JSON 数据。

TUI 中的四个入口是不同的数据视图：`/ui`、`/theme` 展示 UI 与 Theme contribution；
它们的 owner 可以是 Client-only 或双向 Package。`/plugins` 只读展示当前已加载的
静态 Plugin。`/cordis-plugins` 展示 Cordis 模式创建的临时 Plugin run，它们可随当前
运行生命周期被停止、替换或回收。

ACP client 是一个 Cordis 插件：提供 `ctx.acpClient`，讲 ACP，**不包、不 import 任何 harness**。TUI 是挂在这棵 **client 树** 上的壳（Rust 占 TTY）。配色等 TUI 插件 `inject` 的是 client 侧服务，不是 Martty 包名，也不是 dsh-acp。

主路径 `dsh --profile martty` 启动两个进程：Host 进程的 Base Cordis 树直接挂完整 ACP plugin；Host runner 再启动独立的 TUI Client 进程。两者只通过 TUI Client 的标准 stdin/stdout 讲 ACP，不共享 Cordis service、plugin id、`inject` 或 fiber。Client 进程的 fd 3/4 只继承用户 TTY，再映射为 Rust painter 的 stdin/stdout；Host↔Client ACP 不走 fd 3/4。

独立入口 `martty` 保持通用 ACP client 语义：它的 Client 树可以 spawn `dsh-acp`、`dsh --profile acp` 或其它 ACP agent，也可以接调用方提供的 stream。

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

运行中由 Enter 产生的 follow-up FIFO 属于 Martty Client：composer 为空时按
Enter 会把队首立即 steer 给当前轮次，deferred 时放回队首；否则在 session 变为
空闲后作为下一次 `session/prompt` 发出。Rust 把 FIFO 的只读投影经
本地 `_dsh/cordis/tui/queue/update` 交给 Client tree；内置 `tuiQueue` service 折叠快照，
`queue-view` Client Plugin 再向现有 `conversation.input.dock` 注册 composer 展示。
Queue 标题位于 composer 顶沿，全部等待条目始终在同一个 composer 边框内、输入区上方
展开；`Alt+↑` 进入选择态后可用 `↑/↓` 任选一条编辑。等待内容不进入 timeline，出队时才
创建普通 user timeline cell；编辑、删除和预览更新也都不进入 ACP。没有 Client tree 的
独立 painter 保留原生 shelf 作为降级显示。
`Send Now` 的并发 prompt 若被 agent 拒绝，ACP transport
只向 Client 回报 deferred，由同一 Client FIFO 接管，不在 transport 内保留第二份重试队列。

Agent/subagent 导航也走同一条 Client compositor 边界：Rust painter 把主会话与子会话的
只读快照送到内置 `tuiAgents` service，`agents-view` Client Plugin 再把紧凑导航注册到
`conversation.navigation.dock`。这个 slot 位于 composer 内部，输入区与 `Standard`
模式行之间；默认只画 `· Agents · 完成/总数` 摘要，`↓` 后才展开全部条目；
`←/→` 移动、`Enter` 打开、`Esc` 折叠，不创建弹窗或鼠标 action。无 Client tree 时
Rust 保留同语义的原生导航作为降级路径。

标准 ACP `tool_call` 即使携带 subagent `_meta` 也始终先按原事件进入父会话 transcript；
解析器只在其后追加 `SubagentStarted` 投影。终态 `tool_call_update` 同样先生成原始
`ToolResult` 闭合请求块，再追加 `SubagentFinished`，subagent 生命周期不会替换工具事件。

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
  tui-queue    接收 Rust painter 的本地 Queue 快照，提供 tuiQueue
  queue-view   注入 tuiQueue + tuiSlots，向 conversation.input.dock 注册全部条目与选择状态
  tui-agents   接收 Rust painter 的本地 Agent 快照，提供 tuiAgents
  agents-view  注入 tuiAgents + tuiSlots，向 conversation.navigation.dock 注册导航
  stats-view   注入 acpSessionStats + tuiSlots
  status-view  注入 acpSessionStatus + acpSessionStats + tuiCommands +
               tuiOverlay，注册 /status 命令
  harness-view 注入 tuiCommands + tuiOverlay + acpClient + acpSessionStatus；
               standalone 启动与切换只 initialize；首个 prompt 才 session/new；
               会话尚未开始时即时替换 ACP 子进程，首个 session/prompt 后或
               session/load 恢复旧会话时先确认，再 initialize 新 Harness；
               profile 主路径仍由 Host 拥有 runtime
  tui-presets  提供 tuiPresets，注册持久化 /ui 选择与组合生命周期
  martty-preset default UI Plugin，组合 welcome.hero + welcome.info
  deepseek-logo 注入 tuiPresets + tuiSlots，注册 deepseek UI Plugin；
               组合 DeepSeek welcome.hero + 动态 welcome.info
  acp-session-status 提供 acpSessionStatus（连接/服务端/认证/会话/
               模型/effort/权限/plan/agent 等运行状态事实）
  tui-slots    提供 tuiSlots，声明 welcome.hero / welcome.info / chrome.right /
               conversation.input.dock / conversation.navigation.dock /
               conversation.composer.dock
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
tui-slots     TuiNode 树 → welcome.hero / welcome.info / chrome.right / input / navigation / telemetry dock registry
plan-view     标准 ACP Plan 投影 → input dock 摘要 + /plan-view overlay
tui-queue     Rust Client FIFO 快照 → Client tree 的 tuiQueue service
queue-view    tuiQueue → input dock 顶沿标题 + composer 内全部等待条目
tui-agents    Rust Agent/session 快照 → Client tree 的 tuiAgents service
agents-view   tuiAgents → composer 内部 navigation dock + inline 会话选择
stats-view    标准 ACP usage/timing 投影 → composer dock 统计行
status-view   acpSessionStatus + acpSessionStats → /status markdown overlay
harness-view  Harness registry + session started 状态 → /harness 确认 + 换进程
martty-preset default UI Plugin → Martty Hero + 原生动态信息区
deepseek-logo deepseek UI Plugin → DeepSeek Hero + 原生动态信息区
acp-session-status 标准 ACP 运行状态投影：连接/服务端/认证/会话/模型/
              effort/权限/plan/agent —— 不累计 token 或耗时（那是
              acpSessionStats 的职责）
Client runner Client inspect + code.client 挂载/停止
TUI 壳        消费 Theme、slot 与通用 overlay 快照树；把本地 Queue/Agent 快照路由给对应 service
acp-client    session/new · authenticate · prompt · cancel · config · commands
              standalone 使用稳定代理流，可替换 ACP 子进程而不断开 TUI compositor；
              启动/替换时清空旧 Agent capability 并 initialize；首个 prompt 前 session/new
              提供 acpSessionConfig（观察标准快照；set 仍由 Rust 发标准 ACP）
              提供 acpSessionPlan（观察标准 plan update；内置 Plan 插件只是消费者）
              提供 acpSessionStats 与通用、随 effect 回收的 ACP observer registry
              `_dsh/cordis/*` 是 initialize 协商后的 Cordis 扩展
              `_dsh/cordis/tui/*` 是 mux 隔离的 painter 子域
Rust 画布     输入、keymap、Client FIFO、现有 widget 读 Theme/slot、kitty、剪贴板
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
palette、command、overlay、slot 与 RPC 随同一个 Fiber 一起上下线。`martty --demo`
保持 `default`；`--demo-skin` 仍是静态 gallery 演示路径。常驻 gallery 包
`ayu`（dark=Ayu、light=Ayu
Light）、`catppuccin`（dark=Catppuccin Mocha、light=Catppuccin
Latte）、`kanagawa`（dark=Kanagawa Wave、light=Kanagawa
Lotus）、`one`（dark=One Dark、light=One
Light）、`tomorrow`（dark=Tomorrow Night
Bright、light=Tomorrow）、`everforest` / `iceberg` /
`solarized`（均为 dark+light 双变体），色值取自
[terminalcolors.com](https://terminalcolors.com/themes/)，随 Client boot
以 sibling insert 行注册，`/theme` 直接可切。

## 控制面与绘制面

控制面走 ACP 已有方法（含 `authenticate`，对标 Backchat 的 probe / 登录面板：表单走 `_meta`，Terminal Auth 占 TTY）。Client `initialize` 声明 `fs.readTextFile` / `fs.writeTextFile` 与 `terminal`（createTerminal，有别于 `auth.terminal`）；会话中途 `auth_required` 打开 client `/auth`，不要让用户去敲 `/login`。Transcript 来自 `session/update`。配色是 **client 树** 上的 `tuiTheme.register`；送到 Rust 画布的是 `_dsh/cordis/tui/theme/update` notification，不是第三方插件 API。`_meta.dsh.cordis` 只协商能力，chrome 数据必须走扩展 notification。

## 主题

Token **名**封闭，见 [plugins.md](plugins.md)。内置 `default` 的色值仍是冷蓝灰。配色包可以（也必须）为全部 token 提供 `#RRGGBB`。对话节点以后若开放，节点上仍然只写 token 名，不写 RGB。

`/theme toggle` 或 `ctrl+t` 切当前主题的 dark/light。`/theme <id>` 切整个 Theme Plugin。kitty 宠物是 RGBA
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
