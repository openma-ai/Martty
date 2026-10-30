<!--
合并前写全下面六段。放在本描述或一条评论里均可，但必须对应当前 head SHA。
不适用的段落写「不适用」并说明原因。填写说明见 CONTRIBUTING.md「证据报告」。
-->

## Issue 关联与关闭关键字 / Issue linking

<!--
base 决定用哪一类关键字：
- base 是默认分支 main → 写 Closes #N（Fixes / Resolves 等价），一个 issue 一个关键字。
- base 不是 main（堆叠进 feat/* 等）→ 写 Refs #N 或 Part of #N，不写关闭关键字。
集成 PR 合并进 main 时，把子 PR 提到的 issue 全部汇总成 Closes #…。
-->

- Base 分支:
- 关联 issue:

- [ ] base 是 `main`：正文写 `Closes #N`（也可用 `Fixes` / `Resolves`）
- [ ] base 不是 `main`（例如堆叠进 `feat/*`）：正文写 `Refs #N` 或 `Part of #N`，不写 `Closes` / `Fixes` / `Resolves`；关闭由合并进 `main` 的集成 PR 承担
- [ ] 只在提交主题里写 `(#N)` 不会关闭 issue：GitHub 只在 PR 合并进默认分支时按正文的关闭关键字关闭

## 问题 / 动机

<!-- 缺陷：贴在真实产品上跑过的命令、版本和原样输出。新能力：谁在什么场景下需要它。 -->

- [ ] 缺陷复现来自真实产品（命令 + 输出），或本段已说明这不是缺陷

## 根因

<!-- 代码或外部依赖的哪一层。上游变更和本仓库的缺陷分开写。 -->

## 改动说明

<!-- 做了什么、刻意没做什么。点名关键文件。 -->

## 验证证据

- Head SHA:
- CI（workflow、job、该 SHA 的 run 链接）:
- 测试（名称与通过数）:
- UI / 可见行为（截图或录屏；图片可放在孤立分支 `pr-assets`，用 raw 链接）:

- [ ] 上面的 CI 链接就是当前 head SHA，不是更早的 push

## 未验证的部分

<!-- 没跑什么、为什么。mock / fixture / 测试替身和真实产品或上游分开写。 -->

## 风险与回滚

<!-- 最坏情况，以及 revert 这篇 squash 提交或发修复版本的办法。 -->
