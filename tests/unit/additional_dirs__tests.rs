use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("martty-add-dir-{name}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn accepts_an_absolute_directory_and_drops_the_workspace() {
    let root = scratch("ok");
    let extra = root.join("lib");
    let workspace = root.join("app");
    fs::create_dir_all(&extra).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let extra = fs::canonicalize(&extra).unwrap();
    let workspace = fs::canonicalize(&workspace).unwrap();
    let home = fs::canonicalize(&home).unwrap();

    let accepted = validate(
        &[
            extra.to_str().unwrap(),
            workspace.to_str().unwrap(),
            extra.to_str().unwrap(),
        ],
        workspace.to_str().unwrap(),
        Some(home.to_str().unwrap()),
    )
    .unwrap();
    assert_eq!(accepted, vec![extra.to_string_lossy().into_owned()]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn rejects_relative_root_home_missing_and_workspace_only() {
    let root = scratch("bad");
    let workspace = root.join("app");
    let home = root.join("home");
    let file = root.join("notes.txt");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(&file, "x").unwrap();
    let workspace = fs::canonicalize(&workspace).unwrap();
    let home = fs::canonicalize(&home).unwrap();
    let file = fs::canonicalize(&file).unwrap();
    let cwd = workspace.to_str().unwrap();
    let home_s = home.to_str().unwrap();

    let relative = validate(&["lib"], cwd, Some(home_s)).unwrap_err();
    assert!(relative.contains("absolute path"), "{relative}");
    let root_err = validate(&["/"], cwd, Some(home_s)).unwrap_err();
    assert!(root_err.contains("filesystem root"), "{root_err}");
    let home_err = validate(&[home_s], cwd, Some(home_s)).unwrap_err();
    assert!(home_err.contains("home directory"), "{home_err}");
    let missing = validate(&["/no/such/martty-add-dir"], cwd, Some(home_s)).unwrap_err();
    assert!(missing.contains("existing directory"), "{missing}");
    let not_dir = validate(&[file.to_str().unwrap()], cwd, Some(home_s)).unwrap_err();
    assert!(not_dir.contains("existing directory"), "{not_dir}");
    let only_workspace = validate(&[cwd], cwd, Some(home_s)).unwrap_err();
    assert!(only_workspace.contains("workspace"), "{only_workspace}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn filesystem_root_is_only_the_root_component() {
    assert!(is_filesystem_root(std::path::Path::new("/")));
    assert!(!is_filesystem_root(std::path::Path::new("/tmp")));
}
