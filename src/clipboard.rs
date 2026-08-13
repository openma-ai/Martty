//! Clipboard delivery — a scaled-down homage to xai-grok-pager's routed
//! clipboard write (`clipboard_write_with_route`): best-effort legs that
//! never hard-fail the UI.
//!
//! Legs, in order:
//! 1. native tool — `pbcopy` on macOS, `wl-copy`/`xclip`/`xsel` elsewhere
//! 2. tmux paste buffer (`tmux load-buffer -`) when running inside tmux
//! 3. OSC 52 to the controlling terminal — always on Linux, and whenever
//!    tmux/SSH is detected or the native leg failed; wrapped in a tmux
//!    passthrough envelope (`ESC Ptmux; … ESC \`, payload ESCs doubled)
//!    when inside tmux, mirroring grok's `set_text_osc52`.

use std::io::Write;
use std::process::{Command, Stdio};

/// Copy `text` to the user's clipboard. Returns `true` when at least one
/// delivery leg succeeded (OSC 52 counts as "emitted", like grok's
/// unverified-remote case — the outer terminal may still ignore it).
pub fn copy(text: &str) -> bool {
    let native_ok = native_copy(text);
    let mut ok = native_ok;
    if in_tmux() {
        ok |= pipe_cmd("tmux", &["load-buffer", "-"], text);
    }
    if cfg!(target_os = "linux") || in_tmux() || is_remote() || !native_ok {
        ok |= osc52_copy(text);
    }
    ok
}

fn in_tmux() -> bool {
    std::env::var_os("TMUX").is_some()
}

fn is_remote() -> bool {
    ["SSH_TTY", "SSH_CONNECTION", "SSH_CLIENT"]
        .iter()
        .any(|k| std::env::var_os(k).is_some())
}

fn native_copy(text: &str) -> bool {
    if cfg!(target_os = "macos") {
        return pipe_cmd("pbcopy", &[], text);
    }
    for (cmd, args) in [
        ("wl-copy", &[][..]),
        ("xclip", &["-selection", "clipboard"][..]),
        ("xsel", &["-ib"][..]),
    ] {
        if pipe_cmd(cmd, args, text) {
            return true;
        }
    }
    false
}

/// Spawn `cmd` with `text` on stdin. These tools exit immediately after
/// reading stdin, so a plain wait is fine at this project's scale (grok
/// bounds the wait with a 2s deadline for wedged tmux servers).
fn pipe_cmd(cmd: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
    }
    matches!(child.wait(), Ok(status) if status.success())
}

fn osc52_copy(text: &str) -> bool {
    // Stay under common terminal OSC payload caps (~100KB post-base64).
    const MAX: usize = 72 * 1024;
    let mut t = text;
    if t.len() > MAX {
        let mut cut = MAX;
        while cut > 0 && !t.is_char_boundary(cut) {
            cut -= 1;
        }
        t = &t[..cut];
    }
    let seq = format!("\x1b]52;c;{}\x07", base64(t.as_bytes()));
    let framed = if in_tmux() {
        // tmux passthrough: outer sequence reaches the real terminal only
        // inside a DCS envelope with every ESC in the payload doubled.
        format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b"))
    } else {
        seq
    };
    let mut out = std::io::stdout();
    out.write_all(framed.as_bytes()).and_then(|_| out.flush()).is_ok()
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64("选中即复制".as_bytes()), "6YCJ5Lit5Y2z5aSN5Yi2");
    }
}
