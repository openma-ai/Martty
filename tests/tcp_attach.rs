use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn attach_tcp_connects_to_loopback_and_authenticates_first() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let address = listener.local_addr().expect("listener address");
    let token = "test-one-time-token";
    let mut child = Command::new(env!("CARGO_BIN_EXE_dsh-tui"))
        .args(["--attach-tcp", &address.to_string()])
        .env("DSH_TUI_ATTACH_TOKEN", token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn dsh-tui");

    let deadline = Instant::now() + Duration::from_secs(3);
    let (stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("accept loopback connection: {error}"),
        }
        if let Some(status) = child.try_wait().expect("inspect dsh-tui") {
            panic!("dsh-tui exited before connecting: {status}");
        }
        assert!(Instant::now() < deadline, "dsh-tui did not connect in time");
        thread::sleep(Duration::from_millis(10));
    };

    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set token read timeout");
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .expect("read authentication token");
    assert_eq!(line.trim_end(), token);

    let _ = child.kill();
    let _ = child.wait();
}
