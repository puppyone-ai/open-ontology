//! Private operator registration, not scheduler/offset or source-write authority.
use crate::io::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const RECORD: &str = "oo.source.logical-registration/v1";
const OBSERVATION: &str = "oo.source.identity-observation/v1";

fn hash(value: &Value) -> Result<String> {
    Ok(sha(&bytes(value)?))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

fn hex_digest(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

pub fn input_keys(input: &Value, fields: &[&str], code: &str) -> Result<()> {
    let mut expected = fields.iter().copied().collect::<BTreeSet<_>>();
    if input.get("source_id").is_some() {
        require(valid_id(text(input, "source_id")?), "logical_source_id")?;
        expected.insert("source_id");
    }
    require(
        input
            .as_object()
            .is_some_and(|o| o.keys().map(String::as_str).collect::<BTreeSet<_>>() == expected),
        code,
    )
}

pub fn connection_digest(operator: &Value, engine: &str) -> Result<String> {
    require(matches!(engine, "mysql" | "oracle"), "identity_engine")?;
    let mut connection = json!({"type":engine});
    for key in ["host", "port", "database", "user", "secret_ref"] {
        require(!operator[key].is_null(), "identity_connection")?;
        connection[key] = operator[key].clone();
    }
    require(
        operator["port"]
            .as_u64()
            .is_some_and(|n| (1..=65535).contains(&n))
            && text(operator, "secret_ref")?.starts_with("secret://"),
        "identity_connection",
    )?;
    for key in ["host", "database", "user", "secret_ref"] {
        require(!text(operator, key)?.is_empty(), "identity_connection")?;
    }
    if engine == "oracle" {
        connection["service"] = json!(text(operator, "service")?);
    }
    // Transport material changes must never be hidden by an unchanged endpoint.
    for key in ["password_file", "ca_file", "tls_server_name"] {
        if let Some(value) = operator.get(key) {
            connection[key] = value.clone();
        }
    }
    hash(&connection)
}

fn scope(operator: &Value, engine: &str) -> Result<Value> {
    let mut value = json!({"database":text(operator, "database")?});
    if engine == "oracle" {
        value["owner"] = json!(text(operator, "owner")?);
    }
    Ok(value)
}

fn members(operator: &Value, engine: &str) -> Result<Value> {
    let selected = array(
        operator,
        if engine == "oracle" {
            "authorized_tables"
        } else {
            "tables"
        },
    )?;
    require(
        !selected.is_empty() && selected.len() <= 4096,
        "identity_membership",
    )?;
    let prefix = format!(
        "{}.",
        text(
            operator,
            if engine == "oracle" {
                "owner"
            } else {
                "database"
            }
        )?
    );
    let names = selected
        .iter()
        .map(|v| {
            let name = v.as_str().ok_or("identity_table")?;
            require(
                name.starts_with(&prefix) && name.len() > prefix.len() && !name.contains('\0'),
                "identity_table_scope",
            )?;
            Ok(name.to_owned())
        })
        .collect::<Result<BTreeSet<_>>>()?;
    require(names.len() == selected.len(), "identity_duplicate_table")?;
    Ok(json!(names))
}

fn operator_digest(operator: &Value) -> Result<String> {
    let mut value = operator.clone();
    value
        .as_object_mut()
        .ok_or("identity_operator")?
        .remove("source_registration");
    hash(&value)
}

fn pin(path: &Path) -> Result<Value> {
    Ok(json!({"path":path,"sha256":digest(path)?}))
}

fn read_pin(reference: &Value) -> Result<Value> {
    require(
        reference.as_object().is_some_and(|o| o.len() == 2) && hex_digest(&reference["sha256"]),
        "identity_pin",
    )?;
    let path = private(text(reference, "path")?)?;
    require(
        fs::metadata(&path)?.len() <= 32 * 1024 * 1024
            && digest(&path)? == text(reference, "sha256")?,
        "identity_pin_drift",
    )?;
    load(path)
}

pub fn validate_registry(registry: &Value, engine: &str) -> Result<()> {
    let protocol = match engine {
        "mysql" => "oo.mysql.native-operators/v1",
        "oracle" => "oo.oracle.source-operators/v1",
        _ => return Err("identity_engine".into()),
    };
    require(
        registry["protocol"] == protocol
            && registry.as_object().is_some_and(|o| {
                o.keys()
                    .all(|k| ["protocol", "operators", "source_identities"].contains(&k.as_str()))
            })
            && registry["operators"].is_array()
            && registry
                .get("source_identities")
                .is_none_or(Value::is_object),
        "identity_registry",
    )?;
    let mut paths = BTreeSet::new();
    for value in array(registry, "operators")? {
        require(
            paths.insert(private(value.as_str().ok_or("identity_operator_path")?)?.canonicalize()?),
            "identity_duplicate_operator",
        )?;
    }
    Ok(())
}

/// A logical id selects the existing private registry; it is not free-form override authority.
pub fn installed(input: &Value, registry_path: &Path, engine: &str) -> Result<Option<PathBuf>> {
    let Some(id) = input.get("source_id") else {
        return Ok(None);
    };
    require(
        valid_id(id.as_str().ok_or("logical_source_id")?),
        "logical_source_id",
    )?;
    let registry = load(private(registry_path)?)?;
    validate_registry(&registry, engine)?;
    let reference = &registry["source_identities"][id.as_str().unwrap()];
    let record = read_pin(reference)?;
    require(
        record["source_id"] == *id && record["engine"] == engine,
        "identity_registry_binding",
    )?;
    let mut found = Vec::new();
    for value in array(&registry, "operators")? {
        let path = private(value.as_str().ok_or("identity_operator_path")?)?;
        let operator = load(&path)?;
        if operator["source_registration"]["record"] == *reference {
            validate_operator(input, &operator, engine)?;
            found.push(path);
        }
    }
    require(found.len() == 1, "identity_operator_unknown_or_ambiguous")?;
    Ok(Some(found.remove(0)))
}

pub fn validate_operator(input: &Value, operator: &Value, engine: &str) -> Result<()> {
    let managed = operator.get("source_registration");
    if input.get("source_id").is_none() && managed.is_none() {
        return Ok(());
    }
    let registration = managed.ok_or("identity_registration_required")?;
    require(
        input.get("source_id").is_some(),
        "identity_explicit_source_required",
    )?;
    let registry_path = private(text(registration, "registry")?)?;
    let registry = load(&registry_path)?;
    validate_registry(&registry, engine)?;
    let record = read_pin(&registration["record"])?;
    require(
        record["protocol"] == RECORD
            && record["engine"] == engine
            && record["source_id"] == input["source_id"]
            && registry["source_identities"][text(input, "source_id")?] == registration["record"]
            && record["operator_config_sha256"] == operator_digest(operator)?
            && record["connection_sha256"] == connection_digest(operator, engine)?
            && record["members"] == members(operator, engine)?,
        "identity_stale_registration",
    )?;
    for key in ["host", "port", "database", "user", "secret_ref"] {
        require(input[key] == operator[key], "identity_connection_mismatch")?;
    }
    if engine == "oracle" {
        require(
            input["service"] == operator["service"]
                && operator["source_id"] == record["source_id"]
                && operator["source_identity"] == record["physical_identity"],
            "identity_oracle_binding",
        )?;
    }
    Ok(())
}

/// Bound runtime artifacts cannot silently inherit a new connection or membership.
pub fn runtime(operator: &Value, artifacts: &[&Value]) -> Result<()> {
    let Some(registration) = operator.get("source_registration") else {
        return Ok(());
    };
    let record = read_pin(&registration["record"])?;
    for artifact in artifacts {
        require(
            artifact["source_id"] == record["source_id"]
                && artifact["source_registration"] == *registration,
            "identity_runtime_rebind_required",
        )?;
    }
    Ok(())
}

pub fn check_state(operator: &Value, state: &Value) -> Result<()> {
    if let Some(registration) = operator.get("source_registration") {
        let record = read_pin(&registration["record"])?;
        require(
            state["source_id"] == record["source_id"],
            "identity_runtime_source_mismatch",
        )?;
    }
    Ok(())
}

pub fn oracle_inventory(operator: &Value, inventory: &Value) -> Result<()> {
    let Some(registration) = operator.get("source_registration") else {
        return Ok(());
    };
    let record = read_pin(&registration["record"])?;
    for table in array(inventory, "tables")? {
        let name = format!("{}.{}", text(table, "owner")?, text(table, "table")?);
        if record["members"]
            .as_array()
            .ok_or("identity_membership")?
            .contains(&json!(name))
        {
            require(
                record["known_table_schema_sha256"][&name]
                    == hash(&json!({"columns":table["columns"],"keys":table["keys"]}))?,
                "identity_schema_adoption_required",
            )?;
        }
    }
    Ok(())
}

fn mysql_query(operator: &Value, sql: &str, calls: &mut Calls) -> Result<Vec<Value>> {
    let o = &operator["onboarding"];
    require(
        o["runtime_transport"] == "host",
        "identity_metadata_host_transport",
    )?;
    let producer = text(o, "producer")?;
    binary(producer)?;
    let mut connection = json!({});
    for key in [
        "host",
        "port",
        "database",
        "user",
        "password_file",
        "ca_file",
        "tls_server_name",
    ] {
        connection[key] = operator[key].clone();
    }
    let request = json!({"protocol":"oo.mysql.source-sql/v1","connection":connection,
        "statement_ms":5000,"max_rows":65537,"max_bytes":16_777_216,"sql":sql});
    let raw = calls.run(
        "identity-mysql-metadata",
        &strings(&[producer, "source-sql"]),
        Some(&bytes(&request)?),
    )?;
    let text = String::from_utf8(raw)?;
    text.lines()
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

fn mysql_metadata(operator: &Value, calls: &mut Calls) -> Result<Value> {
    let physical = mysql_query(
        operator,
        "SELECT JSON_OBJECT('engine','mysql','server_uuid',@@GLOBAL.server_uuid)",
        calls,
    )?;
    require(physical.len() == 1, "identity_physical_mysql")?;
    let mut schemas = json!({});
    for name in members(operator, "mysql")?.as_array().unwrap() {
        let (schema, table) = name
            .as_str()
            .unwrap()
            .split_once('.')
            .ok_or("identity_table")?;
        ident(schema)?;
        ident(table)?;
        let columns = mysql_query(
            operator,
            &format!(
                "SELECT JSON_OBJECT('name',COLUMN_NAME,'ordinal',ORDINAL_POSITION,'type',COLUMN_TYPE,\
             'nullable',IS_NULLABLE,'charset',CHARACTER_SET_NAME,'collation',COLLATION_NAME,\
             'default',COLUMN_DEFAULT,'extra',EXTRA,'generation',GENERATION_EXPRESSION) \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' \
             ORDER BY ORDINAL_POSITION LIMIT 4097"
            ),
            calls,
        )?;
        let keys = mysql_query(
            operator,
            &format!(
                "SELECT JSON_OBJECT('name',INDEX_NAME,'unique',NON_UNIQUE,'seq',SEQ_IN_INDEX,\
             'column',COLUMN_NAME,'prefix',SUB_PART,'collation',COLLATION,'type',INDEX_TYPE) \
             FROM information_schema.STATISTICS WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' \
             ORDER BY INDEX_NAME,SEQ_IN_INDEX LIMIT 4097"
            ),
            calls,
        )?;
        let table_meta = mysql_query(
            operator,
            &format!(
                "SELECT JSON_OBJECT('engine',ENGINE,'collation',TABLE_COLLATION,'type',TABLE_TYPE) \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}'"
            ),
            calls,
        )?;
        require(
            !columns.is_empty()
                && columns.len() <= 4096
                && keys.len() <= 4096
                && table_meta.len() == 1
                && table_meta[0]["type"] == "BASE TABLE",
            "identity_metadata_coverage",
        )?;
        schemas[name.as_str().unwrap()] = json!(hash(
            &json!({"columns":columns,"keys":keys,"table":table_meta})
        )?);
    }
    Ok(json!({"physical_identity":physical[0],"table_schema_sha256":schemas}))
}

/// Fixed metadata-only readers; no arbitrary SQL, baseline prepare, or registration.
pub fn observe(input_path: &Path, operator_path: &Path, out: &Path) -> Result<Value> {
    let input = load(private(input_path)?)?;
    let operator = load(private(operator_path)?)?;
    let engine = text(&input, "type")?;
    match engine {
        "mysql" => crate::six_fields(&input)?,
        "oracle" => crate::oracle::validate_input(&input)?,
        _ => return Err("identity_engine".into()),
    }
    // Observing a proposed private operator does not authorize its logical-id adoption.
    let mut candidate = operator.clone();
    candidate
        .as_object_mut()
        .ok_or("identity_operator")?
        .remove("source_registration");
    let mut legacy = input.clone();
    legacy
        .as_object_mut()
        .ok_or("identity_input")?
        .remove("source_id");
    require(!out.exists(), "identity_new_output_required")?;
    let measured = match engine {
        "mysql" => {
            crate::connection(&legacy, &candidate)?;
            let mut calls = Calls::new(out)?;
            calls.deadlines.default = std::time::Duration::from_secs(30);
            let before = mysql_metadata(&candidate, &mut calls)?;
            let after = mysql_metadata(&candidate, &mut calls)?;
            require(before == after, "identity_metadata_drift")?;
            before
        }
        "oracle" => {
            dir(out)?;
            crate::oracle::check(
                &legacy,
                operator_without_registration(&candidate, out)?.as_path(),
                &out.join("native"),
            )?;
            let inventory = load(out.join("native/source-inventory.json"))?;
            let mut schemas = json!({});
            for table in array(&inventory, "tables")? {
                let name = format!("{}.{}", text(table, "owner")?, text(table, "table")?);
                if members(&candidate, engine)?
                    .as_array()
                    .unwrap()
                    .contains(&json!(name))
                {
                    schemas[&name] = json!(hash(
                        &json!({"columns":table["columns"],"keys":table["keys"]})
                    )?);
                }
            }
            json!({"physical_identity":candidate["source_identity"],"table_schema_sha256":schemas})
        }
        _ => return Err("identity_engine".into()),
    };
    let result = json!({"protocol":OBSERVATION,"engine":engine,
        "connection_sha256":connection_digest(&candidate,engine)?,"scope":scope(&candidate,engine)?,
        "physical_identity":measured["physical_identity"],"table_schema_sha256":measured["table_schema_sha256"],
        "operator_config_sha256":operator_digest(&candidate)?,
        "metadata_only":true,"read_only":true,"business_rows_read":0});
    save(out.join("observation.json"), &result)?;
    Ok(result)
}

fn operator_without_registration(operator: &Value, out: &Path) -> Result<PathBuf> {
    let path = out.join("candidate-operator.json");
    save(&path, operator)?;
    Ok(path)
}

fn transition(
    previous: Option<&Value>,
    request: &Value,
    operator: &Value,
    observed: &Value,
) -> Result<Value> {
    let engine = text(request, "engine")?;
    let id = text(request, "source_id")?;
    require(valid_id(id), "logical_source_id")?;
    let connection = connection_digest(operator, engine)?;
    let selected = members(operator, engine)?;
    let expected_scope = scope(operator, engine)?;
    require(
        observed["protocol"] == OBSERVATION
            && observed["engine"] == engine
            && observed["connection_sha256"] == connection
            && observed["scope"] == expected_scope
            && observed["metadata_only"] == true
            && observed["read_only"] == true
            && observed["business_rows_read"] == 0
            && observed["operator_config_sha256"] == operator_digest(operator)?,
        "identity_observation_binding",
    )?;
    let physical = &observed["physical_identity"];
    if engine == "mysql" {
        require(
            physical.as_object().is_some_and(|o| o.len() == 2)
                && physical["engine"] == "mysql"
                && crate::is_uuid(text(physical, "server_uuid")?),
            "identity_physical_mysql",
        )?;
    } else {
        let fields = [
            "engine",
            "dbid",
            "resetlogs_change",
            "pdb_guid",
            "pdb_name",
            "db_unique_name",
        ];
        require(
            physical.as_object().is_some_and(|o| {
                o.keys().map(String::as_str).collect::<BTreeSet<_>>() == BTreeSet::from(fields)
            }) && physical["engine"] == "oracle"
                && physical == &operator["source_identity"]
                && operator["source_id"] == id,
            "identity_physical_oracle",
        )?;
    }
    let schemas = observed["table_schema_sha256"]
        .as_object()
        .ok_or("identity_schemas")?;
    require(
        schemas.keys().cloned().collect::<BTreeSet<_>>()
            == selected
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect()
            && schemas.values().all(hex_digest),
        "identity_schema_coverage",
    )?;
    let mut known = observed["table_schema_sha256"].clone();
    let mut connection_revision = 1;
    let mut membership_revision = 1;
    if let Some(old) = previous {
        require(
            old["protocol"] == RECORD
                && old["source_id"] == id
                && old["engine"] == engine
                && old["scope"] == expected_scope
                && old["physical_identity"] == *physical,
            "identity_physical_adoption_required",
        )?;
        known = old["known_table_schema_sha256"].clone();
        let map = known.as_object_mut().ok_or("identity_known_schemas")?;
        for (table, fingerprint) in schemas {
            require(
                map.get(table).is_none_or(|old| old == fingerprint),
                "identity_schema_adoption_required",
            )?;
            map.insert(table.clone(), fingerprint.clone());
        }
        connection_revision = old["connection_revision"]
            .as_u64()
            .ok_or("connection_revision")?
            .checked_add(u64::from(old["connection_sha256"] != connection))
            .ok_or("revision_overflow")?;
        membership_revision = old["membership_revision"]
            .as_u64()
            .ok_or("membership_revision")?
            .checked_add(u64::from(old["members"] != selected))
            .ok_or("revision_overflow")?;
        require(
            connection_revision > 0 && membership_revision > 0,
            "identity_revision",
        )?;
        if old["operator_config_sha256"] == operator_digest(operator)?
            && old["connection_sha256"] == connection
            && old["members"] == selected
        {
            return Ok(old.clone());
        }
    }
    Ok(
        json!({"protocol":RECORD,"source_id":id,"engine":engine,"scope":expected_scope,
        "physical_identity":physical,"connection_revision":connection_revision,
        "membership_revision":membership_revision,"connection_sha256":connection,
        "members":selected,"membership_sha256":hash(&selected)?,"known_table_schema_sha256":known,
        "operator_config_sha256":operator_digest(operator)?,"observation":request["observation"],
        "previous_registration_sha256":request["expected_registration_sha256"]}),
    )
}

struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn register(request_path: &Path, out: &Path) -> Result<Value> {
    require(
        out.is_absolute()
            && !out
                .components()
                .any(|p| p == std::path::Component::ParentDir),
        "identity_absolute_output",
    )?;
    let request = load(private(request_path)?)?;
    require(
        request.as_object().is_some_and(|o| {
            o.keys().map(String::as_str).collect::<BTreeSet<_>>()
                == BTreeSet::from([
                    "protocol",
                    "engine",
                    "source_id",
                    "registry",
                    "operator",
                    "observation",
                    "expected_registration_sha256",
                ])
        }) && request["protocol"] == "oo.source.register-logical-identity/v1",
        "identity_request",
    )?;
    let engine = text(&request, "engine")?;
    let id = text(&request, "source_id")?;
    require(valid_id(id), "logical_source_id")?;
    let registry_path = private(text(&request, "registry")?)?;
    let lock_path = registry_path.with_extension("identity.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|_| "identity_registry_locked")?;
    let _guard = Lock(lock_path);
    drop(lock);
    let mut registry = load(&registry_path)?;
    validate_registry(&registry, engine)?;
    let registry_before = fs::read(&registry_path)?;
    let operator_path = private(text(&request, "operator")?)?;
    let mut operator = load(&operator_path)?;
    // Rotation creates a new immutable operator, never edits the retained runtime config.
    operator
        .as_object_mut()
        .ok_or("identity_operator")?
        .remove("source_registration");
    let observed = read_pin(&request["observation"])?;
    let prior_ref = registry["source_identities"][id].clone();
    let previous = if prior_ref.is_null() {
        None
    } else {
        Some(read_pin(&prior_ref)?)
    };
    let record = transition(previous.as_ref(), &request, &operator, &observed)?;
    let unchanged = previous.as_ref() == Some(&record);
    if !unchanged {
        require(
            request["expected_registration_sha256"] == prior_ref["sha256"],
            "identity_revision_conflict",
        )?;
    }
    let fields = if engine == "oracle" {
        vec![
            "type",
            "host",
            "port",
            "service",
            "database",
            "user",
            "secret_ref",
        ]
    } else {
        vec!["host", "port", "database", "user", "secret_ref"]
    };
    let mut retained = Vec::new();
    for item in array(&registry, "operators")? {
        let installed = load(private(item.as_str().ok_or("identity_operator_path")?)?)?;
        if !prior_ref.is_null() && installed["source_registration"]["record"] == prior_ref {
            continue;
        }
        require(
            !fields.iter().all(|k| installed[*k] == operator[*k]),
            "identity_duplicate_connection",
        )?;
        require(
            installed["source_registration"]["record"].is_null()
                || read_pin(&installed["source_registration"]["record"])?["source_id"] != id,
            "identity_duplicate_source",
        )?;
        retained.push(item.clone());
    }
    require(!out.exists(), "identity_new_output_required")?;
    dir(out)?;
    save(out.join("registry-before.json"), &registry)?;
    let reference = if unchanged {
        prior_ref
    } else {
        save(out.join("registration.json"), &record)?;
        pin(&out.join("registration.json"))?
    };
    operator["source_registration"] = json!({"registry":registry_path,"record":reference});
    let mut input = json!({"type":engine,"source_id":id});
    for key in ["host", "port", "database", "user", "secret_ref"] {
        input[key] = operator[key].clone();
    }
    if engine == "oracle" {
        input["service"] = operator["service"].clone();
    }
    // Reuse existing secret resolution/containment checks, without enabling a binding.
    let mut legacy_input = input.clone();
    legacy_input.as_object_mut().unwrap().remove("source_id");
    let mut legacy_operator = operator.clone();
    legacy_operator
        .as_object_mut()
        .unwrap()
        .remove("source_registration");
    if engine == "oracle" {
        crate::oracle::identity_source(&legacy_input, &legacy_operator)?;
    } else {
        crate::connection(&legacy_input, &legacy_operator)?;
    }
    if !unchanged {
        save(out.join("operator.json"), &operator)?;
        retained.push(json!(out.join("operator.json")));
        registry["operators"] = json!(retained);
        if registry.get("source_identities").is_none() {
            registry["source_identities"] = json!({});
        }
        registry["source_identities"][id] = reference.clone();
        require(
            fs::read(&registry_path)? == registry_before,
            "identity_registry_concurrent_change",
        )?;
        let temporary = out.join("registry-next.json");
        save(&temporary, &registry)?;
        fs::rename(&temporary, &registry_path)?;
        fs::File::open(registry_path.parent().ok_or("identity_registry_parent")?)?.sync_all()?;
    }
    save(out.join("input.json"), &input)?;
    let receipt = json!({"protocol":"oo.source.identity-registration-result/v1","source_id":id,
        "engine":engine,"registration":reference,"connection_revision":record["connection_revision"],
        "membership_revision":record["membership_revision"],"idempotent":unchanged,
        "observation":request["observation"],"target_registered":false,"binding_enabled":false,
        "offset_reset":false,"business_rows_read":0,"adoption_executed":false});
    save(out.join("receipt.json"), &receipt)?;
    Ok(receipt)
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
