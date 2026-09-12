# ACP Harness 管理指南

Martty 是面向 DeepSeek Harness 和其他 ACP coding agents 的终端客户端。Harness 是它连接的 Agent 程序，不是模型名称。通过 ACP Registry，你可以先浏览可用程序，再配置启动方式，不必提前猜包名、可执行文件名或 Registry ID。

这篇指南区分三个操作：安装准备文件，配置保存启动命令，切换才连接 Agent 并创建会话。已配置不代表已认证，也不保证 Agent 所需的外部依赖和账号权限已经齐全。

## 从 ACP Registry 查找 Agent

安装 Martty 后，在系统终端执行：

```sh
martty harness find
```

目录合并官方 ACP Registry、本地程序和已保存配置。默认先用本地缓存或随包快照；需要更新时执行 `martty harness find --refresh`。刷新失败仍可浏览已有目录，但首次下载包需要网络。

也可以在 Martty 输入框执行 `/harness`，选择 **+ Add Harness…**，输入名称筛选。已安装或配置项与待下载项分组展示；当前 Harness 标为 `(current)` 并置顶，不能重复选择。目录中的 Codex、Claude Agent、Google Antigravity 等名称用于识别条目，CLI 操作则使用输出给出的 ID。

## CLI：配置下一次启动

从 `find` 输出复制目标的添加命令。以下 `<id>` 是占位符，必须替换为目录给出的 ID，不要原样输入尖括号，也不要用显示名称代替。

```sh
martty harness add <id>
martty harness use <id>
martty
```

`add` 准备并保存配置；`use` 更新下次 standalone 启动的默认项。两者都不会切换已打开的 Martty，也不会发起认证。最后的 `martty` 才启动 Agent，进行 ACP 初始化与空会话创建；这一步不发送模型 prompt。显式启动参数或产品宿主的强制配置可能覆盖保存的默认项。

用 `martty harness list` 检查已保存的配置。如果终端找不到 `martty`，可以使用 `npx --yes martty harness find`；在源码仓库根目录也可执行 `node npm/bin/martty.js harness find`。详细参数见 [Harness CLI 使用指南](harness-cli.md)。

## 本地路径、npx、uvx 和 binary 如何处理

发现可用的本地程序时，Martty 可以复用该路径，不修改系统 PATH。Registry 的 npx/uvx 条目使用它提供的启动 recipe；保存运行器命令不等于包已经下载，CLI 配置后可能在首次启动时下载。

binary 条目按操作系统与架构选择包，校验 SHA-256，并安装到 Martty 自有目录，例如 macOS 的 `~/.martty/bin/<id>/<version>/<platform>`。Windows 使用对应用户目录和平台包，不要求把安装目录加入系统 PATH。

缺少 npx 或 uvx 时，需要准备 Node.js/npm 或 uv；若该条目有当前平台可用的 binary 分发，也可使用该分发方式。Registry 中有些程序是适配器，仍依赖另一个 CLI。遇到 `executable not found` 时，应按错误提示补齐依赖，不能把适配器安装成功当作运行条件已齐全。

## TUI：下载完成后走正常切换

在 `/harness` 菜单选择其他已配置项并按 Enter，才是切换当前运行环境。空会话直接切换；已经发送 prompt 或恢复了历史时，先确认新建会话。切换执行 ACP `initialize` 和 `session/new`，不把旧对话当作新 Harness 的输入。

新增条目需要下载时，进度显示在面板内。Esc 隐藏面板，下载在 Martty 仍运行时继续；完成或失败后会给出提示。安装完成自动保存配置，但后台完成本身不切换。完成面板的 **Enter switch** 使用同一个正常切换入口，**Esc close** 只关闭面板。

保存的默认项只在 ACP 就绪后更新；连接失败保留原默认项。切换 Harness 与切换同一连接内的会话标签不同，后者见 [多会话与消息队列](sessions.md)。

## 认证与连接错误

Agent 要求登录时，在 Martty 执行 `/auth`，按它声明的方式完成网页、表单或终端认证。Agent 自己的 `/login` 命令仍归 Agent 处理。浏览器的“登录完成”不是最终状态：Martty 以 ACP `authenticate` 响应和后续会话结果为准，可用 `/status` 查看状态。

连接失败面板直接展示 Agent 返回的原因、结构化错误及可用的 stderr。先解决缺失程序、依赖、账号权限或服务端拒绝等具体原因，再按 Enter 重试；Esc 关闭后可另选 Harness。连接错误不等于下载失败，也不一定能靠重复登录解决。

## 移除配置和私有安装资源

先检查 ID，并退出其他仍使用目标 Harness 的 Martty 实例。清理前预览范围：

```sh
martty harness remove <id> --cleanup --dry-run
```

`martty harness remove <id>` 经确认只删除配置，并清除指向它的默认项。需要同时删除 Martty 独占的私有 binary 安装目录时，使用 `martty harness remove <id> --cleanup`。私有文件删除后需重新下载；全局程序、共享 npx/uvx 缓存、历史会话和凭据会保留，不安全或共享的清理目标会被拒绝。

TUI 中选中非当前的已保存项按 Delete，选择仅移除配置或连同私有安装清理，再确认完整路径。删除流程内 Esc 逐级返回，不执行删除。完整面板操作见 [Harness TUI 使用指南](harness-tui.md)。
