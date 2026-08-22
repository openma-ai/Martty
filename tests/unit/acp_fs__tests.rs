use super::*;

#[test]
fn slice_text_matches_backchat_line_limit() {
    let text = "a\nb\nc\nd";
    assert_eq!(slice_text(text, None, None), text);
    assert_eq!(slice_text(text, Some(2), Some(2)), "b\nc");
    assert_eq!(slice_text(text, Some(1), Some(10)), text);
    assert_eq!(slice_text(text, Some(5), None), "");
}

#[test]
fn inside_cwd_is_the_root_or_a_child_not_a_sibling_prefix() {
    assert!(is_inside_cwd(Path::new("/w/src/a.rs"), "/w"));
    assert!(is_inside_cwd(Path::new("/w"), "/w"));
    assert!(is_inside_cwd(Path::new("/w/."), "/w"));
    assert!(is_inside_cwd(Path::new("src/a.rs"), "/w"));
    assert!(!is_inside_cwd(Path::new("/w-other/x"), "/w"));
    assert!(!is_inside_cwd(Path::new("/etc/passwd"), "/w"));
    assert!(!is_inside_cwd(Path::new("/w/../etc/passwd"), "/w"));
}

#[test]
fn write_then_read_round_trip_inside_temp() {
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-fs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let file = dir.join("nested/out.txt");
    write_text_file(&file, "hello\nworld").expect("write");
    assert_eq!(
        read_text_file(&file, Some(2), Some(1)).expect("read"),
        "world"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
