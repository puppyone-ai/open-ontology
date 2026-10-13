//! Map Pi RPC records onto the product events frozen in CONTRACT.md.
//! Recognized thinking records are bookkeeping. Their text is redacted and is not a finish.

use crate::{Error, ErrorKind, ProductEvent, Usage};
use serde_json::{Map, Value};

pub(crate) const REDACTED_TEXT: &str = "redacted";

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
        "thinking_start" | "thinking_delta" | "thinking_end" => accept_thinking(value, event, kind),
        "error" => accept_assistant_error(value, event),
        other => Ok(MapResult::Unknown(format!("message_update:{other}"))),
    }
}

fn accept_thinking(parent: &Value, event: &Value, kind: &str) -> Result<MapResult, Error> {
    let Some(event_obj) = event.as_object() else {
        return Err(Error::new(ErrorKind::Shape, "thinking event"));
    };
    let allowed: &[&str] = match kind {
        "thinking_start" => &["type", "contentIndex"],
        "thinking_delta" => &["type", "contentIndex", "delta"],
        "thinking_end" => &["type", "contentIndex", "content"],
        _ => return Ok(MapResult::Unknown(format!("message_update:{kind}"))),
    };
    if !exact_keys(event_obj, allowed) || require_index(event).is_err() {
        return Ok(MapResult::Unknown(format!("message_update:{kind}")));
    }
    match kind {
        "thinking_delta" => {
            if event.get("delta").and_then(Value::as_str).is_none() {
                return Err(Error::new(ErrorKind::Shape, "thinking_delta"));
            }
        }
        "thinking_end" => {
            if event.get("content").and_then(Value::as_str).is_none() {
                return Err(Error::new(ErrorKind::Shape, "thinking_end"));
            }
        }
        _ => {}
    }
    require_update_keys(parent)?;
    require_usage(parent)?;
    Ok(MapResult::Ignored)
}

fn accept_assistant_error(parent: &Value, event: &Value) -> Result<MapResult, Error> {
    let Some(event_obj) = event.as_object() else {
        return Err(Error::new(ErrorKind::Shape, "assistant error"));
    };
    if !exact_keys(event_obj, &["type", "reason", "error"]) {
        return Ok(MapResult::Unknown("message_update:error".to_owned()));
    }
    let reason = event.get("reason").and_then(Value::as_str).unwrap_or("");
    if reason != "aborted" && reason != "error" {
        return Ok(MapResult::Unknown("message_update:error".to_owned()));
    }
    if !event.get("error").is_some_and(Value::is_object) {
        return Err(Error::new(ErrorKind::Shape, "assistant error"));
    }
    require_update_keys(parent)?;
    require_usage(parent)?;
    Ok(MapResult::Event(ProductEvent::Error {
        reason: format!("assistant-stream:{reason}"),
    }))
}

fn require_update_keys(parent: &Value) -> Result<(), Error> {
    let Some(map) = parent.as_object() else {
        return Err(Error::new(ErrorKind::Shape, "message_update"));
    };
    if exact_keys(map, &["type", "usage", "assistantMessageEvent"]) {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::Shape, "message_update"))
    }
}

fn require_index(event: &Value) -> Result<(), Error> {
    if event.get("contentIndex").and_then(Value::as_u64).is_some() {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::Shape, "thinking contentIndex"))
    }
}

fn require_usage(parent: &Value) -> Result<(), Error> {
    let Some(usage) = parent.get("usage").and_then(Value::as_object) else {
        return Err(Error::new(ErrorKind::Shape, "thinking usage"));
    };
    const ALLOWED: &[&str] = &[
        "input",
        "output",
        "cacheRead",
        "cacheWrite",
        "cacheWrite1h",
        "reasoning",
        "totalTokens",
        "cost",
    ];
    if usage.keys().any(|key| !ALLOWED.contains(&key.as_str())) {
        return Err(Error::new(ErrorKind::Shape, "thinking usage"));
    }
    for key in ["input", "output", "cacheRead", "cacheWrite", "totalTokens"] {
        require_number(usage.get(key))?;
    }
    if let Some(value) = usage.get("reasoning") {
        require_number(Some(value))?;
    }
    if let Some(value) = usage.get("cacheWrite1h") {
        require_number(Some(value))?;
    }
    let Some(cost) = usage.get("cost").and_then(Value::as_object) else {
        return Err(Error::new(ErrorKind::Shape, "thinking usage"));
    };
    const COST: &[&str] = &["input", "output", "cacheRead", "cacheWrite", "total"];
    if cost.len() != COST.len() || COST.iter().any(|key| !cost.contains_key(*key)) {
        return Err(Error::new(ErrorKind::Shape, "thinking usage"));
    }
    for key in COST {
        require_number(cost.get(*key))?;
    }
    Ok(())
}

fn require_number(value: Option<&Value>) -> Result<(), Error> {
    let Some(Value::Number(number)) = value else {
        return Err(Error::new(ErrorKind::Shape, "thinking usage"));
    };
    match number.as_f64() {
        Some(number) if number.is_finite() && number >= 0.0 => Ok(()),
        _ => Err(Error::new(ErrorKind::Shape, "thinking usage")),
    }
}

fn exact_keys(map: &Map<String, Value>, allowed: &[&str]) -> bool {
    map.len() == allowed.len() && allowed.iter().all(|key| map.contains_key(*key))
}

pub(crate) fn redact_raw_line(line: &[u8]) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(line) else {
        if line_mentions_thinking(line) {
            return br#"{"type":"redacted","reason":"unparsed-thinking"}"#.to_vec();
        }
        return line.to_vec();
    };
    if !redact_value(&mut value) {
        return line.to_vec();
    }
    serde_json::to_vec(&value).unwrap_or_else(|_| br#"{"type":"redacted","reason":"unparsed-thinking"}"#.to_vec())
}

fn line_mentions_thinking(line: &[u8]) -> bool {
    let text = String::from_utf8_lossy(line);
    text.contains("thinking_delta") || text.contains("thinking_end") || text.contains("thinking_start") || text.contains("\"thinking\"")
}

fn redact_value(value: &mut Value) -> bool {
    match value {
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= redact_value(item);
            }
            changed
        }
        Value::Object(map) => redact_object(map),
        _ => false,
    }
}

fn redact_object(map: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    let event_kind = map
        .get("assistantMessageEvent")
        .and_then(|event| event.get("type"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(kind) = event_kind.as_deref()
        && let Some(Value::Object(event)) = map.get_mut("assistantMessageEvent")
    {
        changed |= redact_assistant_event(kind, event);
    }
    if map.get("type").and_then(Value::as_str) == Some("thinking")
        && let Some(Value::String(text)) = map.get_mut("thinking")
        && text != REDACTED_TEXT
    {
        text.clear();
        text.push_str(REDACTED_TEXT);
        changed = true;
    }
    if let Some(Value::Object(usage)) = map.get_mut("usage")
        && let Some(Value::String(text)) = usage.get_mut("reasoning")
        && text != REDACTED_TEXT
    {
        text.clear();
        text.push_str(REDACTED_TEXT);
        changed = true;
    }
    for value in map.values_mut() {
        changed |= redact_value(value);
    }
    changed
}

fn redact_assistant_event(kind: &str, event: &mut Map<String, Value>) -> bool {
    let field = match kind {
        "thinking_delta" => "delta",
        "thinking_end" => "content",
        _ => return false,
    };
    if event.get(field).and_then(Value::as_str) == Some(REDACTED_TEXT) {
        return false;
    }
    event.insert(field.to_owned(), Value::String(REDACTED_TEXT.to_owned()));
    true
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

    fn usage_value() -> serde_json::Value {
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

    #[test]
    fn thinking_lifecycle_is_ignored_and_redacted() {
        let secret = "pi-harness-reasoning-secret";
        let start = json!({
            "type": "message_update",
            "usage": usage_value(),
            "assistantMessageEvent": {"type": "thinking_start", "contentIndex": 0}
        });
        assert!(matches!(map_record(&start).unwrap(), MapResult::Ignored));
        let delta = json!({
            "type": "message_update",
            "usage": usage_value(),
            "assistantMessageEvent": {"type": "thinking_delta", "contentIndex": 0, "delta": secret}
        });
        assert!(matches!(map_record(&delta).unwrap(), MapResult::Ignored));
        let end = json!({
            "type": "message_update",
            "usage": usage_value(),
            "assistantMessageEvent": {"type": "thinking_end", "contentIndex": 0, "content": secret}
        });
        assert!(matches!(map_record(&end).unwrap(), MapResult::Ignored));
        let mut without_reasoning = usage_value();
        without_reasoning.as_object_mut().unwrap().remove("reasoning");
        let start_without = json!({
            "type": "message_update",
            "usage": without_reasoning,
            "assistantMessageEvent": {"type": "thinking_start", "contentIndex": 2}
        });
        assert!(matches!(map_record(&start_without).unwrap(), MapResult::Ignored));
        let redacted = super::redact_raw_line(serde_json::to_vec(&delta).unwrap().as_slice());
        let text = String::from_utf8(redacted).unwrap();
        assert!(!text.contains(secret), "{text}");
        assert!(text.contains("\"delta\":\"redacted\""), "{text}");
        let nested = json!({
            "type": "message_end",
            "message": {"content": [{"type": "thinking", "thinking": secret}]}
        });
        let redacted = super::redact_raw_line(&serde_json::to_vec(&nested).unwrap());
        let text = String::from_utf8(redacted).unwrap();
        assert!(!text.contains(secret), "{text}");
        assert!(text.contains("\"thinking\":\"redacted\""), "{text}");
        let bare = json!({
            "type": "message_update",
            "usage": {"reasoning": secret},
            "assistantMessageEvent": {"type": "thinking_delta", "contentIndex": 0, "delta": "x"}
        });
        let redacted = super::redact_raw_line(&serde_json::to_vec(&bare).unwrap());
        let text = String::from_utf8(redacted).unwrap();
        assert!(!text.contains(secret), "{text}");
        let ordinary = br#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"thinking about fixtures"}}"#;
        assert_eq!(super::redact_raw_line(ordinary), ordinary);
        let numeric = serde_json::to_vec(&json!({"usage": {"reasoning": 0, "totalTokens": 1}})).unwrap();
        assert_eq!(super::redact_raw_line(&numeric), numeric);
    }

    #[test]
    fn thinking_shape_failures_do_not_finish_or_echo_text() {
        let secret = "pi-harness-reasoning-secret";
        let extra = json!({
            "type": "message_update",
            "usage": usage_value(),
            "assistantMessageEvent": {"type": "thinking_start", "contentIndex": 0, "delta": secret}
        });
        match map_record(&extra).unwrap() {
            MapResult::Unknown(kind) => {
                assert_eq!(kind, "message_update:thinking_start");
                assert!(!kind.contains(secret));
            }
            other => panic!("{other:?}"),
        }
        let bad_usage = json!({
            "type": "message_update",
            "usage": {"input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0, "reasoning": secret, "totalTokens": 2, "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}},
            "assistantMessageEvent": {"type": "thinking_delta", "contentIndex": 0, "delta": secret}
        });
        let err = map_record(&bad_usage).unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::Shape);
        assert_eq!(err.detail, "thinking usage");
        assert!(!err.detail.contains(secret));
        for kind in ["start", "done", "thinking_partial"] {
            let value = json!({
                "type": "message_update",
                "assistantMessageEvent": {"type": kind}
            });
            match map_record(&value).unwrap() {
                MapResult::Unknown(name) => assert_eq!(name, format!("message_update:{kind}")),
                other => panic!("{kind} {other:?}"),
            }
        }
        let level = json!({"type": "thinking_level_changed", "level": "high"});
        assert!(matches!(map_record(&level).unwrap(), MapResult::Unknown(kind) if kind == "thinking_level_changed"));
    }

    #[test]
    fn assistant_stream_error_is_fail_closed_without_thinking_text() {
        let secret = "pi-harness-reasoning-secret";
        let value = json!({
            "type": "message_update",
            "usage": usage_value(),
            "assistantMessageEvent": {
                "type": "error",
                "reason": "error",
                "error": {
                    "role": "assistant",
                    "content": [{"type": "thinking", "thinking": secret}],
                    "stopReason": "error",
                    "errorMessage": "provider failed"
                }
            }
        });
        match map_record(&value).unwrap() {
            MapResult::Event(ProductEvent::Error { reason }) => {
                assert_eq!(reason, "assistant-stream:error");
                assert!(!reason.contains(secret));
                assert!(!reason.contains("provider failed"));
            }
            other => panic!("{other:?}"),
        }
        let aborted = json!({
            "type": "message_update",
            "usage": usage_value(),
            "assistantMessageEvent": {
                "type": "error",
                "reason": "aborted",
                "error": {"role": "assistant", "content": [], "stopReason": "aborted"}
            }
        });
        match map_record(&aborted).unwrap() {
            MapResult::Event(ProductEvent::Error { reason }) => assert_eq!(reason, "assistant-stream:aborted"),
            other => panic!("{other:?}"),
        }
    }
}
