//! Grammar + browser tests for the `@file` mention feature.

use super::*;
use std::fs;
use std::path::PathBuf;

// A tiny hand-rolled temp root keeps the browser tests hermetic (the repo
// deliberately has no tempfile dev-dependency).
struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "martty-file-ref-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp root");
        Self { path: dir }
    }

    fn file(&self, rel: &str) -> PathBuf {
        let p = self.path.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&p, "").expect("write temp file");
        p
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let p = self.path.join(rel);
        fs::create_dir_all(&p).expect("create temp dir");
        p
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// --- grammar --------------------------------------------------------------

#[test]
fn token_at_line_start() {
    let t = active_at_token("@foo", 4).expect("token");
    assert_eq!((t.start, t.end), (0, 4));
    assert_eq!(t.query, "foo");
    assert!(!t.quoted);
}

#[test]
fn bare_at_is_a_token() {
    let t = active_at_token("@", 1).expect("token");
    assert_eq!((t.start, t.end), (0, 1));
    assert_eq!(t.query, "");
}

#[test]
fn inline_token_after_space() {
    let t = active_at_token("look at @src/main.rs now", 16).expect("token");
    assert_eq!(t.query, "src/main.rs");
    assert_eq!(t.end, 20);
}

#[test]
fn caret_must_be_inside_or_on_the_token() {
    // Caret on the `@` itself counts as inside.
    assert!(active_at_token("@foo", 0).is_some());
    // Caret after the token (in the trailing space) is outside.
    assert!(active_at_token("@foo ", 5).is_none());
    // Caret before the token is outside.
    assert!(active_at_token("x @foo", 1).is_none());
}

#[test]
fn email_and_url_hosts_do_not_trigger() {
    assert!(active_at_token("mail me at a@b.com", 18).is_none());
    assert!(active_at_token("handle foo@bar", 12).is_none());
    assert!(active_at_token("https://user@host/path", 18).is_none());
}

#[test]
fn punctuation_is_a_boundary() {
    // The unquoted token runs to whitespace, so the closing paren stays
    // part of it (paths may contain parens); quote for exotic cases.
    let t = active_at_token("try (@src)", 8).expect("token");
    assert_eq!(t.query, "src)");
    assert_eq!(t.end, 10);
}

#[test]
fn quoted_token_allows_spaces() {
    let t = active_at_token("@\"my file.txt\"", 13).expect("token");
    assert!(t.quoted);
    assert_eq!(t.query, "my file.txt");
    assert_eq!(t.end, 13);
}

#[test]
fn unterminated_quote_runs_to_line_end() {
    let t = active_at_token("open @\"dir with space", 21).expect("token");
    assert!(t.quoted);
    assert_eq!(t.query, "dir with space");
    assert_eq!(t.end, 21);
}

#[test]
fn inner_at_inside_quotes_is_path_text() {
    let t = active_at_token("@\"a@b\" tail", 4).expect("token");
    assert_eq!(t.query, "a@b");
}

#[test]
fn last_token_wins_when_caret_is_in_it() {
    let t = active_at_token("@a @b", 4).expect("token");
    assert_eq!(t.query, "b");
}

#[test]
fn token_stops_at_whitespace() {
    let t = active_at_token("@foo bar", 4).expect("token");
    assert_eq!(t.query, "foo");
}

// --- formatting -----------------------------------------------------------

#[test]
fn plain_mention() {
    assert_eq!(format_file_mention("src/main.rs", false, false), Some("@src/main.rs".into()));
}

#[test]
fn dir_mention_keeps_trailing_slash() {
    assert_eq!(format_file_mention("src", true, false), Some("@src/".into()));
}

#[test]
fn whitespace_forces_quoted_form() {
    assert_eq!(
        format_file_mention("my dir", true, false),
        Some("@\"my dir/".into())
    );
    assert_eq!(
        format_file_mention("my dir/file.txt", false, false),
        Some("@\"my dir/file.txt\"".into())
    );
}

#[test]
fn quoted_dir_keeps_the_quote_open() {
    assert_eq!(format_file_mention("src", true, true), Some("@\"src/".into()));
    assert_eq!(
        format_file_mention("src/main.rs", false, true),
        Some("@\"src/main.rs\"".into())
    );
}

#[test]
fn unrepresentable_paths_are_none() {
    assert_eq!(format_file_mention("a\nb", false, false), None);
    assert_eq!(format_file_mention("a\"b", false, false), None);
    assert_eq!(format_file_mention("a\"b", false, true), None);
    assert_eq!(format_file_mention("", false, false), None);
}

// --- relative paths -------------------------------------------------------

#[test]
fn relative_under_base() {
    let base = PathBuf::from("/w");
    assert_eq!(relative_path(&base, &base.join("src/main.rs")), "src/main.rs");
    assert_eq!(relative_path(&base, &base), ".");
}

#[test]
fn relative_above_base() {
    let base = PathBuf::from("/w/sub");
    assert_eq!(relative_path(&base, Path::new("/w")), "..");
    assert_eq!(relative_path(&base, Path::new("/w/sib")), "../sib");
}

#[test]
fn relative_unrelated_falls_back_to_shared_root() {
    // Both live under `/`, so the relative spelling is `..`-prefixed;
    // only truly foreign roots (Windows drives) fall back to absolute.
    let base = PathBuf::from("/w");
    assert_eq!(relative_path(&base, Path::new("/etc/passwd")), "../etc/passwd");
}

// --- browser --------------------------------------------------------------

fn tree() -> (TempRoot, PathBuf) {
    let root = TempRoot::new("browser");
    root.dir("src/nested");
    root.file("src/main.rs");
    root.file("README.md");
    root.file("notes.txt");
    let base = root.path.clone();
    (root, base)
}

fn token(query: &str) -> AtToken {
    AtToken {
        start: 0,
        end: 1 + query.chars().count(),
        query: query.into(),
        quoted: false,
    }
}

#[test]
fn open_lists_the_workspace() {
    let (root, base) = tree();
    let menu = FileMenu::open(&base, 0, &token("")).expect("opens");
    let names: Vec<&str> = menu.explorer().files().iter().map(|f| f.name.as_str()).collect();
    // parent entry first, then dirs, then files — all alphabetical
    assert_eq!(names, ["../", "src/", "README.md", "notes.txt"]);
    drop(root);
}

#[test]
fn open_fails_silently_for_a_missing_workspace() {
    let missing = std::env::temp_dir().join("does-not-exist-martty-file-ref");
    assert!(FileMenu::open(&missing, 0, &token("")).is_none());
}

#[test]
fn query_navigates_the_directory_prefix() {
    let (root, base) = tree();
    let mut menu = FileMenu::open(&base, 0, &token("")).expect("opens");
    menu.apply_query("src/m");
    assert_eq!(menu.explorer().cwd(), &base.join("src"));
    assert_eq!(menu.explorer().current().name, "main.rs");
    drop(root);
}

#[test]
fn query_prefix_selects_best_match() {
    let (root, base) = tree();
    let mut menu = FileMenu::open(&base, 0, &token("")).expect("opens");
    menu.apply_query("re");
    assert_eq!(menu.explorer().current().name, "README.md");
    // directory wins a tie over a file with the same prefix
    menu.apply_query("src");
    assert_eq!(menu.explorer().current().name, "src/");
    drop(root);
}

#[test]
fn missing_middle_segment_treats_the_rest_as_one_last_segment() {
    let (root, base) = tree();
    let mut menu = FileMenu::open(&base, 0, &token("")).expect("opens");
    menu.apply_query("nope/main");
    // cwd stays at the workspace; nothing matches — selection unchanged
    assert_eq!(menu.explorer().cwd(), &base);
    drop(root);
}

#[test]
fn unchanged_query_preserves_arrow_navigation() {
    let (root, base) = tree();
    let mut menu = FileMenu::open(&base, 0, &token("")).expect("opens");
    menu.apply_query("");
    navigate(&mut menu, Input::Down);
    let selected = menu.explorer().selected_idx();
    assert_eq!(selected, 1);
    // Same query again: the browser must not reset the selection.
    menu.apply_query("");
    assert_eq!(menu.explorer().selected_idx(), selected);
    drop(root);
}

#[test]
fn left_moves_to_the_parent() {
    let (root, base) = tree();
    let mut menu = FileMenu::open(&base.join("src"), 0, &token("")).expect("opens");
    navigate(&mut menu, Input::Left);
    assert_eq!(menu.explorer().cwd(), &base);
    drop(root);
}

#[test]
fn current_mention_reflects_the_selection() {
    let (root, base) = tree();
    let mut menu = FileMenu::open(&base, 0, &token("")).expect("opens");
    menu.apply_query("src/m");
    assert_eq!(menu.current_mention().as_deref(), Some("@src/main.rs"));
    navigate(&mut menu, Input::Up); // nested/
    assert_eq!(menu.current_mention().as_deref(), Some("@src/nested/"));
    drop(root);
}

#[test]
fn spaced_paths_quote_automatically() {
    let root = TempRoot::new("spaced");
    root.dir("my dir");
    root.file("my dir/file with space.txt");
    let mut menu = FileMenu::open(&root.path, 0, &token("")).expect("opens");
    menu.apply_query("my dir/f");
    assert_eq!(
        menu.current_mention().as_deref(),
        Some("@\"my dir/file with space.txt\"")
    );
    drop(root);
}

#[test]
fn token_tag_distinguishes_quoted_and_query() {
    assert_eq!(token_tag(false, "re"), "re");
    assert_eq!(token_tag(true, "re"), "\"re");
    assert_eq!(token_tag(false, "readme"), "readme");
    assert_ne!(token_tag(false, "re"), token_tag(false, "readme"));
}

#[test]
fn ctrl_h_toggles_hidden_entries() {
    let root = TempRoot::new("hidden");
    root.dir(".git");
    root.file("README.md");
    let mut menu = FileMenu::open(&root.path, 0, &token("")).expect("opens");
    fn names(menu: &FileMenu) -> Vec<String> {
        menu.explorer()
            .files()
            .iter()
            .map(|f| f.name.clone())
            .collect()
    }
    assert_eq!(names(&menu), ["../", "README.md"]);
    navigate(&mut menu, Input::ToggleShowHidden);
    assert!(menu.explorer().show_hidden());
    assert_eq!(names(&menu), ["../", ".git/", "README.md"]);
    navigate(&mut menu, Input::ToggleShowHidden);
    assert!(!menu.explorer().show_hidden());
    assert_eq!(names(&menu), ["../", "README.md"]);
    drop(root);
}

#[test]
fn drill_rewrites_the_token_query() {
    let (root, base) = tree();
    let mut menu = FileMenu::open(&base, 0, &token("")).expect("opens");
    // Simulate the app's Tab drill on the src/ entry: retoken + apply.
    menu.retoken(
        0,
        &AtToken {
            start: 0,
            end: 6,
            query: "src/".into(),
            quoted: false,
        },
    );
    menu.apply_query("src/");
    assert_eq!(menu.explorer().cwd(), &base.join("src"));
    drop(root);
}
