use serde::Deserialize;

use crate::fault::Fault;
use crate::grammar::model_id;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub selected: Option<String>,
    pub candidate_count: u64,
    pub grammar_match_count: u64,
}

#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelItem>,
}

#[derive(Deserialize)]
struct ModelItem {
    id: Option<String>,
}

pub const REQUIRED_MODEL: &str = "grok-4.7";

pub fn exact_model(body: &[u8], required: &str) -> Result<Option<String>, Fault> {
    if !model_id(required) {
        return Err(Fault::config());
    }
    let list: ModelList = serde_json::from_slice(body).map_err(|_| Fault::upstream())?;
    if list.data.len() > 128 {
        return Err(Fault::response_budget());
    }
    let found = list.data.iter().any(|item| item.id.as_deref() == Some(required));
    Ok(found.then(|| required.to_string()))
}

pub fn select_model(body: &[u8]) -> Result<Selection, Fault> {
    let list: ModelList = serde_json::from_slice(body).map_err(|_| Fault::upstream())?;
    if list.data.len() > 128 {
        return Err(Fault::response_budget());
    }
    let mut selected = None;
    let mut grammar_match_count = 0_u64;
    for item in &list.data {
        let Some(id) = item.id.as_deref() else {
            continue;
        };
        if model_id(id) {
            grammar_match_count = grammar_match_count.saturating_add(1);
            if selected.is_none() {
                selected = Some(id.to_string());
            }
        }
    }
    Ok(Selection {
        selected,
        candidate_count: u64::try_from(list.data.len()).unwrap_or(u64::MAX),
        grammar_match_count,
    })
}

#[cfg(test)]
mod tests {
    use super::select_model;

    #[test]
    fn selects_first_grammar_id_and_hides_the_rest() {
        let body = br#"{"data":[{"id":"openai/gpt"},{"id":"widget-local.1"},{"id":"other_model"}]}"#;
        let selection = select_model(body).expect("parse");
        assert_eq!(selection.selected.as_deref(), Some("widget-local.1"));
        assert_eq!(selection.candidate_count, 3);
        assert_eq!(selection.grammar_match_count, 2);
    }

    #[test]
    fn exact_model_accepts_only_the_required_id() {
        let listed = br#"{"data":[{"id":"grok-4.3"},{"id":"grok-4.7"}]}"#;
        assert_eq!(
            super::exact_model(listed, super::REQUIRED_MODEL).expect("parse").as_deref(),
            Some("grok-4.7")
        );
        let older = br#"{"data":[{"id":"grok-4.3"}]}"#;
        assert_eq!(super::exact_model(older, super::REQUIRED_MODEL).expect("parse"), None);
    }
}
