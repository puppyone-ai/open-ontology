//! Map one Pi JSONL record onto the harness tool events.
//! Bookkeeping types listed in the Pi harness contract stay unmapped.
//! Any other type is unknown. This probe does not treat a turn end as a completed answer.

use crate::json::{self, Json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mapped {
    ToolStarted { name: String, call_id: String, args_summary: String },
    ToolFinished { call_id: String, status: String, output_summary: String },
    Bookkeeping,
    Unknown { kind: String },
}

pub fn map_line(line: &str) -> Result<Mapped, String> {
    let value = json::parse(line.trim_end_matches(['\r', '\n']))?;
    map_value(&value)
}

pub fn map_value(value: &Json) -> Result<Mapped, String> {
    let kind = value.get("type").and_then(Json::as_str).unwrap_or("").to_owned();
    if kind == "message_update" {
        return map_update(value);
    }
    if kind == "tool_execution_end" {
        return map_tool_end(value);
    }
    if is_bookkeeping(&kind) {
        return Ok(Mapped::Bookkeeping);
    }
    Ok(Mapped::Unknown { kind })
}

fn map_update(value: &Json) -> Result<Mapped, String> {
    let event = value.get("assistantMessageEvent").ok_or("message_update has no event")?;
    let kind = event.get("type").and_then(Json::as_str).unwrap_or("");
    if kind == "toolcall_end" {
        let call = event.get("toolCall").ok_or("toolcall_end has no toolCall")?;
        let name = call.get("name").and_then(Json::as_str).unwrap_or("").to_owned();
        let call_id = call.get("id").and_then(Json::as_str).unwrap_or("").to_owned();
        let args = call.get("arguments").map(Json::compact).unwrap_or_else(|| "null".to_owned());
        if name.is_empty() || call_id.is_empty() {
            return Err("toolcall_end is missing name or id".to_owned());
        }
        return Ok(Mapped::ToolStarted { name, call_id, args_summary: clip(&args) });
    }
    if matches!(kind, "text_start" | "text_delta" | "text_end" | "toolcall_start" | "toolcall_delta") {
        return Ok(Mapped::Bookkeeping);
    }
    Ok(Mapped::Unknown { kind: format!("message_update:{kind}") })
}

fn map_tool_end(value: &Json) -> Result<Mapped, String> {
    let call_id = value.get("toolCallId").and_then(Json::as_str).unwrap_or("").to_owned();
    if call_id.is_empty() {
        return Err("tool_execution_end is missing toolCallId".to_owned());
    }
    let is_error = value.get("isError").and_then(Json::as_bool).unwrap_or(true);
    let status = if is_error { "error" } else { "ok" };
    let output = text_blocks(value.get("result").and_then(|result| result.get("content")));
    Ok(Mapped::ToolFinished { call_id, status: status.to_owned(), output_summary: clip(&output) })
}

fn text_blocks(content: Option<&Json>) -> String {
    let Some(Json::Array(items)) = content else {
        return String::new();
    };
    let mut out = String::new();
    for item in items {
        if item.get("type").and_then(Json::as_str) == Some("text")
            && let Some(text) = item.get("text").and_then(Json::as_str)
        {
            out.push_str(text);
        }
    }
    out
}

fn is_bookkeeping(kind: &str) -> bool {
    matches!(
        kind,
        "response"
            | "agent_start"
            | "agent_end"
            | "agent_settled"
            | "turn_start"
            | "turn_end"
            | "message_start"
            | "message_end"
            | "tool_execution_start"
            | "tool_execution_update"
    )
}

fn clip(text: &str) -> String {
    if text.len() <= 512 {
        return text.to_owned();
    }
    let mut end = 512;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[..end].to_owned();
    out.push_str("...");
    out
}

pub fn response_ok(line: &str, id: &str) -> Result<bool, String> {
    let value = json::parse(line.trim_end_matches(['\r', '\n']))?;
    Ok(value.get("type").and_then(Json::as_str) == Some("response")
        && value.get("id").and_then(Json::as_str) == Some(id)
        && value.get("success").and_then(Json::as_bool) == Some(true))
}

pub fn prompt_started(line: &str) -> Result<bool, String> {
    let value = json::parse(line.trim_end_matches(['\r', '\n']))?;
    let disposition = value.get("data").and_then(|data| data.get("disposition")).and_then(Json::as_str);
    Ok(response_ok(line, "prompt-1")? && disposition == Some("started"))
}
