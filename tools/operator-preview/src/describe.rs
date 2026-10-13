//! Role-visible pack knowledge. The operator file does not admit a column by itself.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use oo_gateway::pack_catalog::{PackCatalog, PackColumn};
use oo_gateway::ColumnSpec;
use serde_json::{json, Map, Value};

use crate::authority::{load_authority, load_policy, Authority, ImportId};
use crate::durable::{execution_id, refuse_symlink, sha256_hex, write_durable};
use crate::output::{finished_error, finished_value, shell, Fail, Finished};
use crate::{CANDIDATE_PROTOCOL, DESCRIBE_PROTOCOL, PACK_RECEIPT_PROTOCOL};

pub fn describe(path: &Path) -> Finished {
    match describe_value(path) {
        Ok(value) => finished_value(None, &value),
        Err((fail, value)) => {
            if let Some(value) = value {
                finished_value(Some(fail), &value)
            } else {
                finished_error(fail)
            }
        }
    }
}

fn describe_value(path: &Path) -> Result<Value, (Fail, Option<Value>)> {
    let id = execution_id().map_err(|fail| (fail, None))?;
    let authority = load_authority(path).map_err(|fail| (fail, None))?;
    let bound = match bind_preview(&authority) {
        Ok(bound) => bound,
        Err(fail) => {
            let value = refusal(&authority, &id, "pack_mismatch", DESCRIBE_PROTOCOL);
            return Err((fail, Some(value)));
        }
    };
    let mut fields = Map::new();
    fields.insert("protocol".to_string(), json!(DESCRIBE_PROTOCOL));
    fields.insert("execution_id".to_string(), json!(id));
    fields.insert("outcome".to_string(), json!("described"));
    fields.insert("import".to_string(), import_value(&authority.import));
    fields.insert("knowledge".to_string(), bound.knowledge);
    fields.insert("policy".to_string(), bound.policy);
    fields.insert("draft_pack".to_string(), bound.draft);
    fields.insert(
        "pack_receipt_protocol".to_string(),
        json!(PACK_RECEIPT_PROTOCOL),
    );
    let value = shell(fields);
    if authority.receipt_dir.as_os_str().is_empty() {
        return Err((Fail::Request, None));
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| (Fail::Io, None))?;
    write_durable(&authority.receipt_dir, &format!("{id}.json"), &bytes)
        .map_err(|fail| (fail, None))?;
    Ok(value)
}

pub(crate) struct Bound {
    pub(crate) knowledge: Value,
    pub(crate) policy: Value,
    pub(crate) draft: Value,
}

pub fn bind_preview(authority: &Authority) -> Result<Bound, Fail> {
    let policy = load_policy(&authority.policy_path)?;
    if policy.project != authority.project
        || policy.login != authority.user
        || policy.generation != authority.import.import_generation
        || policy.config_digest != authority.import.target_config_sha256
    {
        return Err(Fail::Pack);
    }
    let receipt = verified_receipt(&authority.pack_dir, &authority.receipt_path)?;
    let binding = receipt.get("binding").ok_or(Fail::Pack)?;
    if binding.get("import_generation").and_then(Value::as_i64) != Some(authority.import.import_generation)
        || binding.get("import_hash").and_then(Value::as_str) != Some(authority.import.import_hash.as_str())
        || binding.get("snapshot_id").and_then(Value::as_str) != Some(authority.import.snapshot_id.as_str())
        || binding.get("source_id").and_then(Value::as_str) != Some(authority.import.source_id.as_str())
        || binding.get("manifest_sha256").and_then(Value::as_str)
            != Some(authority.import.manifest_sha256.as_str())
        || binding.get("target_config_sha256").and_then(Value::as_str)
            != Some(authority.import.target_config_sha256.as_str())
    {
        return Err(Fail::Pack);
    }
    if receipt.get("publication").and_then(Value::as_str) != Some("not_published")
        || receipt.get("review").and_then(Value::as_str) != Some("unreviewed")
    {
        return Err(Fail::Pack);
    }
    let admitted = admitted_fields(&authority.pack_dir, &receipt)?;
    let catalog = PackCatalog::load_dir(&authority.role_pack_dir).map_err(|_| Fail::Pack)?;
    let found = catalog
        .find(
            &authority.project,
            &authority.role,
            &authority.policy_version,
            &authority.knowledge_version,
        )
        .ok_or(Fail::Pack)?;
    let manifest = confirmed_manifest(&authority.role_pack_dir, &found.pack_digest)?;
    if manifest.get("project").and_then(Value::as_str) != Some(authority.project.as_str())
        || manifest.get("role").and_then(Value::as_str) != Some(authority.role.as_str())
        || manifest
            .pointer("/knowledge/id")
            .and_then(Value::as_str)
            != Some(authority.knowledge_id.as_str())
        || manifest
            .pointer("/knowledge/version")
            .and_then(Value::as_str)
            != Some(authority.knowledge_version.as_str())
        || manifest.pointer("/policy/id").and_then(Value::as_str)
            != Some(authority.policy_pack_id.as_str())
        || manifest
            .pointer("/policy/version")
            .and_then(Value::as_str)
            != Some(authority.policy_version.as_str())
    {
        return Err(Fail::Pack);
    }
    let set = oo_gateway::policy::compile_column_acl_set(std::slice::from_ref(&policy.acl))
        .map_err(|_| Fail::Policy)?;
    if set.role() != authority.role {
        return Err(Fail::Pack);
    }
    // `authority.binds` is required earlier and is not this check. The policy
    // table must be admitted by the sealed pack and equal to the role pack.
    let dataset = bind_role_visible(
        &policy.acl.table.namespace,
        &policy.acl.table.name,
        &policy.acl.table.columns,
        &found.columns,
        &admitted,
    )?;
    let columns: Vec<Value> = found
        .columns
        .iter()
        .filter(|column| column.table == dataset)
        .map(|column| json!({"table": column.table, "name": column.name}))
        .collect();
    let policy_names: Vec<&str> = policy
        .acl
        .table
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    Ok(Bound {
        knowledge: json!({
            "id": authority.knowledge_id,
            "version": authority.knowledge_version,
            "dataset": dataset,
            "pack_digest": found.pack_digest,
            "columns": columns,
        }),
        policy: json!({
            "project": authority.project,
            "role": authority.role,
            "policy_id": policy.config_digest,
            "policy_pack_id": authority.policy_pack_id,
            "policy_version": authority.policy_version,
            "generation": policy.generation,
            "login": policy.login,
            "dataset": dataset,
            "columns": policy_names,
        }),
        draft: json!({
            "receipt_protocol": PACK_RECEIPT_PROTOCOL,
            "review": "unreviewed",
            "publication": "not_published",
            "human_approval": false,
            "dataset": dataset,
            "admitted_fields": admitted,
            "label": "committed compiler generation re-sealed locally; not a new source and not a new builder chain",
        }),
    })
}

fn bind_role_visible(
    namespace: &str,
    table: &str,
    policy_columns: &[ColumnSpec],
    role_columns: &[PackColumn],
    admitted: &[Value],
) -> Result<String, Fail> {
    if namespace.is_empty()
        || table.is_empty()
        || namespace.contains(['.', '/', '\\', '\0'])
        || table.contains(['.', '/', '\\', '\0'])
    {
        return Err(Fail::Pack);
    }
    let dataset = format!("{namespace}.{table}");
    let mut admitted_columns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for field in admitted {
        let field_dataset = field.get("dataset").and_then(Value::as_str).ok_or(Fail::Pack)?;
        let field_column = field.get("column").and_then(Value::as_str).ok_or(Fail::Pack)?;
        if field_dataset.is_empty() || field_column.is_empty() {
            return Err(Fail::Pack);
        }
        admitted_columns
            .entry(field_dataset.to_string())
            .or_default()
            .insert(field_column.to_string());
    }
    let Some(admitted_names) = admitted_columns.get(&dataset) else {
        return Err(Fail::Pack);
    };
    let mut policy_names = BTreeSet::new();
    for column in policy_columns {
        if column.name.is_empty() || !policy_names.insert(column.name.clone()) {
            return Err(Fail::Pack);
        }
    }
    let mut role_names = BTreeSet::new();
    for column in role_columns {
        if column.table.is_empty() || column.name.is_empty() {
            return Err(Fail::Pack);
        }
        if !admitted_columns.contains_key(&column.table) {
            return Err(Fail::Pack);
        }
        if column.table == dataset && !role_names.insert(column.name.clone()) {
            return Err(Fail::Pack);
        }
    }
    if role_names.is_empty()
        || role_names != policy_names
        || role_names.iter().any(|name| !admitted_names.contains(name))
    {
        return Err(Fail::Pack);
    }
    Ok(dataset)
}

fn verified_receipt(pack_dir: &Path, receipt_path: &Path) -> Result<Value, Fail> {
    refuse_symlink(pack_dir)?;
    let bytes = crate::durable::read_capped(receipt_path, 1_048_576)?;
    let receipt: Value = serde_json::from_slice(&bytes).map_err(|_| Fail::Pack)?;
    if receipt.get("protocol").and_then(Value::as_str) != Some(PACK_RECEIPT_PROTOCOL) {
        return Err(Fail::Pack);
    }
    let files = receipt.get("files").and_then(Value::as_object).ok_or(Fail::Pack)?;
    for group in files.values() {
        match group {
            Value::Array(items) => {
                for item in items {
                    check_hash(pack_dir, item)?;
                }
            }
            Value::Object(_) => check_hash(pack_dir, group)?,
            _ => return Err(Fail::Pack),
        }
    }
    Ok(receipt)
}

fn check_hash(pack_dir: &Path, item: &Value) -> Result<(), Fail> {
    let relative = item.get("path").and_then(Value::as_str).ok_or(Fail::Pack)?;
    let expected = item.get("sha256").and_then(Value::as_str).ok_or(Fail::Pack)?;
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains("..")
        || relative.contains('\\')
        || relative.contains('\0')
    {
        return Err(Fail::Pack);
    }
    let path = pack_dir.join(relative);
    refuse_symlink(&path)?;
    let bytes = fs::read(&path).map_err(|_| Fail::Pack)?;
    if sha256_hex(&bytes) != expected {
        return Err(Fail::Pack);
    }
    Ok(())
}

fn admitted_fields(pack_dir: &Path, receipt: &Value) -> Result<Vec<Value>, Fail> {
    let candidates = receipt
        .pointer("/files/candidates")
        .and_then(Value::as_array)
        .ok_or(Fail::Pack)?;
    let mut out = Vec::new();
    for item in candidates {
        let relative = item.get("path").and_then(Value::as_str).ok_or(Fail::Pack)?;
        let bytes = fs::read(pack_dir.join(relative)).map_err(|_| Fail::Pack)?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Fail::Pack)?;
        if value.get("protocol").and_then(Value::as_str) != Some(CANDIDATE_PROTOCOL) {
            return Err(Fail::Pack);
        }
        let dataset = value.get("dataset").and_then(Value::as_str).unwrap_or("");
        let fields = value.get("fields").and_then(Value::as_array).ok_or(Fail::Pack)?;
        for field in fields {
            if field.get("admitted") != Some(&Value::Bool(true)) {
                continue;
            }
            out.push(json!({
                "dataset": dataset,
                "column": field.get("column").cloned().unwrap_or(Value::Null),
                "data_type": field.get("data_type").cloned().unwrap_or(Value::Null),
                "label": field.get("label").cloned().unwrap_or(Value::Null),
                "description": field.get("description").cloned().unwrap_or(Value::Null),
                "admission": field.get("admission").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    Ok(out)
}

fn confirmed_manifest(dir: &Path, digest: &str) -> Result<Value, Fail> {
    let entries = fs::read_dir(dir).map_err(|_| Fail::Pack)?;
    for entry in entries {
        let entry = entry.map_err(|_| Fail::Pack)?;
        let file_type = entry.file_type().map_err(|_| Fail::Pack)?;
        if file_type.is_symlink() || !file_type.is_file() {
            return Err(Fail::Pack);
        }
        let text = fs::read_to_string(entry.path()).map_err(|_| Fail::Pack)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Fail::Pack)?;
        if value.get("pack_digest").and_then(Value::as_str) != Some(digest) {
            continue;
        }
        let files = value.get("files").and_then(Value::as_array).ok_or(Fail::Pack)?;
        for file in files {
            if file.get("path").and_then(Value::as_str) != Some("manifest.json") {
                continue;
            }
            let manifest = file.get("text").and_then(Value::as_str).ok_or(Fail::Pack)?;
            return serde_json::from_str(manifest).map_err(|_| Fail::Pack);
        }
    }
    Err(Fail::Pack)
}

pub fn import_value(import: &ImportId) -> Value {
    json!({
        "import_generation": import.import_generation,
        "import_hash": import.import_hash,
        "snapshot_id": import.snapshot_id,
        "source_id": import.source_id,
        "manifest_sha256": import.manifest_sha256,
        "target_config_sha256": import.target_config_sha256,
        "seal": "local_mapping_bound_to_re_sealed_compiler_pack",
        "source_dialed": false,
        "builder_chain": "not_claimed",
    })
}

pub fn refusal(authority: &Authority, id: &str, class: &str, protocol: &str) -> Value {
    let mut fields = Map::new();
    fields.insert("protocol".to_string(), json!(protocol));
    fields.insert("execution_id".to_string(), json!(id));
    fields.insert("outcome".to_string(), json!("refused"));
    fields.insert("class".to_string(), json!(class));
    fields.insert("import".to_string(), import_value(&authority.import));
    fields.insert(
        "knowledge".to_string(),
        json!({
            "id": authority.knowledge_id,
            "version": authority.knowledge_version,
        }),
    );
    fields.insert(
        "policy".to_string(),
        json!({
            "project": authority.project,
            "role": authority.role,
            "policy_pack_id": authority.policy_pack_id,
            "policy_version": authority.policy_version,
            "target_config_sha256": authority.import.target_config_sha256,
        }),
    );
    shell(fields)
}

#[cfg(test)]
mod tests {
    use oo_gateway::{ClassStatus, ColumnSpec, PiiClass};
    use serde_json::{json, Value};

    use super::bind_role_visible;
    use crate::output::Fail;

    fn admitted(pairs: &[(&str, &str)]) -> Vec<Value> {
        pairs
            .iter()
            .map(|(dataset, column)| json!({"dataset": dataset, "column": column}))
            .collect()
    }

    fn role(pairs: &[(&str, &str)]) -> Vec<oo_gateway::pack_catalog::PackColumn> {
        pairs
            .iter()
            .map(|(table, name)| oo_gateway::pack_catalog::PackColumn {
                table: (*table).to_string(),
                name: (*name).to_string(),
            })
            .collect()
    }

    fn policy(names: &[&str]) -> Vec<ColumnSpec> {
        names
            .iter()
            .map(|name| ColumnSpec {
                name: (*name).to_string(),
                status: ClassStatus::Confirmed,
                semantic_type: "text".to_string(),
                pii: PiiClass::None,
            })
            .collect()
    }

    #[test]
    fn policy_must_equal_role_visible_admitted_columns() {
        let sealed = admitted(&[
            ("public.batch_c", "id"),
            ("public.batch_c", "qty"),
            ("public.batch_c", "amount"),
            ("public.batch_c", "note"),
            ("public.batch_a", "id"),
            ("public.batch_a", "amount"),
            ("public.batch_a", "label"),
        ]);
        let visible = role(&[
            ("public.batch_c", "id"),
            ("public.batch_c", "qty"),
            ("public.batch_c", "amount"),
            ("public.batch_c", "note"),
        ]);
        let bound = bind_role_visible(
            "public",
            "batch_c",
            &policy(&["note", "id", "amount", "qty"]),
            &visible,
            &sealed,
        );
        assert_eq!(bound.as_deref(), Ok("public.batch_c"));
        assert_eq!(
            bind_role_visible(
                "public",
                "batch_c",
                &policy(&["id", "qty", "amount", "note", "secret"]),
                &visible,
                &sealed,
            ),
            Err(Fail::Pack)
        );
        assert_eq!(
            bind_role_visible(
                "public",
                "batch_c",
                &policy(&["id", "qty", "amount"]),
                &visible,
                &sealed,
            ),
            Err(Fail::Pack)
        );
        assert_eq!(
            bind_role_visible("preview", "sample", &policy(&["id"]), &visible, &sealed),
            Err(Fail::Pack)
        );
        assert_eq!(
            bind_role_visible(
                "public",
                "batch_a",
                &policy(&["id", "amount", "label"]),
                &visible,
                &sealed,
            ),
            Err(Fail::Pack)
        );
        let smuggled = role(&[
            ("public.batch_c", "id"),
            ("preview.sample", "n_i8"),
        ]);
        assert_eq!(
            bind_role_visible(
                "public",
                "batch_c",
                &policy(&["id"]),
                &smuggled,
                &sealed,
            ),
            Err(Fail::Pack)
        );
    }
}
