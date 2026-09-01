//! `@file` mentions: caret-token grammar plus the file-browser menu.
//!
//! The grammar mirrors the DeepSeek Harness `file-reference` grammar
//! (pure functions, no filesystem access): an `@` at a word boundary opens
//! a token that extends to whitespace, or to the closing quote for the
//! `@"…"` form (which allows spaces). Emails and URL hosts never trigger.
//!
//! The browser menu is built on [`ratatui-explorer`] (the issue-62 UI
//! control) and colored from the app [`Theme`] tokens — `panel`/`border`/
//! `fg`/`brand`/`chip_bg` — so it tracks dark/light, palette packs and
//! both UI presets like every other piece of painter chrome. The Rust
//! painter owns the menu; no filesystem traversal is shipped to plugins.
//!
//! Typing keeps the token live: the query's directory prefix navigates
//! the browser (`@src/ma` opens `src/` and jumps to the first `ma*`), and
//! `Tab` on a directory drills down by rewriting the token to `@dir/`
//! (quoted form keeps the quote open: `@"dir/`). When the current
//! directory has no match, the follow search looks at bounded
//! subdirectories (skipping heavyweight trees) and hops the browser to
//! the closest entry, so typing `@abc` finds `docs/abc_ref.md` without a
//! leading path.

use std::path::{Component, Path, PathBuf};

use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui_explorer::{FileExplorer, FileExplorerBuilder, Theme as ExplorerTheme};

pub use ratatui_explorer::Input;

use crate::input::composer::ComposerEditor;
use crate::locale::Locale;
use crate::theme::Theme as AppTheme;

// --- grammar --------------------------------------------------------------

/// An active `@` token on one composer line, in char offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtToken {
    /// Char offset of the `@` within the line.
    pub start: usize,
    /// Char offset just past the token (exclusive).
    pub end: usize,
    /// Text after `@` (or after `@"` in the quoted form), quotes removed.
    pub query: String,
    /// `@"…"` form: the query may contain spaces and runs to the closing
    /// quote (or the line end when unterminated).
    pub quoted: bool,
}

/// The token the caret sits in, if any. `cursor_col` is a char offset.
///
/// Rules (mirroring the harness grammar):
/// - `@` must sit at a word boundary: line start, or after a non-word
///   char. Emails (`a@b`) and URL hosts (`https://user@host`) therefore
///   never trigger.
/// - The caret must be on or inside the token (`start <= col <= end`);
///   `@` alone (empty query) is a valid token.
/// - `@"…"` spans to the closing quote; `@` chars inside quoted regions
///   are not separate tokens.
pub fn active_at_token(line: &str, cursor_col: usize) -> Option<AtToken> {
    let chars: Vec<char> = line.chars().collect();
    let mut found: Option<AtToken> = None;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            let boundary = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
            if boundary {
                let quoted = chars.get(i + 1) == Some(&'"');
                let body = i + 1 + usize::from(quoted);
                let (end, query) = if quoted {
                    match chars[body..].iter().position(|&c| c == '"') {
                        Some(off) => (body + off, chars[body..body + off].iter().collect()),
                        None => (chars.len(), chars[body..].iter().collect()),
                    }
                } else {
                    let mut e = body;
                    while e < chars.len() && !chars[e].is_whitespace() {
                        e += 1;
                    }
                    (e, chars[body..e].iter().collect())
                };
                if i <= cursor_col && cursor_col <= end {
                    found = Some(AtToken {
                        start: i,
                        end,
                        query,
                        quoted,
                    });
                }
                // A quoted token owns everything up to its end; `@` inside
                // it is path text, not a new mention.
                if quoted {
                    i = end.max(i + 1);
                    continue;
                }
            }
        }
        i += 1;
    }
    found
}

/// Stable dismissal identity for a token: quote marker + query text.
/// Esc-dismissed tokens stay closed until this changes.
pub fn token_tag(quoted: bool, query: &str) -> String {
    format!("{}{}", if quoted { "\"" } else { "" }, query)
}

/// Turn a picked relative path into the mention text that replaces the
/// token. Returns `None` when the path is not representable (control
/// chars, or a `"` — the quoted form cannot nest quotes).
///
/// Whitespace forces the quoted form; directories keep their trailing `/`
/// and, quoted, keep the quote open so the user can keep drilling.
pub fn format_file_mention(rel: &str, is_dir: bool, quoted: bool) -> Option<String> {
    if rel.is_empty() || rel.chars().any(|c| c.is_control() || c == '"') {
        return None;
    }
    let mut path = rel.to_string();
    if is_dir && !path.ends_with('/') {
        path.push('/');
    }
    if quoted || path.chars().any(char::is_whitespace) {
        if is_dir {
            Some(format!("@\"{path}"))
        } else {
            Some(format!("@\"{path}\""))
        }
    } else {
        Some(format!("@{path}"))
    }
}

/// Workspace-relative spelling of `path` against `base`: `src/main.rs`
/// under the workspace, `..`-prefixed for ancestors, `.` for the base
/// itself, and the absolute path when the two share no root.
pub fn relative_path(base: &Path, path: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(base) {
        let s = rel.to_string_lossy();
        return if s.is_empty() { ".".into() } else { s.into_owned() };
    }
    let base_comp: Vec<Component> = base.components().collect();
    let path_comp: Vec<Component> = path.components().collect();
    let mut common = 0;
    while common < base_comp.len()
        && common < path_comp.len()
        && base_comp[common] == path_comp[common]
    {
        common += 1;
    }
    if common == 0 {
        return path.to_string_lossy().into_owned();
    }
    let mut out = String::new();
    for _ in common..base_comp.len() {
        out.push_str("../");
    }
    for c in &path_comp[common..] {
        out.push_str(&c.as_os_str().to_string_lossy());
        out.push('/');
    }
    if out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        ".".into()
    } else {
        out
    }
}

/// Match rank for one basename against a query segment: exact = 0,
/// prefix = 1, substring = 2, subsequence = 3 (fuzzy last resort), or
/// `None` when the name does not match at all. Case-insensitive;
/// directory names carry a trailing `/` that never counts.
fn match_score(name: &str, needle: &str) -> Option<u8> {
    let needle = needle.to_lowercase();
    let name = name.strip_suffix('/').unwrap_or(name).to_lowercase();
    let score = if name == needle {
        0
    } else if name.starts_with(&needle) {
        1
    } else if name.contains(&needle) {
        2
    } else if is_subsequence(&needle, &name) {
        3
    } else {
        return None;
    };
    Some(score)
}

/// Every char of `needle` appears in `hay`, in order — the fzf-style last
/// resort (`abc` matches `a1b2c3.rs`).
fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut chars = hay.chars();
    'needle: for n in needle.chars() {
        for c in chars.by_ref() {
            if c == n {
                continue 'needle;
            }
        }
        return false;
    }
    true
}

/// Best explorer row for a query's last segment: exact > prefix > contains
/// > subsequence, directories preferred over files, `../` never matched.
fn best_match(files: &[ratatui_explorer::File], last: &str) -> Option<usize> {
    let mut best: Option<(usize, u8)> = None;
    for (i, file) in files.iter().enumerate() {
        if file.name == "../" {
            continue;
        }
        let Some(score) = match_score(&file.name, last) else {
            continue;
        };
        let total = score * 2 + usize::from(!file.is_dir) as u8;
        if best.is_none_or(|(_, s)| total < s) {
            best = Some((i, total));
        }
    }
    best.map(|(i, _)| i)
}

/// Heavyweight directory names never descended into by the
/// cross-directory follow search.
const SKIP_DIRS: &[&str] = &[
    ".git", ".hg", ".svn", ".idea", "node_modules", "target", "dist",
    "build", ".next", ".cache", ".venv", "venv", "__pycache__", "coverage",
];

/// Directory-visit budget for one follow search, so a huge workspace cannot
/// stall the per-key refresh.
const SEARCH_DIR_BUDGET: usize = 512;

/// Max depth (directory levels below the search root) the follow search
/// explores.
const SEARCH_MAX_DEPTH: usize = 3;

/// Best entry matching `last` anywhere at or below `root` (the follow
/// search: typing `@abc` with no `abc*` in the current directory jumps the
/// browser to the closest `abc*` it can find). Returns the entry's parent
/// directory and its exact name. Hidden entries and heavyweight trees are
/// skipped; shallower matches beat deeper ones on equal rank.
fn cross_dir_search(root: &Path, last: &str) -> Option<(PathBuf, String)> {
    if last.is_empty() {
        return None;
    }
    let mut best: Option<(u8, usize, PathBuf, String)> = None; // (total, depth, parent, name)
    let mut budget = SEARCH_DIR_BUDGET;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        if depth >= SEARCH_MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Hidden entries stay invisible, exactly like the explorer's
            // default view (ctrl+h toggles the browser, not this search).
            if name.starts_with('.') {
                continue;
            }
            // Recurse into plain directories before any matching bookkeep:
            // heavyweight trees are noise for a follow search.
            if is_dir && !SKIP_DIRS.contains(&name.as_str()) {
                stack.push((entry.path(), depth + 1));
            }
            if let Some(score) = match_score(&name, last) {
                let total = score * 2 + u8::from(!is_dir);
                let better = best.as_ref().is_none_or(|(t, d, ..)| {
                    total < *t || (total == *t && depth < *d)
                });
                if better {
                    best = Some((total, depth, dir.clone(), name));
                }
            }
        }
    }
    best.map(|(_, _, parent, name)| (parent, name))
}

// --- the browser menu -----------------------------------------------------

/// Live `@file` browser state. Wraps a [`FileExplorer`] rooted at the
/// workspace; the explorer is the source of truth for the pick, the token
/// text is the source of truth for re-typing.
pub struct FileMenu {
    /// Workspace root the explorer started from (relative mentions are
    /// spelled against it).
    base: PathBuf,
    /// The ratatui-explorer widget (browser + its theme).
    explorer: FileExplorer,
    /// Composer row the token lives on.
    row: usize,
    /// Token char span inside that row.
    start: usize,
    end: usize,
    quoted: bool,
    /// Query the explorer currently mirrors; `apply_query` no-ops on the
    /// same query so arrow navigation is never reset by the per-key
    /// refresh.
    applied_query: String,
    /// Explorer cwd mirror (avoids redundant relisting and keeps the
    /// popup title in lockstep after `Left`/`Right` navigation).
    cwd: PathBuf,
    /// The follow search's last landing spot: `(parent, last_segment)`.
    /// Continuing to type the same segment keeps the browser there instead
    /// of snapping back to the workspace root (shrinking or editing the
    /// segment re-anchors at the root).
    followed: Option<(PathBuf, String)>,
}

impl FileMenu {
    /// Open the browser at `base` for a fresh token. Returns `None` when
    /// the workspace cannot be listed (the menu silently stays closed).
    pub fn open(base: &Path, row: usize, token: &AtToken) -> Option<Self> {
        let explorer = FileExplorerBuilder::default().working_dir(base).build().ok()?;
        Some(Self {
            base: base.to_path_buf(),
            explorer,
            row,
            start: token.start,
            end: token.end,
            quoted: token.quoted,
            applied_query: String::new(),
            cwd: base.to_path_buf(),
            followed: None,
        })
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn quoted(&self) -> bool {
        self.quoted
    }

    /// The token query text the browser currently mirrors (the source of
    /// truth for Esc-dismissal tags).
    pub fn token_query(&self) -> &str {
        &self.applied_query
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn explorer(&self) -> &FileExplorer {
        &self.explorer
    }

    /// Re-anchor the token span (text edited before `@`).
    pub fn retoken(&mut self, row: usize, token: &AtToken) {
        self.row = row;
        self.start = token.start;
        self.end = token.end;
        self.quoted = token.quoted;
        self.applied_query.clear();
        self.followed = None;
    }

    /// Drive the browser from the token query: directory segments before
    /// the last `/` are descended (the trailing slash is the drill
    /// delimiter — `@src` filters the workspace, `@src/` opens the dir),
    /// then the selection jumps to the best match of the last segment.
    /// When the current directory has no match at all, the follow search
    /// looks at (bounded) subdirectories and hops the browser to the
    /// closest entry — typing `@abc` jumps to `src/abc.rs` even when the
    /// workspace root has no `abc*`. No-op when the query did not change
    /// (preserves arrow navigation).
    pub fn apply_query(&mut self, query: &str) {
        if query == self.applied_query {
            return;
        }
        self.applied_query = query.to_string();
        let (nav, last_seg) = query.rsplit_once('/').map_or(("", query), |(d, l)| (d, l));
        // Start where the previous follow search landed when this keystroke
        // extends the same last segment (`@abc` → `@abcd` stays in the
        // found directory); a prefix navigation or a changed/shrunk segment
        // re-anchors at the workspace root.
        let mut cwd = if nav.is_empty() {
            if let Some((parent, prev_last)) = self.followed.take() {
                if last_seg.starts_with(&prev_last) && last_seg != prev_last {
                    parent
                } else {
                    self.base.clone()
                }
            } else {
                self.base.clone()
            }
        } else {
            self.followed = None;
            self.base.clone()
        };
        let mut nav_consumed = 0;
        for seg in nav.split('/') {
            match seg {
                "" | "." => nav_consumed += 1,
                ".." => {
                    if let Some(parent) = cwd.parent() {
                        cwd = parent.to_path_buf();
                    }
                    nav_consumed += 1;
                }
                seg => {
                    let candidate = cwd.join(seg);
                    if candidate.is_dir() {
                        cwd = candidate;
                        nav_consumed += 1;
                    } else {
                        break;
                    }
                }
            }
        }
        // Segments the navigation could not consume stay part of the
        // filter (`nope/main` filters for `nope/main` in the workspace).
        let mut last = nav.split('/').skip(nav_consumed).collect::<Vec<_>>().join("/");
        if !last.is_empty() {
            last.push('/');
        }
        last.push_str(last_seg);
        // The entry list only shows names matching this segment: exact /
        // prefix / contains / subsequence; `../` always stays so a filtered
        // view can still back out. Set the filter before any cwd change so
        // each listing is filtered in one pass.
        if last.is_empty() {
            let _ = self.explorer.remove_filter_map();
        } else {
            let needle = last.clone();
            let _ = self.explorer.set_filter_map(move |file: ratatui_explorer::File| {
                if file.name == "../" || match_score(&file.name, &needle).is_some() {
                    Some(file)
                } else {
                    None
                }
            });
        }
        if cwd != self.cwd && self.explorer.set_cwd(&cwd).is_ok() {
            self.cwd = cwd.clone();
        }
        if !last.is_empty() {
            if let Some(idx) = best_match(self.explorer.files(), &last) {
                self.explorer.set_selected_idx(idx);
            } else if !self
                .explorer
                .files()
                .iter()
                .any(|f| f.name != "../")
            {
                // The filter left nothing visible in this directory: follow
                // the query to the closest cross-tree entry (the same filter
                // applies to the landing listing). Remember the spot so the
                // next keystroke keeps following instead of snapping back to
                // the workspace root.
                if let Some((parent, name)) = cross_dir_search(&cwd, &last) {
                    if self.explorer.set_cwd(&parent).is_ok() {
                        self.cwd = parent.clone();
                        self.followed = Some((parent, last));
                        if let Some(idx) = self
                            .explorer
                            .files()
                            .iter()
                            .position(|f| f.name == name || f.name == format!("{name}/"))
                        {
                            self.explorer.set_selected_idx(idx);
                        }
                    }
                }
            }
        }
    }

    /// The mention text for the currently selected entry (used by both
    /// settle and drill). `None` when there is nothing to pick or the
    /// path is not representable.
    pub fn current_mention(&self) -> Option<String> {
        if self.explorer.files().is_empty() {
            return None;
        }
        let file = self.explorer.current();
        let rel = relative_path(&self.base, &file.path);
        format_file_mention(&rel, file.is_dir, self.quoted)
    }

    /// `@rel` popup title for the current cwd — shared by the painted
    /// explorer chrome and the no-match empty frame.
    pub fn chrome_title(&self, workspace: &str) -> String {
        let rel = relative_path(Path::new(workspace), &self.cwd);
        format!(" @{rel} ")
    }

    /// Bottom hint row — shared by the painted chrome and the no-match
    /// empty frame.
    pub fn chrome_hint(&self, locale: Locale) -> String {
        locale
            .tr(
                " ↑↓ move · ← parent · →/tab open · enter pick · ctrl+h hidden · esc close ",
                " ↑↓ 移动 · ← 上级 · →/tab 进入 · enter 选择 · ctrl+h 隐藏 · esc 关闭 ",
            )
            .to_string()
    }

    /// Browser theme built from the app theme tokens, refreshed every
    /// frame so palette-pack switches and locale changes land immediately.
    pub fn apply_chrome(&mut self, theme: &AppTheme, locale: Locale, workspace: &str) {
        let title = self.chrome_title(workspace);
        let hint = self.chrome_hint(locale);
        let explorer_theme = ExplorerTheme::default()
            .with_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .style(Style::default().bg(theme.panel)),
            )
            .with_style(Style::default().bg(theme.panel).fg(theme.fg))
            .with_item_style(Style::default().fg(theme.fg))
            .with_dir_style(Style::default().fg(theme.brand).add_modifier(Modifier::BOLD))
            .with_highlight_item_style(
                Style::default()
                    .fg(theme.brand)
                    .bg(theme.chip_bg)
                    .add_modifier(Modifier::BOLD),
            )
            .with_highlight_dir_style(
                Style::default()
                    .fg(theme.brand)
                    .bg(theme.chip_bg)
                    .add_modifier(Modifier::BOLD),
            )
            .with_highlight_symbol("▶ ")
            .with_highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .with_scroll_padding(1)
            .with_title_top(move |_| ratatui::text::Line::from(title.clone()))
            .with_title_bottom(move |_| ratatui::text::Line::from(hint.clone()));
        self.explorer.set_theme(explorer_theme);
    }
}

/// Replace the char span `[start, end)` on composer `row` with `text`,
/// leaving the caret after it.
pub fn replace_span(editor: &mut ComposerEditor, row: usize, start: usize, end: usize, text: &str) {
    let line_start = editor
        .lines()
        .iter()
        .take(row)
        .map(|l| l.chars().count() + 1)
        .sum::<usize>();
    editor.delete_char_range(line_start + start, line_start + end);
    editor.insert_str(text);
}

/// Navigate the explorer with a ratatui-explorer [`Input`] and keep the
/// cwd mirror in lockstep. `Left`/`Right` are pure browser navigation —
/// they move the pick position without rewriting the draft token (the
/// token text only changes through typing, `Tab` drill and `Enter`
/// settle).
pub fn navigate(menu: &mut FileMenu, input: Input) {
    if menu.explorer.handle(input).is_ok() {
        menu.cwd = menu.explorer.cwd().clone();
    }
}

#[cfg(test)]
#[path = "../tests/unit/file_ref__tests.rs"]
mod tests;
