use std::process::Command;

#[test]
fn help_names_the_current_expand_shortcut() {
    let output = Command::new(env!("CARGO_BIN_EXE_martty"))
        .arg("--help")
        .output()
        .expect("run martty --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is utf-8");
    assert!(
        stdout.contains("ctrl+o expand"),
        "help must advertise the active expand binding:\n{stdout}"
    );
    assert!(
        !stdout.contains("ctrl+e expand"),
        "help must not advertise the old expand binding:\n{stdout}"
    );
}

#[test]
fn help_names_demo_skin() {
    let output = Command::new(env!("CARGO_BIN_EXE_martty"))
        .arg("--help")
        .output()
        .expect("run martty --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is utf-8");
    assert!(
        stdout.contains("--demo-skin"),
        "help must advertise --demo-skin:\n{stdout}"
    );
    assert!(
        stdout.contains("--agent"),
        "help must advertise --agent:\n{stdout}"
    );
}
