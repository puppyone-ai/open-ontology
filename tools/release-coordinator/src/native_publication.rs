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
                    != publication.receipt["source_generation"]
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
        if ids.len()!=runs.len() || !ids.contains(run_id) || codec!="mysql-gtid/v1"
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
            || table["source_generation"].as_i64() != receipt["source_generation"].as_i64()
            || table["target_generation"].as_i64() != receipt["target_generation"].as_i64()
            || integer_text(&table["applied_pos"]).is_none()
        {
            return Err(StoreError::Input);
        }
    }
    Ok(())
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
    use super::{from_context, validate_context};
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
}
