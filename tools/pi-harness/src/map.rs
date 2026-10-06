//! Map Pi RPC records onto the product events frozen in CONTRACT.md.
//! Tool product events stay unbound until a fresh Pi run records the bytes.

use crate::{Error, ErrorKind, ProductEvent, Usage};
use serde_json::Value;

#[derive(Debug)]
pub(crate) enum MapResult {
    Event(ProductEvent),
    Ignored,
    Unknown(String),
}

pub(crate) fn map_record(value: &Value) -> Result<MapResult, Error> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(ErrorKind::Shape, "record type"))?;
    let result = match kind {
        "turn_start" => MapResult::Event(ProductEvent::TurnStarted),
        "turn_end" => map_turn_end(value)?,
        "message_update" => map_update(value)?,
        "message_end" => map_message_end(value)?,
        "tool_execution_start" | "tool_execution_update" => MapResult::Ignored,
        "tool_execution_end" => map_tool_end(value)?,
        "agent_start" | "agent_end" | "agent_settled" | "message_start" | "response" => {
            MapResult::Ignored
        }
        other => MapResult::Unknown(other.to_owned()),
    };
    Ok(result)
}

fn map_turn_end(value: &Value) -> Result<MapResult, Error> {
    let message = value
        .get("message")
        .ok_or_else(|| Error::new(ErrorKind::Shape, "turn_end message"))?;
    if let Some(error) = message.get("errorMessage").and_then(Value::as_str) {
        return Ok(MapResult::Event(ProductEvent::Error {
            reason: error.to_owned(),
        }));
    }
    let stop = message.get("stopReason").and_then(Value::as_str).unwrap_or("");
    if stop == "aborted" {
        return Ok(MapResult::Event(ProductEvent::Error {
            reason: "aborted".to_owned(),
        }));
    }
    if stop == "stop" {
        return Ok(MapResult::Event(ProductEvent::TurnFinished {
            stop_reason: stop.to_owned(),
            answer: answer_of(message),
            usage: usage_of(message),
        }));
    }
    if stop == "toolUse" {
        return Ok(MapResult::Ignored);
    }
    Ok(MapResult::Unknown(format!("turn_end:{stop}")))
}

fn map_update(value: &Value) -> Result<MapResult, Error> {
    let event = value
        .get("assistantMessageEvent")
        .ok_or_else(|| Error::new(ErrorKind::Shape, "assistantMessageEvent"))?;
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(ErrorKind::Shape, "assistant event type"))?;
    match kind {
        "text_delta" => {
            let delta = event
                .get("delta")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new(ErrorKind::Shape, "text_delta"))?;
            Ok(MapResult::Event(ProductEvent::AssistantDelta {
                text: delta.to_owned(),
            }))
        }
        "text_start" | "text_end" | "toolcall_start" | "toolcall_delta" => Ok(MapResult::Ignored),
        "toolcall_end" => map_toolcall_end(event),
        other => Ok(MapResult::Unknown(format!("message_update:{other}"))),
    }
}

fn map_toolcall_end(event: &Value) -> Result<MapResult, Error> {
    let call = event.get("toolCall").ok_or_else(|| Error::new(ErrorKind::Shape, "toolCall"))?;
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(ErrorKind::Shape, "toolCall.name"))?;
    let call_id = call
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(ErrorKind::Shape, "toolCall.id"))?;
    let arguments = call.get("arguments").ok_or_else(|| Error::new(ErrorKind::Shape, "toolCall.arguments"))?;
    let text = serde_json::to_string(arguments).map_err(|err| Error::new(ErrorKind::Shape, err.to_string()))?;
    Ok(MapResult::Event(ProductEvent::ToolCallStarted {
        name: name.to_owned(),
        call_id: call_id.to_owned(),
        args_summary: summarize(&text),
    }))
}

fn map_tool_end(value: &Value) -> Result<MapResult, Error> {
    let call_id = value
        .get("toolCallId")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(ErrorKind::Shape, "toolCallId"))?;
    let is_error = value
        .get("isError")
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::new(ErrorKind::Shape, "isError"))?;
    let status = if is_error { "error" } else { "ok" };
    Ok(MapResult::Event(ProductEvent::ToolCallFinished {
        call_id: call_id.to_owned(),
        status: status.to_owned(),
        output_summary: summarize(&output_text(value.get("result"))),
    }))
}

fn output_text(result: Option<&Value>) -> String {
    let Some(blocks) = result.and_then(|value| value.get("content")).and_then(Value::as_array) else {
        return String::new();
    };
    let mut out = String::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = block.get("text").and_then(Value::as_str)
        {
            out.push_str(text);
        }
    }
    out
}

fn summarize(text: &str) -> String {
    const CAP: usize = 512;
    if text.len() <= CAP {
        return text.to_owned();
    }
    let mut end = CAP;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[..end].to_owned();
    out.push_str("...");
    out
}

fn map_message_end(value: &Value) -> Result<MapResult, Error> {
    let Some(message) = value.get("message") else {
        return Ok(MapResult::Ignored);
    };
    if message.get("role").and_then(Value::as_str) == Some("assistant")
        && let Some(error) = message.get("errorMessage").and_then(Value::as_str)
    {
        return Ok(MapResult::Event(ProductEvent::Error {
            reason: error.to_owned(),
        }));
    }
    Ok(MapResult::Ignored)
}

fn answer_of(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    let blocks = content.as_array()?;
    let mut out = String::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = block.get("text").and_then(Value::as_str)
        {
            out.push_str(text);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn usage_of(message: &Value) -> Option<Usage> {
    let usage = message.get("usage")?.as_object()?;
    let input = u64_field(usage.get("input"))?;
    let output = u64_field(usage.get("output"))?;
    let total = u64_field(usage.get("totalTokens"))?;
    Some(Usage { input, output, total })
}

fn u64_field(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64().or_else(|| number.as_i64().and_then(|n| u64::try_from(n).ok())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::map_record;
    use crate::map::MapResult;
    use crate::{ProductEvent, Usage};
    use serde_json::json;

    #[test]
    fn observed_turn_maps_without_tool_events() {
        let start = json!({"type": "turn_start"});
        assert!(matches!(map_record(&start).unwrap(), MapResult::Event(ProductEvent::TurnStarted)));
        let delta = json!({
            "type": "message_update",
            "assistantMessageEvent": {"type": "text_delta", "contentIndex": 0, "delta": "fixture-reply"}
        });
        match map_record(&delta).unwrap() {
            MapResult::Event(ProductEvent::AssistantDelta { text }) => assert_eq!(text, "fixture-reply"),
            other => panic!("{other:?}"),
        }
        let end = json!({
            "type": "turn_end",
            "message": {
                "content": [{"type": "text", "text": "fixture-reply"}],
                "usage": {"input": 1, "output": 1, "totalTokens": 2},
                "stopReason": "stop"
            }
        });
        match map_record(&end).unwrap() {
            MapResult::Event(ProductEvent::TurnFinished { stop_reason, answer, usage }) => {
                assert_eq!(stop_reason, "stop");
                assert_eq!(answer.as_deref(), Some("fixture-reply"));
                assert_eq!(usage, Some(Usage { input: 1, output: 1, total: 2 }));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn aborted_and_error_message_are_not_finished() {
        let aborted = json!({"type": "turn_end", "message": {"stopReason": "aborted", "content": []}});
        match map_record(&aborted).unwrap() {
            MapResult::Event(ProductEvent::Error { reason }) => assert_eq!(reason, "aborted"),
            other => panic!("{other:?}"),
        }
        let failed = json!({
            "type": "turn_end",
            "message": {"stopReason": "aborted", "errorMessage": "Request was aborted"}
        });
        match map_record(&failed).unwrap() {
            MapResult::Event(ProductEvent::Error { reason }) => assert_eq!(reason, "Request was aborted"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn captured_tool_records_map_and_tool_use_is_not_finished() {
        for kind in ["agent_start", "agent_end", "agent_settled", "message_start", "response", "tool_execution_start", "tool_execution_update"] {
            let value = json!({"type": kind});
            assert!(matches!(map_record(&value).unwrap(), MapResult::Ignored), "{kind}");
        }
        let partial = json!({
            "type": "message_update",
            "assistantMessageEvent": {"type": "toolcall_start", "contentIndex": 1, "id": "call_fixed", "toolName": "bash"}
        });
        assert!(matches!(map_record(&partial).unwrap(), MapResult::Ignored));
        let started = json!({
            "type": "message_update",
            "assistantMessageEvent": {
                "type": "toolcall_end",
                "contentIndex": 1,
                "toolCall": {
                    "type": "toolCall",
                    "id": "call_fixed",
                    "name": "bash",
                    "arguments": {"command": "printf 'pi-harness-fixture\n'"}
                }
            }
        });
        match map_record(&started).unwrap() {
            MapResult::Event(ProductEvent::ToolCallStarted { name, call_id, args_summary }) => {
                assert_eq!(name, "bash");
                assert_eq!(call_id, "call_fixed");
                assert!(args_summary.contains("printf 'pi-harness-fixture\\n'"), "{args_summary}");
            }
            other => panic!("{other:?}"),
        }
        let finished = json!({
            "type": "tool_execution_end",
            "toolCallId": "call_fixed",
            "toolName": "bash",
            "isError": false,
            "result": {"content": [{"type": "text", "text": "pi-harness-fixture\n"}]}
        });
        match map_record(&finished).unwrap() {
            MapResult::Event(ProductEvent::ToolCallFinished { call_id, status, output_summary }) => {
                assert_eq!(call_id, "call_fixed");
                assert_eq!(status, "ok");
                assert_eq!(output_summary, "pi-harness-fixture\n");
            }
            other => panic!("{other:?}"),
        }
        let tool_use = json!({"type": "turn_end", "message": {"stopReason": "toolUse", "content": []}});
        assert!(matches!(map_record(&tool_use).unwrap(), MapResult::Ignored));
    }
}
