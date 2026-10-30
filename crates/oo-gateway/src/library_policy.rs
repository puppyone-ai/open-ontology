use std::collections::BTreeSet;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::policy::{ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass};

pub struct LibraryPolicy {
    pub policies: Vec<ColumnAclPolicy>,
    pub vector: Value,
    pub catalog: Value,
    pub project: String,
    pub role: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub config_digest: String,
}

fn sha(value: &Value) -> Result<String, &'static str> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| "library_json")?)
    ))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("library_shape")
}

fn storage_names(table: &Value) -> Result<(&str, &str), &'static str> {
    let original = (text(table, "schema")?, text(table, "table")?);
    match (table.get("storage_schema"), table.get("storage_table")) {
        (None, None) => Ok(original),
        (Some(_), Some(_)) => {
            let stored = (text(table, "storage_schema")?, text(table, "storage_table")?);
            let alias = |source:&str, target:&str| {
                let normalized=oo_builder::nfc(source);
                let encoded=format!("q_{}",normalized.bytes().map(|b|format!("{b:02x}")).collect::<String>());
                !source.chars().any(char::is_control) && (target==normalized || target==encoded)
            };
            if !alias(original.0,stored.0) || !alias(original.1,stored.1) {return Err("library_binding");}
            Ok(stored)
        }
        _ => Err("library_binding"),
    }
}

fn closed_keys(value: &Value, required: &[&str], optional: &[&str]) -> bool {
    value.as_object().is_some_and(|o| required.iter().all(|k| o.contains_key(*k))
        && o.keys().all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str())))
}

fn validate_c1_shard(shard: &Value) -> Result<(), &'static str> {
    let r = &shard["receipt"];
    let codec = text(r, "position_codec")?;
    let oracle = codec == "oracle-capture-sequence/v1";
    if !closed_keys(shard, &["source_id","anchor_run","receipt_sha256","receipt"], &[])
        || !closed_keys(r, &["protocol","run_id","run_ids","source_id","source_generation","target_generation",
            "source_epoch","position_codec","position_lineage","applied_pos","consistency",
            "consistency_grade","cross_table_consistent","tables"], if oracle { &["oracle_buffer"] } else { &[] })
        || !matches!(codec, "oracle-capture-sequence/v1" | "mysql-gtid/v1")
        || r["protocol"] != "oo.pipeline.native-publication-receipt/v1"
        || r["source_id"] != shard["source_id"] || r["run_id"] != shard["anchor_run"]
        || r["consistency"] != "transactional" || r["consistency_grade"] != "T"
        || r["cross_table_consistent"] != true || r["source_epoch"].as_i64().is_none_or(|n| n < 1)
        || ["source_generation","target_generation"].iter().any(|k| r[*k].as_i64().is_none_or(|n| n < 1))
        || shard["receipt_sha256"] != sha(r)?
        || !crate::query_executor::watermark_identity(codec, text(r,"position_lineage")?, text(r,"applied_pos")?)
    { return Err("library_c1_receipt"); }
    if oracle {
        let b = &r["oracle_buffer"];
        let id = &b["source_identity"];
        let cursor = &b["final_cursor"];
        if !closed_keys(b, &["checkpoint_kind","source_offset_write","source_identity","final_cursor",
            "commit_scn","context_sha256","bundle_sha256","spool_sha256","typed_schema_sha256",
            "connector_offsets_sha256","connector_history_sha256"], &[])
            || !closed_keys(id, &["engine","dbid","resetlogs_change","pdb_guid","pdb_name","db_unique_name"], &[])
            || !closed_keys(cursor, &["transaction_ordinal","transaction_id"], &[])
            || b["checkpoint_kind"] != "buffer_consumption" || b["source_offset_write"] != false
            || id["engine"] != "oracle" || cursor["transaction_ordinal"] != r["applied_pos"]
            || r["position_lineage"] != format!("oracle.{}.{}.{}", text(id,"dbid")?,
                text(id,"resetlogs_change")?, text(id,"pdb_guid")?.to_ascii_lowercase())
            || ["context_sha256","bundle_sha256","spool_sha256","typed_schema_sha256",
                "connector_offsets_sha256","connector_history_sha256"].iter()
                .any(|k| b[*k].as_str().is_none_or(|s| s.len()!=64
                    || !s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))))
        { return Err("library_c1_buffer"); }
        let token = |v:&Value| v.as_str().is_some_and(|s| !s.is_empty() && s.len()<=128
            && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-$#".contains(&c)));
        if !token(&id["pdb_name"]) || !token(&id["db_unique_name"])
            || id["pdb_guid"].as_str().is_none_or(|s| s.len()!=32
                || !s.bytes().all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c)))
            || (r["applied_pos"]=="0" && (!cursor["transaction_id"].is_null() || !b["commit_scn"].is_null()))
            || (r["applied_pos"]!="0" && (!token(&cursor["transaction_id"])
                || b["commit_scn"].as_str().is_none_or(|s| s.is_empty() || s.len()>38
                    || s.starts_with('0') || !s.bytes().all(|c| c.is_ascii_digit()))))
        { return Err("library_c1_buffer"); }
    }
    let tables = r["tables"].as_array().ok_or("library_c1_vector")?;
    let runs = r["run_ids"].as_array().ok_or("library_c1_vector")?;
    let ids: BTreeSet<_> = runs.iter().filter_map(Value::as_str).collect();
    let mut names = BTreeSet::new();
    let mut seen_runs = BTreeSet::new();
    if tables.is_empty() || tables.len()!=runs.len() || ids.len()!=runs.len()
        || !ids.contains(text(r,"run_id")?) { return Err("library_c1_vector"); }
    for t in tables {
        if !closed_keys(t, &["schema","table","run_id","physical_name","accepted_rows","canonical_digest",
            "source_generation","target_generation","applied_pos","rejects"], &["storage_schema","storage_table"])
            || !ids.contains(text(t,"run_id")?) || !seen_runs.insert(text(t,"run_id")?)
            || !names.insert(storage_names(t)?) || !t["rejects"].is_array()
            || t["source_generation"]!=r["source_generation"] || t["target_generation"]!=r["target_generation"]
            || t["applied_pos"]!=r["applied_pos"]
            || t["accepted_rows"].as_i64().is_none_or(|n| n<0)
            || text(t,"canonical_digest")?.len()!=64
            || !text(t,"canonical_digest")?.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || text(t,"physical_name")?.len()!=63 || !text(t,"physical_name")?.starts_with("pt_")
            || !text(t,"physical_name")?[3..].bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        { return Err("library_c1_vector"); }
    }
    Ok(())
}

fn local_operator_review(document: &Value) -> Result<bool, &'static str> {
    let envelope = &document["envelope"];
    if envelope["contract"] != "oo.operator.library-pack/v1" {
        return Ok(false);
    }
    let review = &envelope["review"];
    let visibility = &document["visibility"];
    if envelope["operation"] != "prepareLocalBuildPack"
        || envelope["management_identity"] != "operator_local"
        || review["review_origin"] != "operator_pinned_file"
        || review["project"] != document["effective"]["project"]
        || review["role"] != visibility["role"]
        || review["principal"] != visibility["principal"]
        || envelope["producer"]["review_sha256"] != sha(review)?
    {
        return Err("library_operator_review");
    }
    for table in visibility["datasets"].as_array().ok_or("library_operator_review")? {
        let name = text(table, "dataset")?;
        let definition = document["effective"]["datasets"].as_array().ok_or("library_operator_review")?
            .iter().find(|d| d["dataset"] == name).ok_or("library_operator_review")?;
        for column in table["columns"].as_array().ok_or("library_operator_review")? {
            let column = column.as_str().ok_or("library_operator_review")?;
            let identity = format!("{name}.{column}");
            let decision = &review["columns"][&identity];
            let effective = definition["columns"].as_array().ok_or("library_operator_review")?
                .iter().find(|c| c["name"] == column).ok_or("library_operator_review")?;
            let pii = text(effective, "pii")?;
            if decision["pii"] != pii
                || decision["semantic_type"] != effective["semantic_type"]
                || decision["reviewer"] != effective["classification"]["by"]
                || !decision["reviewer"].as_str().is_some_and(|s| !s.trim().is_empty())
                || !matches!((pii, decision.get("explicit_allow")),
                    ("none", None | Some(Value::Bool(false)))
                    | ("personal" | "sensitive" | "restricted", Some(Value::Bool(true))))
                || review["hidden_columns"].as_array().ok_or("library_operator_review")?
                    .iter().any(|c| c == &identity)
            {
                return Err("library_operator_review");
            }
        }
    }
    Ok(true)
}

pub fn compile_library(document: &Value) -> Result<LibraryPolicy, &'static str> {
    let object = document.as_object().ok_or("library_shape")?;
    let keys = [
        "protocol",
        "envelope",
        "effective",
        "visibility",
        "publication",
        "bindings",
        "login",
        "ceilings",
    ];
    if object.len() != keys.len()
        || keys.iter().any(|k| !object.contains_key(*k))
        || document["protocol"] != "oo.gateway.library-reviewed/v1"
    {
        return Err("library_shape");
    }
    let envelope = &document["envelope"];
    let producer = &envelope["producer"];
    let effective = &document["effective"];
    let visibility = &document["visibility"];
    // Both inputs come from the operator-owned reviewed directory, never an HTTP request.
    let reviewed_origin = local_operator_review(document)?
        || (envelope["contract"] == "oo.selfbuild-control-api/v1"
            && matches!(envelope["operation"].as_str(), Some("getBuildPack" | "compileBuildPack"))
            && envelope["management_identity"] == "verified");
    if !reviewed_origin
        || envelope["published"] != false
        || envelope["active"] != false
        || envelope["producer_authenticated"] != false
        || envelope["compiled_pack"] != "not_published"
        || envelope["data_grants"] != "none"
        || producer["protocol"] != "oo.selfbuild.library-pack/v1"
        || producer["published"] != false
        || producer["business_owner_signed"] != false
        || producer["review_sha256"].as_str().is_none_or(|s| {
            s.len() != 64
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        || envelope["output_sha256"] != sha(producer)?
        || producer["effective_sha256"] != sha(effective)?
        || producer["visibility_sha256"] != sha(visibility)?
    {
        return Err("library_review");
    }
    let ceilings: oo_builder::RolePackCeilings =
        serde_json::from_value(document["ceilings"].clone()).map_err(|_| "library_limits")?;
    if ceilings.max_datasets == 0
        || ceilings.max_datasets > 4096
        || ceilings.max_effective_columns > 4096
    {
        return Err("library_limits");
    }
    let pack = oo_builder::compile_role_pack_with_ceilings(
        &effective.to_string(),
        &visibility.to_string(),
        &ceilings,
    )
    .map_err(|_| "library_pack")?;
    let artifact: Value = serde_json::from_str(&pack.artifact).map_err(|_| "library_pack")?;
    if producer["artifact"] != artifact || producer["pack_digest"] != pack.pack_digest {
        return Err("library_pack");
    }
    let publication = &document["publication"];
    let receipt = &publication["receipt"];
    if publication["protocol"] != "oo.pipeline.native-library-publication/v1"
        || receipt["library"]["protocol"] != "oo.pipeline.native-library-publication/v1"
        || publication["receipt_sha256"] != sha(receipt)?
    {
        return Err("library_receipt");
    }
    let library_id = text(&receipt["library"], "library_id")?;
    let shards = receipt["shards"].as_array().ok_or("library_receipt")?;
    let has_c1 = shards.iter().any(|s| s["receipt"]["consistency"]=="transactional"
        || s["receipt"].get("oracle_buffer").is_some());
    if has_c1 {
        let members = receipt["library"]["manifest"]["sources"].as_array().ok_or("library_receipt")?;
        let declared = receipt["library"]["shards"].as_array().ok_or("library_receipt")?;
        let mut seen = BTreeSet::new();
        let mut count = 0_usize;
        if !closed_keys(publication,&["protocol","run_id","receipt_sha256","receipt"],&[])
            || !closed_keys(receipt,&["library","shards"],&[])
            || !closed_keys(&receipt["library"],&["protocol","library_id","manifest","manifest_sha256","expected_tables","shards"],&[])
            || !closed_keys(&receipt["library"]["manifest"],&["protocol","library_id","sources"],&[])
            || receipt["library"]["manifest"]["protocol"]!="oo.library.membership/v1"
            || !shards.iter().any(|s|s["anchor_run"]==publication["run_id"])
            || receipt["library"]["manifest_sha256"] != sha(&receipt["library"]["manifest"])?
            || receipt["library"]["manifest"]["library_id"] != library_id
            || members.len()!=shards.len() || declared.len()!=shards.len()
        { return Err("library_receipt"); }
        for shard in shards {
            let source = text(shard,"source_id")?;
            if !seen.insert(source) || declared.iter().filter(|v| **v==source).count()!=1
                || shard["receipt_sha256"] != sha(&shard["receipt"])?
            { return Err("library_receipt"); }
            if shard["receipt"]["consistency"]=="transactional" || shard["receipt"].get("oracle_buffer").is_some() {
                validate_c1_shard(shard)?;
            }
            let matched:Vec<_> = members.iter().filter(|m| m["source_id"]==source).collect();
            if matched.len()!=1 {return Err("library_receipt");}
            let member = matched[0];
            let names = member["tables"].as_array().ok_or("library_receipt")?;
            let tables = shard["receipt"]["tables"].as_array().ok_or("library_receipt")?;
            if names.len()!=tables.len() {return Err("library_receipt");}
            for table in tables {
                let (schema,name) = storage_names(table)?;
                if member["schema"]!=schema || names.iter().filter(|v| **v==name).count()!=1 {
                    return Err("library_binding");
                }
            }
            // A shard is not a 32-table batch. Bound the complete library by
            // its existing explicit capacity, while validating every member.
            count = count.checked_add(tables.len()).ok_or("library_limits")?;
            if count > ceilings.max_datasets as usize { return Err("library_limits"); }
        }
        if receipt["library"]["expected_tables"].as_u64()!=Some(count as u64) {return Err("library_receipt");}
    }
    let project = text(effective, "project")?.to_owned();
    let role = text(visibility, "role")?.to_owned();
    let slug = role.strip_prefix("oo_r_").ok_or("library_role")?;
    let login = text(document, "login")?;
    let mut policies = Vec::new();
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for visible in visibility["datasets"]
        .as_array()
        .ok_or("library_visibility")?
    {
        let dataset = text(visible, "dataset")?;
        if !seen.insert(dataset.to_owned()) {
            return Err("library_visibility");
        }
        let (schema, table) = dataset.split_once('.').ok_or("library_visibility")?;
        let bindings: Vec<_> = document["bindings"]
            .as_array()
            .ok_or("library_binding")?
            .iter()
            .filter(|b| b["dataset"] == dataset)
            .collect();
        if bindings.len() != 1 {
            return Err("library_binding");
        }
        let binding = bindings[0];
        for key in ["config_digest", "type_digest", "input_digest"] {
            if binding[key].as_str().is_none_or(|s| {
                s.len() != 64
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }) {
                return Err("library_binding");
            }
        }

        let source = text(binding, "source")?;
        let shards: Vec<_> = receipt["shards"]
            .as_array()
            .ok_or("library_receipt")?
            .iter()
            .filter(|s| s["source_id"] == source)
            .collect();
        if shards.len() != 1 {
            return Err("library_binding");
        }
        let tables: Vec<_> = shards[0]["receipt"]["tables"]
            .as_array()
            .ok_or("library_receipt")?
            .iter()
            .filter(|t| storage_names(t).is_ok_and(|names| names == (schema,table)))
            .collect();
        if tables.len() != 1 || tables[0]["source_generation"] != binding["generation"] {
            return Err("library_binding");
        }
        let published = tables[0];
        let definitions: Vec<_> = effective["datasets"]
            .as_array()
            .ok_or("library_visibility")?
            .iter()
            .filter(|t| t["dataset"] == dataset)
            .collect();
        if definitions.len() != 1 {
            return Err("library_visibility");
        }
        let mut columns = Vec::new();
        let mut expected = Vec::new();
        for name in visible["columns"].as_array().ok_or("library_visibility")? {
            let name = name.as_str().ok_or("library_visibility")?;
            let matches: Vec<_> = definitions[0]["columns"]
                .as_array()
                .ok_or("library_visibility")?
                .iter()
                .filter(|c| c["name"] == name)
                .collect();
            if matches.len() != 1 {
                return Err("library_classification");
            }
            let column = matches[0];
            // The reviewed, hash-bound visibility allowlist grants this column, not all PII.
            let pii = match column["pii"].as_str() {
                Some("none") => PiiClass::None,
                Some("personal" | "sensitive" | "restricted")
                    if column["classification"]["by"]
                        .as_str()
                        .is_some_and(|reviewer| !reviewer.trim().is_empty()) =>
                {
                    PiiClass::Declared {
                        explicit_allow: true,
                    }
                }
                _ => return Err("library_classification"),
            };
            let status = match column["classification"]["state"].as_str() {
                Some("confirmed") => ClassStatus::Confirmed,
                Some("auto_accepted") => ClassStatus::AutoAccepted,
                _ => return Err("library_classification"),
            };
            columns.push(ColumnSpec {
                name: name.to_owned(),
                semantic_type: text(column, "semantic_type")?.to_owned(),
                status,
                pii,
            });
            expected.push(json!({"name":name,"data_type":text(column,"data_type")?}));
        }
        let policy = ColumnAclPolicy {
            project: project.clone(),
            config_digest: text(binding, "config_digest")?.to_owned(),
            generation: binding["generation"].as_i64().ok_or("library_binding")?,
            role: slug.to_owned(),
            login: login.to_owned(),
            table: LogicalTable {
                namespace: schema.to_owned(),
                name: table.to_owned(),
                columns,
            },
            row_filter: None,
        };
        crate::policy::compile_column_acl_native(&policy).map_err(|_| "library_policy")?;
        entries.push(json!({"dataset":dataset,"source_id":source,"config_digest":binding["config_digest"],"source_generation":published["source_generation"],"target_generation":published["target_generation"],"run_id":published["run_id"],"stored_namespace":"_oo","stored_physical":published["physical_name"],"expected_columns":expected,"type_digest":binding["type_digest"],"input_digest":binding["input_digest"]}));
        policies.push(policy);
    }
    if policies.is_empty() {
        return Err("library_visibility");
    }
    let config_digest = policies[0].config_digest.clone();
    let vector = json!(entries);
    let catalog = json!({"protocol":"oo.gateway.library-binding/v1","entries":vector,"library_id":library_id,"receipt_sha256":publication["receipt_sha256"],"pack_digest":pack.pack_digest,"limits":{"max_tables":ceilings.max_datasets,"max_columns":ceilings.max_effective_columns}});
    Ok(LibraryPolicy {
        policies,
        vector,
        catalog,
        project,
        role,
        policy_version: text(&visibility["policy"], "version")?.to_owned(),
        knowledge_version: text(&visibility["knowledge"], "version")?.to_owned(),
        config_digest,
    })
}

#[cfg(test)]
mod operator_review_tests {
    use super::*;

    fn input() -> Value {
        let review = json!({"project":"shop","role":"oo_r_admin","principal":"admin",
            "review_origin":"operator_pinned_file","hidden_columns":[],
            "columns":{"app.customers.contact":{"pii":"personal","semantic_type":"phone",
                "reviewer":"operator","explicit_allow":true}}});
        json!({"envelope":{"contract":"oo.operator.library-pack/v1",
            "operation":"prepareLocalBuildPack","management_identity":"operator_local",
            "producer":{"review_sha256":sha(&review).unwrap()},"review":review},
            "effective":{"project":"shop","datasets":[{"dataset":"app.customers","columns":[
                {"name":"contact","pii":"personal","semantic_type":"phone",
                 "classification":{"state":"confirmed","by":"operator"}}]}]},
            "visibility":{"role":"oo_r_admin","principal":"admin",
                "datasets":[{"dataset":"app.customers","columns":["contact"]}]}})
    }

    #[test]
    fn local_review_is_bound_to_exact_role_columns_and_explicit_pii_grant() {
        assert_eq!(local_operator_review(&input()), Ok(true));
        for path in ["/envelope/review/role", "/envelope/review/principal",
            "/envelope/review/columns/app.customers.contact/explicit_allow",
            "/envelope/review/columns/app.customers.contact/pii",
            "/envelope/management_identity", "/envelope/producer/review_sha256"] {
            let mut bad = input();
            *bad.pointer_mut(path).unwrap() = Value::Null;
            assert!(local_operator_review(&bad).is_err(), "{path}");
        }
        let mut bad = input();
        bad["envelope"]["review"]["columns"]["app.customers.contact"]["explicit_allow"] = json!(false);
        bad["envelope"]["producer"]["review_sha256"] = json!(sha(&bad["envelope"]["review"]).unwrap());
        assert!(local_operator_review(&bad).is_err());
        let mut hidden = input();
        hidden["envelope"]["review"]["hidden_columns"] = json!(["app.customers.contact"]);
        hidden["envelope"]["producer"]["review_sha256"] = json!(sha(&hidden["envelope"]["review"]).unwrap());
        assert!(local_operator_review(&hidden).is_err());
    }

    #[test]
    fn other_origins_are_not_local_operator_authority() {
        let mut other = input();
        other["envelope"]["contract"] = json!("oo.selfbuild-control-api/v1");
        assert_eq!(local_operator_review(&other), Ok(false));
        other["envelope"]["contract"] = json!("unknown");
        assert_eq!(local_operator_review(&other), Ok(false));
    }
}
