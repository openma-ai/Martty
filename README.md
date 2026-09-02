<p align="center">
  <img src="assets/martty-lockup.svg" width="650" alt="Martty terminal lockup" />
</p>

<h1 align="center">Martty</h1>

<p align="center">
  DSH-first Agent TUI，使用与 DSH 同款的 Cordis 插件能力，也可连接其他兼容 ACP agent。
</p>

<p align="center">
  <a href="README.md">中文</a> · <a href="README.en.md">English</a>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/martty"><img src="https://img.shields.io/npm/v/martty?logo=npm&color=cb3837" alt="npm version" /></a>
  <a href="https://www.npmjs.com/package/martty"><img src="https://img.shields.io/npm/dm/martty" alt="npm downloads" /></a>
  <a href="https://github.com/openma-ai/Martty/actions/workflows/package-npm.yml"><img src="https://github.com/openma-ai/Martty/actions/workflows/package-npm.yml/badge.svg" alt="CI" /></a>
  <img src="https://img.shields.io/node/v/martty" alt="Node.js 18+" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT" /></a>
</p>

## 快速开始

> 通过 `npm install` 安装时，本机需已安装 [Node.js](https://nodejs.org/)，且可能需要 [pnpm](https://pnpm.io/)。
>
> AI agent / 自动化安装步骤见 [Agent 安装说明](docs/agent-install.md)。

全局安装后直接启动：

```sh
npm install --global martty
martty
```

不想安装到全局，可以直接运行：

```sh
npx --yes martty
```

Martty 内置 ACP 连接层，默认启动并连接 DSH。已有 DSH 环境时，也可以把 Martty
安装到独立 profile，交给 DSH 管理插件和升级：

```sh
npm install --global @deepseek-ai/dsh
dsh plugin --profile martty add martty@latest
dsh --profile martty
```

只想看界面，可以运行：

```sh
martty --demo
martty --demo-skin
```

## Agent UI

Martty 在终端中呈现完整的 agent 工作过程，包括流式回复、推理、工具调用、
subagent、Plan、token 用量和持久化会话。图片可以从文件或剪贴板加入 prompt；
工具输出、Markdown、代码块和图片预览都使用终端原生交互。

回合运行时可以继续排队消息，也可以立即 steer 当前 agent。会话通过 `/new`、
`/resume` 和 `--session-id` 管理，workspace、模型、权限和界面选择会随会话恢复。

<p align="center">
  <img src="assets/screenshots/agent-turn.png" width="720"
       alt="Martty 中的 Markdown 回复、工具调用和运行状态" />
</p>

常用操作：

| 按键 / 命令 | 行为 |
|---|---|
| `enter` | 发送消息；composer 为空且 Queue 非空时立即发送队首 |
| `ctrl+enter` | 立即 steer 当前 agent（macOS 也可用 `⌘⏎`） |
| `alt+↑` | 选择任意 Queue 条目；`↑/↓` 移动，`enter` 编辑，`ctrl+d` 删除 |
| `↓` · `←/→` · `enter` | 空输入时展开 Agent 导航、移动并打开；`esc` 折叠 |
| `esc` | 中断当前回合并保留草稿 |
| `/` | 打开命令与参数候选 |
| `/model` · `/agent` | 选择模型和 Agent Preset |
| `/harness [id]` | 在 TUI 内切换 Harness 并启动新的空会话 |
| `/permission` · `shift+tab` | 选择或轮换权限模式 |
| `/image <path>` · `/clip` | 添加本地图片或剪贴板图片 |
| `!cmd` | 在 workspace 的会话级本地 shell 中执行命令 |
| `/help` · `/keys` | 查看命令与快捷键 |
| `/vim` | 切换 vim 模态编辑（默认关闭；normal 模式 h/j/k/l 移动、dd 删行、i 插入） |

### 文本编辑（composer 输入框）

| 按键 | 行为 |
|---|---|
| `←` / `→` | 移动光标（`ctrl+b` / `ctrl+f`） |
| `↑` / `↓` | 按屏幕行移动（多行草稿） |
| `home` / `end` | 行首 / 行尾（`ctrl+a` / `ctrl+e`；macOS `⌘←/→`） |
| `alt+←` / `alt+→` | 按词移动（Linux/Win `ctrl+←/→`） |
| `⌫` / `delete` | 删除前 / 后一个字符 |
| `ctrl+w` | 删除光标前一个词 |
| `ctrl+k` / `ctrl+u` | 删到行尾 / 行首（macOS `⌘⌫` 删到行首） |
| `ctrl+shift+k` | 删除整行 |
| `ctrl+z` / `ctrl+shift+z` | 撤销 / 重做（macOS `⌘z` / `⌘⇧z`） |
| `ctrl+y` | 粘贴最近删除的文本（yank） |
| `shift+←/→/↑/↓` · `shift+home/end` | 扩展选区 |
| `ctrl+shift+c` / `ctrl+x` | 复制 / 剪切选区（键盘与鼠标拖选均适用） |
| `shift+enter` / `ctrl+j` | 草稿内换行 |

完整的编辑快捷键、鼠标操作（点击定位、拖选复制）与开发者集成说明，见
[Composer 输入控件文档](docs/composer-input.md)。

## 插件系统

Martty 的界面不是一组写死的开关。UI、主题、Slot、命令、Overlay 和 Session 视图
都由 Cordis Plugin 组合，并随 Plugin 生命周期一起加载和卸载。这套插件系统也是
Martty 的自进化路径：Agent 可以读取当前能力，创建新的界面 Plugin，并在运行中继续
观察和修改。

一个 Cordis Plugin Package 可以包含两个部分：

```text
Cordis Plugin Package
├── code.host       # 可选，运行在 Host Cordis tree
└── code.client     # 可选，运行在 Martty Client Cordis tree
```

只包含 `code.host` 是 Host-only，只包含 `code.client` 是 Client-only，两者都有就是
双向 Plugin。双向 Plugin 仍是一个 Plugin run；Client half 可以通过
`host.call(method, args)` 调用同一 run 的 Host half。

### 四个 Plugin 视图

| 视图 | 显示什么 |
|---|---|
| `/ui` | UI Plugin 列表，例如 Martty 与 DeepSeek；选择后切换整套 UI 组合 |
| `/theme` | Theme Plugin 列表；选择后切换配色及该 Plugin 的其他能力 |
| `/plugins` | 当前已经加载的静态 Plugin，只读 |
| `/cordis-plugins` | Cordis 模式刚创建的临时 Plugin；可随 run 停止、替换或回收 |

`/ui` 和 `/theme` 是视图，不是 Plugin 类型。列表中的 Plugin 可以是 Client-only，
也可以是同时带有 Host half 与 Client half 的双向 Plugin。

Theme Plugin 与明暗模式彼此独立。使用 `/theme` 选择 Theme Plugin，使用
`/theme toggle` 或 `ctrl+t` 切换当前 Theme Plugin 的 dark/light 变体。输入
`/theme ` 时，上拉候选会把 `toggle` 与 Theme Plugin 分区显示。

### 六个 Slot

带有 Client half 的 Plugin 可以通过 `tuiSlots.register` 向六个位置提供界面内容：

| Slot | 类型 | 位置与用途 |
|---|---|---|
| `welcome.hero` | single · root | 欢迎页品牌区 |
| `welcome.info` | single · root | 欢迎页版本、模型、workspace 与 session 信息 |
| `chrome.right` | list · root | 主界面右栏，适合监控面板和持续状态 |
| `conversation.input.dock` | list · session | 输入框上方，适合 Plan、任务和 Goal 摘要 |
| `conversation.navigation.dock` | list · session | composer 内部、输入区与模式行之间，适合 Agent、branch 与 session 导航 |
| `conversation.composer.dock` | list · session | composer 外层底部，适合 token、耗时等紧凑统计 |

节点类型、更新和卸载生命周期见 [Plugin API：`tuiSlots`](docs/plugins.md#当前可调用-tuislots)，
字段定义见 [`tui-node.v0.schema.json`](docs/tui-node.v0.schema.json)。

### 创建 Plugin

Creator 可以检查当前 Host 与 Client 暴露的 Service、Slot 和 Schema，再生成
`code.host`、`code.client` 或双向 Package。Client-only Artifact 可以保存到
`$MARTTY_HOME/plugins/<artifact-id>/plugin.json`；包含 `code.host` 的 Package
由当前 Harness 管理。

自进化过程是一个可观察的闭环：

```text
inspect → generate → run → observe → update / rollback → save
```

Creator 先读取真实 API，再运行生成的 Plugin。装载错误、Schema 错误和绘制结果都能
反馈到下一次修改；满意后再保存，不满意可以更新、停止或回滚。Plugin 只能使用公开的
Service 和语义节点，不能直接操作 TTY、raw mode 或终端坐标。

### 从临时 Plugin 升级为常驻 Plugin

Cordis 模式里的 Plugin 默认只属于当前进程。验证完成后，可以把它升级为重启后仍会
加载的常驻 Plugin：

```text
临时 Plugin → 验证当前 Package → 保存或打包 → 启动时加载
```

可用 `/liang on`、`/liang off` 显式控制。缺省关闭，`/liang on` 召唤。

Client-only Plugin 可以直接走短路径：

```text
cordis_define → cordis_run → tui_plugin_save → $MARTTY_HOME/plugins
```

Martty 会在下次启动时重新发现这个 Artifact。包含 `code.host` 的 Host-only 或双向
Plugin 不能写进 `.martty`；Creator 先用 `cordis_inspect_self` 读取已经验证的 Package
源码，再把它整理成普通 Cordis npm Package 或本地 Package，安装进 profile：

```sh
dsh plugin --profile martty add <package-or-path>
```

安装后的 Package 随 profile 启动，由 Harness 管理 Host half，并把 Client entry 交给
Martty。当前没有把任意动态 Package 一键转换为静态 Package 的 `promote` 命令；升级
会显式经过保存或打包，避免把一次临时 run 自动写入长期配置。

安装第三方 Package：

```sh
dsh plugin --profile martty add <package-or-path>
```

完整的 API、生命周期与示例见 [插件开发文档](docs/plugins.md)。

## 连接其他 ACP agent

Martty 的 ACP client 是内置的。默认配置连接 DSH；连接其他 ACP server 时，只需
修改 agent 或 stream 配置，不需要替换 Martty 的 ACP 层。

Standalone 模式可以指定启动命令：

```sh
DSH_TUI_AGENT="<acp-command> [args...]" martty
```

也可以把多个 ACP harness 保存到 Martty settings，并选择下次 standalone 启动使用的
一项：

```sh
martty harness list
martty harness add codex
martty harness add local --label "Local ACP" --command local-acp --arg --stdio
martty harness use local
martty --check-runtime
```

三个入口共享同一份 registry：可以直接编辑 `settings.json`，使用上述
`martty harness` CLI，或在运行中的 TUI 输入 `/harness` 打开原生单选表单；
`/harness <id>` 可直接切换。TUI 选择会立即替换 standalone ACP 子进程；若当前会话
已经发送过 prompt 或由 `/session` 恢复，则先确认，且原会话仍可从 `/session` 返回。

`harness list` 会列出已保存项、包内置 DSH runtime，以及 `PATH` 中名称以
`-acp` / `_acp` 结尾的可执行文件；当前项以 `*` 标记。`add` / `use` 写入
`$MARTTY_HOME/settings.json` 的 `harnesses` 与 `activeHarness`，并保留同文件中的主题、
语言和 UI Plugin 设置。选择在**下一次 standalone 启动**生效，启动时依次通过 ACP
`initialize` 与 `session/new` 连接选中的 Harness，因此欢迎页在首条消息前就能显示
Agent 上报的模型与 effort。这只绑定空会话，不会启动模型 turn。不会把旧 Harness 的会话带到新 Harness；
`dsh --profile martty` 的 Host runtime 仍由该 profile 所有。

`martty harness add codex` 是快捷配方，会保存通过 `npx` 启动
`@agentclientprotocol/codex-acp` 的配置；无需自己猜 Codex CLI 参数。

Standalone 启动优先级是 `--agent`、`DSH_TUI_AGENT`、产品注入的
`defaultHarness`、持久化的 `activeHarness`、包内置回退值。`/harness` 只更新
`activeHarness`，不修改产品默认值；`--agent` 仍是不改保存选择的一次性覆盖。

Cordis 嵌入场景可以使用 `config.agent: { command, args }` 启动 ACP server，或使用
`config.stream` 接入调用方已有的标准管道。单次运行也可以使用 `--agent` 与重复的
`--agent-arg` 覆盖启动命令。

只实现标准 ACP 的 agent 可以正常使用会话、认证、prompt、permission、配置与
`session/update`。支持 DSH Cordis 扩展的 agent 还可以提供动态 Plugin、Client 能力
发现和 Package RPC。

配置示例见 [Agent 接入](docs/agent-setup.md)。

## 运行架构

`dsh --profile martty` 启动两个进程。DSH Host 进程运行 Base Cordis tree、ACP Plugin
和 Agent；独立的 Martty Client 进程运行 UI、Theme、Slot、Overlay 与 Session Service。
两边通过 ACP 通信，不共享 Plugin Fiber 或 Service 对象。

Rust/ratatui painter 由 Martty Client 驱动并独占用户 TTY。ACP 使用 Client 进程的
stdin/stdout；用户 TTY 使用 fd 3/4，两条通道互不混用。

架构细节与 Mermaid 源文件：

| 文档 | 内容 |
|---|---|
| [运行架构](docs/architecture.md) | Host、Client、ACP 与 painter 的边界 |
| [Composer 输入控件](docs/composer-input.md) | 文本输入使用说明：快捷键、鼠标、撤销/重做与开发者集成 |
| [Agent 安装说明](docs/agent-install.md) | 面向 AI agent / 自动化：可验证的安装步骤与排查 |
| [Plugin API](docs/plugins.md) | Service、Slot、Overlay、生命周期与 Package RPC |
| [运行时 Mermaid](docs/diagrams/runtime-architecture.mmd) | DSH-first 双进程数据流 |
| [Plugin Mermaid](docs/diagrams/plugin-system.mmd) | Host half、Client half 与 Plugin 视图 |
| [ACP 连接 Mermaid](docs/diagrams/acp-connectivity.mmd) | 默认 DSH 与其他 ACP server 的连接方式 |
| [迁移计划](docs/migration.md) | 已完成能力与后续阶段 |

## 从源码构建

需要 Rust stable 和 Node.js 18+：

```sh
make rust-test
node --test scripts/package-native.test.mjs
bash scripts/build-npm.sh
```

开发环境使用：

```sh
make tui-test
```

GitHub Actions 会构建 macOS arm64/x64、Linux arm64/x64 和 Windows x64 原生二进制，
再将它们打包进 npm 发布物。

## 项目结构

| 路径 | 内容 |
|---|---|
| `src/` | Rust TUI、输入、绘制、ACP 状态与会话生命周期 |
| `npm/` | Cordis Client、内置 ACP、Plugin Runner、CLI 与原生二进制入口 |
| `scripts/` | 构建、打包、协议校验与资源生成 |
| `assets/` | Logo、截图与界面资产 |
| `docs/` | 架构、Plugin API、Schema、迁移与 Agent 接入文档 |

<details>
<summary><strong>从旧包名迁移</strong></summary>

旧的 DeepSeek Harness TUI 包名为 `@openma/deepseek-harness-tui`，旧 profile 通常叫
`tui`。迁移到 Martty：

```sh
dsh plugin --profile tui remove @openma/deepseek-harness-tui
dsh plugin --profile martty add martty@latest
```

旧配置和会话数据会在首次启动时复制到 Martty 路径，不会删除原数据。

</details>

## License

[MIT](LICENSE)。Martty 与 DeepSeek、xAI 无关联。
