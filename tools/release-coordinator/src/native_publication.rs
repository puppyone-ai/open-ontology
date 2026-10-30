use serde_json::Value;
use std::collections::BTreeSet;

use crate::error::StoreError;

pub const PROTOCOL: &str = "oo.pipeline.native-publication-receipt/v1";

#[derive(Clone, Debug)]
pub struct Publication {
    pub run_id: String,
    pub receipt_sha256: String,
    pub receipt: Value,
}

pub fn from_context(value: &Value) -> Result<Option<Publication>, StoreError> {
    let Some(context) = value.get("publication") else {
        return Ok(None);
    };
    let object = context.as_object().ok_or(StoreError::Input)?;
    if object.len() != 4 || object.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) && object.get("protocol").and_then(Value::as_str) != Some(crate::native_library::PROTOCOL) {
        return Err(StoreError::Input);
    }
    let run_id = object
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|id| uuid_ok(id))
        .ok_or(StoreError::Input)?;
    let receipt_sha256 = object
        .get("receipt_sha256")
        .and_then(Value::as_str)
        .filter(|sha| hex64(sha))
        .ok_or(StoreError::Input)?;
    let receipt = object.get("receipt").cloned().ok_or(StoreError::Input)?;
    if crate::native_library::is_library(&receipt) {
        crate::native_library::validate(&receipt, run_id)?;
        if object.get("protocol").and_then(Value::as_str) != Some(crate::native_library::PROTOCOL) { return Err(StoreError::Input); }
    } else {
        if object.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) { return Err(StoreError::Input); }
        // Only the library envelope proves the complete multi-table membership.
        // Preserve the legacy single-collection boundary at its entry point.
        if receipt["tables"].as_array().is_some_and(|tables| tables.len() > 256)
            || receipt.get("run_ids").and_then(Value::as_array).is_some_and(|runs| runs.len() > 32)
        {
            return Err(StoreError::Input);
        }
        validate_receipt(&receipt, run_id)?;
    }
    if crate::canon::canonical_sha(&receipt) != receipt_sha256 {
        return Err(StoreError::Input);
    }
    Ok(Some(Publication {
        run_id: run_id.to_owned(),
        receipt_sha256: receipt_sha256.to_owned(),
        receipt,
    }))
}

pub fn validate_context(value: &Value) -> Result<(), StoreError> {
    let object = value.as_object().ok_or(StoreError::Input)?;
    let has_datasets = object.contains_key("datasets");
    if !has_datasets
        || object
            .keys()
            .any(|key| !["datasets", "publication"].contains(&key.as_str()))
    {
        return Err(StoreError::Input);
    }
    if let Some(publication) = from_context(value)? {
        let entries = object
            .get("datasets")
            .and_then(Value::as_array)
            .ok_or(StoreError::Input)?;
        let receipt_tables = crate::native_library::tables(&publication.receipt)?;
        if entries.len() != receipt_tables.len() {
            return Err(StoreError::Input);
        }
        let library_tables = if crate::native_library::is_library(&publication.receipt) {
            let mut indexed = std::collections::BTreeMap::new();
            for table in &receipt_tables {
                let (schema, name) = storage_names(table)?;
                if indexed.insert(format!("{schema}.{name}"), table).is_some() {
                    return Err(StoreError::Input);
                }
            }
            Some(indexed)
        } else {
            None
        };
        let mut datasets = BTreeSet::new();
        for (entry, ordered_table) in entries.iter().zip(&receipt_tables) {
            let table = if let Some(indexed) = &library_tables {
                *indexed.get(entry["dataset"].as_str().ok_or(StoreError::Input)?)
                    .ok_or(StoreError::Input)?
            } else { ordered_table };
            let dataset = entry
                .get("dataset")
                .and_then(Value::as_str)
                .ok_or(StoreError::Input)?;
            let (schema, name) = dataset.split_once('.').ok_or(StoreError::Input)?;
            let source = entry
                .get("source")
                .and_then(Value::as_str)
                .ok_or(StoreError::Input)?;
            let generation = entry
                .get("generation")
                .and_then(Value::as_i64)
                .ok_or(StoreError::Input)?;
            if !datasets.insert(dataset.to_owned())
                || source
                    != (if crate::native_library::is_library(&publication.receipt) { &table["source_id"] } else { &publication.receipt["source_id"] })
                        .as_str()
                        .ok_or(StoreError::Input)?
                || generation
                    != (if per_table_collection(&publication.receipt) || crate::native_library::is_library(&publication.receipt) { &table["source_generation"] } else { &publication.receipt["source_generation"] })
                        .as_i64()
                        .ok_or(StoreError::Input)?
                || (schema, name) != storage_names(table)?
            {
                return Err(StoreError::Input);
            }
        }
    }
    Ok(())
}

/// Validate every member; library callers additionally bind full membership and
/// explicit publication capacity. Non-library limits belong to `from_context`.
pub(crate) fn validate_receipt(receipt: &Value, run_id: &str) -> Result<(), StoreError> {
    if receipt.as_object().is_none()
        || receipt["protocol"].as_str() != Some(PROTOCOL)
        || receipt["run_id"].as_str() != Some(run_id)
        || receipt["source_id"].as_str().is_none()
        || receipt["source_generation"]
            .as_i64()
            .is_none_or(|value| value < 1)
        || receipt["target_generation"]
            .as_i64()
            .is_none_or(|value| value < 1)
        || receipt["position_codec"].as_str().is_none()
        || receipt["consistency"]
            .as_str()
            .is_none_or(|value| !["transactional", "per_table"].contains(&value))
        || receipt["consistency_grade"]
            .as_str()
            .is_none_or(|value| !["T", "S1", "S0", "B"].contains(&value))
        || receipt["cross_table_consistent"].as_bool().is_none()
    {
        return Err(StoreError::Input);
    }
    if receipt.get("collection_mode").is_some() && (!per_table_collection(receipt) || receipt.get("run_ids").is_none()) {
        return Err(StoreError::Input);
    }
    let codec = receipt["position_codec"].as_str().unwrap();
    let position = integer_text(&receipt["applied_pos"]).ok_or(StoreError::Input)?;
    if position.starts_with('-') || (codec == "snapshot-zero/v1" && position != "0") {
        return Err(StoreError::Input);
    }
    let tables = receipt["tables"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or(StoreError::Input)?;
    if let Some(runs)=receipt.get("run_ids") {
        let runs=runs.as_array().filter(|r|r.len()==tables.len()).ok_or(StoreError::Input)?;
        let ids=runs.iter().map(|v|v.as_str().filter(|s|uuid_ok(s)).ok_or(StoreError::Input)).collect::<Result<BTreeSet<_>,_>>()?;
        if per_table_collection(receipt) {
            if ids.len()!=runs.len() || !ids.contains(run_id)
                || !["snapshot-zero/v1", "partition-signature/v1"].contains(&codec) || position!="0"
                || receipt["consistency"]!="per_table" || receipt["cross_table_consistent"]!=false
                || !["S0", "B"].contains(&receipt["consistency_grade"].as_str().unwrap_or(""))
                || tables.iter().map(|t|t["run_id"].as_str().filter(|id|ids.contains(id)).ok_or(StoreError::Input)).collect::<Result<BTreeSet<_>,_>>()?.len()!=ids.len()
                || !tables.iter().any(|t| t["run_id"]==run_id && t["source_generation"]==receipt["source_generation"] && t["target_generation"]==receipt["target_generation"])
            {return Err(StoreError::Input);}
        } else if ids.len()!=runs.len() || !ids.contains(run_id)
            || !matches!(codec, "mysql-gtid/v1" | "oracle-capture-sequence/v1")
            || receipt["consistency"]!="transactional" || receipt["consistency_grade"]!="T"
            || receipt["cross_table_consistent"]!=true || receipt["source_epoch"].as_i64().is_none_or(|n|n<1)
            || (codec=="mysql-gtid/v1" && (receipt.get("oracle_buffer").is_some()
                || receipt["position_lineage"].as_str().is_none_or(|s|s.strip_prefix("mysql.").is_none_or(|id|!uuid_ok(id)))))
            || tables.iter().map(|t|t["run_id"].as_str().filter(|id|ids.contains(id)).ok_or(StoreError::Input)).collect::<Result<BTreeSet<_>,_>>()?.len()!=ids.len()
        {return Err(StoreError::Input);}
    }
    if codec == "oracle-capture-sequence/v1" {
        validate_oracle_c1(receipt)?;
    }
    let mut names = BTreeSet::new();
    for table in tables {
        let schema = table["schema"].as_str().ok_or(StoreError::Input)?;
        let name = table["table"].as_str().ok_or(StoreError::Input)?;
        if !names.insert(format!("{schema}.{name}"))
            || table["physical_name"]
                .as_str()
                .is_none_or(|item| !item.starts_with("pt_") || item.len() != 63)
            || table["accepted_rows"]
                .as_i64()
                .is_none_or(|value| value < 0)
            || table["canonical_digest"]
                .as_str()
                .is_none_or(|value| !hex64(value))
            || (if per_table_collection(receipt) {
                table["source_generation"].as_i64().is_none_or(|n|n<1 || n>receipt["source_generation"].as_i64().unwrap())
                || table["target_generation"].as_i64().is_none_or(|n|n<1 || n>receipt["target_generation"].as_i64().unwrap())
                || !matches!((table["capture_method"].as_str(),table["delete_detection"].as_str()),(Some("c4_partition"),Some("partition_replace")) | (Some("c5_diff"),Some("reconcile")))
                || !["S0","B"].contains(&table["consistency_grade"].as_str().unwrap_or(""))
                || (receipt["consistency_grade"]=="S0" && table["consistency_grade"]!="S0")
            } else {table["source_generation"].as_i64() != receipt["source_generation"].as_i64()
                || table["target_generation"].as_i64() != receipt["target_generation"].as_i64()})
            || integer_text(&table["applied_pos"]).is_none()
        {
            return Err(StoreError::Input);
        }
    }
    Ok(())
}

/// Lookup names are separate from the original hash-bound source identity.
pub(crate) fn storage_names(table: &Value) -> Result<(&str, &str), StoreError> {
    let nonempty = |v: &Value| v.as_str().is_some_and(|s| !s.is_empty());
    if !nonempty(&table["schema"]) || !nonempty(&table["table"]) {
        return Err(StoreError::Input);
    }
    match (table.get("storage_schema"), table.get("storage_table")) {
        (None, None) => Ok((table["schema"].as_str().unwrap(), table["table"].as_str().unwrap())),
        (Some(s), Some(t)) if nonempty(s) && nonempty(t)
            && storage_alias(table["schema"].as_str().unwrap(), s.as_str().unwrap())
            && storage_alias(table["table"].as_str().unwrap(), t.as_str().unwrap())
            => Ok((s.as_str().unwrap(), t.as_str().unwrap())),
        _ => Err(StoreError::Input),
    }
}

fn storage_alias(original: &str, stored: &str) -> bool {
    let normalized = oo_builder::nfc(original);
    let encoded = format!("q_{}", normalized.bytes().map(|b| format!("{b:02x}")).collect::<String>());
    !original.chars().any(char::is_control) && (stored == normalized || stored == encoded)
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|v| v.len() == keys.len() && keys.iter().all(|k| v.contains_key(*k)))
}

fn oracle_token(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-$#".contains(&b)))
}

fn decimal(value: &Value, nonzero: bool) -> bool {
    value.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 38
        && s.bytes().all(|b| b.is_ascii_digit()) && (s.len() == 1 || !s.starts_with('0'))
        && (!nonzero || s != "0"))
}

fn validate_oracle_c1(receipt: &Value) -> Result<(), StoreError> {
    let buffer = &receipt["oracle_buffer"];
    let identity = &buffer["source_identity"];
    let cursor = &buffer["final_cursor"];
    let ordinal = cursor["transaction_ordinal"].as_str().and_then(|s|
        s.parse::<u64>().ok().filter(|n| n.to_string() == s));
    if !exact_keys(receipt, &["protocol","run_id","run_ids","source_id","source_generation",
        "target_generation","source_epoch","position_codec","position_lineage","applied_pos",
        "consistency","consistency_grade","cross_table_consistent","tables","oracle_buffer"])
        || receipt["consistency"] != "transactional" || receipt["consistency_grade"] != "T"
        || receipt["cross_table_consistent"] != true
        || !exact_keys(buffer, &["checkpoint_kind","source_offset_write","source_identity",
            "final_cursor","commit_scn","context_sha256","bundle_sha256","spool_sha256",
            "typed_schema_sha256","connector_offsets_sha256","connector_history_sha256"])
        || buffer["checkpoint_kind"] != "buffer_consumption" || buffer["source_offset_write"] != false
        || !exact_keys(identity, &["engine","dbid","resetlogs_change","pdb_guid","pdb_name","db_unique_name"])
        || identity["engine"] != "oracle" || !decimal(&identity["dbid"], true)
        || !decimal(&identity["resetlogs_change"], false)
        || !oracle_token(&identity["pdb_name"]) || !oracle_token(&identity["db_unique_name"])
        || identity["pdb_guid"].as_str().is_none_or(|s| s.len()!=32
            || !s.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)))
        || ["context_sha256","bundle_sha256","spool_sha256","typed_schema_sha256",
            "connector_offsets_sha256","connector_history_sha256"].iter()
            .any(|k| buffer[*k].as_str().is_none_or(|s| !hex64(s)))
        || !exact_keys(cursor, &["transaction_ordinal","transaction_id"])
        || ordinal.is_none()
    { return Err(StoreError::Input); }
    let ordinal = ordinal.unwrap();
    let lineage = format!("oracle.{}.{}.{}", identity["dbid"].as_str().unwrap(),
        identity["resetlogs_change"].as_str().unwrap(), identity["pdb_guid"].as_str().unwrap().to_lowercase());
    if receipt["position_lineage"] != lineage || receipt["applied_pos"] != ordinal.to_string()
        || (ordinal == 0 && (!cursor["transaction_id"].is_null() || !buffer["commit_scn"].is_null()))
        || (ordinal > 0 && (!oracle_token(&cursor["transaction_id"]) || !decimal(&buffer["commit_scn"], true)))
    { return Err(StoreError::Input); }
    let mut identities = BTreeSet::new();
    for table in receipt["tables"].as_array().ok_or(StoreError::Input)? {
        if !exact_keys(table, &["schema","table","run_id","physical_name","accepted_rows",
            "canonical_digest","source_generation","target_generation","applied_pos","rejects",
            "storage_schema","storage_table"])
            || table["applied_pos"] != receipt["applied_pos"] || !table["rejects"].is_array()
            || !identities.insert(storage_names(table)?)
        { return Err(StoreError::Input); }
    }
    Ok(())
}

pub fn per_table_collection(receipt: &Value) -> bool {
    receipt["collection_mode"].as_str()==Some("per_table")
}

pub fn publication_sql(receipt: &Value) -> &'static str {
    if crate::native_library::is_library(receipt) {
        "SELECT _oo.publish_native_library(($3::text::jsonb)->'library', $2, ($3::text::jsonb)->'shards')::text WHERE $1::text IS NOT NULL"
    } else if per_table_collection(receipt) {
        "SELECT _oo.publish_native_per_table_collection($1::text::uuid, $2, $3::text::jsonb)::text"
    } else if receipt.get("run_ids").is_some() {
        "SELECT _oo.publish_native_c1_collection($1::text::uuid, $2, $3::text::jsonb)::text"
    } else {
        "SELECT _oo.publish_native_full_import($1::text::uuid, $2, $3::text::jsonb)::text"
    }
}

fn integer_text(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) if number.as_i64().is_some_and(|n| n >= 0) => {
            Some(number.to_string())
        }
        Value::String(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            Some(text.clone())
        }
        _ => None,
    }
}

fn uuid_ok(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, byte)| match i {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::{from_context, publication_sql, validate_context};
    use serde_json::json;

    #[test]
    fn oracle_cursor_has_no_million_cycle_ceiling_but_stays_canonical_and_bound() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/oracle_c1_library.json")).unwrap();
        let mut receipt = original["shards"][0]["receipt"].clone();
        for ordinal in ["1000001", "18446744073709551615"] {
            receipt["oracle_buffer"]["final_cursor"]["transaction_ordinal"] = json!(ordinal);
            receipt["oracle_buffer"]["final_cursor"]["transaction_id"] = json!("TX1");
            receipt["oracle_buffer"]["commit_scn"] = json!("42");
            receipt["applied_pos"] = json!(ordinal);
            for table in receipt["tables"].as_array_mut().unwrap() {
                table["applied_pos"] = json!(ordinal);
            }
            assert!(super::validate_oracle_c1(&receipt).is_ok());
        }
        for wrong in ["01000001", "-1", "1.5", "18446744073709551616"] {
            let mut bad = receipt.clone();
            bad["oracle_buffer"]["final_cursor"]["transaction_ordinal"] = json!(wrong);
            assert!(super::validate_oracle_c1(&bad).is_err());
        }
        receipt["tables"][0]["applied_pos"] = json!("1000000");
        assert!(super::validate_oracle_c1(&receipt).is_err());
    }

    fn wide_library_context(count: usize) -> serde_json::Value {
        let mut receipt: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/oracle_c1_library.json")).unwrap();
        let template = receipt["shards"][0]["receipt"]["tables"][0].clone();
        let tables: Vec<_> = (1..=count).map(|index| {
            let mut table = template.clone();
            let name = format!("T{index:04}");
            table["table"] = json!(name);
            table["storage_table"] = json!(format!("q_{}",
                name.bytes().map(|b| format!("{b:02x}")).collect::<String>()));
            table["run_id"] = json!(format!("00000000-0000-4000-8000-{index:012x}"));
            table["physical_name"] = json!(format!("pt_{index:060x}"));
            table
        }).collect();
        receipt["library"]["manifest"]["sources"][0]["tables"] =
            json!(tables.iter().map(|t| t["storage_table"].clone()).collect::<Vec<_>>());
        receipt["library"]["expected_tables"] = json!(count);
        receipt["library"]["manifest_sha256"] =
            json!(crate::canon::canonical_sha(&receipt["library"]["manifest"]));
        receipt["shards"][0]["receipt"]["run_ids"] =
            json!(tables.iter().map(|t| t["run_id"].clone()).collect::<Vec<_>>());
        receipt["shards"][0]["receipt"]["tables"] = json!(tables);
        let entries: Vec<_> = tables.iter().map(|t| json!({
            "dataset":format!("{}.{}", t["storage_schema"].as_str().unwrap(),
                t["storage_table"].as_str().unwrap()),
            "source":"oracle_fixture", "config_digest":"a".repeat(64), "generation":1
        })).collect();
        let mut context = json!({"datasets":entries,"publication":{
            "protocol":crate::native_library::PROTOCOL,
            "run_id":receipt["shards"][0]["anchor_run"], "receipt":receipt}});
        rehash_library(&mut context);
        context
    }

    fn rehash_library(context: &mut serde_json::Value) {
        let receipt = &mut context["publication"]["receipt"];
        receipt["shards"][0]["receipt_sha256"] =
            json!(crate::canon::canonical_sha(&receipt["shards"][0]["receipt"]));
        context["publication"]["receipt_sha256"] =
            json!(crate::canon::canonical_sha(&context["publication"]["receipt"]));
    }

    #[test]
    fn complete_library_shard_has_no_legacy_total_table_ceiling() {
        for count in [33, 257] {
            let context = wide_library_context(count);
            assert!(crate::target::validate_native_context(&context).is_ok(), "{count}");
            let mut legacy = context.clone();
            legacy["publication"]["protocol"] = json!(super::PROTOCOL);
            legacy["publication"]["receipt"] = context["publication"]["receipt"]["shards"][0]["receipt"].clone();
            legacy["publication"]["receipt_sha256"] =
                json!(crate::canon::canonical_sha(&legacy["publication"]["receipt"]));
            assert!(from_context(&legacy).is_err(), "legacy contract remains bounded");
        }
        let original = wide_library_context(33);
        for fault in 0..5 {
            let mut bad = original.clone();
            let receipt = &mut bad["publication"]["receipt"]["shards"][0]["receipt"];
            match fault {
                0 => { receipt["run_ids"].as_array_mut().unwrap().pop(); },
                1 => receipt["tables"][32] = receipt["tables"][0].clone(),
                2 => receipt["tables"][32]["target_generation"] = json!(2),
                3 => receipt["tables"][32]["canonical_digest"] = json!("not_a_digest"),
                _ => { bad["datasets"].as_array_mut().unwrap().pop(); },
            }
            rehash_library(&mut bad);
            assert!(crate::target::validate_native_context(&bad).is_err(), "{fault}");
        }
    }

    #[test]
    fn c1_collection_requires_closed_complete_run_vector() {
        let mut value=context("36");
        let publication=&mut value["publication"];
        let run=publication["run_id"].clone();
        let receipt=&mut publication["receipt"];
        receipt["run_ids"]=json!([run.clone()]);receipt["tables"][0]["run_id"]=run;
        receipt["position_codec"]=json!("mysql-gtid/v1");receipt["position_lineage"]=json!("mysql.11111111-2222-4333-8444-555555555555");
        receipt["source_epoch"]=json!(1);receipt["consistency"]=json!("transactional");receipt["consistency_grade"]=json!("T");receipt["cross_table_consistent"]=json!(true);
        publication["receipt_sha256"]=json!(crate::canon::canonical_sha(&publication["receipt"]));
        assert!(from_context(&value).is_ok());
        for (key,bad) in [("run_ids",json!([])),("source_epoch",json!(0)),("position_codec",json!("snapshot-zero/v1")),("consistency_grade",json!("S1"))] {
            let mut wrong=value.clone();wrong["publication"]["receipt"][key]=bad;
            wrong["publication"]["receipt_sha256"]=json!(crate::canon::canonical_sha(&wrong["publication"]["receipt"]));
            assert!(from_context(&wrong).is_err(),"{key}");
        }
    }

    fn context(position: &str) -> serde_json::Value {
        let mut value = json!({
            "datasets": [{"dataset":"ledger.ledger_rows","source":"oo_dl01w1_44","config_digest":"006876333fa1dcbe780111c763867398c585a53e6a56330b3485a83e7bb3bf5f","generation":21}],
            "publication": {
                "protocol":"oo.pipeline.native-publication-receipt/v1",
                "run_id":"f60ec34c-ad57-4a90-a631-2b2ef1bf7c8d",
                "receipt_sha256":"b8ddf98d33d621d85370a60868f11d642be040c3d51289a0f095138a50c4020d",
                "receipt": {
                    "protocol":"oo.pipeline.native-publication-receipt/v1",
                    "run_id":"f60ec34c-ad57-4a90-a631-2b2ef1bf7c8d",
                    "source_id":"oo_dl01w1_44","source_generation":21,"target_generation":21,
                    "position_codec":"snapshot-zero/v1","applied_pos":position,
                    "consistency":"per_table","consistency_grade":"B","cross_table_consistent":false,
                    "tables":[{"schema":"ledger","table":"ledger_rows","physical_name":"pt_0066bff803a11ab0d712a544c0d4ca18783ce3d0d0474ed961f13b2f8ced","accepted_rows":5,"canonical_digest":"ad6a8cf12d22b9caea360a9f6988fef7de4309f3d28bc1312374871aee08d3b1","source_generation":21,"target_generation":21,"applied_pos":"0"}]
                }
            }
        });
        let digest = crate::canon::canonical_sha(&value["publication"]["receipt"]);
        value["publication"]["receipt_sha256"] = json!(digest);
        value
    }

    #[test]
    fn snapshot_zero_is_valid_and_bound_to_exact_dataset_vector() {
        assert!(
            validate_context(&context("0")).is_ok(),
            "{:?}",
            validate_context(&context("0"))
        );
        assert!(from_context(&context("0")).unwrap().is_some());
    }

    #[test]
    fn snapshot_zero_cannot_claim_nonzero_position_or_wrong_dataset() {
        assert!(validate_context(&context("1")).is_err());
        let mut wrong = context("0");
        wrong["datasets"][0]["generation"] = json!(20);
        assert!(validate_context(&wrong).is_err());
    }

    #[test]
    fn doris_per_table_vector_keeps_real_s0_generations_and_dispatches_separately() {
        let value = per_table_context();
        let publication = from_context(&value).unwrap().unwrap();
        assert!(validate_context(&value).is_ok());
        assert_eq!(publication_sql(&publication.receipt),
            "SELECT _oo.publish_native_per_table_collection($1::text::uuid, $2, $3::text::jsonb)::text");

        for (path, bad) in [
            ("/publication/receipt/consistency_grade", json!("T")),
            ("/publication/receipt/cross_table_consistent", json!(true)),
            ("/publication/receipt/position_codec", json!("mysql-gtid/v1")),
            ("/publication/receipt/tables/1/delete_detection", json!("reconcile")),
            ("/publication/receipt/tables/0/source_generation", json!(12)),
        ] {
            let mut wrong = value.clone();
            *wrong.pointer_mut(path).unwrap() = bad;
            wrong["publication"]["receipt_sha256"] = json!(
                crate::canon::canonical_sha(&wrong["publication"]["receipt"]));
            assert!(from_context(&wrong).is_err(), "{path}");
        }
        let mut incomplete = value;
        incomplete["publication"]["receipt"]["run_ids"] = json!(["11111111-2222-4333-8444-555555555555"]);
        incomplete["publication"]["receipt_sha256"] = json!(
            crate::canon::canonical_sha(&incomplete["publication"]["receipt"]));
        assert!(from_context(&incomplete).is_err(), "run_ids must cover every table");
    }

    fn per_table_context() -> serde_json::Value {
        let baseline = "11111111-2222-4333-8444-555555555555";
        let anchor = "66666666-7777-4888-8999-aaaaaaaaaaaa";
        let physical = format!("pt_{}", "a".repeat(60));
        let receipt = json!({
            "protocol":"oo.pipeline.native-publication-receipt/v1",
            "collection_mode":"per_table",
            "run_id":anchor,
            "run_ids":[baseline,anchor],
            "source_id":"dorispreview",
            "source_generation":11,
            "target_generation":19,
            "position_codec":"partition-signature/v1",
            "position_lineage":"doris.partition.part_day",
            "applied_pos":"0",
            "consistency":"per_table",
            "consistency_grade":"S0",
            "cross_table_consistent":false,
            "tables":[
                {"schema":"oo_dl01w1_43","table":"dup_events","run_id":baseline,
                 "physical_name":physical,"accepted_rows":2,"canonical_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                 "source_generation":2,"target_generation":4,"applied_pos":"0",
                 "capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":"S0"},
                {"schema":"oo_dl01w1_43","table":"part_day","run_id":anchor,
                 "physical_name":physical,"accepted_rows":3,"canonical_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                 "source_generation":11,"target_generation":19,"applied_pos":"0",
                 "capture_method":"c4_partition","delete_detection":"partition_replace","consistency_grade":"S0"}
            ]
        });
        let digest = crate::canon::canonical_sha(&receipt);
        json!({
            "datasets":[
                {"dataset":"oo_dl01w1_43.dup_events","source":"dorispreview","generation":2},
                {"dataset":"oo_dl01w1_43.part_day","source":"dorispreview","generation":11}
            ],
            "publication":{"protocol":"oo.pipeline.native-publication-receipt/v1","run_id":anchor,
                "receipt_sha256":digest,"receipt":receipt}
        })
    }
}
