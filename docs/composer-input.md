# Composer 文本输入控件使用说明

Composer（底部输入区）自 v0.2.27 起基于 [`ratatui-textarea`](https://docs.rs/ratatui-textarea/latest/ratatui_textarea/) 构建，
封装在 `src/input/composer.rs` 的 `ComposerEditor` 中。本文档分两部分：
**终端用户操作指南**（怎么打字、编辑、撤销）和**开发者集成说明**（怎么接线、扩展、避坑）。

---

## 一、终端用户操作指南

### 1. 基本输入

| 按键 | 行为 |
|---|---|
| 普通字符 | 插入到光标处（中文、emoji、组合字符按字素处理） |
| `enter` | 发送草稿；草稿为空且有排队消息时，发送队首 |
| `shift+enter` / `ctrl+j` | 草稿内换行（多行草稿，readline 风格） |
| `ctrl+enter` | 立即发送（steer 当前轮次；macOS 也可用 `⌘⏎`）——老终端可能退化为普通 `enter` |
| `tab` | 补全 `/` 命令 / 切换选择 |
| `esc` | 空闲时清空草稿（`↑` 可召回）；运行中中断当前轮次 |

### 2. 光标移动

| 按键 | 行为 | 备注 |
|---|---|---|
| `←` / `→` | 左移 / 右移一个字符 | `ctrl+b` / `ctrl+f` 同 |
| `home` / `end` | 当前**屏幕行**首 / 尾（软换行感知） | `ctrl+a` / `ctrl+e`、`⌘←` / `⌘→` 同 |
| `alt+←` / `alt+→` | 按词左移 / 右移 | Linux/Win 用 `ctrl+←` / `ctrl+→`；macOS 也支持 `esc-b` / `esc-f` |
| `↑` / `↓` | 上 / 下一**屏幕行**（跨软换行，保持列） | 多行草稿内移动；空草稿时是输入历史 |
| `⌘↑` / `⌘↓` | 对话顶部 / 底部 | macOS |

### 3. 删除与剪切

| 按键 | 行为 |
|---|---|
| `⌫` | 删除光标前一个字符 |
| `delete` | 删除光标后一个字符（输入时 `ctrl+d` 同；空草稿时 `ctrl+d` 下翻半页） |
| `ctrl+w` | 删除光标前一个词（`ctrl+⌫`、macOS `⌥⌫` 同） |
| `ctrl+k` | 删除到当前屏幕行尾 |
| `ctrl+shift+k` | 删除整行（含换行，下一行上移） |
| `ctrl+u` | 删除到当前屏幕行首（空草稿时上翻半页；macOS `⌘⌫` 同） |
| `ctrl+c` | 清空草稿；无草稿时连按 2 次退出程序 |

### 4. 撤销 / 重做

| 操作 | 按键 |
|---|---|
| 撤销 | `ctrl+z`（macOS：`⌘z`） |
| 重做 | `ctrl+shift+z`（macOS：`⌘⇧z`） |
| 粘贴最近删除的文本 | `ctrl+y` |

- 草稿内任意编辑（插入、删除、整词删除、kill、粘贴）都进入撤销历史，**最多记住最近 50 步**
- `ctrl+k`/`ctrl+u` 等 kill 操作会把删除的文本存入 yank 缓冲区，`ctrl+y` 在光标处粘贴回来（readline 习惯）
- 撤销/重做后，光标回到对应编辑位置
- 与 `↑`/`↓` 的**输入历史**（历史提交记录）互不干扰
- 整文替换（如从输入历史回填、恢复排队消息编辑）会清空撤销历史

### 4.0 vim 模式（可选，默认关闭）

`/vim` 开启（`/vim off` 关闭），状态栏显示 `-- INSERT --` / `-- NORMAL --`。

| normal 按键 | 行为 |
|---|---|
| `h` `j` `k` `l` | 左 / 下 / 上 / 右（屏幕行） |
| `w` `b` `e` | 下一个词 / 上一个词 / 词尾 |
| `0` / `$` | 逻辑行首 / 行尾 |
| `x` / `X` | 删除光标处 / 光标前字符 |
| `dd` | 删除整行 |
| `u` | 撤销 |
| `p` | 粘贴 yank |
| `gg` / `G` | 文档首 / 末行行首 |
| `i` / `a` / `A` | 光标处 / 光标后 / 行尾进入插入 |
| `I` / `o` / `O` | 行首插入 / 下方新行 / 上方新行 |
| `esc` | 返回 normal（插入模式下不触发清草稿/中断） |

- 其他字母/数字/符号在 normal 模式被当作未定义命令消费，不会输入到草稿（要输入文字先 `i`）
- **Ctrl/⌘ 组合键始终不受影响**（steer、undo/redo、剪切等照常）
- 中文输入：normal 模式下请先 `i` 进入插入，再切换输入法
- `/vim` 开启后从 insert 模式开始，行为与原来完全一致，直到按 `esc`

### 4.1 键盘选区（shift 扩展选择）

| 按键 | 行为 |
|---|---|
| `shift+←` / `shift+→` | 向左 / 向右扩展选择（字符级） |
| `shift+↑` / `shift+↓` | 按屏幕行扩展（软换行感知） |
| `shift+home` / `shift+end` | 扩展到行首 / 行尾 |
| `shift+ctrl+←` / `shift+ctrl+→`（macOS `⌥⇧←/→`） | 按词扩展 |
| `⌘⇧←` / `⌘⇧→` · `ctrl+shift+e` | 扩展到行首 / 行尾（macOS / readline） |
| `ctrl+shift+c` | 复制选区到系统剪贴板（同时存入 yank） |
| `ctrl+x` | 剪切选区（系统剪贴板 + yank + 删除） |
| `ctrl+y` | 粘贴 yank |

- 复制 / 剪切对**鼠标拖选**同样生效（优先键盘选区，其次鼠标拖选）：
  拖选后 `ctrl+x` 剪切拖选内容、`ctrl+shift+c` 复制（高亮保留）

- 选区高亮为反色，与鼠标拖选一致
- 普通移动、编辑、点击光标会取消选区（shift 移动则扩展）
- 插入字符时如有选区，会先删除选区（替换语义）

### 5. 输入历史

- 草稿为空时按 `↑` / `↓` 浏览历史提交
- 浏览中编辑草稿即退出历史模式；`↓` 越过最新一条会恢复浏览前的草稿
- `esc` 清空草稿后用 `↑` 召回

### 6. 图片 chip（内联图片）

- 通过 `/image <路径> [说明]` 或 `ctrl+v`（剪贴板图片）在光标处插入 `[image n]` 芯片
- 芯片随草稿文本一起发送（图文交替）
- **悬停**芯片显示预览卡片；**鼠标点击**可定位光标
- `⌫` 或 `delete` 碰到芯片会整块删除（并取消暂存的图片），不会只删一个括号
- 编辑删除文本中的芯片文本时，托盘自动同步清理

### 6.1 `@file` 路径提及（文件浏览器）

在词边界输入 `@`（行首、空白或标点后；email、URL 中的 `@` 不触发）打开
workspace 文件浏览器，悬停在输入区上方：

| 按键 | 行为 |
|---|---|
| `↑` / `↓` | 移动选择（浏览器打开时输入区的上下键让位） |
| `←` | 上一级目录（纯浏览，不改草稿） |
| `→` / `tab` | 目录行下钻 / 文件行落定；下钻把 token 改写为 `@dir/` |
| `enter` | 落定：文件 → `@path`，目录 → `@dir/`（带空格路径自动转 `@"…"` 引号形式） |
| `ctrl+h` | 显示 / 隐藏 隐藏文件和隐藏目录 |
| `esc` | 关闭浏览器；同一 token 文本不变不会重新弹出 |
| `home` / `end` / `pgup` / `pgdn` | 首 / 尾 / 翻页 |

- 输入会实时驱动浏览器：`@src/m` 自动进入 `src/` 并选中第一个 `ma*` 条目
- 引号形式 `@"dir with space/` 在目录下钻时保持引号打开，可继续输入
- 选中的只是提示文本（`@path`），与文件内容、附件无关；宿主按
  workspace 相对路径解析（尾斜杠 = 目录）

### 7. 鼠标

| 操作 | 行为 |
|---|---|
| 左键点击 | 光标定位到点击位置（宽字符按中点分左右） |
| 左键拖选 | 高亮选中，松开时自动复制到剪贴板（`esc` 或再次点击清除高亮） |
| 滚轮 | 滚动对话区（输入区悬停时同样滚动对话） |

### 8. 文本粘贴

- 使用终端的系统粘贴（macOS `⌘v`、Linux/Win `ctrl+shift+v` 或鼠标中键）
- 粘贴内容中的换行会被折叠为空格（单行草稿习惯），`shift+enter` 可手动换行

### 9. 多行草稿的滚动

草稿超过输入区高度时自动出现光标跟随滚动（光标始终可见），无需手动操作。

> 完整键位表随时在程序内用 `/keys` 查看，并随反漂移测试保证与真实键位一致。

---

## 二、开发者集成说明

### 1. 架构

```
ComposerEditor (src/input/composer.rs)
├── textarea: TextArea<'static>   ← ratatui-textarea 编辑状态（行模型 + 50 步 undo 历史）
├── history / hist_pos / stash    ← 输入历史（原 Input 迁移，与编辑历史无关）
├── scroll_top                    ← 滚动镜像（与 widget 内部 viewport 同公式逐帧同步）
└── layout: Option<LayoutMap>     ← 布局镜像（库不公开的屏幕↔缓冲区映射，按需重建）
```

- **行模型**：`TextArea` 内部是 `Vec<String>` + `(row, col)` 光标；对外通过 `buf()`（join 成单串）、
  `cursor_char()`（全局 char 偏移，含换行）桥接旧 API 的调用点。
- **布局镜像 `LayoutMap`**：库的 `screen_map` 是私有的，但点击命中、芯片定位、视觉行移动、
  滚动同步都需要它。`LayoutMap` 用与库相同的 glyph-wrap 算法（grapheme 簇、tab 停靠、显示宽度）重建，
  保证与 widget 渲染永不错位。

### 2. 常用 API

```rust
// 编辑
insert_char(c) / insert_str(s) / insert_newline() / insert_char('\n')
backspace() / delete_forward() / delete_word_back()
delete_char_range(start, end)   // 整段删除（[image n] token 用）
undo() / redo()                 // 返回是否真的改了文本
clear() / set(String)           // set 会清空 undo 历史，不清 hist_pos

// 光标
move_left/right/up/down()       // ↑↓ 按屏幕行（软换行感知）
word_left/right()
line_start(w) / line_end(w)     // 视觉行（软换行边界上游语义）
kill_to_end(w) / kill_to_start(w)
set_cursor_char(offset)         // 点击定位
cursor_char()                   // 全局 char 偏移

// 文本访问
lines() -> &[String]            // 行模型（零拷贝）
buf() -> String                 // 全文（join，含换行）
is_empty()                      // 语义同旧 buf.is_empty()
char_to_rowcol(offset)

// 布局镜像（渲染 / 鼠标）
layout(wrap_width) -> &LayoutMap
screen_to_char(w, row, col)     // 屏幕格 → char 偏移（宽字符中点语义）
screen_to_char_end(w, row, col) // 拖选右端
visual_row_count(w)             // 软换行后的屏幕行数（composer 高度计算用）
screen_cursor()                 // 光标屏幕位置（需渲染过）
update_scroll_top(height)       // 滚动镜像前进（渲染后调用）

// 底层
textarea_mut() -> &mut TextArea // 直接操作 widget（会失效化布局镜像）
```

### 3. 渲染流程（`ui.rs::draw_input`）

```
1. 空草稿 → 自绘占位符（不渲染 TextArea）
2. 非空：
   a. prompt 列（"❯ " + 续行空格对齐）
   b. ta.set_style(style)  按 / ! 前缀着色
   c. f.render_widget(&*ta, text_area)   text_area = well 右移 prompt 宽
   d. buffer 修补：chip 高亮（layout 定位）+ 拖选反色
   e. 终端光标 = screen_cursor() - scroll_top
   f. update_scroll_top(height)  ← 必须每帧，且紧跟渲染
```

### 4. 键位接线（三步）

1. `src/input/keymap.rs`：`Action` 枚举加变体，`classify()` 加绑定
2. `src/app.rs::dispatch`：match 分支调用 `ComposerEditor` 方法；
   若属于文本编辑，加入 dispatch 开头的"编辑动作"列表（清拖选、关 slash 补全）
3. `KEY_ROWS` 加文档行（`/keys` 面板），反漂移测试会强制 probes 与 `classify` 一致

### 5. 注意事项 / 坑

- **`textarea_mut()` 会失效化布局镜像**：编辑 → 渲染 → `update_scroll_top` 的顺序不能乱；
  编辑后先别读 `layout()` 之外的状态。
- **`set()` 清空 undo 历史**（内部重建 `TextArea`），但**不清 `hist_pos`**（输入历史导航依赖它存活）。
- **零宽字符**（组合音标等）按 grapheme 簇占 1 列，与库一致；旧实现的逐 char 宽度计算已废弃。
- **word 边界含标点**（`fn foo(a)` 拆成 5 个词），比旧实现（仅空白）更 readline 化。
- **光标在软换行边界时**：显示在下一行首（widget 模型），但 `line_start/end`、kill 系列按上游行语义
  （与旧实现一致）。
- **不要调用 `textarea.input(key)`**：默认键位（如 `ctrl+c` 复制、`ctrl+x` 剪切）与 app 快捷键语义不同；
  全部走 `classify → dispatch` 显式调用。
- 未启用的库能力：搜索高亮（`search` feature）、行号、placeholder（自绘）。

### 6. `@file` 浏览器（`src/file_ref.rs`）

- **语法层零依赖**：`active_at_token(line, cursor_col)` 是纯函数（词边界、
  email/URL carve-out、`@"…"` 引号形式），`format_file_mention` 只产出提示文本。
- **浏览器是 ratatui-explorer**（issue #62 指定的 UI 控件）：`FileMenu` 包一个
  以 `cfg.workspace` 为根的 `FileExplorer`；目录浏览、排序、隐藏文件过滤都由库负责，
  Rust 侧不写文件系统遍历。
- **每次按键后的同步点在 `app.rs::refresh_file_menu`**（handle_key 末尾、vim
  消费分支、composer 鼠标点击三处）：重新扫描光标所在行的 token，query 变了才驱动
  浏览器（`apply_query` 对相同 query 是 no-op，箭头导航不会被重置）；token 消失、
  vim normal 或 slash 菜单打开时关闭。
- **Esc 的 dismissed 状态存在 App 上**（`file_menu_dismissed`，tag 见
  `file_ref::token_tag`）：菜单重建（`FileMenu` 每次打开重新 `build`）不能丢它。
- **主题适配**：`FileMenu::apply_chrome` 每帧用 `app.theme` token 重建
  ratatui-explorer 的 `Theme`（panel 面板、border 边框、brand 目录、chip_bg 选中行），
  palette 包和 locale 切换即时生效；与 slash 菜单弹层同几何（composer 上方，宽 72）。

### 7. 测试

- `tests/unit/input__composer__tests.rs`：布局镜像、点击映射、视觉行、kill、滚动镜像、undo/redo
- `tests/unit/app__mode_tests.rs` / `app__selection_tests.rs`：键位全链路、鼠标选择
- `tests/unit/file_ref__tests.rs`：语法（边界、引号、email、URL）、格式化、浏览器导航
- `tests/unit/app__at_menu_tests.rs`：键交互、token 替换、下钻、dismiss、vim/slash 共存
- `tests/unit/ui__tests.rs`：真实 buffer 渲染断言（chip 样式、拖选反色、多行堆叠、@ 弹层）
- `tests/unit/input__keymap__tests.rs`：键位表反漂移（每个文档行必须能被 classify 解析）
