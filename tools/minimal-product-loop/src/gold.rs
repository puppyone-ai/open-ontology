//! Frozen questions and cells from the public 100k fixture formulas.
//!
//! These oracles are acceptance checks. A configured source is not rejected
//! because its counts differ, and a production branch is not selected from
//! these table names. The question text does not contain the answer or a SQL statement.

use serde_json::{json, Value};

use pg_e2e::gold::{
    account_party, entry_amount_minor, entry_qty, format_minor, party_active, party_display, party_key,
    ACCOUNT_ROWS, ENTRY_ROWS, PARTY_ROWS,
};

#[derive(Clone, Copy, Debug)]
pub struct FrozenQuestion {
    pub id: &'static str,
    pub text: &'static str,
}

pub fn frozen_questions() -> &'static [FrozenQuestion] {
    &[
        FrozenQuestion {
            id: "count-entry",
            text: "How many rows are in shop.entry? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "fk-account",
            text: "What is the party key of shop.account row 0? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "null-party",
            text: "For shop.party key p00000, return the key, the display name, and the active flag. Write the SQL yourself.",
        },
        FrozenQuestion {
            id: "qty-bounds",
            text: "Return entry id and quantity for the first three shop.entry rows, ordered by entry id. Write the SQL yourself.",
        },
        FrozenQuestion {
            id: "amount-exact",
            text: "Return the amount of shop.entry row 3. Write the SQL yourself and keep the exact numeric text.",
        },
        FrozenQuestion {
            id: "null-display-count",
            text: "How many shop.party rows have a null display name? Write the SQL yourself.",
        },
    ]
}

pub fn question_text(id: &str) -> Option<&'static str> {
    frozen_questions().iter().find(|item| item.id == id).map(|item| item.text)
}

pub fn bulk_questions() -> &'static [FrozenQuestion] {
    &[
        FrozenQuestion {
            id: "count-batch-c",
            text: "How many rows are in public.batch_c? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "id-overlap",
            text: "How many public.batch_a ids also appear in public.batch_c? There is no foreign key between these tables. Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "amount-when-qty-above",
            text: "What is the exact sum of public.batch_c.amount for rows whose qty is greater than 2? Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "null-amount",
            text: "How many public.batch_a rows have a null amount? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "batch-a-amount",
            text: "Return the amount of the public.batch_a row whose id is 2. Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "batch-c-qty-bounds",
            text: "Return id and qty for the public.batch_c rows whose id is 1 or 2, ordered by id. Write the SQL yourself and run it with oo query.",
        },
    ]
}

/// Older ids still resolve to a fixture sentence. They are not the frozen set,
/// and `binary-row` has no positive cell. Callers must not treat these cells
/// as a product success signal.
fn legacy_bulk(id: &str) -> Option<&'static str> {
    match id {
        "decimal-row" => bulk_questions().iter().find(|item| item.id == "batch-a-amount").map(|item| item.text),
        "qty-above" => Some("How many public.batch_c rows have a quantity greater than two? Write the SQL yourself and run it with oo query."),
        "binary-row" => Some("Return the blob of the public.batch_b row whose id is 1. Write the SQL yourself and run it with oo query."),
        _ => None,
    }
}

pub fn question_text_for(set: &str, id: &str) -> Option<&'static str> {
    match set {
        "shop" => question_text(id),
        "bulk" => bulk_questions()
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.text)
            .or_else(|| legacy_bulk(id)),
        _ => None,
    }
}

fn batch_a_amount(id: i64) -> Option<&'static str> {
    match id {
        1 => None,
        2 => Some("1234567890123456789012345678901234.1234"),
        _ => Some("0.0001"),
    }
}

fn batch_c_qty(id: i64) -> i64 {
    if id == 1 {
        i64::MAX
    } else if id == 2 {
        i64::MIN
    } else {
        id
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn expected_counts() -> (i64, i64, i64) {
    (i64::from(PARTY_ROWS), i64::from(ACCOUNT_ROWS), i64::from(ENTRY_ROWS))
}

pub fn expected_cells(id: &str) -> Option<Vec<Vec<Value>>> {
    match id {
        "count-entry" => Some(vec![vec![json!(ENTRY_ROWS.to_string())]]),
        "fk-account" => Some(vec![vec![json!("0"), json!(account_party(0))]]),
        "null-party" => Some(vec![vec![
            json!(party_key(0)),
            match party_display(0) {
                Some(text) => Value::String(text),
                None => Value::Null,
            },
            json!(party_active(0)),
        ]]),
        "qty-bounds" => Some(
            (0..3)
                .map(|index| vec![json!(index.to_string()), json!(entry_qty(index).to_string())])
                .collect(),
        ),
        "amount-exact" => {
            let minor = entry_amount_minor(3)?;
            Some(vec![vec![json!(format_minor(minor))]])
        }
        "null-display-count" => {
            let count = (0..PARTY_ROWS).filter(|index| party_display(*index).is_none()).count();
            Some(vec![vec![json!(count.to_string())]])
        }
        "count-batch-c" => Some(vec![vec![json!("90000")]]),
        "id-overlap" => Some(vec![vec![json!("2000")]]),
        "amount-when-qty-above" => Some(vec![vec![json!("134997.0000")]]),
        "null-amount" => Some(vec![vec![json!("1")]]),
        "batch-a-amount" | "decimal-row" => Some(vec![vec![json!(batch_a_amount(2).expect("amount"))]]),
        "batch-c-qty-bounds" => Some(vec![
            vec![json!("1"), json!("9223372036854775807")],
            vec![json!("2"), json!("-9223372036854775808")],
        ]),
        // Legacy acceptance cell. The product ask path does not read this.
        "qty-above" => {
            let count = (1..=90_000).filter(|id| batch_c_qty(*id) > 2).count();
            Some(vec![vec![json!(count.to_string())]])
        }
        "binary-row" => None,
        _ => None,
    }
}

/// Acceptance comparison only. The ask path does not call this.
#[cfg_attr(not(test), allow(dead_code))]
pub fn cells_match(id: &str, rows: &[Vec<Value>]) -> bool {
    expected_cells(id).is_some_and(|expected| expected == rows)
}

#[cfg(test)]
mod tests {
    use super::{expected_cells, expected_counts, frozen_questions};
    use serde_json::json;

    #[test]
    fn frozen_questions_match_the_public_gold_and_hide_the_answer() {
        assert_eq!(expected_counts(), (2000, 8000, 90000));
        assert_eq!(expected_cells("count-entry").expect("count")[0][0].as_str(), Some("90000"));
        assert_eq!(expected_cells("amount-exact").expect("amount")[0][0].as_str(), Some("1234567890.1234"));
        assert_eq!(expected_cells("null-display-count").expect("nulls")[0][0].as_str(), Some("118"));
        let qty = expected_cells("qty-bounds").expect("qty");
        let min = i64::MIN.to_string();
        let max = i64::MAX.to_string();
        assert_eq!(qty.len(), 3);
        assert_eq!(qty[0][1].as_str(), Some(min.as_str()));
        assert_eq!(qty[1][1].as_str(), Some(max.as_str()));
        assert_eq!(qty[2][1].as_str(), Some("0"));
        let party = &expected_cells("null-party").expect("party")[0];
        assert_eq!(party[0].as_str(), Some("p00000"));
        assert!(party[1].is_null());
        assert_eq!(party[2].as_bool(), Some(true));
        assert_eq!(expected_cells("fk-account").expect("fk")[0][0].as_str(), Some("0"));
        for question in frozen_questions() {
            assert!(!question.text.contains("SELECT"), "{}", question.id);
            assert!(!question.text.contains("90000"), "{}", question.id);
            assert!(!question.text.contains("1234567890"), "{}", question.id);
        }
    }

    #[test]
    fn bulk_questions_use_the_public_formula_and_hide_the_cells() {
        use super::{bulk_questions, question_text_for};
        let card = include_str!("../../../testing/minimal-loop-acceptance/BULK-QUESTIONS.md");
        assert_eq!(bulk_questions().len(), 6);
        assert!(super::cells_match("count-batch-c", &expected_cells("count-batch-c").expect("count")));
        assert!(!super::cells_match("count-batch-c", &[vec![json!("1")]]));
        assert!(!super::cells_match("binary-row", &[]));
        assert_eq!(expected_cells("count-batch-c").expect("count")[0][0].as_str(), Some("90000"));
        assert_eq!(expected_cells("id-overlap").expect("overlap")[0][0].as_str(), Some("2000"));
        assert_eq!(expected_cells("amount-when-qty-above").expect("sum")[0][0].as_str(), Some("134997.0000"));
        assert_eq!(expected_cells("null-amount").expect("null")[0][0].as_str(), Some("1"));
        assert_eq!(
            expected_cells("batch-a-amount").expect("amount")[0][0].as_str(),
            Some("1234567890123456789012345678901234.1234")
        );
        let bounds = expected_cells("batch-c-qty-bounds").expect("bounds");
        assert_eq!(bounds.len(), 2);
        assert_eq!(bounds[0][1].as_str(), Some("9223372036854775807"));
        assert_eq!(bounds[1][1].as_str(), Some("-9223372036854775808"));
        assert_eq!(expected_cells("qty-above").expect("legacy qty")[0][0].as_str(), Some("89999"));
        assert!(expected_cells("binary-row").is_none());
        assert!(question_text_for("bulk", "decimal-row").is_some());
        assert!(question_text_for("bulk", "binary-row").unwrap().contains("blob"));
        assert!(!bulk_questions().iter().any(|item| item.id == "binary-row" || item.id == "qty-above"));
        for question in bulk_questions() {
            assert!(card.contains(question.text), "{}", question.id);
            assert!(!question.text.contains("SELECT"), "{}", question.id);
            assert!(!question.text.contains("90000"), "{}", question.id);
            assert!(!question.text.contains("134997"), "{}", question.id);
            assert!(!question.text.contains("1234567890"), "{}", question.id);
            assert!(!question.text.contains("0001ff"), "{}", question.id);
            assert!(!question.text.contains("9223372036854775807"), "{}", question.id);
        }
    }

    #[test]
    fn fixture_questions_stay_on_their_acceptance_set() {
        use super::question_text_for;
        assert!(question_text_for("bulk", "count-batch-c").is_some());
        assert!(question_text_for("shop", "count-entry").is_some());
        assert!(question_text_for("configured", "count-batch-c").is_none());
        assert!(question_text_for("sealed", "count-entry").is_none());
        assert!(question_text_for("bulk", "count-entry").is_none());
    }
}
