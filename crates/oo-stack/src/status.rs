//! Closed public review state. A missing receipt is not a review hold.
use serde_json::{Value, json};

pub fn review_status(report: &mut Value) {
    let pending = report["tables"]["review_pending"].as_array();
    let held = pending.and_then(|rows| {
        if rows.is_empty() && !report["cycle"].is_object() {
            None
        } else {
            Some(rows.iter().any(|row| row["scope"] == "library"))
        }
    });
    report["held_by_review"] = held.map_or(Value::Null, Value::Bool);
    report["review_state"] = json!(match held {
        Some(true) => "held_by_review",
        Some(false) => "not_held",
        None => "unmeasured",
    });
    // Review evidence can name hidden columns. Public status gets only a code,
    // scope and count, never the owner's raw change descriptions.
    if let Some(rows) = report["tables"]["review_pending"].as_array_mut() {
        for row in rows {
            *row = json!({"scope":if row["scope"] == "library" { "library" } else { "table" },
                "code":"held_by_review"});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_hold_requires_explicit_library_evidence_and_hides_columns() {
        let mut report = json!({"tables":{"review_pending":[{"scope":"library",
            "table":"private-canary", "changes":["hidden-column-canary"]}]}});
        review_status(&mut report);
        assert_eq!(report["held_by_review"], true);
        assert_eq!(report["review_state"], "held_by_review");
        assert!(!report.to_string().contains("canary"));
        for (mut report, expected) in [
            (json!({"publication":"not_ready"}), Value::Null),
            (json!({"tables":{"review_pending":[]}}), Value::Null),
            (
                json!({"cycle":{"result":"complete"},"tables":{"review_pending":[]}}),
                json!(false),
            ),
            (
                json!({"tables":{"review_pending":[{"scope":"table"}]}}),
                json!(false),
            ),
        ] {
            review_status(&mut report);
            assert_eq!(report["held_by_review"], expected);
        }
    }
}
