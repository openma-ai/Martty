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
