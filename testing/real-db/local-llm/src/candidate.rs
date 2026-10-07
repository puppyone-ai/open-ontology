use serde::Deserialize;
use serde_json::Value;

const CANDIDATE_PROTOCOL: &str = "oo.ontology-dialogue.sql-candidate/v1";

#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    finish_reason: Option<String>,
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

#[derive(Deserialize)]
struct Candidate {
    protocol: String,
    sql: String,
}

pub fn is_sql_candidate(body: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let Ok(completion) = serde_json::from_value::<Completion>(value) else {
        return false;
    };
    if completion.choices.len() != 1 || completion.choices[0].finish_reason.as_deref() != Some("stop") {
        return false;
    }
    let Some(total) = completion
        .usage
        .prompt_tokens
        .checked_add(completion.usage.completion_tokens)
    else {
        return false;
    };
    if total != completion.usage.total_tokens
        || completion.usage.prompt_tokens > 100_000
        || completion.usage.completion_tokens > 4096
        || completion.usage.total_tokens > 100_000
    {
        return false;
    }
    let Some(content) = completion.choices[0].message.content.as_deref() else {
        return false;
    };
    let Ok(candidate_value) = serde_json::from_str::<Value>(content) else {
        return false;
    };
    if candidate_value.as_object().is_none_or(|object| object.len() != 2) {
        return false;
    }
    let Ok(candidate) = serde_json::from_value::<Candidate>(candidate_value) else {
        return false;
    };
    candidate.protocol == CANDIDATE_PROTOCOL
        && !candidate.sql.is_empty()
        && !candidate.sql.contains('\0')
        && candidate.sql.len() <= 65536
}

#[cfg(test)]
mod tests {
    use super::is_sql_candidate;

    #[test]
    fn accepts_one_stop_candidate() {
        let content = r#"{"protocol":"oo.ontology-dialogue.sql-candidate/v1","sql":"select 1"}"#;
        let body = format!(
            r#"{{"choices":[{{"finish_reason":"stop","message":{{"content":{content:?}}}}}],"usage":{{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}}}"#
        );
        assert!(is_sql_candidate(body.as_bytes()));
        assert!(!is_sql_candidate(br#"{"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#));
    }
}
