//! In-process stand-in for `pi --mode rpc`. Not a Pi build and not a sandbox.
//! The mode is `stub-mode.txt` in the child cwd. The public proxy marker is never written.

#![forbid(unsafe_code)]

use serde_json::{json, Value};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const LINE_LIMIT: usize = 1_048_576;
const REASONING_SECRET: &str = "pi-harness-reasoning-secret";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pi-harness-stub: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    write_text("stub-argv", &args.join("\n"))?;
    let mut env_lines: Vec<String> = env::vars().map(|(key, value)| format!("{key}={value}")).collect();
    env_lines.sort();
    write_text("stub-env", &env_lines.join("\n"))?;
    if args.iter().any(|arg| arg == "--version") {
        println!("1.0.4");
        let _ = io::stdout().flush();
        write_text("stub-version", "1")?;
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--help") {
        println!("--mode");
        return Ok(());
    }
    let mode = fs::read_to_string("stub-mode.txt").map_err(|err| format!("mode: {err}"))?;
    let mode = mode.trim().to_owned();
    if let Some(dir) = arg_value(&args, "--session-dir") {
        fs::create_dir_all(&dir).map_err(|err| format!("session dir: {err}"))?;
        fs::write(Path::new(&dir).join("session.jsonl"), b"{\"type\":\"session\"}\n").map_err(|err| format!("session file: {err}"))?;
    }
    write_text("stub-rpc", "1")?;
    serve(&mode)
}

fn serve(mode: &str) -> Result<(), String> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let n = reader.read_until(b'\n', &mut line).map_err(|err| format!("stdin: {err}"))?;
        if n == 0 {
            return Ok(());
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        append_command(&line)?;
        let value: Value = serde_json::from_slice(&line).map_err(|err| format!("command json: {err}"))?;
        let id = value.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("").to_owned();
        if kind == "prompt" && mode == "reject" {
            write_line(&mut stdout, &fail(&id, "prompt", "rejected"))?;
            continue;
        }
        if kind == "switch_session" && mode == "switch-cancel" {
            write_line(&mut stdout, &ok(&id, "switch_session", json!({"cancelled": true})))?;
            continue;
        }
        if kind == "get_messages" {
            write_line(&mut stdout, &ok(&id, "get_messages", messages()))?;
            continue;
        }
        if kind == "switch_session" {
            write_line(&mut stdout, &ok(&id, "switch_session", json!({"cancelled": false})))?;
            continue;
        }
        if kind == "abort" {
            write_line(&mut stdout, &ok(&id, "abort", json!({})))?;
            if mode == "abort" || mode == "abort-thinking" {
                return Ok(());
            }
            continue;
        }
        if kind != "prompt" {
            write_line(&mut stdout, &ok(&id, &kind, json!({})))?;
            continue;
        }
        match mode {
            "reorder" => {
                write_line(&mut stdout, &ok("decoy", "prompt", json!({"disposition": "started"})))?;
                write_line(&mut stdout, &event(json!({"type": "turn_start"})))?;
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_turn(&mut stdout, "stop", None)?;
            }
            "huge" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                let mut big = vec![b'x'; LINE_LIMIT + 8];
                big.push(b'\n');
                stdout.write_all(&big).map_err(|err| format!("huge: {err}"))?;
                stdout.flush().map_err(|err| format!("flush: {err}"))?;
            }
            "partial" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                stdout.write_all(br#"{"type":"turn_start""#).map_err(|err| format!("partial: {err}"))?;
                stdout.flush().map_err(|err| format!("flush: {err}"))?;
                return Ok(());
            }
            "eof" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                return Ok(());
            }
            "delay" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                thread::sleep(Duration::from_secs(30));
            }
            "ignore" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(&mut stdout, &text_delta("fixture-reply"))?;
                thread::sleep(Duration::from_secs(30));
            }
            "abort" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(&mut stdout, &text_delta("fixture-reply"))?;
            }
            "unknown" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(&mut stdout, &event(json!({"type": "extension_ui_request"})))?;
            }
            "aborted" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(
                    &mut stdout,
                    &event(json!({"type": "turn_end", "message": {"stopReason": "aborted", "content": []}})),
                )?;
            }
            "errored" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(
                    &mut stdout,
                    &event(json!({
                        "type": "turn_end",
                        "message": {"stopReason": "stop", "errorMessage": "provider failed", "content": []}
                    })),
                )?;
            }
            "message-error" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(
                    &mut stdout,
                    &event(json!({"type": "message_end", "message": {"role": "assistant", "errorMessage": "boom"}})),
                )?;
            }
            "backpressure" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                let text = "a".repeat(200_000);
                for index in 1..=40 {
                    write_line(&mut stdout, &text_delta(&text))?;
                    write_text("stub-progress", &index.to_string())?;
                }
            }
            "finished" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_turn(&mut stdout, "stop", None)?;
            }
            "thinking-tools" => {
                // Protocol fixture. These bytes match the pinned Pi 1.0.4 wire shapes.
                // They are not a Pi process and not a model answer.
                write_thinking(&mut stdout, "thinking_start", None, None)?;
                write_thinking(&mut stdout, "thinking_delta", Some(REASONING_SECRET), None)?;
                write_thinking(&mut stdout, "thinking_end", None, Some(REASONING_SECRET))?;
                write_line(&mut stdout, &ok("decoy", "prompt", json!({"disposition": "started"})))?;
                write_line(&mut stdout, &event(json!({"type": "turn_start"})))?;
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_tool(&mut stdout, "call_read", "read", json!({"path": "README.md"}), "readme-bytes")?;
                write_tool(
                    &mut stdout,
                    "call_bash",
                    "bash",
                    json!({"command": "printf 'query_id=q_fixed\\n'"}),
                    "query_id=q_fixed\n",
                )?;
                write_line(&mut stdout, &text_delta("fixture-reply"))?;
                let message = json!({
                    "stopReason": "stop",
                    "content": [{"type": "text", "text": "fixture-reply"}],
                    "usage": {"input": 1, "output": 1, "totalTokens": 2}
                });
                write_line(&mut stdout, &event(json!({"type": "turn_end", "message": message})))?;
                write_line(&mut stdout, &event(json!({"type": "agent_settled"})))?;
            }
            "thinking-eof" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(&mut stdout, &event(json!({"type": "turn_start"})))?;
                write_thinking(&mut stdout, "thinking_start", None, None)?;
                write_thinking(&mut stdout, "thinking_delta", Some(REASONING_SECRET), None)?;
                write_thinking(&mut stdout, "thinking_end", None, Some(REASONING_SECRET))?;
                return Ok(());
            }
            "thinking-unknown" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_thinking(&mut stdout, "thinking_start", None, None)?;
                write_line(
                    &mut stdout,
                    &event(json!({
                        "type": "message_update",
                        "usage": pinned_usage(),
                        "assistantMessageEvent": {"type": "thinking_partial", "contentIndex": 0}
                    })),
                )?;
            }
            "abort-thinking" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(&mut stdout, &event(json!({"type": "turn_start"})))?;
                write_thinking(&mut stdout, "thinking_start", None, None)?;
                write_thinking(&mut stdout, "thinking_delta", Some(REASONING_SECRET), None)?;
            }
            "thinking-error" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_line(
                    &mut stdout,
                    &event(json!({
                        "type": "message_update",
                        "usage": pinned_usage(),
                        "assistantMessageEvent": {
                            "type": "error",
                            "reason": "error",
                            "error": {
                                "role": "assistant",
                                "content": [{"type": "thinking", "thinking": REASONING_SECRET}],
                                "stopReason": "error",
                                "errorMessage": "provider failed"
                            }
                        }
                    })),
                )?;
            }
            "thinking-partial" => {
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                write_thinking(&mut stdout, "thinking_start", None, None)?;
                let partial = format!(
                    "{{\"type\":\"message_update\",\"assistantMessageEvent\":{{\"type\":\"thinking_delta\",\"delta\":\"{REASONING_SECRET}\""
                );
                stdout.write_all(partial.as_bytes()).map_err(|err| format!("partial: {err}"))?;
                stdout.flush().map_err(|err| format!("flush: {err}"))?;
                return Ok(());
            }
            "tools-61" => {
                // not_upstream_real: sixty-one synthetic toolcall_end records, not a Pi transcript.
                write_line(&mut stdout, &ok(&id, "prompt", json!({"disposition": "started"})))?;
                for index in 1..=61 {
                    let call_id = format!("c{index}");
                    write_line(
                        &mut stdout,
                        &event(json!({
                            "type": "message_update",
                            "assistantMessageEvent": {
                                "type": "toolcall_end",
                                "contentIndex": 1,
                                "toolCall": {
                                    "type": "toolCall",
                                    "id": call_id,
                                    "name": "bash",
                                    "arguments": {"command": "printf 'x'"}
                                }
                            }
                        })),
                    )?;
                }
            }
            other => return Err(format!("unknown mode {other}")),
        }
    }
}

fn write_turn(stdout: &mut impl Write, stop: &str, error: Option<&str>) -> Result<(), String> {
    write_line(stdout, &event(json!({"type": "turn_start"})))?;
    write_line(stdout, &text_delta("fixture-reply"))?;
    let mut message = json!({
        "stopReason": stop,
        "content": [{"type": "text", "text": "fixture-reply"}],
        "usage": {"input": 1, "output": 1, "totalTokens": 2}
    });
    if let Some(error) = error {
        message["errorMessage"] = json!(error);
    }
    write_line(stdout, &event(json!({"type": "turn_end", "message": message})))?;
    write_line(stdout, &event(json!({"type": "agent_settled"})))?;
    Ok(())
}

fn text_delta(text: &str) -> Vec<u8> {
    event(json!({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": text}}))
}

fn pinned_usage() -> Value {
    json!({
        "input": 1,
        "output": 1,
        "cacheRead": 0,
        "cacheWrite": 0,
        "reasoning": 0,
        "totalTokens": 2,
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}
    })
}

fn write_thinking(stdout: &mut impl Write, kind: &str, delta: Option<&str>, content: Option<&str>) -> Result<(), String> {
    let mut assistant = json!({"type": kind, "contentIndex": 0});
    if let Some(delta) = delta {
        assistant["delta"] = json!(delta);
    }
    if let Some(content) = content {
        assistant["content"] = json!(content);
    }
    write_line(stdout, &event(json!({"type": "message_update", "usage": pinned_usage(), "assistantMessageEvent": assistant})))
}

fn write_tool(stdout: &mut impl Write, call_id: &str, name: &str, arguments: Value, output: &str) -> Result<(), String> {
    write_line(
        stdout,
        &event(json!({
            "type": "message_update",
            "usage": pinned_usage(),
            "assistantMessageEvent": {
                "type": "toolcall_end",
                "contentIndex": 1,
                "toolCall": {"type": "toolCall", "id": call_id, "name": name, "arguments": arguments}
            }
        })),
    )?;
    write_line(
        stdout,
        &event(json!({
            "type": "tool_execution_end",
            "toolCallId": call_id,
            "toolName": name,
            "isError": false,
            "result": {"content": [{"type": "text", "text": output}]}
        })),
    )
}

fn messages() -> Value {
    json!({"messages": [{"role": "assistant", "content": [{"type": "text", "text": "fixture-reply"}]}]})
}

fn event(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).expect("event json")
}

fn ok(id: &str, command: &str, data: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": id,
        "type": "response",
        "command": command,
        "success": true,
        "data": data
    }))
    .expect("response json")
}

fn fail(id: &str, command: &str, error: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": id,
        "type": "response",
        "command": command,
        "success": false,
        "error": error
    }))
    .expect("error json")
}

fn write_line(stdout: &mut impl Write, bytes: &[u8]) -> Result<(), String> {
    stdout.write_all(bytes).map_err(|err| format!("stdout: {err}"))?;
    stdout.write_all(b"\n").map_err(|err| format!("stdout: {err}"))?;
    stdout.flush().map_err(|err| format!("flush: {err}"))?;
    Ok(())
}

fn append_command(line: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("stub-commands.jsonl")
        .map_err(|err| format!("commands: {err}"))?;
    file.write_all(line).map_err(|err| format!("commands: {err}"))?;
    file.write_all(b"\n").map_err(|err| format!("commands: {err}"))?;
    Ok(())
}

fn write_text(path: &str, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|err| format!("{path}: {err}"))
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|arg| arg == flag).and_then(|index| args.get(index + 1).cloned())
}
