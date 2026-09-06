//! NDJSON JSON-RPC 2.0 over stdio — the deepseek-harness SDK runtime protocol.
//!
//! One JSON object per line. Client requests: `initialize`, `session/prompt`,
//! `shutdown`. Server notifications: `session.event`, `session.status`,
//! `subagent.started` / `subagent.finished`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::bus::AppEvent;

type SharedWriter = Arc<Mutex<Option<Box<dyn Write + Send>>>>;

pub struct RuntimeProcess {
    child: Arc<Mutex<Option<Child>>>,
    stdin: SharedWriter,
    /// Serializes writers. Never held across the blocking write itself:
    /// `write_line` *takes the writer out* of `stdin` for the duration, so
    /// a wedged child blocking a write can never starve `kill()` or the
    /// reader thread (they only ever need the shared slot, briefly).
    write_turn: Arc<Mutex<()>>,
    pending: Arc<Mutex<HashMap<String, SyncSender<Result<Value, RpcFailure>>>>>,
    next_id: AtomicU64,
    pub stderr_tail: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Clone)]
pub struct RpcFailure {
    pub code: Option<i64>,
    pub message: String,
}

impl std::fmt::Display for RpcFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(code) => write!(f, "rpc error {code}: {}", self.message),
            None => write!(f, "rpc error: {}", self.message),
        }
    }
}

impl std::error::Error for RpcFailure {}

impl RuntimeProcess {
    /// Spawn the runtime and start reader/stderr pump threads that forward
    /// notifications and lifecycle facts to the app event bus.
    pub fn spawn(
        bin: &str,
        envs: &[(String, String)],
        cwd: &str,
        bus: Sender<AppEvent>,
    ) -> Result<Self> {
        let mut cmd = Command::new(bin);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(cwd);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn harness runtime: {bin}"))?;

        let stdin = child.stdin.take().context("runtime stdin unavailable")?;
        let stdout = child.stdout.take().context("runtime stdout unavailable")?;
        let stderr = child.stderr.take().context("runtime stderr unavailable")?;

        let proc = RuntimeProcess {
            child: Arc::new(Mutex::new(Some(child))),
            stdin: Arc::new(Mutex::new(Some(Box::new(stdin) as Box<dyn Write + Send>))),
            write_turn: Arc::new(Mutex::new(())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            stderr_tail: Arc::new(Mutex::new(Vec::new())),
        };

        proc.start_reader(stdout, bus.clone());

        // stderr pump: keep a diagnostics tail and forward lines to the bus.
        {
            let tail = Arc::clone(&proc.stderr_tail);
            std::thread::Builder::new()
                .name("dsh-stderr".into())
                .spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        {
                            let mut t = tail.lock().unwrap();
                            t.push(line.clone());
                            let len = t.len();
                            if len > 200 {
                                t.drain(0..len - 200);
                            }
                        }
                        let _ = bus.send(AppEvent::RuntimeStderr(line));
                    }
                })
                .expect("spawn stderr reader");
        }

        Ok(proc)
    }

    /// Attach to the private demo compositor over inherited pipe fds.
    /// `reader` receives server frames; `writer` carries ours.
    pub fn attach(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        bus: Sender<AppEvent>,
    ) -> Self {
        let proc = RuntimeProcess {
            child: Arc::new(Mutex::new(None)),
            stdin: Arc::new(Mutex::new(Some(Box::new(writer) as Box<dyn Write + Send>))),
            write_turn: Arc::new(Mutex::new(())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            stderr_tail: Arc::new(Mutex::new(Vec::new())),
        };
        proc.start_reader(reader, bus);
        proc
    }

    /// stdout/frame reader: route responses to pending waiters, notifications
    /// to the bus; on EOF fail in-flight requests and report exit.
    fn start_reader(&self, stream: impl Read + Send + 'static, bus: Sender<AppEvent>) {
        let pending = Arc::clone(&self.pending);
        let stdin_slot = Arc::clone(&self.stdin);
        let write_turn = Arc::clone(&self.write_turn);
        let child_slot = Arc::clone(&self.child);
        std::thread::Builder::new()
            .name("dsh-frames".into())
            .spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    route_message(msg, &pending, &stdin_slot, &write_turn, &bus);
                }
                let waiters: Vec<_> = pending.lock().unwrap().drain().collect();
                for (_, tx) in waiters {
                    let _ = tx.try_send(Err(RpcFailure {
                        code: None,
                        message: "harness runtime closed".into(),
                    }));
                }
                // Poll with try_wait instead of a blocking wait: the lock
                // must stay acquirable so kill() can SIGKILL a runtime that
                // closed stdout but keeps running.
                let code = loop {
                    {
                        let mut guard = child_slot.lock().unwrap();
                        match guard.as_mut() {
                            // kill() already reaped the child.
                            None => break None,
                            Some(child) => match child.try_wait() {
                                Ok(Some(status)) => break status.code(),
                                Ok(None) => {}
                                Err(_) => break None,
                            },
                        }
                    }
                    std::thread::sleep(Duration::from_millis(20));
                };
                let _ = bus.send(AppEvent::RuntimeExited(code));
            })
            .expect("spawn frame reader");
    }

    fn write_line(&self, value: &Value) -> Result<()> {
        let _turn = self.write_turn.lock().unwrap();
        // Take the writer out for the duration of the (possibly blocking)
        // write: nobody else ever blocks on the shared slot behind a stuck
        // pipe — kill() clears the slot instead of waiting for this write.
        let mut writer = {
            let mut guard = self.stdin.lock().unwrap();
            guard.take().context("harness runtime stdin closed")?
        };
        let result = (|| {
            let mut payload = serde_json::to_vec(value)?;
            payload.push(b'\n');
            writer.write_all(&payload)?;
            writer.flush()?;
            Ok(())
        })();
        // Put the writer back unless kill() already claimed the slot.
        let mut guard = self.stdin.lock().unwrap();
        if guard.is_none() {
            return result; // runtime killed; the writer dies with it
        }
        *guard = Some(writer);
        result
    }

    /// Blocking JSON-RPC request. Call off the UI thread.
    pub fn request(&self, method: &str, params: Option<Value>, timeout: Duration) -> Result<Value> {
        let id = format!("dsb-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx): (
            SyncSender<Result<Value, RpcFailure>>,
            Receiver<Result<Value, RpcFailure>>,
        ) = mpsc::sync_channel(1);
        self.pending.lock().unwrap().insert(id.clone(), tx);

        let mut msg = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(p) = params {
            msg["params"] = p;
        }
        if let Err(err) = self.write_line(&msg) {
            self.pending.lock().unwrap().remove(&id);
            return Err(err);
        }

        match rx.recv_timeout(timeout) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(failure)) => Err(anyhow!(failure)),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                let tail = self.stderr_snapshot(6);
                if tail.is_empty() {
                    bail!("{method} timed out waiting for harness runtime")
                }
                bail!(
                    "{method} timed out waiting for harness runtime\n{}",
                    tail.join("\n")
                )
            }
        }
    }

    pub fn stderr_snapshot(&self, n: usize) -> Vec<String> {
        let tail = self.stderr_tail.lock().unwrap();
        tail.iter().rev().take(n).rev().cloned().collect()
    }

    /// Hard interrupt: SIGKILL the runtime. In-flight requests fail fast and
    /// the durable JSONL session survives for the next spawn.
    pub fn kill(&self) {
        {
            let mut stdin = self.stdin.lock().unwrap();
            *stdin = None; // drop -> EOF for the child
        }
        let mut guard = self.child.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *guard = None;
    }

    /// Polite shutdown; falls back to kill.
    pub fn shutdown(&self) {
        let _ = self.request("shutdown", None, Duration::from_millis(1200));
        self.kill();
    }

    /// Are we attached to an external demo peer rather than owning a child?
    #[allow(dead_code)]
    pub fn is_attached(&self) -> bool {
        self.child.lock().unwrap().is_none() && self.stdin.lock().unwrap().is_some()
    }

    pub fn is_alive(&self) -> bool {
        let mut guard = self.child.lock().unwrap();
        match guard.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => self.stdin.lock().unwrap().is_some(),
        }
    }
}

fn route_message(
    msg: Value,
    pending: &Arc<Mutex<HashMap<String, SyncSender<Result<Value, RpcFailure>>>>>,
    stdin_slot: &SharedWriter,
    write_turn: &Mutex<()>,
    bus: &Sender<AppEvent>,
) {
    let id = msg.get("id");
    let method = msg.get("method").and_then(Value::as_str);
    match (id, method) {
        // Server-initiated request (e.g. interaction plugins). The minimal
        // composition never sends one; answer method-not-found so the runtime
        // never deadlocks waiting on us. Best-effort: the reply only goes out
        // when the write path is idle — a wedged runtime must never stop the
        // reader thread from draining stdout.
        (Some(id), Some(method)) => {
            let reply = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("dsb: unhandled server request {method}") }
            });
            if let Ok(_turn) = write_turn.try_lock() {
                let writer = stdin_slot.lock().unwrap().take();
                if let Some(mut stdin) = writer {
                    if let Ok(mut payload) = serde_json::to_vec(&reply) {
                        payload.push(b'\n');
                        let _ = stdin.write_all(&payload);
                        let _ = stdin.flush();
                    }
                    let mut guard = stdin_slot.lock().unwrap();
                    if guard.is_none() {
                        // kill() claimed the slot while we wrote: let the
                        // writer die with the runtime.
                    } else {
                        *guard = Some(stdin);
                    }
                }
            }
            let _ = bus.send(AppEvent::RuntimeStderr(format!(
                "unhandled server request: {method}"
            )));
        }
        // Response to one of our requests.
        (Some(id), None) => {
            let key = match id {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let waiter = pending.lock().unwrap().remove(&key);
            if let Some(tx) = waiter {
                let outcome = if let Some(err) = msg.get("error") {
                    Err(RpcFailure {
                        code: err.get("code").and_then(Value::as_i64),
                        message: err
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("JSON-RPC error")
                            .to_string(),
                    })
                } else {
                    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = tx.try_send(outcome);
            }
        }
        // Notification.
        (None, Some(method)) => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let _ = bus.send(AppEvent::Rpc {
                method: method.to_string(),
                params,
            });
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "../tests/unit/proto__tests.rs"]
mod tests;
