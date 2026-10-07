//! Evidence cards. Comment text is untrusted data. Value histograms stay withheld.

use serde_json::{json, Map, Value};

use crate::canonical::{canonical_sha256, object};
use crate::error::PgBuilderError;

const WITHHELD: &str = "withheld";

pub fn assemble_cards(
    catalog: &Value,
    profile: &Value,
    comments: &Value,
    schema_hash: &str,
    evidence_hash: &str,
    max_tables: usize,
) -> Result<Value, PgBuilderError> {
    let catalog_map = object(catalog, "catalog.schema")?;
    if catalog_map.get("protocol").and_then(Value::as_str) != Some("oo.source.pgsql/v1") {
        return Err(PgBuilderError::new("catalog.schema", "the catalog protocol is not the source catalog"));
    }
    reject_profile_values(profile)?;
    let profile_map = object(profile, "profile.schema")?;
    if profile_map.get("sample_policy").and_then(Value::as_str) != Some("none") {
        return Err(PgBuilderError::new("profile.samples", "sample policy is not none"));
    }
    let comment_map = object(comments, "comment.schema")?;
    let tables = catalog_map
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog has no tables"))?;
    if tables.is_empty() || tables.len() > max_tables {
        return Err(PgBuilderError::new("budget.tables", "the table count is outside the budget"));
    }
    let mut cards = Vec::new();
    for table in tables {
        cards.push(one_card(table, profile_map, comment_map)?);
    }
    cards.sort_by(|left, right| {
        (left["schema"].as_str(), left["name"].as_str()).cmp(&(right["schema"].as_str(), right["name"].as_str()))
    });
    if string_field(catalog_map, "schema_hash")? != schema_hash {
        return Err(PgBuilderError::new("catalog.stale", "the catalog schema hash does not match the seal"));
    }
    Ok(json!({
        "protocol": "oo.pg-builder.cards/v1",
        "schema_version": crate::CARD_SCHEMA,
        "evidence_hash": evidence_hash,
        "schema_hash": schema_hash,
        "sample_policy": "none",
        "tables": cards
    }))
}

pub fn approval_matches(cards: &Value, approval: &Value) -> Result<(), PgBuilderError> {
    let cards_map = object(cards, "approval.schema")?;
    let approval_map = object(approval, "approval.schema")?;
    require_exact(
        approval_map,
        &[
            "approved",
            "authenticated",
            "cards_sha256",
            "evidence_hash",
            "parent_auth",
            "protocol",
            "reviewer_at",
            "reviewer_label",
            "schema_hash",
            "tables",
        ],
    )?;
    if approval_map.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.card-approval/v1") {
        return Err(PgBuilderError::new("approval.schema", "the approval protocol is wrong"));
    }
    if approval_map.get("approved") != Some(&Value::Bool(true)) {
        return Err(PgBuilderError::new("approval.required", "the caller did not approve the cards"));
    }
    if approval_map.get("authenticated") != Some(&Value::Bool(false))
        || approval_map.get("parent_auth").and_then(Value::as_str) != Some("required")
    {
        return Err(PgBuilderError::new(
            "approval.authentication",
            "local operator review is not authenticated RBAC",
        ));
    }
    let cards_hash = canonical_sha256(cards);
    if approval_map.get("evidence_hash") != cards_map.get("evidence_hash")
        || approval_map.get("schema_hash") != cards_map.get("schema_hash")
        || approval_map.get("cards_sha256").and_then(Value::as_str) != Some(cards_hash.as_str())
    {
        return Err(PgBuilderError::new("approval.stale", "the approval does not match the cards"));
    }
    let approved = approval_map
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("approval.schema", "the approval has no tables"))?;
    let listed = cards_map
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("approval.schema", "the cards have no tables"))?;
    if approved.len() != listed.len() {
        return Err(PgBuilderError::new("approval.stale", "the approval does not match the cards"));
    }
    for card in listed {
        let dataset = card.get("dataset").and_then(Value::as_str).unwrap_or("");
        let hash = card.get("card_sha256").and_then(Value::as_str).unwrap_or("");
        let found = approved.iter().any(|row| {
            row.get("dataset").and_then(Value::as_str) == Some(dataset)
                && row.get("card_sha256").and_then(Value::as_str) == Some(hash)
        });
        if !found {
            return Err(PgBuilderError::new("approval.stale", "the approval does not match the cards"));
        }
    }
    Ok(())
}

fn one_card(table: &Value, profile: &Map<String, Value>, comments: &Map<String, Value>) -> Result<Value, PgBuilderError> {
    let map = object(table, "catalog.schema")?;
    let schema = string_field(map, "schema")?;
    let name = string_field(map, "name")?;
    let dataset = format!("{schema}.{name}");
    let columns = map
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "a table has no columns"))?;
    let mut column_cards = Vec::new();
    for column in columns {
        let column_map = object(column, "catalog.schema")?;
        let column_name = string_field(column_map, "name")?;
        let stats = column_stats(profile, &schema, &name, &column_name)?;
        column_cards.push(json!({
            "attnum": column_map.get("attnum").cloned().unwrap_or(Value::Null),
            "name": column_name,
            "type_name": string_field(column_map, "type_name")?,
            "not_null": column_map.get("not_null").cloned().unwrap_or(Value::Bool(false)),
            "untrusted_comment": comment_value(comments, &schema, &name, Some(&column_name))?,
            "null_frac": stats.0,
            "n_distinct": stats.1,
            "correlation": stats.2,
            "most_common_vals": WITHHELD,
            "histogram_bounds": WITHHELD
        }));
    }
    let mut card = json!({
        "dataset": dataset,
        "schema": schema,
        "schema_ident": string_field(map, "schema_ident")?,
        "name": name,
        "name_ident": string_field(map, "name_ident")?,
        "relkind": string_field(map, "relkind")?,
        "untrusted_table_comment": comment_value(comments, &schema, &name, None)?,
        "comment_authority": "untrusted_data",
        "reltuples": reltuples(profile, &schema, &name)?,
        "columns": column_cards,
        "foreign_keys": map.get("foreign_keys").cloned().unwrap_or(Value::Array(Vec::new()))
    });
    let hash = canonical_sha256(&card);
    card.as_object_mut()
        .expect("card object")
        .insert("card_sha256".to_owned(), Value::String(hash));
    Ok(card)
}

fn column_stats(
    profile: &Map<String, Value>,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<(Value, Value, Value), PgBuilderError> {
    let Some(row) = find_column(profile, schema, table, column)? else {
        return Ok((Value::Null, Value::Null, Value::Null));
    };
    Ok((
        row.get("null_frac").cloned().unwrap_or(Value::Null),
        row.get("n_distinct").cloned().unwrap_or(Value::Null),
        row.get("correlation").cloned().unwrap_or(Value::Null),
    ))
}

fn reltuples(profile: &Map<String, Value>, schema: &str, table: &str) -> Result<Value, PgBuilderError> {
    let Some(row) = find_table(profile, schema, table)? else {
        return Ok(Value::Null);
    };
    Ok(row.get("reltuples").cloned().unwrap_or(Value::Null))
}

fn find_table<'a>(
    profile: &'a Map<String, Value>,
    schema: &str,
    table: &str,
) -> Result<Option<&'a Map<String, Value>>, PgBuilderError> {
    let tables = profile
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("profile.schema", "the profile has no tables"))?;
    for item in tables {
        let map = object(item, "profile.schema")?;
        if map.get("schema").and_then(Value::as_str) == Some(schema)
            && map.get("name").and_then(Value::as_str) == Some(table)
        {
            return Ok(Some(map));
        }
    }
    Ok(None)
}

fn find_column<'a>(
    profile: &'a Map<String, Value>,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<Option<&'a Map<String, Value>>, PgBuilderError> {
    let Some(table_row) = find_table(profile, schema, table)? else {
        return Ok(None);
    };
    let columns = table_row
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("profile.schema", "a profile table has no columns"))?;
    for item in columns {
        let map = object(item, "profile.schema")?;
        if map.get("name").and_then(Value::as_str) == Some(column) {
            return Ok(Some(map));
        }
    }
    Ok(None)
}

fn comment_value(
    comments: &Map<String, Value>,
    schema: &str,
    table: &str,
    column: Option<&str>,
) -> Result<Value, PgBuilderError> {
    let tables = comments
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("comment.schema", "the comment document has no tables"))?;
    for item in tables {
        let map = object(item, "comment.schema")?;
        if map.get("schema").and_then(Value::as_str) != Some(schema)
            || map.get("name").and_then(Value::as_str) != Some(table)
        {
            continue;
        }
        let text = if let Some(column) = column {
            let mut found = None;
            if let Some(columns) = map.get("columns").and_then(Value::as_array) {
            for entry in columns {
                let entry_map = object(entry, "comment.schema")?;
                if entry_map.get("name").and_then(Value::as_str) == Some(column) {
                    found = entry_map.get("text").cloned();
                }
            }
            }
            found.unwrap_or(Value::Null)
        } else {
            map.get("text").cloned().unwrap_or(Value::Null)
        };
        return Ok(hash_comment(text));
    }
    Ok(Value::Null)
}

fn hash_comment(text: Value) -> Value {
    match text {
        Value::Null => Value::Null,
        Value::String(body) => json!({
            "text": body,
            "byte_len": body.len(),
            "sha256": crate::sha256_hex(body.as_bytes()),
            "authority": "untrusted_data"
        }),
        _ => Value::Null,
    }
}

fn reject_profile_values(profile: &Value) -> Result<(), PgBuilderError> {
    let raw = serde_json::to_string(profile).unwrap_or_default();
    for key in ["most_common_vals", "most_common_freqs", "histogram_bounds", "sample_values", "samples_masked"] {
        if raw.contains(&format!("\"{key}\"")) {
            return Err(PgBuilderError::new("profile.values", "value-bearing statistics are withheld"));
        }
    }
    Ok(())
}

fn string_field(map: &Map<String, Value>, key: &str) -> Result<String, PgBuilderError> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "a catalog field is missing"))
}

fn require_exact(map: &Map<String, Value>, keys: &[&str]) -> Result<(), PgBuilderError> {
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err(PgBuilderError::new("approval.schema", "the approval shape is wrong"));
    }
    Ok(())
}
