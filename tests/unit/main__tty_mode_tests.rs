#[cfg(unix)]
#[test]
fn repairs_a_tty_that_an_agent_restored_to_canonical_mode() {
    let mut master = -1;
    let mut slave = -1;
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0, "openpty: {}", std::io::Error::last_os_error());

    let mut before = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(slave, &mut before) }, 0);
    assert_ne!(before.c_lflag & libc::ICANON, 0);
    assert_ne!(before.c_lflag & libc::ECHO, 0);

    super::repair_raw_mode_fd(slave).expect("repair raw mode");

    let mut after = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(slave, &mut after) }, 0);
    assert_eq!(after.c_lflag & libc::ICANON, 0);
    assert_eq!(after.c_lflag & libc::ECHO, 0);

    unsafe {
        libc::close(slave);
        libc::close(master);
    }
}
