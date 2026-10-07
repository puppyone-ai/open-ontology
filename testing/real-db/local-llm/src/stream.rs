use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct UsageBits {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

pub fn assemble_sse(body: &[u8]) -> Result<Vec<u8>, ()> {
    let text = std::str::from_utf8(body).map_err(|_| ())?;
    if text.is_empty() || !text.ends_with("\n\n") {
        return Err(());
    }
    let mut content = String::new();
    let mut saw_delta = false;
    let mut finish: Option<String> = None;
    let mut usage: Option<UsageBits> = None;
    let mut done = false;
    for event in text.split("\n\n") {
        if event.is_empty() {
            continue;
        }
        if done {
            return Err(());
        }
        let Some(payload) = event_data(event)? else {
            continue;
        };
        if payload == "[DONE]" {
            done = true;
            continue;
        }
        let value: Value = serde_json::from_str(&payload).map_err(|_| ())?;
        if let Some(raw_usage) = value.get("usage") {
            if raw_usage.is_null() {
                continue;
            }
            if usage.is_some() {
                return Err(());
            }
            usage = Some(serde_json::from_value(raw_usage.clone()).map_err(|_| ())?);
        }
        let choices = match value.get("choices") {
            Some(Value::Array(choices)) => choices,
            Some(_) => return Err(()),
            None => continue,
        };
        if choices.len() > 1 {
            return Err(());
        }
        let Some(choice) = choices.first() else {
            continue;
        };
        if let Some(index) = choice.get("index")
            && index.as_u64() != Some(0)
        {
            return Err(());
        }
        if let Some(reason) = choice.get("finish_reason")
            && !reason.is_null()
        {
            let reason = reason.as_str().ok_or(())?.to_string();
            if finish.as_ref().is_some_and(|current| current != &reason) {
                return Err(());
            }
            finish = Some(reason);
        }
        if let Some(delta) = choice.get("delta")
            && let Some(piece) = delta.get("content")
            && !piece.is_null()
        {
            content.push_str(piece.as_str().ok_or(())?);
            saw_delta = true;
        }
        if let Some(message) = choice.get("message")
            && let Some(piece) = message.get("content")
            && !piece.is_null()
        {
            if saw_delta {
                return Err(());
            }
            content = piece.as_str().ok_or(())?.to_string();
        }
    }
    let (Some(finish_reason), Some(usage)) = (finish, usage) else {
        return Err(());
    };
    let envelope = serde_json::json!({
        "choices": [{
            "finish_reason": finish_reason,
            "message": {"role": "assistant", "content": content}
        }],
        "usage": {
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total_tokens
        }
    });
    serde_json::to_vec(&envelope).map_err(|_| ())
}

fn event_data(event: &str) -> Result<Option<String>, ()> {
    let mut data: Option<String> = None;
    for line in event.split('\n') {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if line.starts_with("event:") || line.starts_with("id:") || line.starts_with("retry:") {
            continue;
        }
        let Some(rest) = line.strip_prefix("data:") else {
            return Err(());
        };
        let value = rest.strip_prefix(' ').unwrap_or(rest);
        match data {
            Some(mut existing) => {
                existing.push('\n');
                existing.push_str(value);
                data = Some(existing);
            }
            None => data = Some(value.to_string()),
        }
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::assemble_sse;

    #[test]
    fn finish_and_usage_become_one_envelope() {
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"{\\\"sql\\\":\\\"ok\\\"}\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        );
        let envelope = assemble_sse(body.as_bytes()).expect("assemble");
        let value: serde_json::Value = serde_json::from_slice(&envelope).expect("json");
        assert_eq!(value["choices"][0]["finish_reason"], "stop");
        assert_eq!(value["choices"][0]["message"]["content"], "{\"sql\":\"ok\"}");
        assert_eq!(value["usage"]["total_tokens"], 3);
    }

    #[test]
    fn missing_usage_is_rejected() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"},\"finish_reason\":\"stop\"}]}\n\n";
        assert!(assemble_sse(body.as_bytes()).is_err());
    }

    #[test]
    fn cut_body_is_rejected() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n";
        assert!(assemble_sse(body.as_bytes()).is_err());
    }
}
