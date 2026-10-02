# 贡献指南

**English.** Canonical contribution guide for the openma-ai org. It lives in `open-managed-agents` first; copy it to other org repos or an org `.github` repo and replace only **本仓库**. Shared rules: conventional-commit titles, squash merge, small PRs, green CI with a root cause for failures, a compatibility check on dependency bumps plus a follow-up bump downstream after release, private security reports, and an evidence report on every PR before merge.

以下各节是组织约定。「本仓库」只适用于 Martty（`openma-ai/Martty`）。复制到别的仓库时换掉「本仓库」一节，并改掉文中指向本仓库文件的链接。

## 分支 / Branches

从 `main` 拉出。人工分支用小写：

```text
<type>/<kebab-summary>
```

`type` 与 PR 标题类型一致。近期合并：`fix/ci-minio-image`（#227）、`feat/sql-realtime-fanout`（#222）、`refactor/split-node-assembly`（#234）、`docs/discord-community`（#197）。关联 issue 时把编号放进名字，例如 `fix/196-session-update-idle`。

工具前缀保持原样：`dependabot/…`、`codex/…`、`cursor/…`。deepseek-harness-acp 的 dsh 升级分支是 `codex/bump-dsh-<version>`。

一个分支一件事。跟上 `main` 用 rebase。

## PR 标题与 squash / PR titles

标题用 [Conventional Commits](https://www.conventionalcommits.org/)。squash 之后它就是 `main` 上的提交说明，GitHub 再追加 `(#编号)`：

```text
<type>(<scope>): <祈使句，说明做了什么>
```

`scope` 可省略。常用 type：`feat` `fix` `refactor` `perf` `docs` `test` `ci` `chore`。依赖用 `chore(deps):`。一篇 PR 一个 type。近期合进去的标题也不都是这个格式：#224 是 `fix+feat(...)`；#237 是 `Workspace persistence semantics: durable_mount vs fenced checkpoint_restore, shared Session outputs`；#239 是 `CMA retry_status semantics + live-found fixes (...)`。新 PR 用单一 conventional type。

发版提交的主题是 `release: vX.Y.Z`（见「发布」），普通 PR 不用这个前缀。

合并方式是 **squash**。本仓库近期 `main` 上每篇 PR 是一个单父提交，主题即 PR 标题。#202 写明仓库不接受 merge commit，因此把多篇依赖 PR 合成一篇再 squash。deepseek-harness-acp 历史上有过 merge commit（#30）；新 PR 按 squash 合。

## 小 PR / Small PRs

一次改一个问题或一个职责。`main-node` 控制面拆分是一串短 PR（#225、#228–#234），每篇只动一层。文档、重命名、行为变更分开。

lockfile 冲突时可以把多篇依赖更新合成一篇，跑一次完整 CI（#202 包含 #201–#206）。描述里列出被包含的 PR。

## 证据报告 / Evidence report

**合并前，PR 描述或一条评论里必须有证据报告，并且对应当前 head SHA。** 缺段，或证据还停在旧 SHA 上，就不合并。仓库没有把这件事做成 status check：作者填写，维护者核对。模板是 `.github/pull_request_template.md`。

六段都要出现。没有内容就写「不适用」并给一句原因。

### 问题 / 动机

缺陷要有**在真实产品上**的复现：命令、版本、原样输出。只写推理不够。新能力写清谁在什么场景下需要它。

### 根因

写到代码或外部依赖的哪一层。上游变更（镜像仓库、npm 发布）和本仓库的缺陷分开写。

### 改动说明

做了什么、刻意没做什么。点名关键文件，不贴大段 diff。

### 验证证据

- 当前 head SHA。
- 该 SHA 上的 CI run 链接，写明 workflow 和 job。旧 push 的绿 run 不算。
- 跑过的测试名称和通过数（例如 `8 files / 42 tests`）。本地和 CI 都写。
- 改了 UI 或可见行为时，把截图或录屏嵌进 PR。可以直接拖进 GitHub。需要稳定链接时，推到孤立分支 `pr-assets`：

  ```bash
  git checkout --orphan pr-assets
  git rm -rf .
  mkdir -p pr-<编号>
  # 只放 png / webm。不要放密钥，也不要放未剪辑的大体积录屏。
  git add pr-<编号>
  git commit -m "pr-assets: <编号>"
  git push -u origin pr-assets
  ```

  链接形式：`https://raw.githubusercontent.com/openma-ai/<repo>/pr-assets/pr-<编号>/<file>`。`pr-assets` 只存证据，不在上面开发。

### 未验证的部分

写明没跑的检查和原因。作者自己的 mock、fixture、测试替身，与真实产品或上游行为分开。mock 通过不等于 KVM 沙箱、托管环境或下游仓库已经验证。

### 风险与回滚

最坏情况，以及怎么退回：revert 这篇 squash 提交，或发一个修复版本。发版和迁移要写用户会看到什么。

### 示例

#227 的缩写，只示范格式。新 PR 按自己的改动重写。

> **问题 / 动机。** `pnpm test:integration:storage` 在 CI run [36001613340](https://github.com/openma-ai/open-managed-agents/actions/runs/36001613340) 的 global setup 失败，测试还没开始。日志是 MinIO 匿名拉取 `401 unauthorized`。干净机器上 `docker pull quay.io/minio/minio@sha256:d249d1fb…` 同样 401。
>
> **根因。** 仓库代码没有变化。`quay.io/minio/minio` 停止匿名拉取。
>
> **改动说明。** 测试镜像改为可匿名拉取的 `cgr.dev/chainguard/minio`，并钉住 manifest digest。
>
> **验证证据。** 本地 `pnpm test:integration:storage`：8 files / 42 tests 通过。合并前该 PR head 上的 CI storage 步骤通过。
>
> **未验证的部分。** 这是 CI 用的 MinIO 镜像，不是产品运行时依赖。没有改 S3 条件写相关的产品代码，也就没有另做产品级 S3 手工验证。
>
> **风险与回滚。** 只影响存储集成测试。revert 该提交即回到旧镜像引用。

## CI / 必须是绿的

合并前，当前 head SHA 上该 PR 该跑的 CI 全部成功。失败先读日志，写出根因，再改代码或改测试。不要对同一 SHA 反复 Re-run，直到碰巧变绿再合。

Re-run 可以用来收集第二次日志。第一次红、第二次绿时，报告里写明两次差异（超时、外部注册表、被 concurrency 取消的 run）。说不清原因就继续查。#227 的处理是确认 MinIO 注册表 401，然后更换镜像。

`concurrency.cancel-in-progress: true` 会取消同一 ref 上还在跑的旧 workflow。被取消的 run 不是 flake；看新 SHA 上的 run。

## 依赖升级 / Dependency upgrades

Dependabot 和手工 lockfile 更新都要做兼容性检查。CI 变绿只是其中一步：

- 读上游 changelog / release notes，列出行为变化。
- 跑本仓库已有的兼容矩阵，而不是只跑默认单测。deepseek-harness-acp 的 job `dsh-compatibility` 按 `runtime/compatibility.json` 安装多个 `@deepseek-ai/dsh` 并做 profile smoke。定时 workflow `dsh-update.yml` 会打开 `chore: upgrade bundled dsh to <version>`。#33 给这个 workflow 加了 Cursor agent 复查；人仍然负责合并。
- 升级 PR 不顺便给本包打版本。dsh 自动 PR 的正文写明：This PR does not bump or release the ACP package。
- 适配修不好就不合并。

发布之后，下游另开 bump PR，把依赖改到刚发布的版本，并跑下游自己的 CI：

- Martty 的 `npm/package.json` 依赖 `@openma/deepseek-harness-acp`。CHANGELOG 记录过随 0.4.29、0.4.31 的升级；#135 跟上了 0.4.35 的打包修复。
- openma-common 打 tag 之后，两个消费仓库改到新 tag 并提交 lockfile（该仓库 `CONTRIBUTING.md` 的 release checklist）。

## 发布 / Release

以该仓库的 workflow 为准。组织里实际有两种。

**打 tag。** deepseek-harness-acp、Martty、openma-common：

1. 版本写进清单。Martty 还要求 tag、`npm/package.json`、`Cargo.toml` 一致（`scripts/check-release-tag.mjs`）。
2. dsh 与 Martty 在 `main` 上的发版提交主题为 `release: vX.Y.Z`（dsh `v0.4.36`、Martty `v0.3.0`）。openma-common 是发版 PR 合并后再打同名 tag。
3. `git tag vX.Y.Z && git push origin vX.Y.Z`。tag 指向 `main` 上的那次提交。
4. tag 触发发布：dsh `release.yml` 先确认 tag 在 `main` 上，再跑测试、dsh 兼容矩阵和 standalone smoke，然后用 npm OIDC 发布。Martty `package-npm.yml` 监听 `v*.*.*`。
5. openma-common 是 `private: true` 的 git 依赖：打 tag 后更新消费方，不发 npm。

**Changesets。** 用来发布 `@openma/cli` / `@openma/sdk`。步骤在「本仓库」。本仓库的 `version-pr` 会跑 MySQL 集成，但没有 `Enable KVM for Litebox`；没有 `/dev/kvm` 时 Litebox 用例会失败。

发版提交只含版本和 changelog。功能先进普通 PR。发版后按上一节给下游开 bump PR。

## 安全 / Security

私下报告，不要开公开 issue。本仓库没有 `SECURITY.md`。私下渠道写在「本仓库：Martty」。复制到别的仓库时改成那个仓库的私下渠道。

发行物里不带调试端口，也不带密钥：

- 发布的 Node 进程、镜像 `CMD`、安装包里不开 `--inspect`、`9229`，也不开 Chrome `--remote-debugging-port`。
- 镜像只暴露产品端口，不额外 `EXPOSE` 调试端口。
- `.env`、`.dev.vars`、token、keystore 不进 git、npm 包、GHCR 镜像或桌面安装包。

依赖安全公告单独修（Backchat 有 `chore: prepare Backchat v0.0.9 security release`）。修法仍走普通 PR 和证据报告；公告细节走私下渠道。

## 本仓库：Martty

组织约定从 [open-managed-agents `09bbbd37`](https://github.com/openma-ai/open-managed-agents/commit/09bbbd37b9cf3b2c62c4aa5df1298b2ff4c6043f)（#240）复制。上面只改了两处：文首「本仓库」那一句，以及安全一节里指向该仓库 `SECURITY.md` 的相对链接。本仓库原先没有 `CONTRIBUTING.md` 或 `SECURITY.md`。

仓库是 `openma-ai/Martty`（`npm/package.json` 的 `repository.url`，以及 `package-npm.yml` 里 `github.repository == 'openma-ai/Martty'`）。根目录没有 `package.json`。清单在 `npm/package.json`、`npm/creator/package.json`、`website/package.json`。`README.en.md` 的 Build from source 写明 JavaScript 依赖和测试脚本属于 `npm/`。

### 工具链

Rust 没有 `rust-toolchain` 文件，`Cargo.toml` 也没有 `rust-version`。edition 是 `2021`。`[profile.release]` 是 `lto = true`、`strip = true`。README「从源码构建」和 `README.en.md` 要求 Rust stable。`package-npm.yml` 的 `test` 与 `native` 执行 `rustup update stable` 和 `rustup default stable`，不钉次版本。`package`、`publish`、`release` 不跑 rustup。

Node 分三处，不要合成一个版本：

- 发布的 TUI 包 `npm/package.json`：`engines.node` 为 `>=22.19.0`。README 徽章和「从源码构建」写 Node.js 22.19+。
- `Package and publish npm` 的 `actions/setup-node@v6` 使用 `node-version: "24"`，在 `test`、`native`、`package`、`publish` 四个 job。`publish` 另有 `npm install --global npm@^11.15.0`。
- 网站 `website/.node-version` 在 2026-10-02 核对时是 `24.18.0`。`website/package.json` 的 `engines.node` 是 `>=24 <25`。`website-check.yml` 用 `node-version-file: website/.node-version`。

JS 依赖用 npm，不是 Corepack / pnpm install。CI 与 `README.en.md` 的安装命令是 `npm ci --prefix npm --ignore-scripts --no-audit --no-fund`。仓库没有 `packageManager` 字段。profile 安装矩阵需要 `PATH` 上的 pnpm；`test` job 用 `pnpm/action-setup@v6`，`version: "10.2.0"`（`README.en.md` 写了这个 CI 版本）。

`README.en.md` 写明 JS 测试里的本地 PTY fixture 用 Python 3 和 `pexpect`。`make real-agent-e2e` 会请求模型，是 opt-in（`Makefile` 注释、`AGENTS.md`）。

### 构建与测试

在仓库根目录，与 `README.en.md` 一致：

```bash
npm ci --prefix npm --ignore-scripts --no-audit --no-fund
cargo test --locked
cargo check --locked --tests
npm test --prefix npm
```

单个 JS 文件：`node --test scripts/<name>.test.mjs`。`npm/package.json` 的 `test` 脚本用 `node --test` 跑 `scripts/*.test.mjs`。`make rust-test` 与 `make rust-build` 经 `scripts/cargo-guard.sh`（`Makefile`）。`target` 大于 `DSH_TUI_RUST_CACHE_MAX_GIB`（默认 20 GiB），或 `target` 非空且磁盘可用空间低于 `DSH_TUI_RUST_DISK_MIN_GIB`（默认 10 GiB）时，脚本先对这个 `CARGO_TARGET_DIR` 执行 `cargo clean`。本机只打当前平台：`bash scripts/build-npm.sh`。无 TTY 看一帧：`cargo run --locked -- --dump-frame 100x34`。开发 profile：`make tui-test`（先构建 `target/debug/martty`，再 `dsh --profile tui-test`）。

Rust 单元测试在 `tests/unit/`，由所属 `src/*.rs` 末尾挂进来，例如 `src/demo.rs` 的 `#[cfg(test)] #[path = "../tests/unit/demo__tests.rs"]`。两个 workflow 里没有 clippy 或 rustfmt 步骤。`AGENTS.md` 也写了没有这两道门禁。小内存机器上链接测试二进制可能被杀掉；`AGENTS.md` 的重试是 `RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo test`。`release` 开了 LTO，本地核对用 debug 构建。

### CI

只有两个 workflow。

`Package and publish npm`（`.github/workflows/package-npm.yml`）在发往 `main` 的 pull request、`main` 的 push，以及 tag `v*.*.*` 上跑。没有 path 过滤，也没有 `concurrency`。

| Job id | 显示名 | 跑什么 |
|---|---|---|
| `test` | `Test unix`（`ubuntu-24.04`）、`Test windows`（`windows-2025`） | 顺序：Node 24、pnpm 10.2.0、`rustup update stable` 与 `rustup default stable`。tag 上才跑 `node scripts/check-release-tag.mjs "${{ github.ref_name }}" npm/package.json Cargo.toml`。然后 unix 跑 `cargo test --locked`，windows 跑 `cargo check --locked --tests`。之后两边 `npm ci --prefix npm --ignore-scripts --no-audit --no-fund`。unix 再 `npm test --prefix npm`。windows 再用 `rustc` 编译 `scripts/profile-smoke-launcher.rs`，并 `npm run test:profile-install-matrix --prefix npm`，环境变量 `DSH_TUI_MATRIX_CASE=missing`。`scripts/profile-install-matrix.test.mjs` 有五条 `matrixCase`：`missing`、`base-only`、`current-acp`、`twice`、`old-acp`。CI 这一步只跑 `missing` |
| `native` | `Build darwin-arm64`、`Build darwin-x64`、`Build linux-x64`、`Build linux-arm64`、`Build win32-x64` | `cargo build --release --locked --target`。目标与 runner：`aarch64-apple-darwin`（`macos-15`）、`x86_64-apple-darwin`（`macos-15-intel`）、`x86_64-unknown-linux-musl` 与 `aarch64-unknown-linux-musl`（`ubuntu-24.04`）、`x86_64-pc-windows-msvc`（`windows-2025`）。Linux 再跑 `node scripts/check-static-elf.mjs` 和 `node scripts/smoke-old-linux.mjs`（Ubuntu 20.04 容器；arm64 先装 QEMU） |
| `package` | `Assemble npm package` | 需要 `test` 和 `native`。`scripts/package-native.mjs package` 打平台包，`node scripts/package-alias.mjs npm npm-martty martty` 生成别名包，再 `npm pack` |
| `publish` | `Publish npm` | 仅 `startsWith(github.ref, 'refs/tags/')` 且 `github.repository == 'openma-ai/Martty'`。Environment `npm`，`id-token: write`。tag 匹配 `*-alpha.*`、`*-beta.*` 或 `*-rc.*` 时 dist-tag 为 `beta`，否则 `latest`。`npm publish --access public --tag "$DIST_TAG" --provenance`。先发 `openma-martty-*.tgz`，再发 `martty-[0-9]*.tgz` |
| `release` | `Create GitHub release` | 条件与 `publish` 相同，且需要 `publish`。`contents: write`。`gh release create` 附上 `dist/*.tgz`。tag 名含 `-` 时加 `--prerelease` |

`Website checks`（`.github/workflows/website-check.yml`）只在 `website/**` 或该 workflow 文件变化时，对 `main` 的 PR 和 push 跑。`concurrency` group 是 `website-check-${{ github.ref }}`，`cancel-in-progress: true`。Job `validate`，`ubuntu-24.04`，工作目录 `website`：`npm ci --ignore-scripts --no-audit --no-fund`，然后 `npm run ci`。`website/package.json` 里 `ci` 是 `npm test && npm run build && wrangler deploy --dry-run`。

### 发布

`scripts/check-release-tag.mjs` 的参数是 tag、一份 `package.json`、一份 `Cargo.toml`。tag 必须等于 `v` 加 npm `version`，且 `Cargo.toml` 里 `[package]` 的 `version` 必须与 npm 相同。workflow 只在 tag 上、只传入 `npm/package.json` 和 `Cargo.toml`。`npm-martty/` 在 `.gitignore`，脚本不读它。`scripts/package-alias.mjs` 每次从 `npm/` 生成别名目录，并拒绝别名 `version` 与源包不一致。`AGENTS.md` 写过 tag、`npm/package.json`、`Cargo.toml`、`npm-martty/package.json` 四处一致；tag 门禁实际执行的是上面这条命令。

`scripts/release.mjs` 只在干净的 `main` 上跑。它改 `npm/package.json`、`npm/package-lock.json`、`Cargo.toml`、`Cargo.lock`，提交主题是 `release: vX.Y.Z`，并在本地打同名 tag。它不改 `CHANGELOG.md`。用法：`node scripts/release.mjs <version> [--dry-run]`。成功后打印 `git push origin main --tags`。用户可见变更写在 `CHANGELOG.md` 的 `[Unreleased]`（该文件开头，以及 `AGENTS.md`）。

2026-10-02 在 `main` 上核对：`release: v0.2.40` 是 `1093f11`；`release: v0.3.0` 是 `38dc350`，tag `v0.3.0` 指向这次提交。`v0.3.1` 是附注 tag（对象 `c5e3b44`，tag 说明 `Martty 0.3.1`）。那次提交的主题是 `Package Martty native binaries by platform`，不是 `release: v0.3.1`。当时 `Cargo.toml` 与 `npm/package.json` 的 version 都是 `0.3.1`。#137 合入后这两处仍是 `0.3.1`，ACP 升级记在 `[Unreleased]`。

tag `v*.*.*` 走上一节的 `publish` 和 `release`。本仓库没有 `.changeset/`，也没有 `version-pr`。组织约定里「步骤在「本仓库」」说的是 open-managed-agents 原文里 `@openma/cli` / `@openma/sdk` 的 Changesets 步骤（`09bbbd37` 的 `CONTRIBUTING.md`），不是 Martty。

### dsh-acp 的后续 bump

`npm/package.json` 将 `@openma/deepseek-harness-acp` 钉死版本。2026-10-02 核对时是 `0.4.36`；`devDependencies` 里的 `@deepseek-ai/dsh` 是 `0.2.0-rc.2`。这个包是 dsh-acp。

规则：deepseek-harness-acp 每次兼容发布之后，本仓库另开一篇 bump PR。上游 `release.yml`（以 tag [`v0.4.36`](https://github.com/openma-ai/deepseek-harness-acp/releases/tag/v0.4.36) 上的文件为准）在发 npm 之前跑 job `dsh-compatibility`：按 `runtime/compatibility.json` 安装多个 `@deepseek-ai/dsh` 并做 profile smoke。bump PR 把 `npm/package.json` 和 `npm/package-lock.json` 改到刚发布的版本，适配器捆绑的 Host 代际变了就把 `@deepseek-ai/dsh` 一起改，然后跑本仓库 CI。这篇 PR 不给 Martty 改 version，也不打 tag。

已打开并合并的跟进（不是对每个上游 tag 的普查）：

- #111：`0.4.27` → `0.4.29`，Martty 版本停在 `0.2.33`。`CHANGELOG.md` 记了这次 bump。
- #127：`0.4.29` → `0.4.31`。同一篇里还有 `release: v0.2.38`（`b0e4e76`），并用 merge commit 合入（`90b0909`）。那次没有把升级和发版拆开。
- #135：`0.4.31` → `0.4.35`，Martty 版本停在 `0.2.39`。上游 [v0.4.35](https://github.com/openma-ai/deepseek-harness-acp/releases/tag/v0.4.35) 的说明是打包修复（deepseek-harness-acp #31）。
- #137：`0.4.35` → `0.4.36`，与上游 v0.4.36 同一天。Martty 版本停在 `0.3.1`。`CHANGELOG.md` 的 `[Unreleased]` 写了捆绑的 Harness `0.2.0-rc.2`，以及开发基线改为 `@deepseek-ai/dsh` `0.2.0-rc.2`。

新的 bump PR 按这条规则与发版拆开。#127 那种把 `release:` 放进同一篇的做法不再重复。

### 安全

没有 `SECURITY.md`。2026-10-02，`GET /repos/openma-ai/Martty/private-vulnerability-reporting` 返回 `{"enabled":false}`。安全问题不要开公开 issue，也不要改送到 open-managed-agents 的 advisory。本仓库还没有启用的私下入口。
