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
    if object.len() != 4 || object.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
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
    validate_receipt(&receipt, run_id)?;
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
        let receipt_tables = publication.receipt["tables"]
            .as_array()
            .ok_or(StoreError::Input)?;
        if entries.len() != receipt_tables.len() {
            return Err(StoreError::Input);
        }
        let mut datasets = BTreeSet::new();
        for (entry, table) in entries.iter().zip(receipt_tables) {
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
                    != publication.receipt["source_id"]
                        .as_str()
                        .ok_or(StoreError::Input)?
                || generation
                    != (if per_table_collection(&publication.receipt) { &table["source_generation"] } else { &publication.receipt["source_generation"] })
                        .as_i64()
                        .ok_or(StoreError::Input)?
                || schema != table["schema"].as_str().ok_or(StoreError::Input)?
                || name != table["table"].as_str().ok_or(StoreError::Input)?
            {
                return Err(StoreError::Input);
            }
        }
    }
    Ok(())
}

fn validate_receipt(receipt: &Value, run_id: &str) -> Result<(), StoreError> {
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
        .filter(|items| !items.is_empty() && items.len() <= 256)
        .ok_or(StoreError::Input)?;
    if let Some(runs)=receipt.get("run_ids") {
        let runs=runs.as_array().filter(|r|r.len()==tables.len() && r.len()<=32).ok_or(StoreError::Input)?;
        let ids=runs.iter().map(|v|v.as_str().filter(|s|uuid_ok(s)).ok_or(StoreError::Input)).collect::<Result<BTreeSet<_>,_>>()?;
        if per_table_collection(receipt) {
            if ids.len()!=runs.len() || !ids.contains(run_id)
                || !["snapshot-zero/v1", "partition-signature/v1"].contains(&codec) || position!="0"
                || receipt["consistency"]!="per_table" || receipt["cross_table_consistent"]!=false
                || !["S0", "B"].contains(&receipt["consistency_grade"].as_str().unwrap_or(""))
                || tables.iter().map(|t|t["run_id"].as_str().filter(|id|ids.contains(id)).ok_or(StoreError::Input)).collect::<Result<BTreeSet<_>,_>>()?.len()!=ids.len()
                || !tables.iter().any(|t| t["run_id"]==run_id && t["source_generation"]==receipt["source_generation"] && t["target_generation"]==receipt["target_generation"])
            {return Err(StoreError::Input);}
        } else if ids.len()!=runs.len() || !ids.contains(run_id) || codec!="mysql-gtid/v1"
            || receipt["consistency"]!="transactional" || receipt["consistency_grade"]!="T"
            || receipt["cross_table_consistent"]!=true || receipt["source_epoch"].as_i64().is_none_or(|n|n<1)
            || receipt["position_lineage"].as_str().is_none_or(|s|s.strip_prefix("mysql.").is_none_or(|id|!uuid_ok(id)))
            || tables.iter().map(|t|t["run_id"].as_str().filter(|id|ids.contains(id)).ok_or(StoreError::Input)).collect::<Result<BTreeSet<_>,_>>()?.len()!=ids.len()
        {return Err(StoreError::Input);}
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

pub fn per_table_collection(receipt: &Value) -> bool {
    receipt["collection_mode"].as_str()==Some("per_table")
}

pub fn publication_sql(receipt: &Value) -> &'static str {
    if per_table_collection(receipt) {
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
