use crate::error::StoreError;
use serde_json::Value;
use std::collections::BTreeSet;

pub const PROTOCOL: &str = "oo.pipeline.native-library-publication/v1";

/// Data-only advances the immutable vector, never its source identity or membership.
pub fn data_only_successor(previous: &Value, current: &Value) -> Result<(), StoreError> {
    for mapping in [previous, current] {
        crate::target::validate_native_context(mapping)?;
    }
    let old = crate::native_publication::from_context(previous)?.ok_or(StoreError::Input)?;
    let new = crate::native_publication::from_context(current)?.ok_or(StoreError::Input)?;
    if !is_library(&old.receipt) || !is_library(&new.receipt)
        || old.receipt["library"] != new.receipt["library"]
        || old.receipt_sha256 == new.receipt_sha256
    { return Err(StoreError::Input); }
    let dataset_keys = |mapping: &Value| -> Result<BTreeSet<String>, StoreError> {
        mapping["datasets"].as_array().ok_or(StoreError::Input)?.iter().map(|v| {
            let mut v = v.clone();
            v.as_object_mut().ok_or(StoreError::Input)?.remove("generation");
            Ok(crate::canon::canonical(&v))
        }).collect()
    };
    if dataset_keys(previous)? != dataset_keys(current)? { return Err(StoreError::Input); }
    for before in old.receipt["shards"].as_array().ok_or(StoreError::Input)? {
        let after = new.receipt["shards"].as_array().ok_or(StoreError::Input)?.iter()
            .find(|v| v["source_id"] == before["source_id"]).ok_or(StoreError::Input)?;
        let (a, b) = (&before["receipt"], &after["receipt"]);
        let oracle = a["position_codec"] == "oracle-capture-sequence/v1";
        if !oracle && a["position_codec"] != "mysql-gtid/v1"
            || b["position_codec"] != a["position_codec"]
            || a["source_epoch"] != b["source_epoch"]
            || a["position_lineage"] != b["position_lineage"]
            || oracle && (a["oracle_buffer"]["source_identity"] != b["oracle_buffer"]["source_identity"]
                || a["oracle_buffer"]["typed_schema_sha256"] != b["oracle_buffer"]["typed_schema_sha256"])
            || a["run_id"] == b["run_id"]
            || ["source_generation", "target_generation"].iter().any(|key|
                b[*key].as_i64().unwrap_or(0) <= a[*key].as_i64().unwrap_or(0))
        { return Err(StoreError::Input); }
        let position = |v: &Value| v["applied_pos"].as_str()
            .and_then(|s| s.parse::<u64>().ok()).ok_or(StoreError::Input);
        if position(b)? <= position(a)? { return Err(StoreError::Input); }
        let names = |v: &Value| -> Result<BTreeSet<String>, StoreError> {
            v["tables"].as_array().ok_or(StoreError::Input)?.iter().map(|t| {
                let (schema, table) = crate::native_publication::storage_names(t)?;
                if t["rejects"].as_array().is_none_or(|r| !r.is_empty()) {
                    return Err(StoreError::Input);
                }
                Ok(crate::canon::canonical(&serde_json::json!([
                    t["schema"], t["table"], schema, table
                ])))
            }).collect()
        };
        if names(a)? != names(b)? { return Err(StoreError::Input); }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/observations/data_only.rs"]
mod data_only_tests;

pub fn is_library(receipt: &Value) -> bool {
    receipt["library"]["protocol"].as_str() == Some(PROTOCOL)
}

pub fn tables(receipt: &Value) -> Result<Vec<Value>, StoreError> {
    if !is_library(receipt) {
        return receipt["tables"]
            .as_array()
            .cloned()
            .ok_or(StoreError::Input);
    }
    let mut tables = Vec::new();
    for shard in receipt["shards"].as_array().ok_or(StoreError::Input)? {
        for entry in shard["receipt"]["tables"]
            .as_array()
            .ok_or(StoreError::Input)?
        {
            let mut table = entry.clone();
            table["source_id"] = shard["source_id"].clone();
            tables.push(table);
        }
    }
    Ok(tables)
}

pub fn validate(receipt: &Value, anchor: &str) -> Result<(), StoreError> {
    let object = receipt.as_object().ok_or(StoreError::Input)?;
    let library = &receipt["library"];
    let metadata = library.as_object().ok_or(StoreError::Input)?;
    if object.len() != 2
        || !object.contains_key("shards")
        || metadata.len() != 6
        || metadata.keys().any(|key| {
            ![
                "protocol",
                "library_id",
                "manifest",
                "manifest_sha256",
                "expected_tables",
                "shards",
            ]
            .contains(&key.as_str())
        })
        || !is_library(receipt)
        || library["library_id"]
            .as_str()
            .is_none_or(|id| id.is_empty() || id.len() > 128)
        || library["manifest"]["library_id"] != library["library_id"]
        || library["manifest_sha256"].as_str()
            != Some(crate::canon::canonical_sha(&library["manifest"]).as_str())
    {
        return Err(StoreError::Input);
    }
    let manifest = library["manifest"].as_object().ok_or(StoreError::Input)?;
    if manifest.len() != 3 || library["manifest"]["protocol"] != "oo.library.membership/v1" {
        return Err(StoreError::Input);
    }
    let shards = receipt["shards"]
        .as_array()
        .filter(|s| !s.is_empty())
        .ok_or(StoreError::Input)?;
    let declared = library["shards"].as_array().ok_or(StoreError::Input)?;
    let members = library["manifest"]["sources"]
        .as_array()
        .ok_or(StoreError::Input)?;
    if shards.len() != declared.len() || shards.len() != members.len() {
        return Err(StoreError::Input);
    }
    let mut sources = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut runs = BTreeSet::new();
    for shard in shards {
        let fields = shard.as_object().ok_or(StoreError::Input)?;
        let source = shard["source_id"].as_str().ok_or(StoreError::Input)?;
        let run = shard["anchor_run"].as_str().ok_or(StoreError::Input)?;
        let vector = &shard["receipt"];
        if fields.len() != 4
            || fields.keys().any(|key| {
                !["source_id", "anchor_run", "receipt", "receipt_sha256"].contains(&key.as_str())
            })
            || !sources.insert(source)
            || declared
                .iter()
                .filter(|id| id.as_str() == Some(source))
                .count()
                != 1
            || shard["receipt_sha256"].as_str()
                != Some(crate::canon::canonical_sha(vector).as_str())
            || vector["source_id"] != source
            || !(crate::native_publication::per_table_collection(vector)
                || vector["consistency"] == "transactional")
        {
            return Err(StoreError::Input);
        }
        let receipt_keys = [
            "applied_pos",
            "collection_mode",
            "consistency",
            "consistency_grade",
            "cross_table_consistent",
            "position_codec",
            "position_lineage",
            "protocol",
            "run_id",
            "run_ids",
            "source_generation",
            "source_id",
            "tables",
            "target_generation",
        ];
        let vector_object = vector.as_object().ok_or(StoreError::Input)?;
        if crate::native_publication::per_table_collection(vector) && (vector_object.len() != receipt_keys.len()
            || vector_object
                .keys()
                .any(|k| !receipt_keys.contains(&k.as_str())))
        {
            return Err(StoreError::Input);
        }
        if !crate::native_publication::per_table_collection(vector) {
            let keys = ["applied_pos","consistency","consistency_grade","cross_table_consistent",
                "position_codec","position_lineage","protocol","run_id","run_ids","source_epoch",
                "source_generation","source_id","tables","target_generation"];
            let oracle = vector["position_codec"] == "oracle-capture-sequence/v1";
            if vector_object.len() != keys.len() + usize::from(oracle)
                || keys.iter().any(|k| !vector_object.contains_key(*k))
                || vector_object.keys().any(|k| !keys.contains(&k.as_str()) && !(oracle && k == "oracle_buffer"))
            { return Err(StoreError::Input); }
        }
        crate::native_publication::validate_receipt(vector, run)?;
        let member = members
            .iter()
            .filter(|m| m["source_id"] == source)
            .collect::<Vec<_>>();
        if member.len() != 1 {
            return Err(StoreError::Input);
        }
        if member[0].as_object().is_none_or(|m| {
            m.len() != 3
                || m.keys()
                    .any(|k| !["source_id", "schema", "tables"].contains(&k.as_str()))
        }) {
            return Err(StoreError::Input);
        }
        let schema = member[0]["schema"].as_str().ok_or(StoreError::Input)?;
        let expected = member[0]["tables"].as_array().ok_or(StoreError::Input)?;
        let vector_tables = vector["tables"].as_array().ok_or(StoreError::Input)?;
        if expected.len() != vector_tables.len() {
            return Err(StoreError::Input);
        }
        for table in vector_tables {
            let fields = table.as_object().ok_or(StoreError::Input)?;
            let required = [
                "schema",
                "table",
                "run_id",
                "physical_name",
                "accepted_rows",
                "canonical_digest",
                "source_generation",
                "target_generation",
                "applied_pos",
                "capture_method",
                "delete_detection",
                "consistency_grade",
            ];
            if crate::native_publication::per_table_collection(vector) && (required.iter().any(|k| !fields.contains_key(*k))
                || fields.keys().any(|k| {
                    !required.contains(&k.as_str())
                        && !["rejects", "position_lineage"].contains(&k.as_str())
                }))
            {
                return Err(StoreError::Input);
            }
            if !crate::native_publication::per_table_collection(vector) {
                let keys = ["schema","table","run_id","physical_name","accepted_rows",
                    "canonical_digest","source_generation","target_generation","applied_pos","rejects"];
                if keys.iter().any(|k| !fields.contains_key(*k))
                    || fields.keys().any(|k| !keys.contains(&k.as_str())
                        && !["storage_schema","storage_table"].contains(&k.as_str()))
                    || !table["rejects"].is_array() || table["applied_pos"] != vector["applied_pos"]
                { return Err(StoreError::Input); }
            }
            let (storage_schema, name) = crate::native_publication::storage_names(table)?;
            let table_run = table["run_id"].as_str().ok_or(StoreError::Input)?;
            if storage_schema != schema
                || expected.iter().filter(|t| t.as_str() == Some(name)).count() != 1
                || !identities.insert((schema, name))
                || !runs.insert(table_run)
            {
                return Err(StoreError::Input);
            }
        }
    }
    if !runs.contains(anchor)
        || library["expected_tables"].as_u64() != Some(identities.len() as u64)
    {
        return Err(StoreError::Input);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn oracle_c1_library_retains_original_receipt_and_checks_storage_and_cursor() {
        let receipt: Value = serde_json::from_str(include_str!("../tests/fixtures/oracle_c1_library.json")).unwrap();
        let original = receipt.clone();
        let anchor = receipt["shards"][0]["anchor_run"].as_str().unwrap();
        validate(&receipt, anchor).unwrap();
        let context = json!({"datasets":[{"dataset":"q_415050.q_4954454d53","source":"oracle_fixture",
            "config_digest":"a".repeat(64),"generation":1}],
            "publication":{"protocol":PROTOCOL,"run_id":anchor,
                "receipt_sha256":crate::canon::canonical_sha(&receipt),"receipt":receipt}});
        crate::native_publication::validate_context(&context).unwrap();
        assert_eq!(receipt, original);
        assert_eq!(receipt["shards"][0]["receipt"]["consistency_grade"], "T");
        assert_eq!(receipt["shards"][0]["receipt"]["tables"][0]["schema"], "APP");
        let mut mysql=original.clone();
        mysql["shards"][0]["receipt"].as_object_mut().unwrap().remove("oracle_buffer");
        mysql["shards"][0]["receipt"]["position_codec"]=json!("mysql-gtid/v1");
        mysql["shards"][0]["receipt"]["position_lineage"]=json!("mysql.11111111-2222-4333-8444-555555555555");
        mysql["shards"][0]["receipt_sha256"]=json!(crate::canon::canonical_sha(&mysql["shards"][0]["receipt"]));
        validate(&mysql,anchor).unwrap();
        mysql["shards"][0]["receipt"]["oracle_buffer"]=Value::Null;
        mysql["shards"][0]["receipt_sha256"]=json!(crate::canon::canonical_sha(&mysql["shards"][0]["receipt"]));
        assert!(validate(&mysql,anchor).is_err());
        for (path, value) in [
            ("/consistency_grade", json!("S0")),
            ("/target_generation", json!(2)),
            ("/oracle_buffer/source_offset_write", json!(true)),
            ("/oracle_buffer/final_cursor/transaction_ordinal", json!("2")),
            ("/oracle_buffer/commit_scn", json!("0")),
            ("/oracle_buffer/source_identity/dbid", json!("9")),
            ("/tables/0/schema", json!("FOREIGN")),
            ("/tables/0/storage_schema", json!("foreign")),
            ("/run_ids", json!([])),
        ] {
            let mut bad=original.clone();
            *bad["shards"][0]["receipt"].pointer_mut(path).unwrap()=value;
            bad["shards"][0]["receipt_sha256"]=json!(crate::canon::canonical_sha(&bad["shards"][0]["receipt"]));
            assert!(validate(&bad, anchor).is_err(), "{path}");
        }
        let mut bad=original;
        bad["shards"][0]["receipt"]["unknown"]=json!(true);
        bad["shards"][0]["receipt_sha256"]=json!(crate::canon::canonical_sha(&bad["shards"][0]["receipt"]));
        assert!(validate(&bad, anchor).is_err());
    }

    fn fixture() -> Value {
        let mut shards = Vec::new();
        let mut members = Vec::new();
        for index in 0..3 {
            let source = format!("shard_{index}");
            let mut tables = Vec::new();
            for i in 0..12 {
                tables.push(json!({"schema":"sample","table":format!("t_{index}_{i}"),
                    "run_id":format!("00000000-0000-4000-8000-{:012}",index*12+i+1),
                    "source_generation":i+1,"target_generation":i+1,"physical_name":format!("pt_{:060x}",index*12+i+1),
                    "accepted_rows":1,"canonical_digest":"a".repeat(64),"applied_pos":"0",
                    "capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":"B"}));
            }
            let receipt = json!({"protocol":crate::native_publication::PROTOCOL,"collection_mode":"per_table",
                "run_id":tables[11]["run_id"],"run_ids":tables.iter().map(|t|t["run_id"].clone()).collect::<Vec<_>>(),
                "source_id":source,"source_generation":12,"target_generation":12,"position_codec":"snapshot-zero/v1",
                "position_lineage":"fixture","applied_pos":"0","consistency":"per_table","consistency_grade":"B",
                "cross_table_consistent":false,"tables":tables});
            members.push(json!({"source_id":source,"schema":"sample","tables":tables.iter().map(|t|t["table"].clone()).collect::<Vec<_>>()}));
            shards.push(json!({"source_id":source,"anchor_run":receipt["run_id"],"receipt_sha256":crate::canon::canonical_sha(&receipt),"receipt":receipt}));
        }
        let manifest = json!({"protocol":"oo.library.membership/v1","library_id":"sample-library","sources":members});
        json!({"library":{"protocol":PROTOCOL,"library_id":"sample-library","manifest_sha256":crate::canon::canonical_sha(&manifest),"manifest":manifest,"expected_tables":36,"shards":["shard_0","shard_1","shard_2"]},"shards":shards})
    }

    #[test]
    fn validates_cross_source_vector_and_rejects_closed_protocol_drift() {
        let receipt = fixture();
        let anchor = receipt["shards"][0]["anchor_run"].as_str().unwrap();
        validate(&receipt, anchor).unwrap();
        assert_eq!(tables(&receipt).unwrap().len(), 36);
        for path in [
            "/library/expected_tables",
            "/library/manifest_sha256",
            "/shards/2/source_id",
            "/shards/1/receipt/tables/0/source_generation",
        ] {
            let mut bad = receipt.clone();
            *bad.pointer_mut(path).unwrap() = json!("invalid");
            assert!(validate(&bad, anchor).is_err(), "{path}");
        }
        let mut bad = receipt.clone();
        bad["library"]["unexpected"] = json!(true);
        assert!(validate(&bad, anchor).is_err());
        let context = json!({"publication":{"protocol":PROTOCOL,"run_id":anchor,"receipt_sha256":crate::canon::canonical_sha(&receipt),"receipt":receipt}});
        let publication = crate::native_publication::from_context(&context)
            .unwrap()
            .unwrap();
        assert!(
            crate::native_publication::publication_sql(&publication.receipt)
                .contains("publish_native_library")
        );
    }
}

#[cfg(test)]
mod fixture_exports {
    #[test]
    fn export_anonymous_effective_shape() {
        let (effective, visibility) = crate::live::shop_tables(&["contact", "id"], &["t_0_00"]);
        std::fs::write(
            "/tmp/oo-wf-lib-effective-shape.json",
            serde_json::to_vec_pretty(
                &serde_json::json!({"effective":effective,"visibility":visibility}),
            )
            .unwrap(),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod visibility_budget_tests {
    use serde_json::json;
    #[test]
    fn explicit_library_visibility_ceiling_is_checked_without_weakening_default() {
        let mut visibility = json!({"role":"oo_r_library","datasets":(0..300).map(|i|json!({"dataset":format!("sample.t_{i}"),"columns":["id"],"row_security":"none"})).collect::<Vec<_>>(),"limits":{"max_datasets":4096}});
        assert_eq!(
            crate::apply::visibility_bindings(&visibility)
                .unwrap()
                .len(),
            300
        );
        visibility["limits"]["max_datasets"] = json!(256);
        assert!(crate::apply::visibility_bindings(&visibility).is_err());
        visibility["limits"]
            .as_object_mut()
            .unwrap()
            .remove("max_datasets");
        assert!(crate::apply::visibility_bindings(&visibility).is_err());
        visibility["limits"]["max_datasets"] = json!(true);
        assert!(crate::apply::visibility_bindings(&visibility).is_err());
    }
}
