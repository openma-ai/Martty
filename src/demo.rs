//! `--demo` mode driver.
//!
//! Emits scripted JSON-RPC notifications identical in wire shape to the real
//! deepseek-harness SDK runtime, so the full TUI render path can be exercised
//! without an API key. Everything is sent as [`AppEvent::Rpc`] over the bus.

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::bus::AppEvent;

/// Drive one scripted assistant turn on a background thread.
///
/// Returns immediately; the spawned thread sleeps between emissions and
/// ignores send errors (the receiver may have been dropped).
pub fn run_demo_turn(bus: Sender<AppEvent>, session: String, prompt: String) {
    thread::spawn(move || {
        let d = Driver { bus, session };
        d.run(&prompt);
    });
}

struct Driver {
    bus: Sender<AppEvent>,
    session: String,
}

impl Driver {
    fn run(&self, prompt: &str) {
        let lower = prompt.to_lowercase();
        let want_error = lower.contains("error");
        let want_sub = lower.contains("sub");
        let want_inject = lower.contains("inject");
        let want_long = lower.contains("long");

        // 1. session running.
        self.status("running");

        // Session mode facts — what permission-presets pins on session
        // creation in a real composition (folded into the composer chips).
        let want_plan = lower.contains("plan");
        let want_approve = lower.contains("approve");
        self.event(json!({"type": "permission/preset", "data": {"preset": "workspace-write"}}));
        self.event(json!({"type": "sandbox/mode", "data": {"mode": "workspace-write"}}));
        self.event(json!({"type": "approval/policy", "data": {"policy": "ask"}}));
        if want_plan {
            self.event(json!({"type": "plan/mode", "data": {"active": true}}));
        }

        // 2. turn start.
        self.event(json!({"type": "turn/start", "data": {"turn": 1}}));

        // 'inject' variation: plugin-sourced user message before reasoning.
        if want_inject {
            self.event(json!({
                "type": "user/message",
                "data": {
                    "id": "m-inj",
                    "role": "user",
                    "content": [{"type": "text", "text": "AGENTS.md: keep functions small."}],
                    "source": {"kind": "plugin", "plugin": "dsh-workspace"}
                }
            }));
        }

        // 3. reasoning deltas quoting a fragment of the prompt.
        let fragment: String = prompt.chars().take(32).collect();
        let reasoning = [
            format!("User asks: \"{fragment}\"."),
            " Need to inspect the entrypoint first.".to_string(),
            " Plan: locate fn main, then read".to_string(),
            " the surrounding lines.".to_string(),
            " rg is fastest for that.".to_string(),
            " Then view the file to confirm".to_string(),
            " structure before answering.".to_string(),
            " Keep the reply short.".to_string(),
        ];
        for text in &reasoning {
            self.chunk(json!({"type": "reasoning-delta", "index": 0, "text": text}));
        }

        // 4. text deltas streaming a short intro sentence.
        let intro = "I looked at the entrypoint and traced how the program starts. ";
        for piece in [
            "I looked at the entrypoint ",
            "and traced how the ",
            "program starts. ",
        ] {
            self.chunk(json!({"type": "text-delta", "index": 1, "text": piece}));
        }

        // 4b. per-step assembled message: the real loop assembles each step's
        // assistant message before its tool calls execute.
        self.event(json!({
            "type": "assistant/message",
            "data": {
                "turn": 1,
                "step": 0,
                "message": {
                    "id": "m-demo-0",
                    "role": "assistant",
                    "content": [{"type": "text", "text": intro}],
                    "source": {"kind": "model", "provider": "deepseek-official", "model": "deepseek-v4-flash"}
                }
            }
        }));

        // 5. first tool call: bash. Arguments MUST be a JSON string.
        self.event(json!({
            "type": "tool/call",
            "data": {
                "turn": 1,
                "step": 0,
                "callId": "demo-call-1",
                "name": "bash",
                "arguments": json!({"command": "rg -n 'fn main' src/"}).to_string()
            }
        }));

        // 6. first tool result (error variation flips it to a failure).
        if want_approve {
            self.event(json!({
                "type": "approval/asked",
                "data": {"id": "demo-approval-1", "toolName": "bash", "reason": "writes outside the workspace"}
            }));
            self.event(json!({
                "type": "approval/decided",
                "data": {"id": "demo-approval-1", "outcome": "allowed-once"}
            }));
        }
        if want_error {
            self.event(json!({
                "type": "tool/result",
                "data": {
                    "turn": 1,
                    "step": 0,
                    "error": {"name": "ToolFailed", "code": "EEXEC"},
                    "message": {
                        "role": "user",
                        "content": [{
                            "type": "tool-result",
                            "toolCallId": "demo-call-1",
                            "isError": true,
                            "content": [{
                                "type": "text",
                                "text": "rg: exec failed: No such file or directory (os error 2)\nexit status 127"
                            }]
                        }]
                    }
                }
            }));
        } else {
            self.event(json!({
                "type": "tool/result",
                "data": {
                    "turn": 1,
                    "step": 0,
                    "message": {
                        "role": "user",
                        "content": [{
                            "type": "tool-result",
                            "toolCallId": "demo-call-1",
                            "isError": false,
                            "content": [{"type": "text", "text": "src/main.rs:12:fn main() {"}]
                        }]
                    }
                }
            }));
        }

        // 7. second tool pair: str_replace_editor view.
        self.event(json!({
            "type": "tool/call",
            "data": {
                "turn": 1,
                "step": 0,
                "callId": "demo-call-2",
                "name": "str_replace_editor",
                "arguments": json!({"command": "view", "path": "src/main.rs"}).to_string()
            }
        }));
        self.event(json!({
            "type": "tool/result",
            "data": {
                "turn": 1,
                "step": 0,
                "message": {
                    "role": "user",
                    "content": [{
                        "type": "tool-result",
                        "toolCallId": "demo-call-2",
                        "isError": false,
                        "content": [{
                            "type": "text",
                            "text": "    10  mod bus;\n    11\n    12  fn main() {\n    13      let args = std::env::args();\n    14      run(args)\n    15  }"
                        }]
                    }]
                }
            }
        }));

        // 'sub' variation: a short child-session exchange after the second pair.
        if want_sub {
            let child = format!("{}-child", self.session);
            self.rpc(
                "subagent.started",
                json!({"parentSessionId": self.session, "childSessionId": child}),
            );
            self.event_for(&child, json!({
                "type": "assistant/chunk",
                "data": {"turn": 1, "step": 0, "chunk": {"type": "text-delta", "index": 0, "text": "Child scanning the "}}
            }));
            self.event_for(&child, json!({
                "type": "assistant/chunk",
                "data": {"turn": 1, "step": 0, "chunk": {"type": "text-delta", "index": 0, "text": "module tree now."}}
            }));
            self.event_for(&child, json!({
                "type": "assistant/message",
                "data": {
                    "turn": 1,
                    "step": 1,
                    "message": {
                        "id": "m-demo-child",
                        "role": "assistant",
                        "content": [{"type": "text", "text": "Child scanning the module tree now."}],
                        "source": {"kind": "model", "provider": "deepseek-official", "model": "deepseek-v4-flash"}
                    }
                }
            }));
            self.rpc("subagent.finished", json!({"childSessionId": child}));
        }

        // 8. concluding text deltas (mention the whale playfully).
        let conclusion = if want_long {
            long_conclusion()
        } else {
            "The entrypoint is a thin `fn main` at src/main.rs:12 that hands off to `run`. \
             All clear — the DeepSeek whale surfaced, nodded approvingly, and dove back down. 🐋"
                .to_string()
        };
        for piece in split_pieces(&conclusion) {
            self.chunk(json!({"type": "text-delta", "index": 1, "text": piece}));
        }

        // 9. assembled final assistant message for the concluding step.
        let _ = &intro;
        self.event(json!({
            "type": "assistant/message",
            "data": {
                "turn": 1,
                "step": 1,
                "message": {
                    "id": "m-demo",
                    "role": "assistant",
                    "content": [{"type": "text", "text": conclusion}],
                    "source": {"kind": "model", "provider": "deepseek-official", "model": "deepseek-v4-flash"}
                }
            }
        }));

        // 10. usage chunk.
        self.chunk(json!({
            "type": "usage",
            "usage": {"inputTokens": 1834, "outputTokens": 412, "cachedInputTokens": 1200, "reasoningTokens": 96}
        }));

        // 11. turn end.
        let kind = if want_error { "error" } else { "completed" };
        self.event(json!({"type": "turn/end", "data": {"turn": 1, "reason": {"kind": kind}}}));

        // 12. session idle.
        self.status("idle");
    }

    /// Sleep briefly, then send one JSON-RPC notification. Send errors ignored.
    fn rpc(&self, method: &str, params: Value) {
        thread::sleep(Duration::from_millis(45));
        let _ = self.bus.send(AppEvent::Rpc {
            method: method.to_string(),
            params,
        });
    }

    fn status(&self, status: &str) {
        self.rpc(
            "session.status",
            json!({"sessionId": self.session, "status": status}),
        );
    }

    fn event(&self, event: Value) {
        let sid = self.session.clone();
        self.event_for(&sid, event);
    }

    fn event_for(&self, session_id: &str, event: Value) {
        self.rpc(
            "session.event",
            json!({"sessionId": session_id, "event": event}),
        );
    }

    fn chunk(&self, chunk: Value) {
        self.event(json!({
            "type": "assistant/chunk",
            "data": {"turn": 1, "step": 0, "chunk": chunk}
        }));
    }
}

/// Break `text` into a handful of streaming pieces (never more than 8).
fn split_pieces(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let step = (chars.len() / 6).max(24);
    chars.chunks(step).map(|c| c.iter().collect()).collect()
}

/// ~30-line answer with a rust fenced code block, for wrapping/collapsing tests.
fn long_conclusion() -> String {
    let mut s = String::new();
    s.push_str("Here is the full walkthrough of the entrypoint.\n\n");
    s.push_str("Startup path:\n");
    s.push_str("1. `main` parses argv.\n");
    s.push_str("2. Terminal enters raw mode.\n");
    s.push_str("3. The event bus channel is created.\n");
    s.push_str("4. The runtime subprocess is spawned.\n");
    s.push_str("5. The render loop begins.\n\n");
    s.push_str("Relevant code:\n\n");
    s.push_str("```rust\n");
    s.push_str("fn main() {\n");
    s.push_str("    let args = std::env::args();\n");
    s.push_str("    // set up the bus and the runtime\n");
    s.push_str("    let (tx, rx) = std::sync::mpsc::channel();\n");
    s.push_str("    spawn_runtime(tx.clone());\n");
    s.push_str("    // hand control to the UI loop\n");
    s.push_str("    run(args, tx, rx)\n");
    s.push_str("}\n");
    s.push_str("```\n\n");
    s.push_str("Notes:\n");
    s.push_str("- The loop is single-threaded; everything arrives via AppEvent.\n");
    s.push_str("- Stderr lines are kept for diagnostics.\n");
    s.push_str("- Shell commands report back as ShellDone.\n");
    s.push_str("- Controller updates flow through CtlEvent.\n");
    s.push_str("- Nothing blocks the render path.\n\n");
    s.push_str("Longer lines follow to exercise soft wrapping in narrow panes: ");
    s.push_str("this sentence intentionally rambles on well past any reasonable ");
    s.push_str("terminal width so the wrapping logic has something to chew on.\n\n");
    s.push_str("That's the whole story — the DeepSeek whale breached, gave a ");
    s.push_str("cheerful spout of approval, and slipped back beneath the waves. 🐋\n");
    s
}

#[cfg(test)]
#[path = "../tests/unit/demo__tests.rs"]
mod tests;
