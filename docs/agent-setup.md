# AI Agent 安装与运行指南（dsh-tui）

本文档面向需要在本机安装、升级或验证 dsh-tui 的 AI agent / 自动化脚本。
所有命令都是非交互的；请按顺序执行并检查退出码，任何一步失败都应停止并报告
错误输出，不要继续后续步骤。

## 1. 前置条件

- **Node.js 18+**：执行 `node -v`，输出应为 `v18` 或更高版本。若命令不存在或
  版本过低，按 [Node.js 官网](https://nodejs.org/) 的指导安装，然后重新打开
  shell 再验证。
- **npm**：随 Node.js 提供，用 `npm -v` 验证。

## 2. 安装（推荐：作为 dsh 的 TUI surface plugin）

按顺序执行：

```sh
# 1) 官方 DeepSeek Harness（提供 dsh 命令）
npm install -g @deepseek-ai/dsh

# 2) 把 dsh-tui 作为 TUI surface plugin 装入 profile `tui`
dsh plugin --profile tui add @openma/deepseek-harness-tui@latest
```

注意：

- 第 2 步在 profile `tui` 不存在时会自动创建；
- 第 2 步会把 TUI 及其依赖图按 npm `latest` 标签重新解析，因此同一命令也可
  用于以后升级（见第 4 节）；
- 不需要全局安装 `dsh-tui`，也不需要另装 pnpm；
- 不需要手工整理已有的 ACP profile：TUI bundle 会停用目标 profile 中旧的
  ACP transport/provider 行，只挂载从 TUI 自身依赖图解析出的 ACP plugin。

## 3. 验证

```sh
dsh-tui --version      # 应输出 dsh-tui <版本号> 并退出 0
dsh --version          # 应输出版本号
```

**交互式启动**（需要 TTY，自动化场景慎用）：

```sh
dsh --profile tui
```

**无头验证**（不需要 runtime 或 API key，也不占用长期 TTY）：

```sh
dsh-tui --demo
```

`--demo` 能正常渲染即安装成功；空闲状态连按 2 次 `ctrl+c` 退出。

进阶检查：`dsh-tui --check-runtime` 会 spawn 并 `initialize` 一个真实 ACP
agent、打印协商信息后退出（`--demo` 模式下不可用）。

## 4. 升级

```sh
dsh plugin --profile tui add @openma/deepseek-harness-tui@latest
```

与安装是同一命令，可定期运行。

## 5. 卸载（可选）

```sh
dsh plugin --profile tui remove @openma/deepseek-harness-tui
npm uninstall -g @deepseek-ai/dsh
```

## 6. 故障排查

| 症状 | 处理 |
|---|---|
| `dsh: command not found` | 第 1 步失败，或 npm 全局 bin 不在 PATH；先 `node -v` / `npm -v` 验证 |
| `spawn dsh-acp ENOENT` | 未安装 `dsh-acp`；可改用 `dsh-tui --agent <cmd>` 指向其它 ACP server |
| `no native binary for ...` | 安装包不含当前平台；确认装的是最新版本并查看 README 的支持矩阵 |

## 7. 更多资料

- 完整使用说明：根目录 [README.md](../README.md)
- 插件 API：[docs/plugins.md](plugins.md)
- 运行架构：[docs/architecture.md](architecture.md)
