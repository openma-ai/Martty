use super::*;

fn true_command() -> &'static str {
    if cfg!(target_os = "macos") {
        "/usr/bin/true"
    } else {
        "/bin/true"
    }
}

#[tokio::test]
async fn piped_create_captures_output_and_exit() {
    let broker = TerminalBroker::new(std::env::temp_dir());
    let id = broker
        .create("/bin/echo", &["acp-term".into()], None, &[], None)
        .expect("spawn echo");
    let status = broker.wait(&id).await.expect("wait echo");
    assert_eq!(status.exit_code, Some(0));
    let (output, truncated, _) = broker.output(&id).expect("output echo");
    assert!(output.contains("acp-term"), "output: {output:?}");
    assert!(!truncated);
    broker.release(&id).expect("release echo");
}

#[tokio::test]
async fn kill_then_wait_settles() {
    let broker = TerminalBroker::new(std::env::temp_dir());
    let id = broker
        .create("/bin/sleep", &["30".into()], None, &[], None)
        .expect("spawn sleep");
    broker.kill(&id).expect("kill sleep");
    let status = tokio::time::timeout(Duration::from_secs(3), broker.wait(&id))
        .await
        .expect("wait after kill")
        .expect("wait settles");
    assert!(status.exit_code.is_none() || status.exit_code != Some(0) || status.signal.is_some());
    broker.release(&id).expect("release sleep");
}

#[tokio::test]
async fn wait_after_the_exit_notification_already_fired_returns() {
    // notify_waiters() stores no permit: a wait() started around/after the
    // waiter thread's notification must still settle (registration happens
    // before the status re-check).
    let broker = TerminalBroker::new(std::env::temp_dir());
    let id = broker
        .create(true_command(), &[], None, &[], None)
        .expect("spawn true");
    // Give the waiter thread time to reap and fire notify_waiters() with no
    // listeners registered.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let status = tokio::time::timeout(Duration::from_secs(3), broker.wait(&id))
        .await
        .expect("wait must not hang once exit was already recorded")
        .expect("wait settles");
    assert_eq!(status.exit_code, Some(0));
    broker.release(&id).expect("release true");
}

#[tokio::test]
async fn concurrent_waiters_all_settle() {
    let broker = std::sync::Arc::new(TerminalBroker::new(std::env::temp_dir()));
    let id = broker
        .create(true_command(), &[], None, &[], None)
        .expect("spawn true");
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(tokio::spawn({
            let broker = broker.clone();
            let id = id.clone();
            async move { broker.wait(&id).await }
        }));
    }
    for handle in handles {
        let status = tokio::time::timeout(Duration::from_secs(3), handle)
            .await
            .expect("waiter must settle")
            .expect("join")
            .expect("wait settles");
        assert_eq!(status.exit_code, Some(0));
    }
    broker.release(&id).expect("release true");
}

/// Yields the buffer one byte per read, forcing every multi-byte
/// character to be split across read boundaries.
struct OneByteAtATime(Vec<u8>, usize);

impl Read for OneByteAtATime {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1 >= self.0.len() {
            return Ok(0);
        }
        buf[0] = self.0[self.1];
        self.1 += 1;
        Ok(1)
    }
}

#[tokio::test]
async fn multi_byte_characters_split_across_reads_stay_intact() {
    // "你好 acp-term" — every byte arrives in its own read, so the
    // reader must carry partial UTF-8 sequences across chunks instead
    // of decoding each chunk in isolation.
    let payload = "你好 acp-term".as_bytes().to_vec();
    let rec = Arc::new(TerminalRec {
        child: Mutex::new({
            // A process that outlives the test is never spawned; the
            // child handle only needs to satisfy the record shape.
            let mut cmd = Command::new(true_command());
            cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            cmd.spawn().expect("spawn true")
        }),
        buf: Mutex::new(String::new()),
        truncated: AtomicBool::new(false),
        byte_limit: DEFAULT_BYTE_LIMIT,
        exit: Mutex::new(None),
        notify: Notify::new(),
    });
    spawn_reader(Arc::clone(&rec), OneByteAtATime(payload, 0));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let output = rec.buf.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if output.contains("acp-term") {
            assert!(output.contains("你好"), "output: {output:?}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "output never settled: {output:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
async fn unknown_terminal_ids_error_instead_of_default_success() {
    let broker = TerminalBroker::new(std::env::temp_dir());
    assert!(broker.output("term-missing").is_err());
    assert!(broker.wait("term-missing").await.is_err());
    assert!(broker.kill("term-missing").is_err());
    assert!(broker.release("term-missing").is_err());
}
