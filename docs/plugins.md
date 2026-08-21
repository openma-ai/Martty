# 插件 API

第三方只通过这一页上的 API 扩展 TUI。没有列出的对象、fd、方法，宿主不提供。实现时做成调不到，而不是写在注释里请人别碰。

**当前开放：** 配色包、根级右栏 `chrome.right`、composer 上方的
`conversation.input.dock`、composer 下方的 `conversation.composer.dock`、本地命令/
语义 overlay，以及标准 ACP 当前 Session 的配置、Plan、统计与运行状态目录。三个 Slot 都是
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

`acpSessionStatus` 从标准 ACP initialize / authenticate / session / session/update /
session.event 流量折叠 `/status` 需要的非统计运行状态：state（idle/starting/
running）、connection、server、auth、session 绑定、model、effort、permission、
plan 与 agent preset，提供 `current()` 和 `subscribe(listener)`。它**不**累计任何
token 或耗时——统计只有 `acpSessionStats` 这一套口径。

内置 `status-view` Client Plugin 正是普通消费者：它注册本地 `/status` 命令，用
`tuiOverlay.openView()` 打开一个 markdown 状态视图——运行状态事实取自
`acpSessionStatus.current()`，token/turn/step/LLM/tool/TTFT/rate 全部取自
`acpSessionStats.current()`（与 composer dock 里的 `stats-view` 同一快照来源，
两处读数不会漂移）。Rust painter 只渲染序列化后的 `TuiNode`，不读自己的
transcript accumulator。没有 Client 树的运行（demo、独立 painter）由 Rust 的
精简 fallback 只画运行状态与 ACP 事实，同样不读 token/耗时累计器。

## 当前可调用：`tuiTheme`

`palette` 必须符合 [tui-palette.v0.schema.json](tui-palette.v0.schema.json)。

| 字段 | 规则 |
|---|---|
| `id` | 稳定字符串，与已有 id 冲突则 `register` 失败 |
| `label` | 给 `/theme` 看的短名 |
| `dark` / `light` | 各一份 **完整** token → `#RRGGBB`。缺任意 token、或多了不认识的名字，都失败 |

Token 名封闭：

`bg` `surface` `panel` `fg` `fg_secondary` `fg_tertiary` `caption` `brand` `brand_soft` `bubble_bg` `bubble_fg` `border` `code_bg` `ok` `warn` `err` `hint` `chip_bg`

内置 `default` 仍是现在的冷蓝灰皮，永远不能替换或删除。`ctrl+t` 只在当前主题的
dark/light 之间切。`/theme` 是一个特殊的 **Theme Plugin 单选开关**：选择一个 id
会启动它所属的 Client Plugin，并停止此前选中的 Theme Plugin；选择 `default` 会
停止当前动态 Theme Plugin。

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

宿主声明三个 list slot：根级独立右栏 `chrome.right`、位于 composer 上方的
`conversation.input.dock`，以及紧贴 composer 下方的
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

两个 composer dock 使用相同 API，只需把 `name` 改为对应 slot。需要独立一行或可能
换行的 Plan、任务、Goal 放 `conversation.input.dock`（占 cap 行，与 Tip 竞争同一行、
优先于 Tip）；环境统计等紧凑读数放 `conversation.composer.dock`（内置 stats-view
插件就是默认消费者）。完整内容应由本地 command 打开 overlay，而不是把多行内容塞进
紧凑统计席。

## 当前可调用：`tuiOverlay`

`openSlider(options, handlers)` 提供数值滑条；`openView({ id, title, nodes })` 提供
只读 `TuiNode[]` 弹窗。两者共享一个 modal 席位，返回的 `close()` 和插件 Fiber 同
生命周期。View 支持上下翻阅，Enter 提交、Esc 取消；它是通用节点视图，不认识 Plan。

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

## 加载

Cordis **没有**「挂在某个插件实例下面」的父子指针。`tui-theme`、配色包、
`tui-cordis-client-runner` 和 `tui-runner` 在 profile 里是 **平级 insert**。

真正等 TUI 能力的协议是服务名：

1. `tui-theme` / `tui-slots` 分别提供 `ctx.tuiTheme` / `ctx.tuiSlots`。
2. `tui-cordis-client-runner` 注入两者，向 Host 发布 inspect 目录并执行动态 `code.client`。
3. 第三方插件声明对应 inject；服务不在就 PENDING，不靠 yaml 行序。
4. 静态插件的 disposer 由 Cordis 收集，动态插件的贡献由 Client runner 收集。

不要在 runner 里 `ctx.plugin(ember)` 来叠第三方。第三方包由组合层作为 sibling
挂载。Web 的 `ctx.slots` 与终端的 `ctx.tuiSlots` 是不同服务。

本地包走 `dsh plugin add ./path`，加载 **package exports**。新 insert 一行要重开 TUI 或以后的 `/refresh`。已挂配色包改源码后的热换按阶段落地。

## 后续

`conversation.chat` 等会话槽仍未开放；ACP `session/update` 继续是 transcript 真源。
