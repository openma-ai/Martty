#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io::{ErrorKind, Read};
    use std::os::fd::FromRawFd;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn sigterm_restores_mouse_raw_mode_and_alternate_screen() {
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

        let mut master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let mut child = Command::new(env!("CARGO_BIN_EXE_dsh-tui"))
            .arg("--demo")
            .env("TERM", "xterm-256color")
            .env_remove("TERM_PROGRAM")
            .stdin(Stdio::from(slave.try_clone().expect("clone PTY stdin")))
            .stdout(Stdio::from(slave.try_clone().expect("clone PTY stdout")))
            .stderr(Stdio::from(slave))
            .spawn()
            .expect("start TUI on PTY");

        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK,) },
            0
        );
        let mut output = Vec::new();
        let ready_deadline = Instant::now() + Duration::from_secs(3);
        while !output
            .windows(b"\x1b[?1049h".len())
            .any(|w| w == b"\x1b[?1049h")
        {
            let mut chunk = [0_u8; 8192];
            match master.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => panic!("read PTY startup: {error}"),
            }
            if Instant::now() >= ready_deadline {
                let _ = child.kill();
                panic!("TUI did not enter alternate screen before SIGTERM test");
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            child.try_wait().expect("check live TUI").is_none(),
            "TUI must still be running when SIGTERM is sent"
        );
        assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);

        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            let mut chunk = [0_u8; 8192];
            match master.read(&mut chunk) {
                Ok(0) => {}
                Ok(read) => output.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
                Err(error) => panic!("read PTY during shutdown: {error}"),
            }
            if let Some(status) = child.try_wait().expect("poll TUI exit") {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                panic!("TUI did not settle SIGTERM through its normal teardown");
            }
            thread::sleep(Duration::from_millis(20));
        };

        let drain_deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let mut chunk = [0_u8; 8192];
            match master.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= drain_deadline {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                // PTY masters report EIO after the slave closes on macOS/Linux.
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("read PTY transcript: {error}"),
            }
        }

        assert!(
            status.success(),
            "SIGTERM should become a clean app shutdown: {status}"
        );
        assert!(
            output
                .windows(b"\x1b[?1000l".len())
                .any(|w| w == b"\x1b[?1000l"),
            "mouse tracking must be disabled before exit: {}",
            String::from_utf8_lossy(&output).escape_debug()
        );
        assert!(
            output
                .windows(b"\x1b[?1049l".len())
                .any(|w| w == b"\x1b[?1049l"),
            "alternate screen must be left before exit"
        );
    }

    use std::os::fd::AsRawFd;
}
