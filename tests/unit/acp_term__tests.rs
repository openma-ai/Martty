use super::*;

#[tokio::test]
async fn piped_create_captures_output_and_exit() {
    let broker = TerminalBroker::new(std::env::temp_dir());
    let id = broker
        .create("/bin/echo", &["acp-term".into()], None, &[], None)
        .expect("spawn echo");
    let status = broker.wait(&id).await;
    assert_eq!(status.exit_code, Some(0));
    let (output, truncated, _) = broker.output(&id);
    assert!(output.contains("acp-term"), "output: {output:?}");
    assert!(!truncated);
    broker.release(&id);
}

#[tokio::test]
async fn kill_then_wait_settles() {
    let broker = TerminalBroker::new(std::env::temp_dir());
    let id = broker
        .create("/bin/sleep", &["30".into()], None, &[], None)
        .expect("spawn sleep");
    broker.kill(&id);
    let status = tokio::time::timeout(Duration::from_secs(3), broker.wait(&id))
        .await
        .expect("wait after kill");
    assert!(status.exit_code.is_none() || status.exit_code != Some(0) || status.signal.is_some());
    broker.release(&id);
}

#[tokio::test]
async fn wait_after_the_exit_notification_already_fired_returns() {
    // notify_waiters() stores no permit: a wait() started around/after the
    // waiter thread's notification must still settle (registration happens
    // before the status re-check).
    let broker = TerminalBroker::new(std::env::temp_dir());
    let id = broker
        .create("/bin/true", &[], None, &[], None)
        .expect("spawn true");
    // Give the waiter thread time to reap and fire notify_waiters() with no
    // listeners registered.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let status = tokio::time::timeout(Duration::from_secs(3), broker.wait(&id))
        .await
        .expect("wait must not hang once exit was already recorded");
    assert_eq!(status.exit_code, Some(0));
    broker.release(&id);
}

#[tokio::test]
async fn concurrent_waiters_all_settle() {
    let broker = std::sync::Arc::new(TerminalBroker::new(std::env::temp_dir()));
    let id = broker
        .create("/bin/true", &[], None, &[], None)
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
            .expect("join");
        assert_eq!(status.exit_code, Some(0));
    }
    broker.release(&id);
}

