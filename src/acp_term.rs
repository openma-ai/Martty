//! Piped ACP `terminal/*` broker (Backchat session terminals, not Terminal Auth).
//!
//! `ClientCapabilities.terminal` is `createTerminal` plus output / wait / kill /
//! release. Spawn uses pipes — the TTY stays with the TUI. Kill remaining
//! children when the broker drops.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::TerminalExitStatus;
use tokio::sync::Notify;

const DEFAULT_BYTE_LIMIT: usize = 1_048_576;

struct TerminalRec {
    child: Mutex<std::process::Child>,
    buf: Mutex<String>,
    truncated: AtomicBool,
    byte_limit: usize,
    exit: Mutex<Option<TerminalExitStatus>>,
    notify: Notify,
}

pub struct TerminalBroker {
    cwd: PathBuf,
    next_id: AtomicU64,
    terms: Mutex<HashMap<String, Arc<TerminalRec>>>,
}

impl TerminalBroker {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            next_id: AtomicU64::new(1),
            terms: Mutex::new(HashMap::new()),
        }
    }

    pub fn create(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<PathBuf>,
        env: &[(String, String)],
        output_byte_limit: Option<u64>,
    ) -> Result<String, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd.as_ref().unwrap_or(&self.cwd))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in env {
            cmd.env(name, value);
        }
        let mut child = cmd
            .spawn()
            .map_err(|err| format!("createTerminal: {err}"))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let rec = Arc::new(TerminalRec {
            child: Mutex::new(child),
            buf: Mutex::new(String::new()),
            truncated: AtomicBool::new(false),
            byte_limit: output_byte_limit
                .map(|n| n as usize)
                .unwrap_or(DEFAULT_BYTE_LIMIT)
                .max(1),
            exit: Mutex::new(None),
            notify: Notify::new(),
        });
        if let Some(stdout) = stdout {
            spawn_reader(Arc::clone(&rec), stdout);
        }
        if let Some(stderr) = stderr {
            spawn_reader(Arc::clone(&rec), stderr);
        }
        spawn_waiter(Arc::clone(&rec));
        let id = format!(
            "term-{}-{}",
            std::process::id(),
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        self.terms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), rec);
        Ok(id)
    }

    pub fn output(&self, terminal_id: &str) -> (String, bool, Option<TerminalExitStatus>) {
        let Some(rec) = self.get(terminal_id) else {
            return (String::new(), false, None);
        };
        let output = rec.buf.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let truncated = rec.truncated.load(Ordering::Relaxed);
        let exit = rec.exit.lock().unwrap_or_else(|e| e.into_inner()).clone();
        (output, truncated, exit)
    }

    pub async fn wait(&self, terminal_id: &str) -> TerminalExitStatus {
        let Some(rec) = self.get(terminal_id) else {
            return TerminalExitStatus::new();
        };
        loop {
            // Register interest *before* re-checking the status:
            // notify_waiters() stores no permit, so a notification fired
            // between the check and the await would otherwise be lost and
            // wait() would sleep forever with the exit status already set.
            let notified = rec.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(status) = rec.exit.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                return status;
            }
            notified.await;
        }
    }

    pub fn kill(&self, terminal_id: &str) {
        if let Some(rec) = self.get(terminal_id) {
            let _ = rec.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
    }

    pub fn release(&self, terminal_id: &str) {
        self.kill(terminal_id);
        self.terms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(terminal_id);
    }

    fn get(&self, terminal_id: &str) -> Option<Arc<TerminalRec>> {
        self.terms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(terminal_id)
            .cloned()
    }
}

impl Drop for TerminalBroker {
    fn drop(&mut self) {
        let terms: Vec<Arc<TerminalRec>> = self
            .terms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_, rec)| rec)
            .collect();
        for rec in terms {
            let _ = rec.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
    }
}

fn spawn_reader(rec: Arc<TerminalRec>, mut stream: impl Read + Send + 'static) {
    std::thread::Builder::new()
        .name("dsh-acp-term-io".into())
        .spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => append_output(&rec, &chunk[..n]),
                    Err(_) => break,
                }
            }
        })
        .ok();
}

fn spawn_waiter(rec: Arc<TerminalRec>) {
    std::thread::Builder::new()
        .name("dsh-acp-term-wait".into())
        .spawn(move || loop {
            let waited = {
                let mut child = rec.child.lock().unwrap_or_else(|e| e.into_inner());
                match child.try_wait() {
                    Ok(Some(status)) => Some(exit_status(status)),
                    Ok(None) => None,
                    Err(_) => Some(TerminalExitStatus::new()),
                }
            };
            if let Some(status) = waited {
                *rec.exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(status);
                rec.notify.notify_waiters();
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        })
        .ok();
}

fn append_output(rec: &TerminalRec, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let mut buf = rec.buf.lock().unwrap_or_else(|e| e.into_inner());
    buf.push_str(&text);
    if buf.len() > rec.byte_limit {
        let mut cut = buf.len() - rec.byte_limit;
        while cut < buf.len() && !buf.is_char_boundary(cut) {
            cut += 1;
        }
        buf.replace_range(0..cut, "");
        rec.truncated.store(true, Ordering::Relaxed);
    }
}

fn exit_status(status: std::process::ExitStatus) -> TerminalExitStatus {
    let mut out = TerminalExitStatus::new();
    if let Some(code) = status.code() {
        out = out.exit_code(code as u32);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            out = out.signal(signal.to_string());
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/acp_term__tests.rs"]
mod tests;
