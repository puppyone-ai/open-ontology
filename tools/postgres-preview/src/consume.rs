//! Attach the fixed typed consumer to the sealed authority already on disk.
//!
//! The binary is the author-complete operator-preview at 6d9d6cbb. This module
//! does not compile that tree, does not read the source, and does not call the model.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::admit::sha256_bytes;
use crate::config::EndpointFile;
use crate::error::Fail;
use crate::pg::{backend_active, connect_read_only, endpoint_from_files, hold_mapped_table, mapped_table_name};
use crate::spawn::{finish_child, run_binary, spawn_binary};

pub const CONSUMER_COMMIT: &str = "6d9d6cbb98f249709a462915afc8f45e4db3c047";
pub const CONSUMER_SHA256: &str = "f7fdba26e8ec13bfd5a423f741f80c9e238abd74b39f8ea96e592f1b34c76a78";
pub const ACL_COMMIT: &str = "6d9d6cbb98f249709a462915afc8f45e4db3c047";
pub const ACL_SHA256: &str = "7c7ce6714f7877c9f75b1a59d9c61c31a95013d2bc0c057d3bd8c1dad37d43a4";
pub const PROVISION_SHA256: &str = "c715a44f3391caaca32dfca81c7945c361a8ccf0ec5ee015d903c387e59d4522";
const ACL_PROTOCOL: &str = "oo.gateway.install-column-acl-cli/v1";
const PROVISION_PROTOCOL: &str = "oo.gateway.provision-mapping-reader-cli/v1";

pub struct Bind {
    pub project: String,
    pub source_id: String,
}

pub fn read_bind(bytes: &[u8]) -> Result<Bind, Fail> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Fail::new("consumer.request", 3))?;
    let project = value.get("project").and_then(Value::as_str).unwrap_or("").to_owned();
    let source_id = value.pointer("/import/source_id").and_then(Value::as_str).unwrap_or("").to_owned();
    if project.is_empty() || source_id.is_empty() {
        return Err(Fail::new("consumer.request", 3));
    }
    Ok(Bind { project, source_id })
}

pub fn bind_split(bind: &Bind) -> bool {
    bind.project != bind.source_id
}

pub fn typed_sql() -> &'static str {
    "SELECT hub_id, hub_token, capacity_grams, note IS NULL FROM public.hubs ORDER BY hub_id LIMIT 7"
}

pub fn hidden_sql() -> &'static str {
    "SELECT secret_note FROM public.hubs ORDER BY hub_id LIMIT 1"
}

pub fn attach(
    out_dir: &Path,
    preview_bin: &Path,
    source: &EndpointFile,
    target: &EndpointFile,
) -> Result<(), Fail> {
    let binary = bytea_binary(preview_bin);
    let digest = sha256_bytes(&fs::read(&binary).map_err(|_| Fail::new("consumer.binary", 3))?);
    if digest != CONSUMER_SHA256 {
        return Err(Fail::new("consumer.binary", 3));
    }
    let authority = out_dir.join("stages").join("sealed").join("authority.json");
    let bytes = fs::read(&authority).map_err(|_| Fail::new("consumer.request", 3))?;
    let bind = read_bind(&bytes)?;
    let authority_doc: Value = serde_json::from_slice(&bytes).map_err(|_| Fail::new("consumer.request", 3))?;
    let dir = out_dir.join("consumer-attach");
    fs::create_dir_all(&dir).map_err(|_| Fail::new("config.read", 1))?;
    let source_open = postgres_open(source);
    let split = bind_split(&bind);
    if source_open {
        write_result(&dir, &result_body(&bind, &digest, &AttachFacts {
            split,
            source_open: true,
            oracle_literals: false,
            inflight: "not_started",
            rollback: false,
            describe: -1,
            queried: -1,
            hidden_exit: -1,
            wide: -1,
            cancel: -1,
            reopen: -1,
            install_exit: -1,
            install_effect: "not_started",
            provision_exit: -1,
            provision_effect: "not_started",
            reason: "source is still open; typed query waits until it is closed",
        }))?;
        return Err(Fail::new("consumer.pending", 8));
    }
    let installed = install_reader(preview_bin, &dir, &authority_doc, target)?;
    let provisioned = if installed.status == 0 && matches!(installed.effect.as_str(), "installed" | "replay") {
        let policy = read_policy(&authority_doc)?;
        let login = policy.get("login").and_then(Value::as_str).unwrap_or("").to_owned();
        match suspend_pipeline_inherit(target, &login) {
            Ok(effect) => {
                fs::write(dir.join("pipeline-inherit.txt"), effect.as_bytes()).map_err(|_| Fail::new("config.read", 1))?;
                provision_reader(preview_bin, &dir, &authority_doc, target)?
            }
            Err(fail) => {
                fs::write(dir.join("pipeline-inherit.txt"), b"error").map_err(|_| Fail::new("config.read", 1))?;
                write_result(&dir, &result_body(&bind, &digest, &AttachFacts {
                    split,
                    source_open: false,
                    oracle_literals: false,
                    inflight: "not_started",
                    rollback: false,
                    describe: -1,
                    queried: -1,
                    hidden_exit: -1,
                    wide: -1,
                    cancel: -1,
                    reopen: -1,
                    install_exit: installed.status,
                    install_effect: installed.effect.as_str(),
                    provision_exit: -1,
                    provision_effect: "not_started",
                    reason: "pipeline runtime inherit was not suspended",
                }))?;
                return Err(fail);
            }
        }
    } else {
        ReaderStep { status: -1, effect: "not_started".to_owned() }
    };
    if installed.status != 0
        || !matches!(installed.effect.as_str(), "installed" | "replay")
        || !mapped_table_name(&installed.physical)
        || provisioned.status != 0
        || !matches!(provisioned.effect.as_str(), "installed" | "replay")
    {
        write_result(&dir, &result_body(&bind, &digest, &AttachFacts {
            split,
            source_open: false,
            oracle_literals: false,
            inflight: "not_started",
            rollback: false,
            describe: -1,
            queried: -1,
            hidden_exit: -1,
            wide: -1,
            cancel: -1,
            reopen: -1,
            install_exit: installed.status,
            install_effect: installed.effect.as_str(),
            provision_exit: provisioned.status,
            provision_effect: provisioned.effect.as_str(),
            reason: if installed.status != 0 || !matches!(installed.effect.as_str(), "installed" | "replay") {
                "column acl install did not create oo_r_reader"
            } else if !mapped_table_name(&installed.physical) {
                "column acl install did not return a mapped table"
            } else {
                "mapping reader provision did not grant the login"
            },
        }))?;
        return Err(Fail::new("consumer.pending", saturate_exit(installed.status)));
    }
    let query = dir.join("query.json");
    write_query(&query, typed_sql(), 30_000)?;
    let hidden = dir.join("hidden.json");
    write_query(&hidden, hidden_sql(), 30_000)?;
    let wide_policy = dir.join("policy-wide.json");
    let wide_authority = dir.join("authority-wide.json");
    write_wide(&authority_doc, &wide_policy, &wide_authority)?;
    let describe = invoke(&binary, &dir, "describe", &["describe", "--authority", path_str(&authority)])?;
    let query_authority = isolated_authority(&authority, &dir, "query")?;
    let queried = invoke(&binary, &dir, "query", &["query", "--authority", path_str(&query_authority), "--query", path_str(&query)])?;
    let hidden_exit = invoke(&binary, &dir, "hidden", &["query", "--authority", path_str(&authority), "--query", path_str(&hidden)])?;
    let wide = invoke(
        &binary,
        &dir,
        "widen",
        &["query", "--authority", path_str(&wide_authority), "--query", path_str(&query)],
    )?;
    let state_dir = authority_doc.get("state_dir").and_then(Value::as_str).unwrap_or("");
    let (cancel, inflight, rollback) = inflight_cancel(
        &binary,
        &dir,
        &authority,
        Path::new(state_dir),
        target,
        &installed.physical,
        "SELECT hub_id FROM public.hubs ORDER BY hub_id LIMIT 1",
    )?;
    let reopen_authority = isolated_authority(&authority, &dir, "reopen")?;
    let reopen = invoke(&binary, &dir, "reopen", &["query", "--authority", path_str(&reopen_authority), "--query", path_str(&query)])?;
    let query_stdout = fs::read(dir.join("query.stdout")).unwrap_or_default();
    let oracle_literals = oracle_literals_present(&query_stdout);
    write_result(&dir, &result_body(&bind, &digest, &AttachFacts {
        split,
        source_open: false,
        oracle_literals,
        inflight,
        rollback,
        describe,
        queried,
        hidden_exit,
        wide,
        cancel,
        reopen,
        install_exit: installed.status,
        install_effect: installed.effect.as_str(),
        provision_exit: provisioned.status,
        provision_effect: provisioned.effect.as_str(),
        reason: if split {
            "authority project is not import source_id; the consumer returns request before a session"
        } else {
            "authority project matches import source_id"
        },
    }))?;
    if split {
        return Err(Fail::new("consumer.request", 3));
    }
    if describe != 0 {
        return Err(Fail::new("consumer.pending", saturate_exit(describe)));
    }
    if queried != 0 || !oracle_literals || reopen != 0 {
        return Err(Fail::new("consumer.pending", saturate_exit(queried)));
    }
    if hidden_exit != 6 {
        return Err(Fail::new("consumer.pending", saturate_exit(hidden_exit)));
    }
    if wide != 9 {
        return Err(Fail::new("consumer.pending", saturate_exit(wide)));
    }
    if inflight != "user_cancel" || cancel != 0 || !rollback {
        return Err(Fail::new("consumer.pending", saturate_exit(cancel)));
    }
    Ok(())
}

/// Consume every admitted table and every allowed column through the fixed Pack.
/// The source must already be unavailable. Each request gets its own durable audit.
pub fn attach_all(out_dir: &Path, preview_bin: &Path, source: &EndpointFile, target: &EndpointFile) -> Result<(), Fail> {
    if postgres_open(source) { return Err(Fail::new("consumer.source_open", 8)); }
    let binary = bytea_binary(preview_bin);
    if sha256_bytes(&fs::read(&binary).map_err(|_| Fail::new("consumer.binary", 3))?) != CONSUMER_SHA256 {
        return Err(Fail::new("consumer.binary", 3));
    }
    let authority = out_dir.join("stages/sealed/authority.json");
    let authority_bytes = fs::read(&authority).map_err(|_| Fail::new("consumer.request", 3))?;
    if bind_split(&read_bind(&authority_bytes)?) {
        return Err(Fail::new("consumer.request", 3));
    }
    let catalog: Value = serde_json::from_slice(&fs::read(out_dir.join("stages/catalog-import.stdout")).map_err(|_| Fail::new("consumer.request", 3))?).map_err(|_| Fail::new("consumer.request", 3))?;
    let dir = out_dir.join("consumer-all"); fs::create_dir_all(&dir).map_err(|_| Fail::new("config.read", 1))?;
    let queries = table_queries(&catalog)?;
    let mut policies = Vec::new();
    let mut authorities = Vec::new();
    for (index, (dataset, _)) in queries.iter().enumerate().step_by(2) {
        let table_authority = out_dir.join(format!("stages/sealed-table-{}/authority.json", index / 2));
        let table_doc: Value = serde_json::from_slice(&fs::read(&table_authority).map_err(|_| Fail::new("consumer.request", 3))?).map_err(|_| Fail::new("consumer.request", 3))?;
        let table_policy = read_policy(&table_doc)?;
        if policy_dataset(&table_policy)?.as_str() != dataset || bind_split(&read_bind(&fs::read(&table_authority).map_err(|_| Fail::new("consumer.request", 3))?)?) {
            return Err(Fail::new("consumer.request", 3));
        }
        policies.push(table_policy);
        authorities.push(table_authority);
    }
    let login = policies.first().and_then(|policy| policy.get("login")).and_then(Value::as_str).ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let slug = policies.first().and_then(|policy| policy.get("role")).and_then(Value::as_str).ok_or(Fail::new("consumer.request", 3))?;
    if slug.is_empty() || !slug.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
        return Err(Fail::new("consumer.request", 3));
    }
    let role_name = format!("oo_r_{slug}");
    let acl_dir = dir.join("acl-set");
    fs::create_dir_all(&acl_dir).map_err(|_| Fail::new("config.read", 1))?;
    let mut installed = install_reader_set(preview_bin, &acl_dir, &policies, target)?;
    if installed.status != 0 && acl_mismatch(&acl_dir) {
        let dropped = drop_partial_reader(target, &login, &role_name)?;
        fs::write(acl_dir.join("partial-role.txt"), dropped.as_bytes()).map_err(|_| Fail::new("config.read", 1))?;
        let retry = dir.join("acl-set-retry");
        fs::create_dir_all(&retry).map_err(|_| Fail::new("config.read", 1))?;
        installed = install_reader_set(preview_bin, &retry, &policies, target)?;
    }
    if installed.status != 0 { return Err(Fail::new("consumer.acl", 7)); }
    suspend_pipeline_inherit(target, &login)?;
    if provision_reader_set(preview_bin, &acl_dir, &policies, target)?.status != 0 { return Err(Fail::new("consumer.acl", 7)); }
    let mut exits = Vec::new();
    for (i, (dataset, sql)) in queries.iter().enumerate() {
        let name = format!("table-{i}");
        let query = dir.join(format!("{name}.json"));
        fs::write(&query, serde_json::to_vec(&json!({"protocol": "oo.operator-preview.query/v1", "sql": sql,
            "max_rows": 32, "max_result_bytes": 65536, "wall_clock_ms": 30000})).map_err(|_| Fail::new("config.read", 1))?).map_err(|_| Fail::new("config.read", 1))?;
        let table_authority = &authorities[i / 2];
        let isolated = isolated_authority(table_authority, &dir, &name)?;
        let exit = invoke(&binary, &dir, &name, &["query", "--authority", path_str(&isolated), "--query", path_str(&query)])?;
        exits.push(json!({"dataset": dataset, "sql": sql, "exit": exit}));
        if exit != 0 { write_result(&dir, &json!({"queries": exits, "complete": false}))?; return Err(Fail::new("consumer.table", 8)); }
    }
    write_result(&dir, &json!({"queries": exits, "complete": true, "oracle_verified": false,
        "source_open": false, "binary_sha256": CONSUMER_SHA256}))
}

/// The sealed policy role must be the caller pack role before any install.
/// A logistics config leaves `pack_role` empty and skips this check.
pub fn pack_slug_matches(out_dir: &Path, pack_role: &str) -> Result<(), Fail> {
    if pack_role.is_empty() {
        return Ok(());
    }
    let Some(expected) = pack_role.strip_prefix("oo_r_") else {
        return Err(Fail::new("consumer.acl", 7));
    };
    let policy: Value = serde_json::from_slice(
        &fs::read(out_dir.join("stages/sealed-table-0/policy.json")).map_err(|_| Fail::new("consumer.request", 3))?,
    )
    .map_err(|_| Fail::new("consumer.request", 3))?;
    if policy.get("role").and_then(Value::as_str) != Some(expected) {
        return Err(Fail::new("consumer.acl", 7));
    }
    Ok(())
}

/// Re-run denial, widen, inflight cancel, and reopen on the role already installed.
/// This does not call install-column-acl and does not drop a role.
pub fn replay_installed_role(out_dir: &Path, preview_bin: &Path, source: &EndpointFile, target: &EndpointFile) -> Result<(), Fail> {
    if postgres_open(source) {
        return Err(Fail::new("consumer.source_open", 8));
    }
    let binary = bytea_binary(preview_bin);
    let digest = sha256_bytes(&fs::read(&binary).map_err(|_| Fail::new("consumer.binary", 3))?);
    if digest != CONSUMER_SHA256 {
        return Err(Fail::new("consumer.binary", 3));
    }
    let authority = out_dir.join("stages/sealed-table-0/authority.json");
    let authority_doc: Value = serde_json::from_slice(&fs::read(&authority).map_err(|_| Fail::new("consumer.request", 3))?)
        .map_err(|_| Fail::new("consumer.request", 3))?;
    let policy = read_policy(&authority_doc)?;
    let schema = policy.pointer("/table/namespace").and_then(Value::as_str).ok_or(Fail::new("consumer.request", 3))?;
    let table = policy.pointer("/table/name").and_then(Value::as_str).ok_or(Fail::new("consumer.request", 3))?;
    let columns = policy.pointer("/table/columns").and_then(Value::as_array).ok_or(Fail::new("consumer.request", 3))?;
    let names: Vec<&str> = columns.iter().filter_map(|column| column.get("name").and_then(Value::as_str)).collect();
    let first = names.first().copied().ok_or(Fail::new("consumer.request", 3))?;
    let settings = read_doc(&out_dir.join("stages/settings.json"))?;
    let catalog = read_doc(&out_dir.join("stages/catalog-import.stdout"))?;
    let receipt_doc = read_doc(&out_dir.join("stages/sealed-table-0/pack/receipt.json"))?;
    let seal = compile_seal(&authority_doc, &settings, &catalog, &receipt_doc, &policy)?;
    if schema != seal.schema || table != seal.table {
        return Err(Fail::new("consumer.request", 3));
    }
    let declared_receipt = authority_doc.get("pack_receipt").and_then(Value::as_str).unwrap_or("");
    if !declared_receipt.ends_with("/sealed-table-0/pack/receipt.json") {
        return Err(Fail::new("consumer.request", 3));
    }
    let role_name = seal.role.clone();
    let bound = crate::pg::bind_current_generation(
        super_endpoint(target)?,
        seal.source_id.clone(),
        seal.config_digest.clone(),
        seal.generation,
        seal.schema.clone(),
        seal.table.clone(),
        role_name.clone(),
    )?;
    let installed = receipt_physicals(out_dir);
    if !installed.contains(&bound.physical) {
        return Err(Fail::new("consumer.request", 3));
    }
    let physical = bound.physical.clone();
    let before = crate::pg::role_facts(super_endpoint(target)?, role_name.clone())?;
    let dir = out_dir.join("consumer-role-replay");
    fs::create_dir_all(&dir).map_err(|_| Fail::new("config.read", 1))?;
    let _ = fs::remove_file(dir.join("result.json"));
    if names.contains(&"secret_note") {
        return Err(Fail::new("consumer.request", 3));
    }
    let hidden_column = "secret_note";
    let hidden_sql = format!("SELECT {} FROM {}.{} LIMIT 1", quote_ident(hidden_column)?, quote_ident(schema)?, quote_ident(table)?);
    let object_sql = format!("SELECT {} FROM {}.{} LIMIT 1", quote_ident(first)?, quote_ident(schema)?, quote_ident("not_approved")?);
    let allowed_sql = format!(
        "SELECT {} FROM {}.{} ORDER BY {} LIMIT 1",
        quote_ident(first)?,
        quote_ident(schema)?,
        quote_ident(table)?,
        quote_ident(first)?
    );
    let hidden = dir.join("hidden.json");
    let object = dir.join("object.json");
    let query = dir.join("query.json");
    write_query(&hidden, &hidden_sql, 30_000)?;
    write_query(&object, &object_sql, 30_000)?;
    write_query(&query, &allowed_sql, 30_000)?;
    let hidden_authority = isolated_authority(&authority, &dir, "hidden")?;
    let object_authority = isolated_authority(&authority, &dir, "object")?;
    let hidden_exit = invoke(&binary, &dir, "hidden", &["query", "--authority", path_str(&hidden_authority), "--query", path_str(&hidden)])?;
    let hidden_doc = read_stdout(&dir, "hidden")?;
    let object_exit = invoke(&binary, &dir, "object", &["query", "--authority", path_str(&object_authority), "--query", path_str(&object)])?;
    let object_doc = read_stdout(&dir, "object")?;
    let wide_policy = dir.join("policy-wide.json");
    let wide_authority = dir.join("authority-wide.json");
    write_wide(&authority_doc, &wide_policy, &wide_authority)?;
    let wide = invoke(&binary, &dir, "widen", &["query", "--authority", path_str(&wide_authority), "--query", path_str(&query)])?;
    let wide_doc = read_stdout(&dir, "widen")?;
    let state_dir = authority_doc.get("state_dir").and_then(Value::as_str).unwrap_or("");
    let (cancel, inflight, rollback) = inflight_cancel(&binary, &dir, &authority, Path::new(state_dir), target, &physical, &allowed_sql)?;
    let cancel_doc = read_stdout(&dir, "cancel-query")?;
    let reopen_authority = isolated_authority(&authority, &dir, "reopen")?;
    let reopen = invoke(&binary, &dir, "reopen", &["query", "--authority", path_str(&reopen_authority), "--query", path_str(&query)])?;
    let reopen_doc = read_stdout(&dir, "reopen")?;
    let after = crate::pg::role_facts(super_endpoint(target)?, role_name.clone())?;
    let hidden_scope = guard_scope(&hidden_doc, &seal, hidden_column);
    let approved_relation = format!("\"{}\".\"{table}\"", seal.schema);
    let object_scope = guard_scope(&object_doc, &seal, hidden_column)
        && object_sql.contains("\"not_approved\"")
        && !object_sql.contains(&approved_relation);
    let widen_scope = pack_mismatch(&wide_doc, &seal);
    let cancel_scope = canceled_raw(&cancel_doc, &seal);
    let reopen_scope = succeeded_raw(&reopen_doc, &seal);
    let grants_held = before.exists && after.exists && before.grants == after.grants && before.grants > 0;
    let exits_held = hidden_exit == 6 && object_exit == 6 && wide == 9 && inflight == "user_cancel" && cancel == 0 && rollback && reopen == 0;
    let stderr_held = stderr_line(&dir, "hidden", "operator-preview: guard")
        && stderr_line(&dir, "object", "operator-preview: guard")
        && stderr_line(&dir, "widen", "operator-preview: pack");
    if hidden_scope && object_scope && widen_scope && cancel_scope && reopen_scope && grants_held && exits_held && stderr_held {
        write_result(&dir, &json!({
            "protocol": "oo.postgres-preview.role-replay/v1",
            "install_called": false,
            "dataset": format!("{schema}.{table}"),
            "role": role_name,
            "physical_table": physical,
            "snapshot_id": seal.snapshot_id,
            "import_hash": seal.import_hash,
            "import_generation": seal.generation,
            "pack_digest": seal.pack_digest,
            "knowledge_id": seal.knowledge_id,
            "policy_pack_id": seal.policy_pack_id,
            "config_digest": seal.config_digest,
            "max_generation": bound.max_generation,
            "retained_physical_tables": bound.retained,
            "retained_role_grants": 0,
            "hidden_class": "guard",
            "hidden_column": hidden_column,
            "hidden_column_in_scope": false,
            "object_class": "guard",
            "widen_class": "pack_mismatch",
            "source_open": false,
            "binary_sha256": digest,
            "role_exists_before": before.exists,
            "role_grants_before": before.grants,
            "role_exists_after": after.exists,
            "role_grants_after": after.grants,
            "hidden_exit": hidden_exit,
            "object_exit": object_exit,
            "widen_exit": wide,
            "cancel_exit": cancel,
            "inflight_57014": inflight,
            "rollback_confirmed": rollback,
            "reopen_exit": reopen
        }))?;
        return Ok(());
    }
    Err(Fail::new("consumer.pending", 8))
}

struct Seal {
    source_id: String,
    config_digest: String,
    snapshot_id: String,
    import_hash: String,
    generation: i64,
    pack_digest: String,
    knowledge_id: String,
    knowledge_version: String,
    policy_pack_id: String,
    role: String,
    schema: String,
    table: String,
}

fn read_doc(path: &Path) -> Result<Value, Fail> {
    serde_json::from_slice(&fs::read(path).map_err(|_| Fail::new("consumer.request", 3))?).map_err(|_| Fail::new("consumer.request", 3))
}

fn field<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).filter(|text| !text.is_empty())
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn compile_seal(authority: &Value, settings: &Value, catalog: &Value, receipt: &Value, policy: &Value) -> Result<Seal, Fail> {
    let source_id = field(catalog, "/source_id").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let snapshot_id = field(catalog, "/run_id").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let generation = catalog.get("source_generation").and_then(Value::as_i64).filter(|value| *value > 0).ok_or(Fail::new("consumer.request", 3))?;
    let import_hash = field(authority, "/import/import_hash").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let pack_digest = field(receipt, "/pack_digest").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let config_digest = field(catalog, "/config_digest").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let knowledge_id = field(settings, "/knowledge/id").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let knowledge_version = field(settings, "/knowledge/version").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let policy_pack_id = field(settings, "/policy/id").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let policy_version = field(settings, "/policy/version").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let slug = field(policy, "/role").ok_or(Fail::new("consumer.request", 3))?;
    let role = format!("oo_r_{slug}");
    let schema = field(policy, "/table/namespace").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let table = field(policy, "/table/name").ok_or(Fail::new("consumer.request", 3))?.to_owned();
    let generation_text = generation.to_string();
    let same = field(authority, "/project") == Some(source_id.as_str())
        && field(authority, "/import/source_id") == Some(source_id.as_str())
        && field(receipt, "/binding/source_id") == Some(source_id.as_str())
        && field(authority, "/import/snapshot_id") == Some(snapshot_id.as_str())
        && field(receipt, "/binding/snapshot_id") == Some(snapshot_id.as_str())
        && authority.pointer("/import/import_generation").and_then(Value::as_i64) == Some(generation)
        && receipt.pointer("/binding/import_generation").and_then(Value::as_i64) == Some(generation)
        && policy.get("generation").and_then(Value::as_i64) == Some(generation)
        && field(receipt, "/binding/import_hash") == Some(import_hash.as_str())
        && field(authority, "/import/target_config_sha256") == Some(config_digest.as_str())
        && field(receipt, "/binding/target_config_sha256") == Some(config_digest.as_str())
        && field(settings, "/catalog_version") == Some(config_digest.as_str())
        && field(authority, "/knowledge/id") == Some(knowledge_id.as_str())
        && field(authority, "/knowledge/version") == Some(knowledge_version.as_str())
        && field(authority, "/policy_pack_id") == Some(policy_pack_id.as_str())
        && field(authority, "/policy_version") == Some(policy_version.as_str())
        && field(authority, "/role") == Some(role.as_str())
        && knowledge_version == generation_text
        && policy_version == generation_text
        && catalog.get("state").and_then(Value::as_str) == Some("awaiting_publish")
        && hex64(&import_hash)
        && hex64(&pack_digest)
        && hex64(&config_digest);
    if !same {
        return Err(Fail::new("consumer.request", 3));
    }
    Ok(Seal {
        source_id,
        config_digest,
        snapshot_id,
        import_hash,
        generation,
        pack_digest,
        knowledge_id,
        knowledge_version,
        policy_pack_id,
        role,
        schema,
        table,
    })
}

fn import_bound(doc: &Value, seal: &Seal) -> bool {
    field(doc, "/import/snapshot_id") == Some(seal.snapshot_id.as_str())
        && field(doc, "/import/import_hash") == Some(seal.import_hash.as_str())
        && doc.pointer("/import/import_generation").and_then(Value::as_i64) == Some(seal.generation)
        && field(doc, "/import/source_id") == Some(seal.source_id.as_str())
        && field(doc, "/import/target_config_sha256") == Some(seal.config_digest.as_str())
        && field(doc, "/policy/role") == Some(seal.role.as_str())
        && field(doc, "/policy/policy_pack_id") == Some(seal.policy_pack_id.as_str())
        && field(doc, "/knowledge/id") == Some(seal.knowledge_id.as_str())
        && field(doc, "/knowledge/version") == Some(seal.knowledge_version.as_str())
}

fn column_outside(doc: &Value, column: &str) -> bool {
    doc.pointer("/policy/columns").and_then(Value::as_array).is_some_and(|columns| {
        !columns.is_empty() && columns.iter().all(|item| item.as_str() != Some(column))
    })
}

fn guard_scope(doc: &Value, seal: &Seal, column: &str) -> bool {
    import_bound(doc, seal)
        && field(doc, "/knowledge/pack_digest") == Some(seal.pack_digest.as_str())
        && doc.get("class").and_then(Value::as_str) == Some("guard")
        && doc.get("outcome").and_then(Value::as_str) == Some("refused")
        && column_outside(doc, column)
}

fn pack_mismatch(doc: &Value, seal: &Seal) -> bool {
    import_bound(doc, seal)
        && doc.get("class").and_then(Value::as_str) == Some("pack_mismatch")
        && doc.get("outcome").and_then(Value::as_str) == Some("refused")
        && matches!(doc.pointer("/knowledge/pack_digest"), None | Some(Value::Null))
}

fn succeeded_raw(doc: &Value, seal: &Seal) -> bool {
    import_bound(doc, seal)
        && field(doc, "/knowledge/pack_digest") == Some(seal.pack_digest.as_str())
        && doc.get("outcome").and_then(Value::as_str) == Some("succeeded")
}

fn canceled_raw(doc: &Value, seal: &Seal) -> bool {
    import_bound(doc, seal)
        && field(doc, "/knowledge/pack_digest") == Some(seal.pack_digest.as_str())
        && doc.get("outcome").and_then(Value::as_str) == Some("canceled")
        && doc.get("sqlstate").and_then(Value::as_str) == Some("57014")
        && doc.get("rollback_confirmed") == Some(&Value::Bool(true))
}

fn read_stdout(dir: &Path, name: &str) -> Result<Value, Fail> {
    read_doc(&dir.join(format!("{name}.stdout")))
}

fn stderr_line(dir: &Path, name: &str, token: &str) -> bool {
    fs::read_to_string(dir.join(format!("{name}.stderr"))).is_ok_and(|text| text.trim() == token)
}

fn quote_ident(name: &str) -> Result<String, Fail> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(Fail::new("consumer.request", 3));
    }
    Ok(format!("\"{name}\""))
}

fn receipt_physicals(out_dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for rel in ["consumer-all/acl-set-retry/install-column-acl.stdout", "consumer-all/acl-set/install-column-acl.stdout"] {
        let Ok(bytes) = fs::read(out_dir.join(rel)) else { continue };
        let Ok(doc) = serde_json::from_slice::<Value>(&bytes) else { continue };
        let effect = doc.get("effect").and_then(Value::as_str).unwrap_or("");
        if effect != "installed" && effect != "replay" {
            continue;
        }
        let Some(tables) = doc.get("tables").and_then(Value::as_array) else { continue };
        for table in tables {
            if let Some(name) = table.get("physical_table").and_then(Value::as_str) {
                found.push(name.to_owned());
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    found
}

pub async fn capture_table_oracle(out_dir: &Path, source: &EndpointFile) -> Result<(), Fail> {
    let catalog: Value = serde_json::from_slice(&fs::read(out_dir.join("stages/catalog-import.stdout")).map_err(|_| Fail::new("consumer.request", 3))?).map_err(|_| Fail::new("consumer.request", 3))?;
    let endpoint = endpoint_from_files(&source.host, source.port, &source.database, &source.username, &source.password_file, &source.ca_file)?;
    let session = connect_read_only(&endpoint).await?;
    let mut observations = Vec::new();
    for (dataset, sql) in table_queries(&catalog)? {
        let result = session.client.simple_query(&sql).await.map_err(|_| Fail::new("oracle.source", 8))?;
        let mut rows = Vec::new();
        for item in result {
            if let tokio_postgres::SimpleQueryMessage::Row(row) = item {
                rows.push((0..row.len()).map(|i| row.get(i).map(str::to_owned)).collect::<Vec<_>>());
            }
        }
        observations.push(json!({"dataset": dataset, "sql": sql, "rows": rows}));
    }
    let mut counts = Vec::new();
    for table in catalog.get("tables").and_then(Value::as_array).ok_or(Fail::new("oracle.source", 8))? {
        let schema = table.get("logical_schema").and_then(Value::as_str).ok_or(Fail::new("oracle.source", 8))?;
        let name = table.get("logical_table").and_then(Value::as_str).ok_or(Fail::new("oracle.source", 8))?;
        let columns = table.get("columns").and_then(Value::as_array).ok_or(Fail::new("oracle.source", 8))?;
        let first = columns.first().and_then(|column| column.get("logical_name").and_then(Value::as_str)).ok_or(Fail::new("oracle.source", 8))?;
        let sql = format!(
            "SELECT count(*)::text, count(DISTINCT {})::text FROM {}.{}",
            sql_ident(first)?,
            sql_ident(schema)?,
            sql_ident(name)?
        );
        let result = session.client.simple_query(&sql).await.map_err(|_| Fail::new("oracle.source", 8))?;
        let mut got: Option<(String, String)> = None;
        for item in result {
            if let tokio_postgres::SimpleQueryMessage::Row(row) = item
                && row.len() == 2
            {
                got = Some((row.get(0).unwrap_or("").to_owned(), row.get(1).unwrap_or("").to_owned()));
            }
        }
        let (rows, distinct) = got.ok_or(Fail::new("oracle.source", 8))?;
        counts.push(json!({"dataset": format!("{schema}.{name}"), "rows": rows, "distinct_ids": distinct, "id_column": first}));
    }
    fs::write(out_dir.join("source-table-oracle.json"), serde_json::to_vec_pretty(&json!({"protocol": "oo.postgres-preview.source-table-oracle/v1", "queries": observations})).map_err(|_| Fail::new("config.read", 1))?).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(out_dir.join("source-count.json"), serde_json::to_vec_pretty(&json!({"protocol": "oo.postgres-preview.source-count/v1", "tables": counts})).map_err(|_| Fail::new("config.read", 1))?).map_err(|_| Fail::new("config.read", 1))
}

fn sql_ident(name: &str) -> Result<String, Fail> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(Fail::new("oracle.source", 8));
    }
    Ok(format!("\"{name}\""))
}

pub fn table_queries(catalog: &Value) -> Result<Vec<(String, String)>, Fail> {
    fn ident(s: &str) -> Result<String, Fail> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { return Err(Fail::new("consumer.request", 3)); }
        Ok(format!("\"{s}\""))
    }
    let mut queries = Vec::new();
    for table in catalog.get("tables").and_then(Value::as_array).ok_or(Fail::new("consumer.request", 3))? {
        let schema = table.get("logical_schema").and_then(Value::as_str).ok_or(Fail::new("consumer.request", 3))?;
        let name = table.get("logical_table").and_then(Value::as_str).ok_or(Fail::new("consumer.request", 3))?;
        let cols: Vec<String> = table.get("columns").and_then(Value::as_array).ok_or(Fail::new("consumer.request", 3))?.iter().map(|c| ident(c.get("logical_name").and_then(Value::as_str).unwrap_or(""))).collect::<Result<_, _>>()?;
        let first = cols.first().ok_or(Fail::new("consumer.request", 3))?;
        for order in ["ASC", "DESC"] {
            queries.push((format!("{schema}.{name}"), format!("SELECT {} FROM {}.{} ORDER BY {first} {order} LIMIT 25", cols.join(", "), ident(schema)?, ident(name)?)));
        }
    }
    if queries.is_empty() { return Err(Fail::new("consumer.request", 3)); }
    Ok(queries)
}

fn inflight_cancel(
    binary: &Path,
    dir: &Path,
    authority: &Path,
    state_dir: &Path,
    target: &EndpointFile,
    physical: &str,
    sql: &str,
) -> Result<(i32, &'static str, bool), Fail> {
    let Some(parent) = Path::new(&target.password_file).parent() else {
        return Ok((-1, "not_started", false));
    };
    let super_file = parent.join("super");
    let Some(super_path) = super_file.to_str() else {
        return Ok((-1, "not_started", false));
    };
    let endpoint = endpoint_from_files(&target.host, target.port, &target.database, "postgres", super_path, &target.ca_file)?;
    let lock = match hold_mapped_table(endpoint.clone(), physical.to_owned()) {
        Ok(lock) => lock,
        Err(_) => return Ok((-1, "not_started", false)),
    };
    let cancel_query = dir.join("cancel-query.json");
    write_query(&cancel_query, sql, 30_000)?;
    let cancel_authority = isolated_authority(authority, dir, "cancel")?;
    let child = spawn_binary(
        binary,
        &[
            "query".to_owned(),
            "--authority".to_owned(),
            path_str(&cancel_authority).to_owned(),
            "--query".to_owned(),
            path_str(&cancel_query).to_owned(),
        ],
    )?;
    let running = poll_running(state_dir, 200);
    let active = running.as_ref().is_some_and(|(_, pid)| poll_active(&endpoint, *pid));
    let cancel = if active {
        if let Some((id, _)) = running.as_ref() {
            invoke(binary, dir, "cancel", &["cancel", "--authority", path_str(&cancel_authority), "--execution", id])?
        } else {
            -1
        }
    } else {
        -1
    };
    let finished = if cancel == 0 {
        let finished = finish_child(child)?;
        drop(lock);
        finished
    } else {
        drop(lock);
        finish_child(child)?
    };
    fs::write(dir.join("cancel-query.stdout"), &finished.stdout).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("cancel-query.stderr"), &finished.stderr).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("cancel-query.exit"), finished.status.to_string().as_bytes()).map_err(|_| Fail::new("config.read", 1))?;
    let cancel_doc = json_value(&fs::read(dir.join("cancel.stdout")).unwrap_or_default());
    let query_doc = json_value(&finished.stdout);
    let sent = cancel_doc.get("outcome").and_then(Value::as_str) == Some("sent");
    let canceled = query_doc.get("outcome").and_then(Value::as_str) == Some("canceled");
    let sqlstate = query_doc.get("sqlstate").and_then(Value::as_str) == Some("57014");
    let rollback = query_doc.get("rollback_confirmed") == Some(&Value::Bool(true));
    let status = running
        .as_ref()
        .and_then(|(id, _)| execution_status(state_dir, id))
        .unwrap_or_default();
    let inflight = if cancel == 0 && sent && canceled && sqlstate && rollback && status == "canceled" {
        "user_cancel"
    } else if running.is_some() {
        "incomplete"
    } else {
        "not_started"
    };
    Ok((cancel, inflight, rollback))
}

fn poll_running(state_dir: &Path, attempts: u32) -> Option<(String, i32)> {
    for _ in 0..attempts {
        if let Some(found) = running_execution(state_dir) {
            return Some(found);
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

fn poll_active(endpoint: &crate::pg::Endpoint, pid: i32) -> bool {
    for _ in 0..40 {
        if backend_active(endpoint.clone(), pid).unwrap_or(false) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn running_execution(state_dir: &Path) -> Option<(String, i32)> {
    let entries = fs::read_dir(state_dir.join("executions")).ok()?;
    for entry in entries.flatten() {
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if value.get("status").and_then(Value::as_str) != Some("running") {
            continue;
        }
        let Some(id) = value.get("execution_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(pid) = value.get("backend_pid").and_then(Value::as_i64) else {
            continue;
        };
        let Ok(pid) = i32::try_from(pid) else {
            continue;
        };
        if entry.file_name().to_string_lossy().contains(id) {
            return Some((id.to_owned(), pid));
        }
    }
    None
}

fn execution_status(state_dir: &Path, id: &str) -> Option<String> {
    let bytes = fs::read(state_dir.join("executions").join(format!("{id}.json"))).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.get("status").and_then(Value::as_str).map(str::to_owned)
}

fn json_value(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

struct AttachFacts<'a> {
    split: bool,
    source_open: bool,
    oracle_literals: bool,
    inflight: &'a str,
    rollback: bool,
    describe: i32,
    queried: i32,
    hidden_exit: i32,
    wide: i32,
    cancel: i32,
    reopen: i32,
    install_exit: i32,
    install_effect: &'a str,
    provision_exit: i32,
    provision_effect: &'a str,
    reason: &'a str,
}

fn result_body(bind: &Bind, digest: &str, facts: &AttachFacts<'_>) -> Value {
    json!({
        "protocol": "oo.postgres-preview.consumer-attach/v1",
        "consumer_commit": CONSUMER_COMMIT,
        "binary_sha256": digest,
        "called": !facts.source_open,
        "model_called": false,
        "source_captured": false,
        "project": bind.project,
        "source_id": bind.source_id,
        "bind_split": facts.split,
        "source_open": facts.source_open,
        "oracle_literals": facts.oracle_literals,
        "inflight_57014": facts.inflight,
        "rollback_confirmed": facts.rollback,
        "describe_exit": facts.describe,
        "query_exit": facts.queried,
        "hidden_exit": facts.hidden_exit,
        "widen_exit": facts.wide,
        "cancel_exit": facts.cancel,
        "reopen_exit": facts.reopen,
        "install_exit": facts.install_exit,
        "install_effect": facts.install_effect,
        "install_commit": ACL_COMMIT,
        "provision_exit": facts.provision_exit,
        "provision_effect": facts.provision_effect,
        "reason": facts.reason
    })
}

fn write_result(dir: &Path, body: &Value) -> Result<(), Fail> {
    fs::write(dir.join("result.json"), serde_json::to_vec_pretty(body).map_err(|_| Fail::new("config.read", 1))?)
        .map_err(|_| Fail::new("config.read", 1))
}

fn saturate_exit(status: i32) -> u8 {
    if (1..256).contains(&status) { status as u8 } else { 1 }
}

fn oracle_literals_present(stdout: &[u8]) -> bool {
    stdout.windows(36).any(|item| item == b"00000001-0000-4000-8000-000000000001")
        && stdout.windows(36).any(|item| item == b"00000007-0000-4000-8000-000000000007")
        && stdout.windows(20).any(|item| item == b"-9223372036854775808")
}

fn isolated_authority(authority: &Path, dir: &Path, name: &str) -> Result<PathBuf, Fail> {
    let mut doc: Value = serde_json::from_slice(&fs::read(authority).map_err(|_| Fail::new("consumer.request", 3))?)
        .map_err(|_| Fail::new("consumer.request", 3))?;
    let audit = dir.join(format!("audit-{name}"));
    if audit.exists() {
        fs::remove_dir_all(&audit).map_err(|_| Fail::new("config.read", 1))?;
    }
    fs::create_dir_all(&audit).map_err(|_| Fail::new("config.read", 1))?;
    let audit_text = audit.to_str().ok_or(Fail::new("consumer.request", 3))?;
    doc["audit_dir"] = json!(audit_text);
    let path = dir.join(format!("authority-{name}.json"));
    fs::write(&path, serde_json::to_vec(&doc).map_err(|_| Fail::new("config.read", 1))?)
        .map_err(|_| Fail::new("config.read", 1))?;
    Ok(path)
}

fn bytea_binary(preview_bin: &Path) -> PathBuf {
    preview_bin
        .parent()
        .map(|dir| dir.join("operator-preview-bytea"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| preview_bin.to_path_buf())
}

fn write_query(path: &Path, sql: &str, wall_clock_ms: u64) -> Result<(), Fail> {
    let body = json!({
        "protocol": "oo.operator-preview.query/v1",
        "sql": sql,
        "max_rows": 10,
        "max_result_bytes": 4096,
        "wall_clock_ms": wall_clock_ms
    });
    fs::write(path, serde_json::to_vec(&body).map_err(|_| Fail::new("config.read", 1))?).map_err(|_| Fail::new("config.read", 1))
}

fn write_wide(authority: &Value, policy_path: &Path, authority_path: &Path) -> Result<(), Fail> {
    let policy_file = authority.get("policy_path").and_then(Value::as_str).unwrap_or("");
    let mut policy: Value = serde_json::from_slice(&fs::read(policy_file).map_err(|_| Fail::new("consumer.request", 3))?)
        .map_err(|_| Fail::new("consumer.request", 3))?;
    let columns = policy
        .pointer_mut("/table/columns")
        .and_then(Value::as_array_mut)
        .ok_or(Fail::new("consumer.request", 3))?;
    columns.push(json!({
        "name": "secret_note",
        "pii": {"class": "none"},
        "semantic_type": "free_text",
        "status": "confirmed"
    }));
    fs::write(policy_path, serde_json::to_vec(&policy).map_err(|_| Fail::new("config.read", 1))?)
        .map_err(|_| Fail::new("config.read", 1))?;
    let mut wide = authority.clone();
    if let Some(path) = policy_path.to_str() {
        wide["policy_path"] = json!(path);
    }
    fs::write(authority_path, serde_json::to_vec(&wide).map_err(|_| Fail::new("config.read", 1))?)
        .map_err(|_| Fail::new("config.read", 1))
}

fn invoke(binary: &Path, dir: &Path, name: &str, args: &[&str]) -> Result<i32, Fail> {
    let owned: Vec<String> = args.iter().map(|item| (*item).to_owned()).collect();
    let spawned = run_binary(binary, &owned, b"")?;
    fs::write(dir.join(format!("{name}.stdout")), &spawned.stdout).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join(format!("{name}.stderr")), &spawned.stderr).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join(format!("{name}.exit")), spawned.status.to_string().as_bytes()).map_err(|_| Fail::new("config.read", 1))?;
    Ok(spawned.status)
}

fn postgres_open(source: &EndpointFile) -> bool {
    let Ok(endpoint) = endpoint_from_files(
        &source.host,
        source.port,
        &source.database,
        &source.username,
        &source.password_file,
        &source.ca_file,
    ) else {
        return false;
    };
    thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() else {
            return false;
        };
        runtime.block_on(async move { connect_read_only(&endpoint).await.is_ok() })
    })
    .join()
    .unwrap_or(false)
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap_or("")
}

struct InstalledRole {
    status: i32,
    effect: String,
    physical: String,
}

struct ReaderStep {
    status: i32,
    effect: String,
}

fn install_reader_set(preview_bin: &Path, dir: &Path, policies: &[Value], target: &EndpointFile) -> Result<InstalledRole, Fail> {
    let binary = sibling_binary(preview_bin, "install-column-acl")?;
    let digest = sha256_bytes(&fs::read(&binary).map_err(|_| Fail::new("consumer.binary", 3))?);
    if digest != ACL_SHA256 {
        return Err(Fail::new("consumer.binary", 3));
    }
    let document = column_acl_set_document(policies, target)?;
    fs::write(dir.join("install-column-acl.request.json"), &document).map_err(|_| Fail::new("config.read", 1))?;
    let spawned = run_binary(&binary, &[], &document)?;
    fs::write(dir.join("install-column-acl.stdout"), &spawned.stdout).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("install-column-acl.stderr"), &spawned.stderr).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("install-column-acl.exit"), spawned.status.to_string().as_bytes()).map_err(|_| Fail::new("config.read", 1))?;
    let body = json_value(&spawned.stdout);
    let effect = body.get("effect").and_then(Value::as_str).unwrap_or("").to_owned();
    let role = body.get("role").and_then(Value::as_str).unwrap_or("");
    let physical = body.pointer("/tables/0/physical_table").and_then(Value::as_str).unwrap_or("").to_owned();
    let expected = db_role_name(policies)?;
    let status = if spawned.status == 0 && role != expected { 7 } else { spawned.status };
    Ok(InstalledRole { status, effect, physical })
}

fn install_reader(preview_bin: &Path, dir: &Path, authority: &Value, target: &EndpointFile) -> Result<InstalledRole, Fail> {
    let policy = read_policy(authority)?;
    install_reader_set(preview_bin, dir, std::slice::from_ref(&policy), target)
}

fn provision_reader_set(preview_bin: &Path, dir: &Path, policies: &[Value], target: &EndpointFile) -> Result<ReaderStep, Fail> {
    let binary = sibling_binary(preview_bin, "provision-mapping-reader")?;
    let digest = sha256_bytes(&fs::read(&binary).map_err(|_| Fail::new("consumer.binary", 3))?);
    if digest != PROVISION_SHA256 {
        return Err(Fail::new("consumer.binary", 3));
    }
    let login = policies.first().and_then(|policy| policy.get("login")).and_then(Value::as_str).unwrap_or("");
    let document = mapping_reader_set_document(policies, target, login)?;
    fs::write(dir.join("provision-mapping-reader.request.json"), &document).map_err(|_| Fail::new("config.read", 1))?;
    let spawned = run_binary(&binary, &[], &document)?;
    fs::write(dir.join("provision-mapping-reader.stdout"), &spawned.stdout).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("provision-mapping-reader.stderr"), &spawned.stderr).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("provision-mapping-reader.exit"), spawned.status.to_string().as_bytes()).map_err(|_| Fail::new("config.read", 1))?;
    let body = json_value(&spawned.stdout);
    let effect = body.get("effect").and_then(Value::as_str).unwrap_or("").to_owned();
    let role = body.get("role").and_then(Value::as_str).unwrap_or("");
    let granted = body.get("login").and_then(Value::as_str).unwrap_or("");
    let expected = db_role_name(policies)?;
    let status = if spawned.status == 0 && (role != expected || granted != login) { 7 } else { spawned.status };
    Ok(ReaderStep { status, effect })
}

fn db_role_name(policies: &[Value]) -> Result<String, Fail> {
    let slug = policies.first().and_then(|policy| policy.get("role")).and_then(Value::as_str).unwrap_or("");
    if slug.is_empty()
        || slug.starts_with("oo_r_")
        || !slug.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(Fail::new("consumer.request", 3));
    }
    Ok(format!("oo_r_{slug}"))
}

fn provision_reader(preview_bin: &Path, dir: &Path, authority: &Value, target: &EndpointFile) -> Result<ReaderStep, Fail> {
    let binary = sibling_binary(preview_bin, "provision-mapping-reader")?;
    let digest = sha256_bytes(&fs::read(&binary).map_err(|_| Fail::new("consumer.binary", 3))?);
    if digest != PROVISION_SHA256 {
        return Err(Fail::new("consumer.binary", 3));
    }
    let policy = read_policy(authority)?;
    let login = policy.get("login").and_then(Value::as_str).unwrap_or("");
    let document = mapping_reader_document(&policy, target, login)?;
    fs::write(dir.join("provision-mapping-reader.request.json"), &document).map_err(|_| Fail::new("config.read", 1))?;
    let spawned = run_binary(&binary, &[], &document)?;
    fs::write(dir.join("provision-mapping-reader.stdout"), &spawned.stdout).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("provision-mapping-reader.stderr"), &spawned.stderr).map_err(|_| Fail::new("config.read", 1))?;
    fs::write(dir.join("provision-mapping-reader.exit"), spawned.status.to_string().as_bytes()).map_err(|_| Fail::new("config.read", 1))?;
    let body = json_value(&spawned.stdout);
    let effect = body.get("effect").and_then(Value::as_str).unwrap_or("").to_owned();
    let role = body.get("role").and_then(Value::as_str).unwrap_or("");
    let granted = body.get("login").and_then(Value::as_str).unwrap_or("");
    let expected = db_role_name(std::slice::from_ref(&policy))?;
    let status = if spawned.status == 0 && (role != expected || granted != login) { 7 } else { spawned.status };
    Ok(ReaderStep { status, effect })
}

fn sibling_binary(preview_bin: &Path, name: &str) -> Result<PathBuf, Fail> {
    preview_bin.parent().map(|parent| parent.join(name)).ok_or(Fail::new("consumer.binary", 3))
}

fn suspend_pipeline_inherit(target: &EndpointFile, login: &str) -> Result<&'static str, Fail> {
    let endpoint = super_endpoint(target)?;
    let login = login.to_owned();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| Fail::new("connect", 4))?;
        runtime.block_on(crate::pg::suspend_runtime_inherit(&endpoint, &login))
    })
    .join()
    .map_err(|_| Fail::new("connect", 4))?
}

fn drop_partial_reader(target: &EndpointFile, login: &str, role: &str) -> Result<&'static str, Fail> {
    let endpoint = super_endpoint(target)?;
    let login = login.to_owned();
    let role = role.to_owned();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| Fail::new("connect", 4))?;
        runtime.block_on(crate::pg::drop_partial_column_reader(&endpoint, &login, &role))
    })
    .join()
    .map_err(|_| Fail::new("connect", 4))?
}

/// Only a grant mismatch may replace the product reader role. A type, filter,
/// or identity refusal must leave that role in place.
fn acl_mismatch(dir: &Path) -> bool {
    fs::read(dir.join("install-column-acl.stderr"))
        .ok()
        .is_some_and(|bytes| String::from_utf8_lossy(&bytes).contains("mismatch"))
}

fn super_endpoint(target: &EndpointFile) -> Result<crate::pg::Endpoint, Fail> {
    let parent = Path::new(&target.password_file).parent().ok_or(Fail::new("consumer.request", 3))?;
    let super_path = parent.join("super");
    let super_path = super_path.to_str().ok_or(Fail::new("consumer.request", 3))?;
    endpoint_from_files(&target.host, target.port, &target.database, "postgres", super_path, &target.ca_file)
}

fn read_policy(authority: &Value) -> Result<Value, Fail> {
    let policy_path = authority.get("policy_path").and_then(Value::as_str).ok_or(Fail::new("consumer.request", 3))?;
    serde_json::from_slice(&fs::read(policy_path).map_err(|_| Fail::new("consumer.request", 3))?).map_err(|_| Fail::new("consumer.request", 3))
}

/// The sealed compiler policy names one table. It does not carry a top-level dataset string.
fn policy_dataset(policy: &Value) -> Result<String, Fail> {
    let namespace = policy.pointer("/table/namespace").and_then(Value::as_str).unwrap_or("");
    let name = policy.pointer("/table/name").and_then(Value::as_str).unwrap_or("");
    let ident = |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if !ident(namespace) || !ident(name) {
        return Err(Fail::new("consumer.request", 3));
    }
    Ok(format!("{namespace}.{name}"))
}

fn column_acl_set_document(policies: &[Value], target: &EndpointFile) -> Result<Vec<u8>, Fail> {
    if policies.is_empty() {
        return Err(Fail::new("consumer.request", 3));
    }
    let Some(parent) = Path::new(&target.password_file).parent() else {
        return Err(Fail::new("consumer.request", 3));
    };
    let Some(super_path) = parent.join("super").to_str().map(str::to_owned) else {
        return Err(Fail::new("consumer.request", 3));
    };
    let role = policies[0].get("role").and_then(Value::as_str).unwrap_or("");
    let login = policies[0].get("login").and_then(Value::as_str).unwrap_or("");
    if role.is_empty() || role.starts_with("oo_r_") || login.is_empty() {
        return Err(Fail::new("consumer.request", 3));
    }
    let mut items = Vec::with_capacity(policies.len());
    for policy in policies {
        if policy.get("role").and_then(Value::as_str) != Some(role) || policy.get("login").and_then(Value::as_str) != Some(login) {
            return Err(Fail::new("consumer.request", 3));
        }
        items.push(json!({
            "project": policy.get("project").cloned().unwrap_or(Value::Null),
            "config_digest": policy.get("config_digest").cloned().unwrap_or(Value::Null),
            "generation": policy.get("generation").cloned().unwrap_or(Value::Null),
            "role": role,
            "login": login,
            "table": policy.get("table").cloned().unwrap_or(Value::Null),
            "row_filter": Value::Null
        }));
    }
    let body = json!({
        "protocol": ACL_PROTOCOL,
        "policies": items,
        "endpoint": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "user": "postgres"
        },
        "password_file": super_path,
        "ca_file": target.ca_file
    });
    serde_json::to_vec(&body).map_err(|_| Fail::new("config.read", 1))
}

fn mapping_reader_document(policy: &Value, target: &EndpointFile, login: &str) -> Result<Vec<u8>, Fail> {
    mapping_reader_set_document(std::slice::from_ref(policy), target, login)
}

fn mapping_reader_set_document(policies: &[Value], target: &EndpointFile, login: &str) -> Result<Vec<u8>, Fail> {
    let Some(parent) = Path::new(&target.password_file).parent() else {
        return Err(Fail::new("consumer.request", 3));
    };
    let Some(super_path) = parent.join("super").to_str().map(str::to_owned) else {
        return Err(Fail::new("consumer.request", 3));
    };
    if login.is_empty() || policies.iter().any(|policy| policy.get("login").and_then(Value::as_str) != Some(login)) {
        return Err(Fail::new("consumer.request", 3));
    }
    let policies: Value = serde_json::from_slice(&column_acl_set_document(policies, target)?).map_err(|_| Fail::new("config.read", 1))?;
    let body = json!({
        "protocol": PROVISION_PROTOCOL,
        "policies": policies.get("policies").cloned().unwrap_or(Value::Null),
        "login": login,
        "endpoint": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "user": "postgres"
        },
        "password_file": super_path,
        "ca_file": target.ca_file
    });
    serde_json::to_vec(&body).map_err(|_| Fail::new("config.read", 1))
}

#[cfg(test)]
mod tests {
    use super::{
        bind_split, column_acl_set_document, hidden_sql, read_bind, typed_sql, ACL_COMMIT, ACL_SHA256, CONSUMER_COMMIT,
        CONSUMER_SHA256, PROVISION_SHA256,
    };
    use crate::config::EndpointFile;

    #[test]
    fn generic_customer_queries_keep_all_types_and_reject_identifier_injection() {
        let catalog = serde_json::json!({"tables":[{"logical_schema":"customer_east", "logical_table":"delivery_notes", "columns":[{"logical_name":"note_id","oid":20},{"logical_name":"amount","oid":1700},{"logical_name":"receipt","oid":17}]}]});
        let queries = super::table_queries(&catalog).unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].0, "customer_east.delivery_notes");
        assert!(queries[0].1.contains("\"amount\", \"receipt\""));
        assert!(queries[1].1.contains("DESC LIMIT 25"));
        let mut bad = catalog;
        bad["tables"][0]["logical_table"] = serde_json::json!("notes; DELETE FROM notes");
        assert!(super::table_queries(&bad).is_err());
    }

    #[test]
    fn sealed_project_does_not_match_the_import_source() {
        let bind = read_bind(br#"{"project":"logistics","import":{"source_id":"oo_dl01w1_46"}}"#).expect("bind");
        assert!(bind_split(&bind));
        assert_eq!(bind.project, "logistics");
        assert_eq!(bind.source_id, "oo_dl01w1_46");
        assert!(typed_sql().contains("hub_token"));
        assert!(typed_sql().contains("capacity_grams"));
        assert!(typed_sql().contains("LIMIT 7"));
        assert!(!typed_sql().contains("::"));
        assert!(!typed_sql().contains("pg_sleep"));
        assert!(hidden_sql().contains("secret_note"));
        assert!(!hidden_sql().contains('='));
        assert_eq!(CONSUMER_COMMIT.len(), 40);
        assert_eq!(CONSUMER_SHA256.len(), 64);
        assert_eq!(ACL_COMMIT.len(), 40);
        assert_eq!(ACL_SHA256.len(), 64);
        assert_eq!(PROVISION_SHA256.len(), 64);
        let same = read_bind(br#"{"project":"oo_dl01w1_46","import":{"source_id":"oo_dl01w1_46"}}"#).expect("same");
        assert!(!bind_split(&same));
    }

    #[test]
    fn installer_document_keeps_the_role_slug() {
        let policy = serde_json::json!({
            "protocol": "oo.operator-preview.policy/v1",
            "project": "oo_dl01w1_46",
            "config_digest": "df690a6f697d3f78d95d3bf878df7254e16665a15e0e8407bb3dc64f64368306",
            "generation": 1,
            "role": "reader",
            "login": "oo_dl01w1_46_target",
            "table": {
                "namespace": "public",
                "name": "hubs",
                "columns": [{
                    "name": "hub_id",
                    "pii": {"class": "none"},
                    "semantic_type": "identifier",
                    "status": "confirmed"
                }]
            }
        });
        let target = EndpointFile {
            host: "oo-dl01w1-46-tgt".to_owned(),
            port: 5432,
            database: "dst".to_owned(),
            username: "oo_dl01w1_46_target".to_owned(),
            password_file: "/run/secrets/target".to_owned(),
            ca_file: "/run/secrets/ca.pem".to_owned(),
        };
        let bytes = column_acl_set_document(std::slice::from_ref(&policy), &target).expect("document");
        let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(doc["protocol"], "oo.gateway.install-column-acl-cli/v1");
        assert_eq!(doc["policies"][0]["role"], "reader");
        assert_eq!(doc["policies"][0]["row_filter"], serde_json::Value::Null);
        assert_eq!(doc["endpoint"]["user"], "postgres");
        assert_eq!(doc["password_file"], "/run/secrets/super");
        assert!(doc.get("password").is_none());
        let prefixed = serde_json::json!({"role": "oo_r_reader"});
        assert!(column_acl_set_document(std::slice::from_ref(&prefixed), &target).is_err());
        assert_eq!(super::db_role_name(std::slice::from_ref(&policy)).unwrap(), "oo_r_reader");
        let east = serde_json::json!({"role": "east"});
        assert_eq!(super::db_role_name(std::slice::from_ref(&east)).unwrap(), "oo_r_east");
        assert!(super::db_role_name(std::slice::from_ref(&prefixed)).is_err());
    }

    #[test]
    fn compiler_policy_dataset_is_the_table_identity() {
        let shipments = serde_json::json!({
            "table": {"namespace": "public", "name": "shipments", "columns": []}
        });
        assert_eq!(super::policy_dataset(&shipments).unwrap(), "public.shipments");
        let hubs = serde_json::json!({"table": {"namespace": "public", "name": "hubs"}});
        assert_eq!(super::policy_dataset(&hubs).unwrap(), "public.hubs");
        let guessed = serde_json::json!({"dataset": "public.shipments"});
        assert!(super::policy_dataset(&guessed).is_err());
    }

    fn customer_target() -> EndpointFile {
        EndpointFile {
            host: "oo-dl01w1-46-tgt".to_owned(),
            port: 5432,
            database: "dst".to_owned(),
            username: "oo_dl01w1_46_target".to_owned(),
            password_file: "/run/secrets/target".to_owned(),
            ca_file: "/run/secrets/ca.pem".to_owned(),
        }
    }

    fn compiler_policy(table: &str) -> serde_json::Value {
        serde_json::json!({
            "project": "oo_dl01w1_46",
            "config_digest": "df690a6f697d3f78d95d3bf878df7254e16665a15e0e8407bb3dc64f64368306",
            "generation": 1,
            "role": "reader",
            "login": "oo_dl01w1_46_target",
            "table": {"namespace": "public", "name": table, "columns": [{"name": "id"}]}
        })
    }

    #[test]
    fn column_acl_set_keeps_each_compiler_table() {
        let target = customer_target();
        let bytes = super::column_acl_set_document(&[compiler_policy("hubs"), compiler_policy("shipments")], &target).expect("set");
        let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!(doc.get("dataset").is_none());
        assert_eq!(doc["policies"].as_array().map(Vec::len), Some(2));
        assert_eq!(doc["policies"][0]["table"]["name"], "hubs");
        assert_eq!(doc["policies"][1]["table"]["name"], "shipments");
        assert_eq!(doc["policies"][0]["table"]["namespace"], "public");
        assert_eq!(doc["policies"][1]["role"], "reader");
        assert_eq!(doc["policies"][1]["login"], "oo_dl01w1_46_target");
        let mut other = compiler_policy("tracking_events");
        other["role"] = serde_json::json!("auditor");
        assert!(super::column_acl_set_document(&[compiler_policy("hubs"), other], &target).is_err());
        let mut foreign = compiler_policy("tracking_events");
        foreign["login"] = serde_json::json!("other_login");
        assert!(super::mapping_reader_set_document(&[compiler_policy("hubs"), foreign], &target, "oo_dl01w1_46_target").is_err());
    }

    #[test]
    fn acl_mismatch_ignores_other_policy_refusals() {
        let dir = std::env::temp_dir().join(format!("oo-dl01w1-46-acl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("install-column-acl.stderr"), b"install-column-acl: policy mismatch\n").unwrap();
        assert!(super::acl_mismatch(&dir));
        std::fs::write(dir.join("install-column-acl.stderr"), b"install-column-acl: policy type\n").unwrap();
        assert!(!super::acl_mismatch(&dir));
        std::fs::write(dir.join("install-column-acl.stderr"), b"install-column-acl: policy identity\n").unwrap();
        assert!(!super::acl_mismatch(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn seal_docs(generation: i64, knowledge: &str, column: &str) -> (serde_json::Value, serde_json::Value, serde_json::Value, serde_json::Value, serde_json::Value) {
        let digest = "e8c0bb73dbfd02ee29eb683a96d29d98c24d2a502ff7e07c766ae84f69c518bb";
        let import_hash = "2e80f9fec75ffeeb37e002fb3bbd5bd830360e9097fd736ad6c95a16177a8014";
        let pack = "c175f0970faccdb0ff1347da43993360b7e2345521dd55630cc5e7ed9bdfeb79";
        let snapshot = "b8595191-e02b-4581-8e10-3c59606ffa7e";
        let version = generation.to_string();
        let authority = serde_json::json!({
            "project": "oo_dl01w1_46",
            "role": "oo_r_east",
            "knowledge": {"id": knowledge, "version": version},
            "policy_pack_id": "pol-customer_east",
            "policy_version": version,
            "import": {
                "source_id": "oo_dl01w1_46",
                "snapshot_id": snapshot,
                "import_hash": import_hash,
                "import_generation": generation,
                "target_config_sha256": digest
            }
        });
        let settings = serde_json::json!({
            "catalog_version": digest,
            "knowledge": {"id": "know-customer_east", "version": "2"},
            "policy": {"id": "pol-customer_east", "version": "2"}
        });
        let catalog = serde_json::json!({
            "source_id": "oo_dl01w1_46",
            "run_id": snapshot,
            "source_generation": 2,
            "config_digest": digest,
            "state": "awaiting_publish"
        });
        let receipt = serde_json::json!({
            "pack_digest": pack,
            "binding": {
                "source_id": "oo_dl01w1_46",
                "snapshot_id": snapshot,
                "import_hash": import_hash,
                "import_generation": 2,
                "target_config_sha256": digest
            }
        });
        let policy = serde_json::json!({
            "generation": 2,
            "role": "east",
            "table": {"namespace": "customer_east", "name": "delivery_notes", "columns": [{"name": column}]}
        });
        (authority, settings, catalog, receipt, policy)
    }

    fn raw(class: &str, column: &str, pack: Option<&str>) -> serde_json::Value {
        let mut doc = serde_json::json!({
            "class": class,
            "outcome": if class == "guard" || class == "pack_mismatch" { "refused" } else { "succeeded" },
            "import": {
                "snapshot_id": "b8595191-e02b-4581-8e10-3c59606ffa7e",
                "import_hash": "2e80f9fec75ffeeb37e002fb3bbd5bd830360e9097fd736ad6c95a16177a8014",
                "import_generation": 2,
                "source_id": "oo_dl01w1_46",
                "target_config_sha256": "e8c0bb73dbfd02ee29eb683a96d29d98c24d2a502ff7e07c766ae84f69c518bb"
            },
            "knowledge": {"id": "know-customer_east", "version": "2"},
            "policy": {"role": "oo_r_east", "policy_pack_id": "pol-customer_east", "columns": [column]}
        });
        if let Some(pack) = pack {
            doc["knowledge"]["pack_digest"] = serde_json::json!(pack);
        }
        doc
    }

    #[test]
    fn replay_binding_rejects_a_stale_generation_and_an_in_scope_column() {
        let (authority, settings, catalog, receipt, policy) = seal_docs(2, "know-customer_east", "note_id");
        let seal = super::compile_seal(&authority, &settings, &catalog, &receipt, &policy).expect("seal");
        let pack = "c175f0970faccdb0ff1347da43993360b7e2345521dd55630cc5e7ed9bdfeb79";
        let hidden = raw("guard", "note_id", Some(pack));
        assert!(super::guard_scope(&hidden, &seal, "secret_note"));
        let admitted = raw("guard", "secret_note", Some(pack));
        assert!(!super::guard_scope(&admitted, &seal, "secret_note"));
        let exit_only = serde_json::json!({"class": "guard", "outcome": "refused"});
        assert!(!super::guard_scope(&exit_only, &seal, "secret_note"));
        let mut stale = hidden.clone();
        stale["import"]["snapshot_id"] = serde_json::json!("55b58814-64e5-4b3c-8006-c17a5b26d8b5");
        stale["import"]["import_generation"] = serde_json::json!(1);
        stale["knowledge"]["id"] = serde_json::json!("know-logistics");
        assert!(!super::guard_scope(&stale, &seal, "secret_note"));
        let widened = raw("pack_mismatch", "note_id", None);
        assert!(super::pack_mismatch(&widened, &seal));
        let widened_with_pack = raw("pack_mismatch", "note_id", Some(pack));
        assert!(!super::pack_mismatch(&widened_with_pack, &seal));
        let (old_authority, _, _, _, _) = seal_docs(1, "know-logistics", "note_id");
        assert!(super::compile_seal(&old_authority, &settings, &catalog, &receipt, &policy).is_err());
    }
}
