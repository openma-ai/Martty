# 插件 API

## 四个 Plugin 视图

Martty 用四个本地视图区分 contribution、静态加载状态与 Cordis 临时运行状态：

| 视图 | 展示内容 | 生命周期 |
|---|---|---|
| `/ui` | 当前可选的 UI contribution | owner 可以是 Client-only 或双向 Package |
| `/theme` | 当前可选的 Theme contribution | owner 可以是 Client-only 或双向 Package；切换时可联动 owner 启停 |
| `/plugins` | 当前已经加载的静态 Plugin | 只读；不根据条目推断 Host-only、Client-only 或双向 |
| `/cordis-plugins` | Cordis 模式刚创建的临时动态 Plugin | 跟随当前 run，可被停止、替换或回收 |

`/ui` 与 `/theme` 是 Plugin 视图，不是 Package 类型。一个动态双向 Package 的
`code.client` 可以注册 UI 或 Theme contribution，同时由同一 run 的 `code.host`
提供 Host 能力。`/cordis-plugins` 管理的是这个完整 run，而不是只管理其中一半。

Cordis Plugin Package 有两个可选部分：

```text
Cordis Plugin Package
├── code.host       # 可选，运行在 Host Cordis tree
└── code.client     # 可选，运行在 Martty Client Cordis tree
```

只包含 `code.host` 是 Host-only，只包含 `code.client` 是 Client-only，两者都有就是
双向 Package。双向 Package 仍是一个逻辑 Plugin 和一次启动意图；Client half 通过
`host.call(method, args)` 调用同一 run 的 Host half。

第三方只通过这一页上的 API 扩展 TUI。没有列出的对象、fd、方法，宿主不提供。实现时做成调不到，而不是写在注释里请人别碰。

**当前开放：** 配色包、根级右栏 `chrome.right`、composer 顶沿及内部的
`conversation.input.dock`、位于 composer 内部、输入区与模式行之间的 `conversation.navigation.dock`、
最外层的 `conversation.composer.dock`、本地命令/语义 overlay，以及标准 ACP 当前
Session 的配置、Plan、统计与运行状态目录。四个 Slot 都是
可叠加的 list seat，接收结构化 `TuiNode` 树，不进入会话日志。

## 当前可调用：`acpSessionConfig`

动态 Client 插件注入 `acpSessionConfig` 后，可以读取当前 ACP Session 原样发布的
`configOptions`，不需要查询模型源码或猜选项值：

```js
export const inject = ['acpSessionConfig']

export function apply(ctx) {
  const options = ctx.acpSessionConfig.list()
  const current = ctx.acpSessionConfig.current('some-option-id')
  return ctx.acpSessionConfig.subscribe((snapshot) => {
    // snapshot = { sessionId, options }
  })
}
```

`list()` 返回完整 option 描述的深拷贝；`current(id)` 返回该 option 的
`currentValue`。`set(id, value)` 接受标准 ACP 当前支持的字符串 value id 或 boolean，
并校验 option 类型及 select 候选。设置仍由 Rust ACP Client 发标准
`session/set_config_option`；response-only 的完整 `configOptions` 也会同时折回 Rust
和 Client 服务，不依赖 Agent 额外发送 notification。`subscribe(listener)` 的 disposer
归 Plugin run 生命周期所有，停止动态插件后不会继续收更新。

这是任意 ACP option 的目录/状态服务，不认识 effort、模型或具体皮肤；插件从
`list()` 中选择它需要联动的 option id。

## 当前可调用：`acpSessionPlan`

`acpSessionPlan` 从标准 ACP `session/update` 的 `plan`、`plan_update` 与
`plan_removed` 折叠结构化状态，提供 `list()`、`current()` 和
`subscribe(listener)`。插件无需解析 raw ACP，也不依赖 DSH 私有 Plan 消息。

内置 `plan-view` Client Plugin 正是普通消费者：它占用
`conversation.input.dock` 显示当前
进度摘要，并注册本地 `/plan-view`，通过 `tuiOverlay.openView()` 打开审阅清单——
条目以单条 markdown **任务列表**节点呈现（`## Plan · 完成/总数` 标题、
`- [x]`/`- [ ]` 状态框、进行中加粗、取消/失败删除线、priority 后缀），
由转录的 markdown 管线完整渲染；宽终端上审阅面板可扩到 2/3 屏宽。
`/plan` 仍由 agent 的标准 command / mode 语义处理，两者不冲突。

## 当前可调用：`acpSessionStats`

`acpSessionStats` 从标准 ACP prompt response usage、文本/思考首 token、tool call 与
resume usage 更新折叠当前 Session 的 token、cache、turn、step、LLM、tool、TTFT 与
吞吐统计，提供 `current()` 和 `subscribe(listener)`。内置 `stats-view` Client Plugin
是它的默认消费者，向 `conversation.composer.dock` 注入紧凑统计行；Rust 壳不再
直接拥有这块业务 UI。动态插件也可以注入同一服务并选择自己的呈现方式。

## 当前可调用：`acpSessionStatus`

`acpSessionStatus` 从标准 ACP initialize / authenticate / session /
`session/prompt` response / `session/update` 流量，以及可选的 Martty
`session.status` / `session.event` 扩展，折叠 `/status` 需要的非统计运行状态：
state（idle/starting/running）、connection、server、auth、session 绑定、model、
effort、permission、plan 与 agent preset，提供 `current()` 和
`subscribe(listener)`。它**不**累计任何 token 或耗时——统计只有
`acpSessionStats` 这一套口径。

内置 `status-view` Client Plugin 正是普通消费者：它注册本地 `/status` 命令，用
`tuiOverlay.openView()` 打开一个 markdown 状态视图——运行状态事实取自
`acpSessionStatus.current()`，token/turn/step/LLM/tool/TTFT/rate 全部取自
`acpSessionStats.current()`（与 composer dock 里的 `stats-view` 同一快照来源，
两处读数不会漂移）。Rust painter 只渲染序列化后的 `TuiNode`，不读自己的
transcript accumulator。没有 Client 树的运行（demo、独立 painter）由 Rust 的
精简 fallback 只画运行状态与 ACP 事实，同样不读 token/耗时累计器。

## 内置 Queue 投影

follow-up FIFO 是 Rust Client 的交互状态，不是 ACP Session 内容，也不作为第三方服务
开放。painter 只把 `{ count, items, selectedId, editingId, deleteConfirm }` 快照送到本地
Client compositor；内置 `tuiQueue` service 管理订阅，内置 `queue-view` Client Plugin
把它注册进 `conversation.input.dock`。因此排队消息不会先进入 timeline，也不会由 Rust
硬编码一份正常态业务 UI：composer 顶沿显示标题，所有等待条目始终在同一个圆角
composer 内可见；选择/编辑态只增加焦点与操作提示。没有 Client tree 时，原生 shelf 仅作为降级路径。
composer 为空时，标题常驻提示 `enter send first`；Enter 会把队首立即 steer 给
当前轮次，agent deferred 时原条目仍回到队首。

## 内置 Agent 导航投影

主会话与 subagent 列表同样属于 Rust Client 的本地交互状态。painter 把
`{ activeId, selectedId, items: [{ id, label, kind, status }] }` 快照送到内置 `tuiAgents` service；
内置 `agents-view` Client Plugin 把它注册到 `conversation.navigation.dock`，并注册
`/agents` 命令。默认只显示淡色的 `· Agents · 完成/总数` 折叠摘要；`↓` 后在原地展开
主会话与全部 subagent，`←/→` 移动、`Enter` 打开、`Esc` 折叠；整个过程不创建
overlay 或鼠标 action。
没有 Client tree 时由 Rust 原生导航降级显示。

Agent 导航没有引入业务专用颜色 token。`generic.tone` 只接受既有的通用 token 名；
折叠态标签使用 `caption`，运行中的 `完成/总数` 在 `brand_soft` 与 `brand` 间轮换，
失败态使用 `err`，其余沿用 `fg`、`panel` 与 `border`。Theme Plugin 仍只维护一套
小而通用的 token 系统。Plan 摘要使用相同的视觉语法。

## UI Plugin 与两个欢迎页 slot

`tuiPresets` 把多个独立 UI Plugin contribution 组合成一个可持久化选择；它与
组合 Host/Agent 能力的 Agent Preset 分属 Client/Host 两棵树，也不是 Theme 的别名。
`ctx.tuiPresets.register({ id, label }, mount)` 的 `mount` 只组合结构性 UI
contribution（slot、chrome、pet 等），并返回统一 disposer；它不注册、选择或生成
Theme。UI Plugin、Theme Plugin 与 dark/light 是三个独立状态维度。`/ui` 打开原生单选表单，`/ui ` 复用
composer 上方的 slash 菜单列出候选，也可用 `/ui <id>` 直接切换；选择写入
`$MARTTY_HOME/settings.json`，重启后恢复。Creator 可以 inspect `UiPresets` 与相邻 Client
服务，生成调用 `tuiPresets.register` 的 `code.client` Package；`cordis_define/run`
只做当前进程预览，成功后必须由 `tui_plugin_save` 写入
`$MARTTY_HOME/plugins/<artifact-id>/plugin.json`，新 artifact 使用 `kind: "ui"`，重启后恢复。
旧 `kind: "ui-preset"` 会在读取边界归一成 `ui`；`UiPresets`、`tuiPresets` 与
settings 的 `uiPreset` 仅作为兼容性内部名保留。

内置 `default`（Martty）和 `deepseek` 两个 UI Plugin 都装配两个 single root slot：

- `welcome.hero`：居中的品牌区，结构为 `logo + hint`；
- `welcome.info`：左下的版本、模型、workspace、session、凭据、访问说明与帮助区。

两个 preset 当前复用原生动态 info renderer，但它已经是可独立替换的 slot。
`deepseek-logo` 仅注册 `deepseek` preset，不再拥有专用 slash command；使用
`/ui deepseek` 和 `/ui default` 切换。Rust painter 复用旧版响应式鲸鱼/wordmark
primitive，并负责水平居中、整个欢迎块垂直居中与上下等距。切换不进入 ACP prompt
或 transcript。

## 当前可调用：`tuiTheme`

`palette` 必须符合 [tui-palette.v0.schema.json](tui-palette.v0.schema.json)。

| 字段 | 规则 |
|---|---|
| `id` | 稳定字符串，与已有 id 冲突则 `register` 失败 |
| `label` | 给 `/theme` 看的短名 |
| `dark` / `light` | 各一份 **完整** token → `#RRGGBB`。缺任意 token、或多了不认识的名字，都失败 |

Token 名封闭：

`bg` `surface` `panel` `fg` `fg_secondary` `fg_tertiary` `caption` `brand` `brand_soft` `bubble_bg` `bubble_fg` `border` `code_bg` `ok` `warn` `err` `hint` `chip_bg`

内置 `default` 仍是现在的冷蓝灰皮，永远不能替换或删除。`/theme toggle` 与
`ctrl+t` 只在当前主题的 dark/light 之间切；`/theme ` 的上拉候选将 toggle 与
Theme Plugin 分区显示。`/theme <id>` 是一个特殊的
**Theme Plugin 单选开关**：选择一个 id
会启动它所属的 Client Plugin，并停止此前选中的 Theme Plugin；选择 `default` 会
停止当前动态 Theme Plugin。显式选择的 theme id 与 `uiPreset` 并列写入
`$MARTTY_HOME/settings.json`；临时 Creator 预览和自动回退不会改写用户选择。

一个 Theme Plugin 可以同时注册 palette、command、overlay、slot、timer、事务和
Package 内 RPC。它们不写主题条件，也不订阅 active theme；它们只是同一个 Cordis
Fiber 的普通贡献。`/theme` 切换、stop、update 或 unload 时，整个 Fiber 一起撤销。
已停止 Theme Plugin 的 id 仍留在 `/theme` 目录里，供以后重新启动。

`register(palette, { activate: true })` 是动态插件首次 `cordis_run` 的即时预览入口。
Client runner 会把这次运行放进同一个 `/theme` 席位，并替换此前的 Theme Plugin；
目标启动失败则保留原插件。动态 Client facade 只开放 `register`，不开放
`activate()`、`active()` 或 theme `subscribe()`，避免绕过这个插件开关。

这是在改 TUI：composer、状态栏、边框、气泡、代码块、工具卡、提示行全部改色。布局、框线字符、字号、kitty 宠物像素图不变。

Theme Plugin 仍是普通 Cordis Plugin：在一个 `apply` 里向各独立服务注册贡献，
disposers 归同一个 Fiber。`/theme` 只是在这些 Plugin 之间做特殊的单选切换。

Cordis 模式通过 Client inspect 看见这套 API：`cordis_inspect_list` 里的 `Theme`
（platform `client`）来自 TUI 经 ACP 同步的目录，不是 agent 树上的 `tuiTheme`。
`code.client` 注入 `tuiTheme` 并注册 palette；同一 Plugin 的其他能力照常注入各自
服务。不要写网页的 `theme.overrideTokens`、CSS 变量或主题条件。

## 最小插件

```js
export const inject = ["tuiTheme"];

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register({
    id: "ember",
    label: "Ember",
    dark: { /* 18 tokens, see fixtures/demo-skin.v0.json */ },
    light: { /* 18 tokens */ },
  }));
}
```

即时预览只需把注册改为：

```js
ctx.tuiTheme.register(palette, { activate: true })
```

不需要监听主题或手写回切；`/theme` 管理整个 Plugin 的替换。

## 当前可调用：`tuiSlots`

宿主声明四个 list slot：根级独立右栏 `chrome.right`、占 composer 顶沿且可在其边框内
展开的 `conversation.input.dock`、composer 内部位于输入区与模式行之间的
`conversation.navigation.dock`，以及 composer 外侧的紧凑统计席
`conversation.composer.dock`。插件先
`inject` 等待槽位，再用稳定的
contribution id `register` 任意 `TuiNode[]` 组合：`group`、`markdown`、
`reasoning`、`user`、`generic`、`terminal`、`diff`、`image`、`notice`、
`unknown`。完整字段见 [tui-node.v0.schema.json](tui-node.v0.schema.json)。

```js
export const inject = ['tuiSlots']

export function apply(ctx) {
  ctx.tuiSlots.inject('chrome.right', () => {
    const panel = ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'build-monitor', order: 20 },
      [
        { id: 'title', kind: 'markdown', text: '# Build monitor' },
        { id: 'tests', kind: 'generic', title: 'Tests', body: '97 passed', status: 'ok' },
      ],
    )
    return panel.dispose
  })
}
```

`panel.update(nextNodes)` 原地替换该 contribution 的完整树并立即重绘；停止或
卸载插件会撤销 contribution，最后一个 contribution 消失时右栏立刻收起。
终端过窄时宿主暂时隐藏右栏但保留状态，变宽后自动恢复。节点 id 在一个
contribution 内稳定即可，聚合器会用 contribution id 做命名空间。

三个 conversation dock 使用相同 API，只需把 `name` 改为对应 slot。`generic` 节点在
`conversation.input.dock` 中聚合到 cap 行，与 Tip 竞争并优先于 Tip；同一 contribution
临时发布的非 `generic` 结构化节点会在圆角 composer 内、输入区上方展开。内置
`queue-view` 用这个机制持续呈现全部 Queue 条目，并在选择态增加焦点。Plan、Goal 等正常态仍应保持一行摘要，完整审阅内容
用本地 command 打开 overlay。Agent、branch、session 等横向导航放
`conversation.navigation.dock`；环境统计等紧凑读数放 `conversation.composer.dock`
（内置 stats-view 是默认消费者），不要向该统计席塞入多行内容。

`generic` 节点可选的 `tone` 字段直接引用既有通用 Theme token（例如 `brand`、`ok`、
`warn`、`err`、`caption`），不能声明新 token，也不要为单一业务发明颜色语义。瞬时
键盘焦点可用通用布尔字段 `selected: true`，painter 会以反显样式绘制；不要把它当作
持久选中值或另一个状态颜色。

## 当前可调用：`tuiOverlay`

`openSlider(options, handlers)` 提供数值滑条；`openSelect(options, handlers)` 提供
普通单选表单；`openView({ id, title, nodes })` 提供只读 `TuiNode[]` 弹窗。三者共享
一个 modal 席位，返回的 `close()` 和插件 Fiber 同生命周期。View 支持上下翻阅，
Enter 提交、Esc 取消；它是通用节点视图，不认识 Plan。

本地命令可在 `tuiCommands.register` 的 options 中声明
`input: { hint, options: [{ value, label, description }] }`。用户输入 `/name ` 后，Rust
painter 会复用现有上拉 slash 菜单展示候选；注册返回的 disposer 还提供
`update({ input })`，供 UI Plugin、模型等动态目录刷新候选。标准 ACP
`AvailableCommand.input` 当前只有 `hint`，没有枚举候选；Agent 命令因此展示标准
hint，只有 Client 插件扩展或可从 ACP config/catalog 推导的命令才列出候选。

### Host 数据与轮询

动态 Package 的 Host half 用 `harness.handle(method, handler)` 注册仅属于该
Package 的 JSON 方法，Client half 用 `host.call(method, args)` 调用。参数与返回值
必须是无损 JSON；省略 `args` 时 Host 收到 `null`。

需要定时刷新时同时注入 `timer`，使用随 Plugin run 自动回收的
`ctx.interval`，不要调用 Node 的裸 `setInterval`：

```js
return {
  inject: ['tuiSlots', 'timer'],
  apply(ctx) {
    const panel = ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'monitor' },
      [{ id: 'loading', kind: 'notice', level: 'info', text: 'Loading…' }],
    )
    let refreshing = false
    const refresh = async () => {
      if (refreshing) return
      refreshing = true
      try {
        const state = await host.call('read-state')
        panel.update([{ id: 'state', kind: 'generic', title: 'State', body: state.text }])
      } catch (error) {
        panel.update([{ id: 'error', kind: 'notice', level: 'error', text: String(error) }])
      } finally {
        refreshing = false
      }
    }
    void refresh()
    ctx.interval(() => void refresh(), 1000)
  },
}
```

`host.call` 走协商后的 ACP `_dsh/cordis/plugin/invoke` Extension Request；timer 和 Slot 都只存在于 Client 树，
不会进入 `session/update` 或会话历史。

动态 Creator 插件先查询 Client inspect 的 `Slots.list`，再让 `code.client`
注入 `tuiSlots`。这是终端节点协议，不是 Web React 的 `ctx.slots`。
可直接参考未默认挂载的 gallery：`@openma/deepseek-harness-tui/right-demo`。

```yaml
- insert:
    - id: tui-right-demo
      name: '@openma/deepseek-harness-tui/right-demo'
```

## 没有这些 API

插件模块拿不到：TTY、stdin、raw mode、kitty 转义、ratatui、JSON-RPC 方法表、fd 3/4、`_dsh/cordis/tui/*` transport、整屏行列、帧循环、按像素或「底 N 行」的坐标。

节点上不得写 RGB；着色只能引用 token 名。RGB 只出现在配色包的 `dark`/`light` 表里。

## 持久化与加载

### 从 Cordis 临时 run 升级为常驻 Plugin

`cordis_define` / `cordis_run` 创建的是当前进程内的临时 Plugin。先用
`cordis_inspect_self(pluginId, packageId)` 读取当前成功 Package 的完整源码和诊断，
再根据 Package 是否包含 Host half 选择持久化路径。

Client-only Package 在 `cordis_run` 成功后，可以调用 `tui_plugin_save` 写入
`$MARTTY_HOME/plugins/<artifact-id>/plugin.json`。Client 启动时会重新发现并装载它。

只要存在 `code.host`，无论是 Host-only 还是双向 Package，都必须整理成普通 Cordis
npm Package 或本地 Package，再使用下面的命令安装到 profile：

```sh
dsh plugin --profile martty add <package-or-path>
```

安装后，Harness 在启动时加载 Host registrar，并把可序列化的 Client entry 清单交给
Martty Client。当前没有通用的 `promote` 工具把动态定义直接写进 profile；动态预览
与静态安装之间保留显式的保存或打包步骤。

TUI 合并三个来源，按 builtin、已安装 package、Creator artifact 的顺序装入同一
`/ui` 与 `/theme` catalog：

1. builtin 随 TUI 包发布；
2. 第三方 package 由 `dsh plugin --profile martty add <package-or-path>` 安装，其 Host
   registrar 向 `tuiClientPlugins` 登记绝对 Client entry；Host runner 只把这份 JSON
   清单交给独立 Client 进程；
3. Client-only Creator artifact 位于 `$MARTTY_HOME/plugins/<artifact-id>/plugin.json`，由
   `tui_plugin_list/read/save/remove` 管理。已安装 package 与本地 artifact 同 id 时，
   package 优先，本地项作为 shadowed diagnostic 保留而不执行。

`tui_plugin_save` 只接受没有 `code.host` 的 Package。只要存在 Host half，无论是否同时
存在 Client half，完整 Plugin 都归当前 Harness，并使用 Harness 自己的持久安装路径；
Martty 不把 Host 代码写入 `.martty`。

`MARTTY_HOME` 的解析顺序是显式环境变量、`$DSH_HOME/.martty`、`~/.martty`。
旧 `$DSH_HOME/.tui-plugins` 与 `~/.dsh-tui/sessions/dsh-tui-settings.json` 在首次启动时
只复制到新位置；不删除旧数据，也不覆盖已经存在的 Martty 数据。

第三方包的 Host registrar 是普通 Cordis 插件：

```js
export const inject = ['tuiClientPlugins']
export function apply(ctx) {
  return ctx.tuiClientPlugins.register({
    id: 'paper-lantern',
    kind: 'theme',
    entry: new URL('./client.js', import.meta.url).href,
  })
}
```

`client.js` 导出普通的 `inject` / `apply` Client Plugin。包自己的 bundle patch 把
registrar 插入 Host profile；registrar 只登记可序列化 entry，不把 Host fiber、服务或
插件树同步给 Client。

Cordis **没有**「挂在某个插件实例下面」的父子指针。`tui-theme`、配色包、
`tui-cordis-client-runner` 和 `tui-runner` 在 Client 树里是平级生命周期。

真正等 TUI 能力的协议是服务名：

1. `tui-theme` / `tui-slots` 分别提供 `ctx.tuiTheme` / `ctx.tuiSlots`。
2. `tui-cordis-client-runner` 注入两者，向 Host 发布 inspect 目录并执行动态 `code.client`。
3. 第三方 Client module 声明对应 inject；服务不在就 PENDING，不靠 Host yaml 行序。
4. package 与 Creator artifact 都经过相同的受限 Client facade；Theme owner 的
   start/stop 与 UI Plugin 的常驻 catalog 生命周期由 Client manager 收集。

不要在 runner 里 `ctx.plugin(ember)` 来叠第三方。第三方包由组合层作为 sibling
挂载。Web 的 `ctx.slots` 与终端的 `ctx.tuiSlots` 是不同服务。

本地包走 `dsh plugin --profile martty add ./path`，加载 **package exports**。新 registrar
要重开 TUI 或以后的 `/refresh`。已挂配色包改源码后的热换按阶段落地。

## 后续

`conversation.chat` 等会话槽仍未开放；ACP `session/update` 继续是 transcript 真源。
