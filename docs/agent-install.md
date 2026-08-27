# Agent 安装说明（给 AI / 自动化流程看）

本文档面向 **AI agent 与自动化脚本**：每一步都给出可执行命令与可验证的
退出码/输出，不依赖人类判断。安装后必须通过「验证」小节才能判定成功。

仓库：`https://github.com/openma-ai/Martty`（npm 包名 `martty`，旧名
`@openma/deepseek-harness-tui`，同一 CI 发布）。

---

## 0. 环境前置检查

执行以下检查，**全部通过才继续**：

```sh
# Node.js ≥ 18（必须）
node --version | awk -F. '$1 >= 18 {exit 0} {exit 1}' || { echo "need node >= 18"; exit 1; }
# pnpm（npm install 路径可能需要；缺失时按需安装）
command -v pnpm || echo "pnpm missing (may be needed for npm install path)"
# Rust stable + Cargo（仅源码构建路径需要）
rustc --version >/dev/null 2>&1 || echo "rust missing (only needed for source build)"
# dsh（仅 dsh plugin 路径需要）
command -v dsh || echo "dsh missing (only needed for the dsh plugin path)"
```

| 路径 | 需要 |
|---|---|
| `npm install` | Node ≥ 18，pnpm **可能需要**（缺失时报错再装，不必预先安装） |
| `dsh plugin ... add` | Node ≥ 18 + 已安装 dsh（DeepSeek Harness） |
| 源码构建 | Node ≥ 18 + Rust stable + Cargo（`--locked`） |

---

## 1. 安装路径 A：npm 全局安装（最快）

```sh
npm install --global martty
# 若因 pnpm 报错（postinstall 相关），先装 pnpm 再重试：
# npm install --global pnpm && npm install --global martty

command -v martty          # 必须输出路径
martty -V                  # 必须输出版本号（如 0.2.27）
```

不想全局安装：`npx --yes martty`（首次会拉包）。

## 2. 安装路径 B：dsh 管理（推荐给已有 DSH 环境）

```sh
npm install --global @deepseek-ai/dsh
dsh plugin --profile martty add martty@latest
dsh --profile martty
```

验证：

```sh
dsh plugin --profile martty list | grep martty   # 必须列出 martty
# 无 TTY 环境无法交互，用路径 A 的 martty -V 验证二进制即可
```

## 3. 安装路径 C：从源码构建

```sh
git clone https://github.com/openma-ai/Martty.git && cd Martty
make rust-test                                  # cargo test --locked（全量测试）
node --test scripts/package-native.test.mjs     # 原生二进制打包测试
bash scripts/build-npm.sh                       # release 构建 + 打包 npm tarball
```

产物：`dist/martty-*.tgz`（本机安装可用
`npm install --global ./dist/martty-<版本>.tgz`）。

---

## 4. 验证（所有路径通用）

```sh
martty -V                                   # 打印版本，退出码 0
martty --demo --dump-frame 100x30           # headless 渲染一帧文本，30 行输出
martty --demo --dump-frame 100x30 | grep -q '❯' && echo OK
```

- `--dump-frame` 不依赖 TTY，输出一屏文本帧，适合 CI / agent 冒烟。
- 交互验证（有 TTY 时）：`martty --demo` 应出现界面且不崩溃。

## 5. 失败排查

| 症状 | 处理 |
|---|---|
| `martty: command not found` | npm 全局 bin 不在 PATH；检查 `npm prefix -g` 并加入 PATH，或重装 |
| 安装报 pnpm 相关错误 | `npm install --global pnpm` 后重试（路径 A 的前置） |
| `dsh plugin ... add` 失败 | 确认 dsh 已安装且版本匹配；`dsh --version` |
| `--demo --dump-frame` 无输出 | 二进制损坏或平台不匹配（构建时用 `make rust-test` 自检） |
| 源码构建报 Rust 版本错误 | 升级 Rust stable：`rustup update stable` |
| 测试失败 | `cargo test --locked` 必须全绿才算构建可用；不要跳过 |

## 6. 已知约束

- 发布物含各平台原生二进制（Linux ELF / macOS / Windows），**不要跨平台拷贝
  target 产物**；源码构建只产出当前平台。
- `Cargo.lock` 必须提交（`--locked` 构建）。
- Windows 使用 token 认证的 loopback TCP 替代 fd 3/4，无需额外配置。
