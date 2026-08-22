use std::env;
use std::process::{exit, Command};

fn main() {
    let script = env::var_os("DSH_TUI_SMOKE_SCRIPT")
        .expect("DSH_TUI_SMOKE_SCRIPT must point to profile-smoke-tui.mjs");
    let node = env::var_os("DSH_TUI_SMOKE_NODE").unwrap_or_else(|| "node".into());
    let status = Command::new(node)
        .arg(script)
        .args(env::args_os().skip(1))
        .status()
        .expect("failed to launch the profile smoke painter");
    exit(status.code().unwrap_or(1));
}
