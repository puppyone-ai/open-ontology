//! Caller-side input for `write_sealed_pack`.
//!
//! `pack.rs` is owned by the pack writer. This module only checks the object
//! that writer accepts and does not write a pack.

use serde_json::{json, Map, Value};

pub const PACK_INPUT_PROTOCOL: &str = "oo.minimal-product-loop.pack-input/v1";
pub const PACK_RECEIPT_PROTOCOL: &str = "oo.minimal-product-loop.pack-receipt/v1";

pub fn sealed_pack_input(
    catalog: &Value,
    profile: &Value,
    generation: &Value,
    binding: &Value,
    tables: &Value,
) -> Result<Value, &'static str> {
    protocol(catalog, "oo.source.pgsql/v1")?;
    object(catalog)?;
    protocol(profile, "oo.pg-builder.profile/v1")?;
    if profile.get("sample_policy").and_then(Value::as_str) != Some("none") {
        return Err("pack.input");
    }
    protocol(generation, "oo.pg-builder.generation/v1")?;
    if generation.get("outcome").and_then(Value::as_str) != Some("validated") {
        return Err("pack.input");
    }
    if generation.pointer("/transport/model_id").and_then(Value::as_str) != Some("grok-4.7") {
        return Err("pack.input");
    }
    match generation.pointer("/transport/declared_cost/status").and_then(Value::as_str) {
        None | Some("unknown") => {}
        _ => return Err("pack.input"),
    }
    let bound = object(binding)?;
    const KEYS: &[&str] = &[
        "import_generation",
        "import_hash",
        "manifest_sha256",
        "snapshot_id",
        "source_id",
        "target_config_sha256",
    ];
    if bound.len() != KEYS.len() || KEYS.iter().any(|key| !bound.contains_key(*key)) {
        return Err("pack.input");
    }
    let generation_number = bound.get("import_generation").and_then(Value::as_u64).ok_or("pack.input")?;
    if generation_number == 0 {
        return Err("pack.input");
    }
    hex64(bound.get("import_hash").ok_or("pack.input")?)?;
    hex64(bound.get("manifest_sha256").ok_or("pack.input")?)?;
    hex64(bound.get("target_config_sha256").ok_or("pack.input")?)?;
    plain(bound.get("snapshot_id").ok_or("pack.input")?)?;
    plain(bound.get("source_id").ok_or("pack.input")?)?;
    let rows = tables.as_array().ok_or("pack.input")?;
    if rows.is_empty() || rows.len() > 16 {
        return Err("pack.input");
    }
    for row in rows {
        let table = object(row)?;
        plain(table.get("schema").ok_or("pack.input")?)?;
        plain(table.get("name").ok_or("pack.input")?)?;
        if table.get("rows").and_then(Value::as_u64).is_none() {
            return Err("pack.input");
        }
    }
    Ok(json!({
        "protocol": PACK_INPUT_PROTOCOL,
        "catalog": catalog,
        "profile": profile,
        "generation": generation,
        "binding": binding,
        "tables": tables
    }))
}

fn protocol(value: &Value, expect: &str) -> Result<(), &'static str> {
    if value.get("protocol").and_then(Value::as_str) == Some(expect) {
        Ok(())
    } else {
        Err("pack.input")
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, &'static str> {
    value.as_object().ok_or("pack.input")
}

fn plain(value: &Value) -> Result<(), &'static str> {
    match value.as_str() {
        Some(text) if !text.is_empty() && !text.chars().any(char::is_control) => Ok(()),
        _ => Err("pack.input"),
    }
}

fn hex64(value: &Value) -> Result<(), &'static str> {
    let Some(text) = value.as_str() else {
        return Err("pack.input");
    };
    let lower_hex = text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if lower_hex { Ok(()) } else { Err("pack.input") }
}

#[cfg(test)]
mod tests {
    use super::sealed_pack_input;
    use serde_json::json;

    fn generation() -> serde_json::Value {
        json!({
            "protocol": "oo.pg-builder.generation/v1",
            "outcome": "validated",
            "transport": {
                "model_id": "grok-4.7",
                "declared_cost": {"status": "unknown"}
            }
        })
    }

    fn binding() -> serde_json::Value {
        json!({
            "import_hash": "f7c21ea98c20b88b8a7986da37f77772f9d8874f2efc055b48afdb0fef31aed1",
            "snapshot_id": "0000000D-0000000C-1",
            "source_id": "a407bulk",
            "import_generation": 2,
            "manifest_sha256": "77adb990b8aac4ec42ab2e328e7b38cf5cef5c103efc6b13dd7168d1306debc7",
            "target_config_sha256": "379615f4f792d2afa6ecceb851355edc309f1473162f5f6ea7ccdf835c42363d"
        })
    }

    #[test]
    fn sealed_pack_input_keeps_the_supplied_objects() {
        let catalog = json!({"protocol": "oo.source.pgsql/v1", "tables": []});
        let profile = json!({"protocol": "oo.pg-builder.profile/v1", "sample_policy": "none", "tables": []});
        let tables = json!([{"schema": "public", "name": "batch_c", "rows": 90000}]);
        let input = sealed_pack_input(&catalog, &profile, &generation(), &binding(), &tables).expect("input");
        assert_eq!(input.get("protocol").and_then(|value| value.as_str()), Some("oo.minimal-product-loop.pack-input/v1"));
        assert_eq!(input.get("catalog"), Some(&catalog));
        assert_eq!(input.get("generation").and_then(|value| value.pointer("/transport/declared_cost/status")).and_then(|value| value.as_str()), Some("unknown"));
        assert_eq!(input.get("tables"), Some(&tables));
    }

    #[test]
    fn sealed_pack_input_rejects_a_missing_or_unvalidated_generation() {
        let catalog = json!({"protocol": "oo.source.pgsql/v1"});
        let profile = json!({"protocol": "oo.pg-builder.profile/v1", "sample_policy": "none"});
        let tables = json!([{"schema": "public", "name": "batch_c", "rows": 90000}]);
        let mut broken = generation();
        broken["outcome"] = json!("structural_draft");
        assert!(sealed_pack_input(&catalog, &profile, &broken, &binding(), &tables).is_err());
        let mut extra = binding();
        extra["password"] = json!("no");
        assert!(sealed_pack_input(&catalog, &profile, &generation(), &extra, &tables).is_err());
        let sampled = json!({"protocol": "oo.pg-builder.profile/v1", "sample_policy": "rows"});
        assert!(sealed_pack_input(&catalog, &sampled, &generation(), &binding(), &tables).is_err());
    }
}
