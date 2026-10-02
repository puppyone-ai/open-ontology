use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::Value;

use crate::limits::{MAX_DEPTH, MAX_NODES};
use crate::node_names::{NODE_NAMES, OTHER_ONEOF_TAGS};

#[derive(Clone, Debug)]
pub struct Scan {
    pub node_count: u32,
    pub max_depth: u32,
    pub code: Option<&'static str>,
    pub detail: String,
    pub column_refs: u32,
}

pub fn scan_tree(value: &Value) -> Scan {
    let mut state = State {
        nodes: 0,
        max_depth: 0,
        column_refs: 0,
        violation: None,
        detail: String::new(),
        budget: None,
        node_names: node_name_set(),
        other_tags: other_tag_set(),
    };
    walk(value, 0, &mut state);
    let code = state.violation.or(state.budget);
    Scan {
        node_count: state.nodes,
        max_depth: state.max_depth,
        code,
        detail: state.detail,
        column_refs: state.column_refs,
    }
}

struct State {
    nodes: u32,
    max_depth: u32,
    column_refs: u32,
    violation: Option<&'static str>,
    detail: String,
    budget: Option<&'static str>,
    node_names: &'static HashSet<&'static str>,
    other_tags: &'static HashSet<&'static str>,
}

fn node_name_set() -> &'static HashSet<&'static str> {
    static N: OnceLock<HashSet<&'static str>> = OnceLock::new();
    N.get_or_init(|| NODE_NAMES.iter().copied().collect())
}

fn other_tag_set() -> &'static HashSet<&'static str> {
    static O: OnceLock<HashSet<&'static str>> = OnceLock::new();
    O.get_or_init(|| OTHER_ONEOF_TAGS.iter().copied().collect())
}

fn rank(code: &str) -> u8 {
    match code {
        "WRITE_NODE" => 0,
        "SELECT_INTO" => 1,
        "ROW_LOCK" => 2,
        "UTILITY" => 3,
        "UNKNOWN_NODE" => 4,
        "AST_TOO_DEEP" => 5,
        "AST_TOO_MANY_NODES" => 6,
        _ => 9,
    }
}

fn note(state: &mut State, code: &'static str, detail: &str) {
    let replace = match &state.violation {
        None => true,
        Some(prev) => rank(code) < rank(prev),
    };
    if replace {
        state.violation = Some(code);
        state.detail = detail.to_string();
    }
}

fn budget(state: &mut State, code: &'static str) {
    if state.budget.is_none() {
        state.budget = Some(code);
    }
}

fn classify_node(key: &str) -> Option<&'static str> {
    match key {
        "InsertStmt" | "UpdateStmt" | "DeleteStmt" | "MergeStmt" => Some("WRITE_NODE"),
        "IntoClause" => Some("SELECT_INTO"),
        "LockingClause" | "RowMarkClause" | "LockStmt" => Some("ROW_LOCK"),
        "SelectStmt" | "RawStmt" | "SetOperationStmt" => None,
        other if other.ends_with("Stmt") => Some("UTILITY"),
        _ => None,
    }
}

fn walk(value: &Value, depth: u32, state: &mut State) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "ColumnRef" {
                    state.column_refs = state.column_refs.saturating_add(1);
                }
                if state.node_names.contains(key.as_str()) {
                    let next = depth.saturating_add(1);
                    state.nodes = state.nodes.saturating_add(1);
                    if next > state.max_depth {
                        state.max_depth = next;
                    }
                    if let Some(code) = classify_node(key) {
                        note(state, code, key);
                    }
                    if next > MAX_DEPTH {
                        budget(state, "AST_TOO_DEEP");
                        continue;
                    }
                    if state.nodes > MAX_NODES {
                        budget(state, "AST_TOO_MANY_NODES");
                        continue;
                    }
                    walk(child, next, state);
                } else if state.other_tags.contains(key.as_str()) {
                    walk(child, depth, state);
                } else if key.chars().next().is_some_and(|c| c.is_uppercase()) {
                    note(state, "UNKNOWN_NODE", key);
                    walk(child, depth, state);
                } else {
                    walk(child, depth, state);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, depth, state);
            }
        }
        _ => {}
    }
}

/// True when a reparsed select list still has `*` or `relation.*`.
/// `count(*)` is a FuncCall and is not a projection star.
pub fn projection_star_present(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if let Some(select) = map.get("SelectStmt") {
                if target_list_has_star(select) {
                    return true;
                }
            }
            map.values().any(projection_star_present)
        }
        Value::Array(items) => items.iter().any(projection_star_present),
        _ => false,
    }
}

fn target_list_has_star(select: &Value) -> bool {
    let Some(targets) = select.get("target_list").and_then(Value::as_array) else {
        return false;
    };
    targets.iter().any(|target| {
        let Some(res) = target.as_object().and_then(|obj| obj.get("ResTarget")) else {
            return false;
        };
        let Some(val) = res.get("val") else {
            return false;
        };
        star_val(val)
    })
}

fn star_val(val: &Value) -> bool {
    let Some(obj) = val.as_object() else {
        return false;
    };
    if obj.contains_key("AStar") {
        return true;
    }
    let Some(col) = obj.get("ColumnRef") else {
        return false;
    };
    col.get("fields")
        .and_then(Value::as_array)
        .is_some_and(|fields| fields.iter().any(|field| field.as_object().is_some_and(|obj| obj.contains_key("AStar"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ival_is_not_a_node() {
        let scan = scan_tree(&json!({"AConst": {"val": {"Ival": {"ival": 1}}}}));
        assert_eq!(scan.node_count, 1);
        assert_eq!(scan.column_refs, 0);
        assert!(scan.code.is_none());
    }

    #[test]
    fn unknown_pascal_key_fails_closed() {
        let scan = scan_tree(&json!({"FutureStmt": {}}));
        assert_eq!(scan.code, Some("UNKNOWN_NODE"));
    }

    #[test]
    fn write_outranks_depth_budget() {
        let mut value = json!({});
        for _ in 0..130 {
            value = json!({"SelectStmt": {"inner": value}});
        }
        value = json!({"InsertStmt": {"select": value}});
        let scan = scan_tree(&value);
        assert_eq!(scan.code, Some("WRITE_NODE"));
    }

    #[test]
    fn depth_budget_is_one_past_128() {
        let mut value = json!({});
        for _ in 0..129 {
            value = json!({"SelectStmt": {"inner": value}});
        }
        let scan = scan_tree(&value);
        assert_eq!(scan.code, Some("AST_TOO_DEEP"));
        assert!(scan.max_depth > 128);
    }

    #[test]
    fn node_budget_is_one_past_10000() {
        let items: Vec<_> = (0..10_001).map(|_| json!({"AConst": {}})).collect();
        let scan = scan_tree(&json!(items));
        assert_eq!(scan.code, Some("AST_TOO_MANY_NODES"));
    }

    #[test]
    fn count_star_is_not_a_projection_star() {
        let value = json!({"SelectStmt": {"target_list": [{"ResTarget": {"val": {"FuncCall": {"agg_star": true}}}}]}});
        assert!(!projection_star_present(&value));
        let star = json!({"SelectStmt": {"target_list": [{"ResTarget": {"val": {"AStar": {}}}}]}});
        assert!(projection_star_present(&star));
    }
}
