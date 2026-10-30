use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub fn project(
    source: &str,
    schema: &str,
    table: &str,
    records: &Value,
) -> Result<Vec<Value>, &'static str> {
    let records = records
        .as_array()
        .filter(|r| !r.is_empty() && r.len() <= 10000)
        .ok_or("reject_records")?;
    let mut out = Vec::with_capacity(records.len());
    for record in records {
        let object = record.as_object().ok_or("reject_record")?;
        if object.keys().any(|key| {
            ![
                "protocol",
                "source_id",
                "schema",
                "table",
                "column",
                "attnum",
                "path",
                "row_index",
                "key_digest",
                "reason",
                "value_len",
                "run_generation",
                "input_digest",
                "type_policy_version",
                "budget_fact",
                "boundary_sha256",
                "raw_value_recorded",
            ]
            .contains(&key.as_str())
        }) || record["source_id"] != source
            || record["schema"] != schema
            || record["table"] != table
            || record["raw_value_recorded"] == true
        {
            return Err("reject_scope");
        }
        let reason = record["reason"]
            .as_str()
            .filter(|r| {
                [
                    "charset",
                    "row_budget_exceeded",
                    "zero_date",
                    "bool_domain",
                    "nul_byte",
                    "invalid_date",
                    "date_range",
                    "int_range",
                    "decimal_range",
                    "decimal_text",
                    "parse_error",
                    "out_of_range",
                    "invalid_json",
                ]
                .contains(r)
            })
            .ok_or("reject_reason")?;
        let column = record["column"].as_str().unwrap_or("*");
        if column.is_empty()
            || column.len() > 64
            || !column
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'*')
        {
            return Err("reject_column");
        }
        let key = record["key_digest"]
            .as_str()
            .or_else(|| record["boundary_sha256"].as_str())
            .map(str::to_owned)
            .or_else(|| {
                let input = record["input_digest"].as_str()?;
                let row = record["row_index"].as_u64()?;
                if input.len() != 64
                    || !input
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return None;
                }
                Some(format!(
                    "{:x}",
                    Sha256::digest(format!("masked-row/v1\n{input}\n{row}").as_bytes())
                ))
            })
            .ok_or("reject_key")?;
        if key.len() != 64
            || !key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("reject_key");
        }
        let value_len = if record["value_len"].is_null() {
            Value::Null
        } else {
            json!(
                record["value_len"]
                    .as_u64()
                    .filter(|n| *n <= i64::MAX as u64)
                    .ok_or("reject_length")?
            )
        };
        out.push(json!({"pk":key,"column":column,"reason":reason,"raw_value":{"masked":true,"value_len":value_len}}));
    }
    Ok(out)
}
