# dsh-tui — 终端里的 DEEPSEEK HARNESS

中文 | [English](README.en.md)

一个 [grok-build](https://github.com/xai-org/grok-build) 风格的
[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 终端
agent UI，Rust（ratatui）实现。它**直接说 harness 的 SDK JSON-RPC stdio
协议** —— 链路里没有 Python SDK —— 整套界面按 DeepSeek Web UI 的调色板
1:1 上色，鲸鱼也在。

深度求索 · 探索未至之境 — *everything is a plugin.*

![dsh-tui — 鲸鱼 banner、DEEPSEEK HARNESS 字标、Into the Unknown](assets/screenshots/banner.jpg)

鲸鱼由 `scripts/gen_logo.py` 从 harness Web UI 实际下发的 favicon SVG
栅格化而来；字标是 figlet `ansi_shadow`，染成鲸腹灰，终端够宽时右侧跟一个
白边镂空的 `HARNESS`。`src/theme.rs` 的主题 token 与 Web UI
`design-platform.css` 的 `--dsw-*` 变量 1:1 对应（深浅两套，`/theme`
切换）。

## 它展示什么

Web UI 为一个会话呈现的一切，这里从事件流实时呈现：流式推理（可折叠的
`✻ thought · 3.2s · 14 lines`）、流式回复文本、每一次工具调用及其参数与
结果（`bash`、`str_replace_editor`、`read`…）、注入的 plugin 上下文、
subagent 生命周期、token 用量（含 cache 命中）、回合结束原因、运行时状态。

## 交互（致敬 grok-build）

| 按键 | 行为 |
|---|---|
| `enter` | 发送 · **回合进行中排队 follow-up**（协议原生 inbox） |
| `alt+enter` | 立即发送：打断当前回合，下一个跑它 |
| `esc` | 打断（草稿保留）· 空闲时 `esc esc` 清空草稿 |
| `ctrl+c` | 先清草稿 · 再打断 · `ctrl+c ctrl+c` 退出 |
| `↑` | 提示词历史（输入框为空时） |
| `!cmd` | 跑一条本地 shell 命令（客户端执行，不经过 agent） |
| `/` | 斜杠菜单，前缀过滤（`/help /new /model /mode /effort /permission /plan /theme /liang …`） |
| `ctrl+m` | 模型选择器（宿主目录）→ 推理力度选择器 |
| `shift+tab` | 轮换权限预设 · `/permission` 打开选择器 |
| `ctrl+e` | 展开全部思考/工具输出 · `ctrl+t` 主题 · `ctrl+l` 清屏 |
| `pgup/pgdn` `ctrl+u/d` | 滚动 · 支持鼠标滚轮 · `end` 跟住尾部 |
| 鼠标拖选 | 聊天区划选 —— **松手即复制** · 双击复制单词 |

agent 的接缝都是真的，不是装饰：`/mode` 选 agent 预设 —— standard ·
code · minimal · creator（plugin 模式下换成宿主真实的 `agentPresets`
名册；在会话第一个 prompt 时组装），`/model` 在 plugin 模式下即时切换，
`/effort off|high|max` 设定本会话的推理力度，`/plan` 把 plan 模式透传给
宿主。

<p align="center">
  <img src="assets/screenshots/commands.jpg" width="536"
       alt="窄终端上的斜杠菜单 — /liang 高亮，小人在旁听" />
</p>

剪贴板投递是 grok-pager 式的多级路由，绝不硬失败（`src/clipboard.rs`）：
原生工具（`pbcopy` / `wl-copy` / `xclip` / `xsel`）→ tmux paste buffer →
控制终端的 OSC 52（必要时套 tmux passthrough 信封）—— 本机、tmux、SSH
里复制都好使。

## 输入框宠物 · `/liang` 🤫

一个像素画 **梁文锋** —— 爱称 **小难梁** —— 蹲在输入框右缘；`/liang`
召唤。两个状态，跟随运行状态切换：

| 状态 | 精灵 | 气场 |
|---|---|---|
| idle | 🤫 手指抵唇 | 安静 —— 他在想 AGI |
| working | ⌨︎ 敲一台小终端 | 回合流式进行中 —— 亲自上手 |

这个梗，立此存照：DeepSeek 出了名低调的创始人 —— 传说他至今每篇论文都读、
GPU 排队的间隙还在写代码 —— 所以他自然也来你的状态角落兼职：模型思考时
嘘声示意全场安静，回合一开始就疯狂敲键盘。为什么叫小难梁：因为"难"是
家族事业 —— AGI 很难，但他在加班。

机制：帧是真像素（`assets/pet/liang-*.png`，RGBA 透明底），在会说 **kitty
graphics protocol** 的终端上直贴（ghostty · kitty · WezTerm，凭 `$TERM` /
`$TERM_PROGRAM` 嗅探；一次性 transmit-and-display，布局/状态变化时重发 ——
`src/pet.rs`）。不支持像素图的终端退回 `logo_data.rs` 的 XS 半块鲸鱼，
角落永不落空。`/liang` 开关（也支持 `/liang on|off`）；宽度低于 60 列时
他自己退场。

## 安装与运行

npm 包本身就是官方 `dsh` CLI 的 Cordis bundle —— `dsh plugin` 把剩余参数
转发给 profile 目录里的 pnpm。profile 是单包 pnpm workspace（根即唯一
成员），所以动根的命令要带 `-w`：

```sh
dsh plugin --profile tui add -w @openma/deepseek-harness-tui
dsh --profile tui
```

`dsh --profile tui --dump-config` 应能看到 bundle 以稳定插件 id
`tui-runner` 挂载。想从源码构建：`bash scripts/build-npm.sh` 产出
`dist/openma-deepseek-harness-tui-*.tgz`，同一条 `add -w` 命令接受这个
本地路径。

![一个真实的 plugin 模式回合 — 注入的上下文、read 调用、排队中的 follow-up](assets/screenshots/plugin-turn.jpg)

`npm/lib/index.js` 在宿主 TTY 上拉起原生二进制，并挂载官方
`@deepseek-ai/dsh-sdk-jsonrpc-server`，其传输经管道 fd 3/4 重定向
（`dsh-tui --attach-fds`）—— 两种模式下 TUI 说的是同一套协议，而 agent、
工具、providers、持久化全部来自 dsh profile —— 状态行会写
`connected · dsh-tui-shim`，凭据一栏如实显示 `host dsh (credential
seam)`。线级行为由 `scripts/test-attach.mjs` 验证。plugin 模式下没有
回合中打断（回合归宿主所有）。

## 协议

stdio 上的 NDJSON JSON-RPC 2.0（`src/proto.rs`）：请求 `initialize` /
`session/prompt` / `shutdown`；通知 `session.event`（持久化目录事件：
`assistant/chunk`、`tool/call`、`tool/result`、`turn/*`、`user/message`
…）、`session.status`、`subagent.started/finished`。standalone 模式下
esc 打断会杀掉 runtime 进程 —— 持久化的 JSONL 会话仍在，下一个 prompt
用同一个 session id 重新拉起。

## 布局

```
src/proto.rs       NDJSON JSON-RPC 客户端（spawn + attach 两种传输）
src/runtime.rs     runtime 二进制 / cordis 配置发现
src/controller.rs  生命周期线程：spawn、initialize、prompt、interrupt
src/events.rs      通知 → UiEvents（有单测）
src/transcript.rs  回滚区 cell + 折行 + 样式
src/app.rs         状态机与按键              src/ui.rs  绘制
src/theme.rs       Web UI 设计 token         src/logo.rs + logo_data.rs
src/pet.rs         /liang — kitty 图形宠物（idle/working 精灵）
src/clipboard.rs   路由式复制：原生工具 → tmux buffer → OSC 52
src/demo.rs        --demo 的脚本化线缆级回合
assets/pet/        小难梁精灵帧              assets/screenshots/
assets/promo/      社交卡片 + build.py（用产品自身像素合成）
npm/               双模式包（CLI shim + dsh bundle 插件）
scripts/           gen_logo.py · build-npm.sh · test-attach.mjs
```

MIT。与 DeepSeek、xAI 均无关联；grok-build 是交互参照，deepseek-harness
是底座。
