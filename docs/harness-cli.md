# Harness CLI 使用指南

这些命令在系统终端中执行，不是在 Martty 输入框中执行。CLI 管理保存的 Harness
配置；已经运行的 TUI 使用 `/harness` 切换，见 [TUI 使用指南](harness-tui.md)。

## 准备命令入口

已安装 Martty 且命令在 PATH 中时，直接检查帮助：

```sh
martty harness --help
```

如果提示 `martty: command not found`，在 Martty **仓库根目录**使用源码入口：

```sh
node npm/bin/martty.js harness --help
```

为方便执行下文命令，可以在仓库根目录的 zsh/bash 中定义一个临时函数：

```sh
martty_repo="$PWD"
martty() {
  node "$martty_repo/npm/bin/martty.js" "$@"
}
```

函数仅在当前 shell 中有效，不会安装全局命令或修改 shell 配置。记录的是仓库绝对
路径，之后切换到其他工作目录仍能使用。如果 `node` 也不在 PATH 中，请先准备 Node.js，
或把函数中的 `node` 换成已安装 Node.js 可执行文件的绝对路径。
Harness 管理命令不启动原生 TUI；运行 TUI 还需准备当前平台的原生二进制，见
[从源码构建](../README.md#从源码构建)。

## 查找、配置、启动

先浏览目录，不必预先知道 Harness 的名字或 ID：

```sh
martty harness find
```

从输出中找到目标，复制它给出的 `martty harness add ...` 命令。下面的 `<id>`
表示该条目的 ID，**必须替换，不能连尖括号原样执行**；不要使用显示名称代替 ID。

```sh
martty harness add <id>
martty harness use <id>
martty
```

`add` 准备并保存配置，已保存的 recipe 会复用；npx/uvx 条目保存启动命令，包可能在
首次启动时下载。binary 条目安装到 Martty 的私有目录。配置成功不等于已经登录或
可以完成模型请求，Agent 还可能需要额外依赖或认证。

`use` 只更新下次 standalone 启动的 `defaultHarness`，不启动 Agent、不认证，也不
切换已经运行的 TUI。最后的 `martty` 才在当前工作目录启动，执行 ACP 初始化与
空会话创建；需要登录时在 TUI 中使用 `/auth`。只创建空会话不会发送模型 prompt。
显式启动覆盖或产品宿主提供的 `forcedHarness` 优先于保存的默认项。

## 查看配置与刷新目录

`list` 查看已保存、内置和发现的 Harness；`find` 浏览官方 ACP Registry 与本地候选。
默认使用本地目录快照，显式刷新失败时仍保留离线目录。

```sh
martty harness list
martty harness find
martty harness find --refresh
```

已知道目标时可加可选搜索词，例如 `martty harness find codex`；这只是过滤目录，
搜索词不一定就是后续 `add` / `use` 接受的 ID。

## 手动配置未收录的 Agent

只有 Registry 没有合适条目时才需要手写命令。下面的 `local` 是你自行命名的配置 ID；
替换示例路径与参数，确保目标程序提供 stdin/stdout ACP 服务，而不只是普通聊天 CLI。

```sh
martty harness add local --label "Local ACP" \
  --command "/absolute/path/to/local-acp" --arg --stdio
martty harness use local
martty
```

多个参数重复使用 `--arg`，包含空格的路径或参数加引号。此操作只写配置，不修改系统 PATH。

## 删除配置与私有资源

先用 `list` 确认 ID。清理安装资源前，退出其他正在使用该 Harness 的 Martty 实例。

```sh
martty harness remove <id> --cleanup --dry-run
martty harness remove <id>
```

第一条只预览清理范围，不修改文件。第二条列出范围并询问确认；默认只删除配置、清除
指向它的默认项，保留安装资源。若需要一并删除 Martty 独占的私有 binary 安装目录，使用：

```sh
martty harness remove <id> --cleanup
```

私有安装文件删除后需要重新下载。全局 npm/uv 工具、外部二进制、共享 npx/uvx 缓存、
历史会话和凭据不会删除；共享目录或符号链接等不安全目标会拒绝清理。
脚本中可显式加 `--yes` 跳过确认；没有终端且未加 `--yes` 时不会执行删除。

## 与 TUI 操作的区别

CLI 的 `add` / `use` 不影响已运行的会话。TUI 的 `/harness` 则作用于当前进程：选择
已配置项后走正常切换流程；当前会话已经发过 prompt 或恢复了历史时，会先确认。
TUI 下载完成面板的 Enter 复用同一个切换入口，Esc 仅关闭；后台下载完成本身不会自动切换。
