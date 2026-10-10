//! Terminal default-background sync (OSC 11/111) end-to-end coverage (#145):
//! emit on start, restore on every exit path, NO_COLOR suppression, tmux.
//! These tests run the real binary on a real PTY — the PTY itself acts as a
//! terminal that ignores OSC 11, so they also prove output stays well-formed
//! on terminals without the extension.

#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io::{ErrorKind, Read};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    const OSC_11_SET: &[u8] = b"\x1b]11;";
    const OSC_111_RESET: &[u8] = b"\x1b]111\x1b\\";
    const ALT_SCREEN: &[u8] = b"\x1b[?1049h";

    fn spawn_tui(extra_env: &[(&str, &str)]) -> (File, Child) {
        let mut master = -1;
        let mut slave = -1;
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0,
            "allocate a real pseudo-terminal"
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        // openpty defaults to a 0x0 window; give the TUI a real size.
        let winsize = libc::winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize) },
            0,
            "set PTY window size"
        );
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_martty"));
        cmd.arg("--demo")
            .env("TERM", "xterm-256color")
            .env_remove("TERM_PROGRAM")
            .stdin(Stdio::from(slave.try_clone().expect("clone PTY stdin")))
            .stdout(Stdio::from(slave.try_clone().expect("clone PTY stdout")))
            .stderr(Stdio::from(slave));
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        let child = cmd.spawn().expect("start TUI on PTY");
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK,) },
            0
        );
        (master, child)
    }

    /// Collect output until `needle` appears, the child exits, or the
    /// deadline hits; returns the accumulated transcript.
    fn collect_until(
        master: &mut File,
        child: &mut Child,
        output: &mut Vec<u8>,
        needle: &[u8],
        timeout: Duration,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let mut chunk = [0_u8; 8192];
            match master.read(&mut chunk) {
                Ok(0) => {}
                Ok(read) => output.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
                Err(error) => panic!("read PTY: {error}"),
            }
            if needle.is_empty()
                || output.windows(needle.len()).any(|w| w == needle)
            {
                return true;
            }
            if child
                .try_wait()
                .expect("poll TUI")
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
        !needle.is_empty() && output.windows(needle.len()).any(|w| w == needle)
    }

    fn signal_and_wait(master: &mut File, child: &mut Child, output: &mut Vec<u8>, sig: i32) -> std::process::ExitStatus {
        assert_eq!(unsafe { libc::kill(child.id() as i32, sig) }, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            collect_until(master, child, output, b"", Duration::from_millis(50));
            if let Some(status) = child.try_wait().expect("poll TUI exit") {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "TUI did not exit after signal {sig}: {}",
                String::from_utf8_lossy(output).escape_debug()
            );
        }
    }

    /// The TUI emits OSC 11 with the canvas colour once it draws, and
    /// `restore_terminal` hands the profile background back with OSC 111 on
    /// a signal-driven shutdown (#145). Runs with the session stats row
    /// (conversation.composer.dock) populated so the covered layout matches
    /// the reported one: the band sat *below* that stats row.
    #[test]
    fn osc11_emitted_and_osc111_restores_on_sigterm() {
        let (mut master, mut child) = spawn_tui(&[("DSH_TUI_DEMO_DOCK", "1")]);
        let mut output = Vec::new();
        assert!(
            collect_until(&mut master, &mut child, &mut output, ALT_SCREEN, Duration::from_secs(5)),
            "TUI never entered the alternate screen"
        );
        assert!(
            collect_until(&mut master, &mut child, &mut output, OSC_11_SET, Duration::from_secs(5)),
            "OSC 11 was never emitted: {}",
            String::from_utf8_lossy(&output).escape_debug()
        );
        assert!(
            collect_until(&mut master, &mut child, &mut output, b"2 turns", Duration::from_secs(5)),
            "stats row (composer.dock slot) did not render: {}",
            String::from_utf8_lossy(&output).escape_debug()
        );
        let status = signal_and_wait(&mut master, &mut child, &mut output, libc::SIGTERM);
        assert!(status.success(), "SIGTERM should end cleanly: {status}");
        assert!(
            output.windows(OSC_111_RESET.len()).any(|w| w == OSC_111_RESET),
            "OSC 111 restore missing on SIGTERM exit: {}",
            String::from_utf8_lossy(&output).escape_debug()
        );
    }

    /// SIGINT walks the same graceful `restore_terminal` path a user
    /// interrupt / normal teardown does: the background must be restored.
    #[test]
    fn restores_bg_on_sigint_exit() {
        let (mut master, mut child) = spawn_tui(&[("DSH_TUI_DEMO_DOCK", "1")]);
        let mut output = Vec::new();
        assert!(
            collect_until(&mut master, &mut child, &mut output, OSC_11_SET, Duration::from_secs(8)),
            "OSC 11 was never emitted"
        );
        let status = signal_and_wait(&mut master, &mut child, &mut output, libc::SIGINT);
        assert!(status.success(), "SIGINT should end cleanly: {status}");
        assert!(
            output.windows(OSC_111_RESET.len()).any(|w| w == OSC_111_RESET),
            "OSC 111 restore missing on SIGINT exit"
        );
    }

    /// The panic hook runs `restore_terminal`: even a crash must hand the
    /// profile background back (#145).
    #[test]
    fn panic_restores_terminal_bg() {
        let (mut master, mut child) =
            spawn_tui(&[("DSH_TUI_TEST_PANIC", "1")]);
        let mut output = Vec::new();
        assert!(
            collect_until(&mut master, &mut child, &mut output, ALT_SCREEN, Duration::from_secs(5)),
            "TUI never entered the alternate screen"
        );
        let status = signal_and_wait(&mut master, &mut child, &mut output, 0);
        assert!(
            output.windows(OSC_111_RESET.len()).any(|w| w == OSC_111_RESET),
            "OSC 111 restore missing on panic: {}",
            String::from_utf8_lossy(&output).escape_debug()
        );
        assert!(
            !status.success(),
            "a panicking child must not exit 0: {status}"
        );
    }

    /// Regression: NO_COLOR suppresses all colour output — including the OSC
    /// background sync — so the terminal's default fg/bg pair stays paired
    /// (light theme + NO_COLOR was white-on-white, #149).
    #[test]
    fn no_color_suppresses_osc_bg_sync() {
        let (mut master, mut child) =
            spawn_tui(&[("NO_COLOR", "1"), ("DSH_TUI_DEMO_DOCK", "1")]);
        let mut output = Vec::new();
        assert!(
            collect_until(&mut master, &mut child, &mut output, ALT_SCREEN, Duration::from_secs(5)),
            "TUI never entered the alternate screen under NO_COLOR"
        );
        // Give the run loop real time to (incorrectly) emit before judging.
        thread::sleep(Duration::from_millis(800));
        collect_until(&mut master, &mut child, &mut output, b"", Duration::from_millis(100));
        assert!(
            !output.windows(OSC_11_SET.len()).any(|w| w == OSC_11_SET),
            "OSC 11 emitted despite NO_COLOR: {}",
            String::from_utf8_lossy(&output).escape_debug()
        );
        let status = signal_and_wait(&mut master, &mut child, &mut output, libc::SIGTERM);
        assert!(status.success(), "SIGTERM should end cleanly: {status}");
    }

    /// Inside tmux the OSC sequences travel through tmux's own emulator:
    /// the TUI must still render, and a signal exit must still restore.
    #[test]
    fn renders_and_restores_inside_tmux() {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        let sock = format!("martty-osc-bg-{}", std::process::id());
        let tmux = |args: &[&str]| {
            Command::new("tmux")
                .arg("-L")
                .arg(&sock)
                .args(args)
                .output()
                .expect("tmux invocation")
        };
        let tag = format!("tmux-cov-{}", std::process::id());
        let pid_file = std::env::temp_dir().join(format!("martty-{tag}.pid"));
        // The wrapper records martty's pid so the test can signal it without
        // pattern-matching cmdlines (which would also match this wrapper and
        // the test's own shell). It stays alive to print the exit marker.
        let shell_cmd = format!(
            "DSH_TUI_DEMO_DOCK=1 {} --demo & echo $! > {}; wait; echo OSC_BG_DONE; sleep 60",
            env!("CARGO_BIN_EXE_martty"),
            pid_file.display()
        );
        assert!(
            tmux(&["new-session", "-d", "-x", "100", "-y", "30", &shell_cmd])
                .status
                .success(),
            "tmux session did not start"
        );
        let capture = |pattern: &str| -> bool {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                let out = tmux(&["capture-pane", "-p"]);
                let text = String::from_utf8_lossy(&out.stdout);
                if text.contains(pattern) {
                    return true;
                }
                thread::sleep(Duration::from_millis(200));
            }
            false
        };
        assert!(
            capture("Standard mode"),
            "TUI did not render inside tmux"
        );
        // SIGTERM the TUI directly (raw mode turns C-c into input bytes, not
        // a signal); it should exit gracefully and hand the pane back.
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .expect("read martty pid file")
            .trim()
            .parse()
            .expect("parse martty pid");
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
        assert!(
            capture("OSC_BG_DONE"),
            "TUI did not exit and restore inside tmux"
        );
        let _ = tmux(&["kill-server"]);
    }
}
