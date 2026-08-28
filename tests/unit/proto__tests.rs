use super::*;

#[cfg(unix)]
#[test]
fn kill_does_not_deadlock_when_stdout_closed_but_process_keeps_running() {
    // Regression: the reader thread must not hold the child lock across a
    // blocking wait() at stdout EOF — kill() needs that lock to SIGKILL a
    // wedged runtime, and the UI thread calls kill() on Esc.
    use std::os::unix::fs::PermissionsExt;
    let script = std::env::temp_dir().join(format!("martty-proto-wedge-{}", std::process::id()));
    std::fs::write(&script, "#!/bin/sh\nexec 1>&-\nsleep 30\n").expect("write stub runtime");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let (bus, _rx) = mpsc::channel();
    let proc = RuntimeProcess::spawn(script.to_str().unwrap(), &[], "/tmp", bus)
        .expect("spawn stub runtime");
    // Let the reader thread hit the EOF path before killing.
    std::thread::sleep(Duration::from_millis(300));

    let (tx, rx) = mpsc::channel();
    let killer = std::thread::spawn(move || {
        proc.kill();
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("kill() deadlocked on the child lock held by the reader's wait()");
    killer.join().expect("killer thread");
    let _ = std::fs::remove_file(&script);
}
