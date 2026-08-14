use std::process::Command;

#[test]
fn help_names_the_current_expand_shortcut() {
    let output = Command::new(env!("CARGO_BIN_EXE_dsh-tui"))
        .arg("--help")
        .output()
        .expect("run dsh-tui --help");

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
