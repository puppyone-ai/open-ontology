//! Catalog, profile, and unreviewed self-build. Publication stays closed.

use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use pg_builder::{
    assemble_cards, canonical_sha256, generate, load_config as load_builder, read_bearer, read_comments,
    read_profile, run_catalog, seal_request, ChatTransport, GenerateIn, LoopbackTransport, PgBuilderError, RawReply,
};
use serde_json::{json, Value};

use crate::bind::{self, BULK_IMPORT_HASH};
use crate::config::{Config, ConfigError};
use crate::doctor::{doctor, ready_bins};
use crate::forward::{self, LLM_DEADLINE_MS};
use crate::pack::{write_bulk_draft, write_draft, PackColumn, PackTable};
use crate::source::{bulk_source_hash, count_source, read_sealed_mapping, SourceCounts};
use crate::state::{base, write_state};

#[derive(Clone, Debug)]
pub struct RunOutcome {
    pub status: String,
    pub source_sha256: String,
    pub rows: Value,
    pub selfbuild: String,
    pub question_set: String,
    pub import_hash: String,
    pub phases: Value,
}

/// Provision only the sealed local target before source credentials are
/// detached. No model, Pi, pack, source read, or publication runs here.
pub fn prepare(path: &Path) -> Result<Value, ConfigError> {
    let config = crate::config::load_config(path)?;
    if !crate::source::connector_decision(&config.database_type).supported {
        return Err(ConfigError::new("unsupported", "the local target connector is not supported"));
    }
    if config.tls != "require" || !config.secret_ref.starts_with("secret://") {
        return Err(ConfigError::new("config.tls", "local target preparation requires TLS and a private secret reference"));
    }
    let sealed = bind::load_sealed(path)?;
    grant_mapping_read(path, &config)?;
    let mut mapping = read_sealed_mapping(&config, &sealed.source_id, sealed.generation, &config.tables, &bind::dev_role_name())
        .map_err(|code| ConfigError::new(code, "the sealed local mapping could not be read"))?;
    let mut bootstrap = "not_invoked";
    let role = prepare_fresh_role(path, &config, "fresh_complete", &mut mapping, &mut bootstrap)?;
    bind::write_dev_policy(&config, &sealed, &mapping)?;
    Ok(json!({
        "protocol": crate::PROTOCOL,
        "status": "local_target_prepared",
        "ready": false,
        "import_hash": sealed.import_hash,
        "mapping_generation": sealed.generation,
        "target_adopt_generation": sealed.target_adopt_generation,
        "role_bootstrap": role.role_bootstrap,
        "role_grant_columns": role.role_grant_columns,
        "publication": "not_published",
        "llm_review": "unreviewed"
    }))
}

pub fn run(path: &Path) -> Result<RunOutcome, ConfigError> {
    doctor(path)?;
    let config = crate::config::load_config(path)?;
    ready_bins(&config)?;
    let relay = AlignRelay::start(&config)?;
    let copy_producer = crate::copy_invoke::invoke(path)?;
    if copy_producer.get("invoked").and_then(Value::as_bool) == Some(true) {
        return run_sealed(path, &config, &relay, copy_producer);
    }
    let counts = count_source(&config).map_err(|_| ConfigError::new("source", "the source count failed"))?;
    let question_set = acceptance_question_set(false, &config.tables, "");
    let builder_text = builder_config(&config, &relay.origin, &config.tables);
    let builder_path = config.work.join("pg-builder.config.json");
    fs::create_dir_all(&config.work).map_err(|_| ConfigError::new("work", "the work directory could not be created"))?;
    fs::write(&builder_path, &builder_text).map_err(|_| ConfigError::new("work", "the builder config could not be written"))?;
    let builder = load_builder(&builder_text).map_err(|_| ConfigError::new("builder.config", "the builder config was refused"))?;
    let profile = read_profile(&builder).map_err(|_| ConfigError::new("profile.query", "the profile read failed"))?;
    let comments = read_comments(&builder).map_err(|_| ConfigError::new("comment.query", "the comment read failed"))?;
    fs::write(config.work.join("profile.json"), serde_json::to_vec(&profile).unwrap_or_default()).ok();
    fs::write(config.work.join("comments.json"), serde_json::to_vec(&comments).unwrap_or_default()).ok();
    let built = if config.catalog_bin.is_file() {
        match selfbuild(&config, &builder, &builder_text, &profile, &comments, "pubfix") {
            Ok(pair) => Some(pair),
            Err(error) => {
                let mut state = base(&config, "blocked");
                state["selfbuild"] = json!("failed");
                state["source_sha256"] = json!(counts.source_sha256);
                let _ = write_state(&config, &state);
                return Err(error);
            }
        }
    } else {
        None
    };
    let generation = built.as_ref().map(|(value, _)| value);
    write_draft(&config.knowledge, &counts, &config.tables, &profile, generation)
        .map_err(|_| ConfigError::new("pack", "the draft pack could not be written"))?;
    let validated = built.as_ref().is_some_and(|(_, ok)| *ok);
    let selfbuild_status = if validated {
        "generated_unreviewed"
    } else if built.is_some() {
        "structural_draft"
    } else {
        "catalog.absent"
    };
    let rows = counted_rows(&config.tables, &counts.rows);
    let mut state = base(&config, loop_status(validated));
    state["source_sha256"] = json!(counts.source_sha256);
    state["server_version"] = json!(counts.version);
    state["question_set"] = json!(question_set);
    state["acceptance_fixture"] = json!(question_set == "shop");
    state["import_hash"] = json!("");
    state["copy_producer"] = copy_producer;
    state["counted_tables"] = json!(config.tables.iter().map(|(schema, table)| format!("{schema}.{table}")).collect::<Vec<_>>());
    state["rows"] = rows.clone();
    state["catalog_input"] = json!("source_actor");
    state["catalog_local_document"] = json!("absent");
    state["selfbuild"] = json!(selfbuild_status);
    state["knowledge"] = json!(config.knowledge.display().to_string());
    if let Some((value, _)) = &built {
        state["generation_outcome"] = json!(value.get("outcome").and_then(Value::as_str).unwrap_or("absent"));
        state["cache"] = json!(value.get("cache").and_then(Value::as_str).unwrap_or("absent"));
        state["socket"] = json!(value.pointer("/transport/socket").and_then(Value::as_str).unwrap_or("absent"));
        if let Some(status) = value.pointer("/transport/http_status").filter(|item| !item.is_null()) {
            state["llm_http_status"] = status.clone();
        }
    }
    write_state(&config, &state).map_err(|_| ConfigError::new("state", "the state file could not be written"))?;
    if !validated {
        let code = if built.is_none() { "catalog.absent" } else { "selfbuild" };
        let message = if built.is_none() {
            "the catalog binary is absent"
        } else {
            "the model draft is not a validated unreviewed generation"
        };
        return Err(ConfigError::new(code, message));
    }
    Ok(RunOutcome {
        status: loop_status(true).to_owned(),
        source_sha256: counts.source_sha256,
        rows,
        selfbuild: selfbuild_status.to_owned(),
        question_set: question_set.to_owned(),
        import_hash: String::new(),
        phases: json!([]),
    })
}

fn run_sealed(path: &Path, config: &Config, relay: &AlignRelay, copy_producer: Value) -> Result<RunOutcome, ConfigError> {
    if copy_producer.get("invoked").and_then(Value::as_bool) != Some(true) {
        return Err(ConfigError::new("copy.required", "the sealed copy producer was not invoked"));
    }
    let line = copy_producer.get("line").and_then(Value::as_str).unwrap_or("");
    let provenance = copy_producer.get("provenance").and_then(Value::as_str).unwrap_or("");
    let replay = line.split(' ').any(|field| field == "replay_complete");
    let origin = copy_producer.get("copy_origin").and_then(Value::as_str).unwrap_or("");
    let disposition = copy_producer.get("producer_disposition").and_then(Value::as_str).unwrap_or("");
    let fresh = provenance == "fresh_complete" && origin == "fresh" && disposition == "fresh_complete" && line.split(' ').nth(3) == Some("none") && !replay;
    let confirmed = provenance == "fresh_complete" && origin == "fresh" && disposition == "replay_complete" && replay;
    if provenance == "cache_replay" {
        if !replay {
            return Err(ConfigError::new("copy.replay", "the copy producer did not replay a sealed manifest"));
        }
    } else if !fresh && !confirmed {
        return Err(ConfigError::new("copy.provenance", "the copy provenance is not a replay or a fresh finish"));
    }
    let sealed = bind::load_sealed(path)?;
    if copy_producer.get("import_hash").and_then(Value::as_str) != Some(sealed.import_hash.as_str()) {
        return Err(ConfigError::new("copy.import", "the sealed import does not match the manifest"));
    }
    let copied_rows = line.split(' ').nth(1).and_then(|text| text.parse::<i64>().ok()).ok_or_else(|| ConfigError::new("copy.rows", "the copy producer row count is absent"))?;
    if copied_rows != sealed.rows {
        return Err(ConfigError::new("copy.rows", "the copy producer row count does not match the manifest"));
    }
    grant_mapping_read(path, config)?;
    let mut mapping = read_sealed_mapping(config, &sealed.source_id, sealed.generation, &config.tables, &bind::dev_role_name())
        .map_err(|code| ConfigError::new(code, "the sealed mapping could not be read"))?;
    if mapping.tables.len() != config.tables.len() {
        return Err(ConfigError::new("source.rows", "the sealed table list does not match the database"));
    }
    let mut sum = 0_i64;
    for table in &mapping.tables {
        let expected = sealed
            .tables
            .iter()
            .find(|item| item.schema == table.logical_schema && item.name == table.logical_table)
            .ok_or_else(|| ConfigError::new("source.rows", "a configured table is absent from the sealed manifest"))?;
        if table.rows != expected.rows || (!expected.physical_name.is_empty() && table.physical_table != expected.physical_name) {
            return Err(ConfigError::new("source.rows", "a sealed table does not match the local database"));
        }
        sum += table.rows;
    }
    if sum != sealed.rows {
        return Err(ConfigError::new("source.rows", "the sealed row total does not match the database"));
    }
    let names: Vec<String> = mapping.tables.iter().map(|table| format!("{}.{}", table.logical_schema, table.logical_table)).collect();
    let row_list: Vec<i64> = mapping.tables.iter().map(|table| table.rows).collect();
    let source_sha256 = bulk_source_hash(config, &names, &row_list, &mapping.version, &sealed.import_hash);
    let counts = SourceCounts {
        rows: row_list.clone(),
        version: mapping.version.clone(),
        source_sha256: source_sha256.clone(),
    };
    fs::create_dir_all(&config.work).map_err(|_| ConfigError::new("work", "the work directory could not be created"))?;
    let mut role_bootstrap = "not_invoked";
    let admitted = match prepare_fresh_role(path, config, provenance, &mut mapping, &mut role_bootstrap) {
        Ok(value) => value,
        Err(error) => {
            let recorded = if role_bootstrap == "created" { "created" } else { "refused" };
            return finish_sealed(SealedFinish {
                config,
                copy_producer: &copy_producer,
                sealed: &sealed,
                mapping: &mapping,
                counts: &counts,
                names: &names,
                provenance,
                selfbuild_status: "structural_draft",
                catalog_status: error.code,
                catalog_reason: error.message,
                generation: None,
                cache_replay: "not_opened",
                role_grant: "refused",
                role_grant_columns: 0,
                catalog: None,
                profile: None,
                manifest_sha256: String::new(),
                target_config_sha256: String::new(),
                role_bootstrap: recorded,
            });
        }
    };
    bind::write_dev_policy(config, &sealed, &mapping)?;
    let (manifest_sha256, target_config_sha256) = match bind::sealed_binding_hashes(path) {
        Ok(pair) => pair,
        Err(error) => {
            return finish_sealed(SealedFinish {
                config,
                copy_producer: &copy_producer,
                sealed: &sealed,
                mapping: &mapping,
                counts: &counts,
                names: &names,
                provenance,
                selfbuild_status: "structural_draft",
                catalog_status: error.code,
                catalog_reason: error.message,
                generation: None,
                cache_replay: "not_opened",
                role_grant: admitted.role_grant,
                role_grant_columns: admitted.role_grant_columns,
                catalog: None,
                profile: None,
                manifest_sha256: String::new(),
                target_config_sha256: String::new(),
                role_bootstrap: admitted.role_bootstrap,
            });
        }
    };
    let builder_text = builder_config(config, &relay.origin, &config.tables);
    let builder_path = config.work.join("pg-builder.config.json");
    fs::write(&builder_path, &builder_text).map_err(|_| ConfigError::new("work", "the builder config could not be written"))?;
    let builder = load_builder(&builder_text).map_err(|_| ConfigError::new("builder.config", "the builder config was refused"))?;
    let mut captured = match crate::local_catalog::capture(config, &sealed, &mapping) {
        Ok(value) => value,
        Err(fault) => {
            return finish_sealed(SealedFinish {
                config,
                copy_producer: &copy_producer,
                sealed: &sealed,
                mapping: &mapping,
                counts: &counts,
                names: &names,
                provenance,
                selfbuild_status: "structural_draft",
                catalog_status: fault.phase,
                catalog_reason: fault.reason,
                generation: None,
                cache_replay: "not_opened",
                role_grant: admitted.role_grant,
                role_grant_columns: admitted.role_grant_columns,
                catalog: None,
                profile: None,
                manifest_sha256: manifest_sha256.clone(),
                target_config_sha256: target_config_sha256.clone(),
                role_bootstrap: admitted.role_bootstrap,
            });
        }
    };
    if let Some(prior) = regular_text(&config.work.join("catalog.json"), 8_000_000) {
        crate::local_catalog::reuse_captured_at(&prior, &mut captured);
    }
    fs::write(config.work.join("catalog.json"), &captured.catalog_text).ok();
    fs::write(config.work.join("catalog-provenance.json"), serde_json::to_vec(&captured.provenance).unwrap_or_default()).ok();
    fs::write(config.work.join("profile.json"), serde_json::to_vec(&captured.profile).unwrap_or_default()).ok();
    fs::write(config.work.join("comments.json"), serde_json::to_vec(&captured.comments).unwrap_or_default()).ok();
    let built = match selfbuild_local(config, &builder, &builder_text, &captured) {
        Ok(value) => value,
        Err(error) => {
            return finish_sealed(SealedFinish {
                config,
                copy_producer: &copy_producer,
                sealed: &sealed,
                mapping: &mapping,
                counts: &counts,
                names: &names,
                provenance,
                selfbuild_status: "structural_draft",
                catalog_status: error.code,
                catalog_reason: error.message,
                generation: None,
                cache_replay: "not_opened",
                role_grant: admitted.role_grant,
                role_grant_columns: admitted.role_grant_columns,
                catalog: None,
                profile: None,
                manifest_sha256: manifest_sha256.clone(),
                target_config_sha256: target_config_sha256.clone(),
                role_bootstrap: admitted.role_bootstrap,
            });
        }
    };
    finish_sealed(SealedFinish {
        config,
        copy_producer: &copy_producer,
        sealed: &sealed,
        mapping: &mapping,
        counts: &counts,
        names: &names,
        provenance,
        selfbuild_status: built.selfbuild_status,
        catalog_status: built.catalog_status,
        catalog_reason: built.catalog_reason,
        generation: built.generation,
        cache_replay: built.cache_replay,
        role_grant: admitted.role_grant,
        role_grant_columns: admitted.role_grant_columns,
        catalog: Some(&captured.catalog),
        profile: Some(&captured.profile),
        manifest_sha256,
        target_config_sha256,
        role_bootstrap: admitted.role_bootstrap,
    })
}

struct AdmittedRole {
    role_grant: &'static str,
    role_grant_columns: i64,
    role_bootstrap: &'static str,
}

fn grant_mapping_read(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let Some(admin) = crate::copy_invoke::fresh_admin(path)? else {
        return Ok(());
    };
    let statements = crate::source::mapping_reader_statements(&config.username)
        .map_err(|_| ConfigError::new("role.grant", "the reader name is not allowed"))?;
    crate::source::apply_role_grants(config, &admin.user, &admin.password, &admin.ca, &statements).map_err(grant_error)
}

fn prepare_fresh_role(
    path: &Path,
    config: &Config,
    provenance: &str,
    mapping: &mut crate::source::MappingRead,
    role_bootstrap: &mut &'static str,
) -> Result<AdmittedRole, ConfigError> {
    crate::copy_invoke::require_target(config, path)?;
    if provenance != "fresh_complete" {
        *role_bootstrap = "not_invoked";
        return Ok(AdmittedRole { role_grant: "not_invoked", role_grant_columns: 0, role_bootstrap: "not_invoked" });
    }
    let Some(admin) = crate::copy_invoke::fresh_admin(path)? else {
        *role_bootstrap = "not_invoked";
        return Ok(AdmittedRole { role_grant: "not_invoked", role_grant_columns: 0, role_bootstrap: "not_invoked" });
    };
    let bootstrap = if mapping.role_state == "absent" {
        let statements = crate::source::dev_role_statements(&bind::dev_role_name(), &config.username)
            .map_err(|_| ConfigError::new("role.grant", "the development role name is not allowed"))?;
        crate::source::apply_admin_statements(config, &admin.user, &admin.password, &admin.ca, &statements).map_err(grant_error)?;
        mapping.role_state = "present".to_owned();
        *role_bootstrap = "created";
        "created"
    } else if mapping.role_state == "present" {
        *role_bootstrap = "present";
        "present"
    } else {
        return Err(ConfigError::new("role.absent", "the existing development role is not present"));
    };
    let (statements, columns) = admitted_grant_sql(mapping)?;
    crate::source::apply_role_grants(config, &admin.user, &admin.password, &admin.ca, &statements).map_err(grant_error)?;
    drop(admin);
    Ok(AdmittedRole { role_grant: "column_select", role_grant_columns: columns, role_bootstrap: bootstrap })
}

fn grant_error(reason: &'static str) -> ConfigError {
    match reason {
        "permission_denied" => ConfigError::new("role.grant", "the development role grant was denied"),
        "role_absent" => ConfigError::new("role.absent", "the existing development role is not present"),
        "relation_absent" => ConfigError::new("role.grant", "a mapped relation is absent"),
        "connect" | "tls" => ConfigError::new("role.grant", "the development role grant could not connect"),
        _ => ConfigError::new("role.grant", "the development role grant was refused"),
    }
}

fn admitted_grant_sql(mapping: &crate::source::MappingRead) -> Result<(Vec<String>, i64), ConfigError> {
    let role = crate::source::quote_ident(&bind::dev_role_name()).map_err(|_| ConfigError::new("role.grant", "the development role name is not allowed"))?;
    let mut statements = vec![format!("GRANT USAGE ON SCHEMA \"_oo\" TO {role}")];
    let mut column_count: i64 = 0;
    for table in &mapping.tables {
        if table.physical_schema != "_oo" {
            return Err(ConfigError::new("role.grant", "a mapped relation is not in the local schema"));
        }
        let physical = crate::source::quote_ident(&table.physical_table).map_err(|_| ConfigError::new("role.grant", "a physical table name is not allowed"))?;
        let mut columns = Vec::new();
        for column in &table.columns {
            if column.name == "_oo_pos" || column.physical_name == "_oo_pos" {
                return Err(ConfigError::new("role.grant", "a metadata column was selected"));
            }
            if !bind::grantable(&column.type_name, column.typmod) {
                continue;
            }
            columns.push(crate::source::quote_ident(&column.physical_name).map_err(|_| ConfigError::new("role.grant", "a physical column name is not allowed"))?);
        }
        if columns.is_empty() {
            return Err(ConfigError::new("role.grant", "a sealed table has no admitted column"));
        }
        column_count += i64::try_from(columns.len()).unwrap_or(0);
        let listed = columns.join(", ");
        statements.push(format!("GRANT SELECT ({listed}) ON TABLE \"_oo\".{physical} TO {role}"));
    }
    Ok((statements, column_count))
}

struct SealedFinish<'a> {
    config: &'a Config,
    copy_producer: &'a Value,
    sealed: &'a crate::bind::SealedCopy,
    mapping: &'a crate::source::MappingRead,
    counts: &'a SourceCounts,
    names: &'a [String],
    provenance: &'a str,
    selfbuild_status: &'static str,
    catalog_status: &'static str,
    catalog_reason: &'static str,
    generation: Option<Value>,
    cache_replay: &'static str,
    role_grant: &'static str,
    role_grant_columns: i64,
    catalog: Option<&'a Value>,
    profile: Option<&'a Value>,
    manifest_sha256: String,
    target_config_sha256: String,
    role_bootstrap: &'static str,
}

fn finish_sealed(finish: SealedFinish<'_>) -> Result<RunOutcome, ConfigError> {
    let question_set = acceptance_question_set(true, &finish.config.tables, &finish.sealed.import_hash);
    let inputs_ready = finish.catalog_status == "validated"
        && finish.selfbuild_status == "generated_unreviewed"
        && finish.catalog.is_some()
        && finish.profile.is_some()
        && finish.generation.is_some();
    let mut pack_writer = "write_bulk_draft";
    let mut pack_fault: Option<&'static str> = None;
    if inputs_ready {
        match write_standard_pack(&finish) {
            Ok(()) => pack_writer = "write_sealed_pack",
            Err(code) => {
                pack_fault = Some(code);
                write_failure_draft(&finish)?;
            }
        }
    } else {
        write_failure_draft(&finish)?;
    }
    let mut rows = serde_json::Map::new();
    for (name, count) in finish.names.iter().zip(&finish.counts.rows) {
        rows.insert(name.clone(), json!(count));
    }
    let ready = pack_fault.is_none() && finish.catalog_status == "validated" && finish.selfbuild_status == "generated_unreviewed";
    let status = loop_status(ready);
    let mut state = base(finish.config, status);
    state["question_set"] = json!(question_set);
    state["acceptance_fixture"] = json!(question_set == "bulk");
    state["import_hash"] = json!(finish.sealed.import_hash);
    state["copy_provenance"] = json!(finish.provenance);
    state["catalog_input"] = json!("local_agent_db_capture");
    state["source_preflight"] = json!("source_preflight_not_invoked");
    state["catalog_pg_execution"] = json!("not_claimed");
    state["catalog_local_document"] = json!("present");
    state["catalog_phase"] = json!(finish.catalog_status);
    state["catalog_reason"] = json!(finish.catalog_reason);
    state["snapshot_id"] = json!(finish.sealed.snapshot_id);
    state["source_id"] = json!(finish.sealed.source_id);
    state["config_digest"] = json!(finish.mapping.config_digest);
    state["role"] = json!(bind::dev_role_name());
    state["role_state"] = json!(finish.mapping.role_state);
    state["role_grant"] = json!(finish.role_grant);
    state["role_grant_columns"] = json!(finish.role_grant_columns);
    state["role_bootstrap"] = json!(finish.role_bootstrap);
    state["pack_writer"] = json!(pack_writer);
    state["source_sha256"] = json!(finish.counts.source_sha256);
    state["server_version"] = json!(finish.mapping.version);
    state["copy_producer"] = finish.copy_producer.clone();
    state["phases"] = finish.copy_producer.get("phases").cloned().unwrap_or_else(|| json!([]));
    state["copy_origin"] = finish.copy_producer.get("copy_origin").cloned().unwrap_or_else(|| json!("absent"));
    state["producer_disposition"] = finish.copy_producer.get("producer_disposition").cloned().unwrap_or_else(|| json!("absent"));
    state["rows"] = Value::Object(rows.clone());
    state["selfbuild"] = json!(finish.selfbuild_status);
    state["catalog"] = json!(finish.catalog_status);
    state["cache_replay"] = json!(finish.cache_replay);
    state["knowledge"] = json!(finish.config.knowledge.display().to_string());
    state["catalog_relations"] = json!("logical_from_physical");
    if let Some(value) = &finish.generation {
        state["generation_outcome"] = json!(value.get("outcome").and_then(Value::as_str).unwrap_or("absent"));
        state["cache"] = json!(value.get("cache").and_then(Value::as_str).unwrap_or("absent"));
    }
    write_state(finish.config, &state).map_err(|_| ConfigError::new("state", "the state file could not be written"))?;
    if let Some(code) = pack_fault {
        return Err(ConfigError::new(code, "the standard draft pack was refused"));
    }
    if !ready {
        return Err(ConfigError::new(finish.catalog_status, finish.catalog_reason));
    }
    Ok(RunOutcome {
        status: status.to_owned(),
        source_sha256: finish.counts.source_sha256.clone(),
        rows: Value::Object(rows),
        selfbuild: finish.selfbuild_status.to_owned(),
        question_set: question_set.to_owned(),
        import_hash: finish.sealed.import_hash.clone(),
        phases: finish.copy_producer.get("phases").cloned().unwrap_or_else(|| json!([])),
    })
}

fn write_failure_draft(finish: &SealedFinish<'_>) -> Result<(), ConfigError> {
    let pack_tables: Vec<PackTable> = finish
        .mapping
        .tables
        .iter()
        .map(|table| PackTable {
            logical_schema: table.logical_schema.clone(),
            logical_table: table.logical_table.clone(),
            physical_schema: table.physical_schema.clone(),
            physical_table: table.physical_table.clone(),
            rows: table.rows,
            columns: table
                .columns
                .iter()
                .map(|column| PackColumn {
                    name: column.name.clone(),
                    type_name: column.type_name.clone(),
                    admitted: bind::grantable(&column.type_name, column.typmod),
                })
                .collect(),
        })
        .collect();
    write_bulk_draft(
        &finish.config.knowledge,
        finish.counts,
        &pack_tables,
        &finish.sealed.import_hash,
        &finish.sealed.snapshot_id,
        finish.generation.as_ref(),
    )
    .map_err(|_| ConfigError::new("pack", "the draft pack could not be written"))
}

fn write_standard_pack(finish: &SealedFinish<'_>) -> Result<(), &'static str> {
    let catalog = finish.catalog.ok_or("pack.input")?;
    let profile = finish.profile.ok_or("pack.input")?;
    let generation = finish.generation.as_ref().ok_or("pack.input")?;
    let import_generation = u64::try_from(finish.sealed.generation).map_err(|_| "pack.input")?;
    let binding = json!({
        "import_generation": import_generation,
        "import_hash": finish.sealed.import_hash,
        "manifest_sha256": finish.manifest_sha256,
        "snapshot_id": finish.sealed.snapshot_id,
        "source_id": finish.sealed.source_id,
        "target_config_sha256": finish.target_config_sha256
    });
    let tables = requested_tables(finish.mapping)?;
    let normalized = pack_profile(profile, finish.mapping);
    let input = crate::pack_input::sealed_pack_input(catalog, &normalized, generation, &binding, &tables)?;
    fs::create_dir_all(&finish.config.work).map_err(|_| "pack")?;
    fs::write(finish.config.work.join("pack-input.json"), serde_json::to_vec(&input).map_err(|_| "pack")?).map_err(|_| "pack")?;
    let receipt = crate::pack::write_sealed_pack(&finish.config.knowledge, &input)?;
    fs::write(finish.config.work.join("pack-receipt.json"), serde_json::to_vec(&receipt).map_err(|_| "pack")?).map_err(|_| "pack")?;
    Ok(())
}

fn requested_tables(mapping: &crate::source::MappingRead) -> Result<Value, &'static str> {
    let mut rows = Vec::new();
    for table in &mapping.tables {
        let count = u64::try_from(table.rows).map_err(|_| "pack.input")?;
        let columns: Vec<Value> = table
            .columns
            .iter()
            .map(|column| {
                json!({
                    "name": column.name,
                    "admitted": bind::grantable(&column.type_name, column.typmod),
                    "type_name": column.type_name
                })
            })
            .collect();
        rows.push(json!({
            "schema": table.logical_schema,
            "name": table.logical_table,
            "rows": count,
            "physical_schema": table.physical_schema,
            "physical_table": table.physical_table,
            "columns": columns
        }));
    }
    Ok(Value::Array(rows))
}

fn pack_profile(profile: &Value, mapping: &crate::source::MappingRead) -> Value {
    let mut out = profile.clone();
    let Some(tables) = out.get_mut("tables").and_then(Value::as_array_mut) else {
        return out;
    };
    for row in tables {
        let Some(object) = row.as_object_mut() else {
            continue;
        };
        let schema = object.get("schema").and_then(Value::as_str).unwrap_or("");
        let name = object.get("name").and_then(Value::as_str).unwrap_or("");
        let reltuples = object.get("reltuples").and_then(Value::as_str).unwrap_or("");
        if is_pack_row_count(reltuples) {
            continue;
        }
        let Some(table) = mapping.tables.iter().find(|table| table.logical_schema == schema && table.logical_table == name) else {
            continue;
        };
        if table.rows >= 0 {
            object.insert("reltuples".to_owned(), Value::String(table.rows.to_string()));
        }
    }
    out
}

fn is_pack_row_count(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty() && bytes.iter().all(|byte| byte.is_ascii_digit()) && (bytes.len() == 1 || bytes[0] != b'0')
}

#[cfg(test)]
fn refuse_unvalidated(catalog_status: &str) -> ConfigError {
    match catalog_status {
        "catalog.absent" => ConfigError::new("catalog.absent", "the catalog binary is absent"),
        "not_validated" => ConfigError::new("selfbuild", "the model draft is not a validated unreviewed generation"),
        _ => ConfigError::new("catalog.refused", "the builder catalog was refused"),
    }
}

pub(crate) fn loop_status(validated: bool) -> &'static str {
    if validated { "draft_ready" } else { "blocked" }
}

pub(crate) fn acceptance_question_set(sealed: bool, tables: &[(String, String)], import_hash: &str) -> &'static str {
    if sealed && import_hash == BULK_IMPORT_HASH && bulk_tables(tables) {
        "bulk"
    } else if sealed {
        "sealed"
    } else if shop_tables(tables) {
        "shop"
    } else {
        "configured"
    }
}

fn regular_text(path: &Path, limit: u64) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > limit {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn counted_rows(tables: &[(String, String)], rows: &[i64]) -> Value {
    let mut map = serde_json::Map::new();
    for ((schema, table), count) in tables.iter().zip(rows) {
        map.insert(format!("{schema}.{table}"), json!(count));
    }
    Value::Object(map)
}

fn bulk_tables(tables: &[(String, String)]) -> bool {
    tables
        == [
            ("public".to_owned(), "batch_a".to_owned()),
            ("public".to_owned(), "batch_b".to_owned()),
            ("public".to_owned(), "batch_c".to_owned()),
        ]
}

fn shop_tables(tables: &[(String, String)]) -> bool {
    tables
        == [
            ("shop".to_owned(), "party".to_owned()),
            ("shop".to_owned(), "account".to_owned()),
            ("shop".to_owned(), "entry".to_owned()),
        ]
}

fn selfbuild(
    config: &Config,
    builder: &pg_builder::Config,
    builder_text: &str,
    profile: &Value,
    comments: &Value,
    source_id: &str,
) -> Result<(Value, bool), ConfigError> {
    let stderr_path = config.work.join("catalog.stderr");
    let bytes = run_catalog(builder, source_id, &config.secret_ref, Some(&stderr_path)).map_err(|_| ConfigError::new("catalog", "catalog-pg failed"))?;
    let catalog_text = String::from_utf8(bytes).map_err(|_| ConfigError::new("catalog", "catalog output is not UTF-8"))?;
    fs::write(config.work.join("catalog.json"), &catalog_text).ok();
    let seal = seal_request(&catalog_text).map_err(|_| ConfigError::new("seal", "the seal was refused"))?;
    let catalog_value: Value = serde_json::from_str(&catalog_text).map_err(|_| ConfigError::new("catalog", "catalog output is not JSON"))?;
    let schema = seal.get("schema_hash").and_then(Value::as_str).ok_or_else(|| ConfigError::new("seal", "schema hash is absent"))?;
    let evidence = seal.get("evidence_hash").and_then(Value::as_str).ok_or_else(|| ConfigError::new("seal", "evidence hash is absent"))?;
    let cards = assemble_cards(&catalog_value, profile, comments, schema, evidence, builder.max_tables)
        .map_err(|_| ConfigError::new("cards", "the cards were refused"))?;
    let tables = cards.get("tables").and_then(Value::as_array).ok_or_else(|| ConfigError::new("cards", "the cards have no tables"))?;
    let listed: Vec<Value> = tables
        .iter()
        .map(|card| json!({"dataset": card.get("dataset").cloned().unwrap_or(Value::Null), "card_sha256": card.get("card_sha256").cloned().unwrap_or(Value::Null)}))
        .collect();
    let approval = json!({
        "protocol": "oo.pg-builder.card-approval/v1",
        "approved": true,
        "authenticated": false,
        "parent_auth": "required",
        "reviewer_label": config.operator_label,
        "reviewer_at": config.operator_at,
        "evidence_hash": cards.get("evidence_hash").cloned().unwrap_or(Value::Null),
        "schema_hash": cards.get("schema_hash").cloned().unwrap_or(Value::Null),
        "cards_sha256": canonical_sha256(&cards),
        "tables": listed
    });
    fs::write(config.work.join("card-approval.json"), serde_json::to_vec(&json!({
        "protocol": "oo.pg-builder.card-approval/v1",
        "approved_cards": true,
        "authenticated": false,
        "parent_auth": "required",
        "llm_review": "unreviewed",
        "publication": "not_published"
    })).unwrap_or_default()).ok();
    let token = read_bearer(&config.bearer_file).map_err(|_| ConfigError::new("config.secret", "the bearer file was refused"))?;
    let command = seal.get("command").and_then(Value::as_str).ok_or_else(|| ConfigError::new("seal", "the seal has no command"))?;
    let mut transport = LoopbackTransport { config: builder.clone(), token: token.clone() };
    let generated = generate(GenerateIn {
        config: builder,
        config_bytes: builder_text.as_bytes(),
        token: &token,
        command_text: command,
        cards: &cards,
        approval: &approval,
        cache_dir: &config.work.join("cache"),
        transport: &mut transport,
    })
    .map_err(|_| ConfigError::new("selfbuild", "the model draft failed"))?;
    fs::write(config.work.join("generation.json"), serde_json::to_vec(&generated).unwrap_or_default()).ok();
    let validated = generated.get("outcome").and_then(Value::as_str) == Some("validated");
    Ok((generated, validated))
}

struct LocalBuild {
    selfbuild_status: &'static str,
    catalog_status: &'static str,
    catalog_reason: &'static str,
    generation: Option<Value>,
    cache_replay: &'static str,
}

struct ClosedTransport;

impl ChatTransport for ClosedTransport {
    fn post(&mut self, _body: &[u8]) -> Result<RawReply, PgBuilderError> {
        Err(PgBuilderError::new("cache.socket", "a cache replay opened a socket"))
    }
}

fn selfbuild_local(
    config: &Config,
    builder: &pg_builder::Config,
    builder_text: &str,
    captured: &crate::local_catalog::LocalCapture,
) -> Result<LocalBuild, ConfigError> {
    let seal = seal_request(&captured.catalog_text).map_err(|error| builder_fault(&error))?;
    let schema = seal.get("schema_hash").and_then(Value::as_str).ok_or_else(|| ConfigError::new("compiler.seal", "schema hash is absent"))?;
    let evidence = seal.get("evidence_hash").and_then(Value::as_str).ok_or_else(|| ConfigError::new("compiler.seal", "evidence hash is absent"))?;
    if schema != captured.schema_hash {
        return Err(ConfigError::new("catalog.stale", "the local schema hash does not match the seal"));
    }
    let cards = assemble_cards(&captured.catalog, &captured.profile, &captured.comments, schema, evidence, builder.max_tables)
        .map_err(|error| builder_fault(&error))?;
    let tables = cards.get("tables").and_then(Value::as_array).ok_or_else(|| ConfigError::new("cards", "the cards have no tables"))?;
    let listed: Vec<Value> = tables
        .iter()
        .map(|card| json!({"dataset": card.get("dataset").cloned().unwrap_or(Value::Null), "card_sha256": card.get("card_sha256").cloned().unwrap_or(Value::Null)}))
        .collect();
    let approval = json!({
        "protocol": "oo.pg-builder.card-approval/v1",
        "approved": true,
        "authenticated": false,
        "parent_auth": "required",
        "reviewer_label": config.operator_label,
        "reviewer_at": config.operator_at,
        "evidence_hash": cards.get("evidence_hash").cloned().unwrap_or(Value::Null),
        "schema_hash": cards.get("schema_hash").cloned().unwrap_or(Value::Null),
        "cards_sha256": canonical_sha256(&cards),
        "tables": listed
    });
    fs::write(config.work.join("card-approval.json"), serde_json::to_vec(&json!({
        "protocol": "oo.pg-builder.card-approval/v1",
        "approved_cards": true,
        "authenticated": false,
        "parent_auth": "required",
        "llm_review": "unreviewed",
        "publication": "not_published",
        "catalog_input": "local_agent_db_capture"
    })).unwrap_or_default()).ok();
    fs::write(config.work.join("seal.json"), serde_json::to_vec(&seal).unwrap_or_default()).ok();
    let token = read_bearer(&config.bearer_file).map_err(|_| ConfigError::new("config.secret", "the bearer file was refused"))?;
    let command = seal.get("command").and_then(Value::as_str).ok_or_else(|| ConfigError::new("compiler.seal", "the seal has no command"))?;
    let mut transport = LoopbackTransport { config: builder.clone(), token: token.clone() };
    let generated = generate(GenerateIn {
        config: builder,
        config_bytes: builder_text.as_bytes(),
        token: &token,
        command_text: command,
        cards: &cards,
        approval: &approval,
        cache_dir: &config.work.join("cache"),
        transport: &mut transport,
    })
    .map_err(|error| builder_fault(&error))?;
    fs::write(config.work.join("generation.json"), serde_json::to_vec(&generated).unwrap_or_default()).ok();
    if generated.get("outcome").and_then(Value::as_str) != Some("validated") {
        return Ok(LocalBuild {
            selfbuild_status: "structural_draft",
            catalog_status: "not_validated",
            catalog_reason: "the model draft is not a validated unreviewed generation",
            generation: Some(generated),
            cache_replay: "not_opened",
        });
    }
    let mut closed = ClosedTransport;
    let replayed = generate(GenerateIn {
        config: builder,
        config_bytes: builder_text.as_bytes(),
        token: &token,
        command_text: command,
        cards: &cards,
        approval: &approval,
        cache_dir: &config.work.join("cache"),
        transport: &mut closed,
    });
    match replayed {
        Ok(doc) if doc.get("cache").and_then(Value::as_str) == Some("hit") => {
            fs::write(config.work.join("generation-replay.json"), serde_json::to_vec(&doc).unwrap_or_default()).ok();
            Ok(LocalBuild {
                selfbuild_status: "generated_unreviewed",
                catalog_status: "validated",
                catalog_reason: "the local capture produced an unreviewed generation",
                generation: Some(doc),
                cache_replay: "hit",
            })
        }
        Ok(doc) => {
            let replay = match doc.get("cache").and_then(Value::as_str) {
                Some("miss") => "miss",
                Some("partial") => "partial",
                _ => "absent",
            };
            fs::write(config.work.join("generation-replay.json"), serde_json::to_vec(&doc).unwrap_or_default()).ok();
            Ok(LocalBuild {
                selfbuild_status: "structural_draft",
                catalog_status: "cache.replay",
                catalog_reason: "a validated draft did not replay from cache",
                generation: Some(doc),
                cache_replay: replay,
            })
        }
        Err(_) => Ok(LocalBuild {
            selfbuild_status: "structural_draft",
            catalog_status: "cache.replay",
            catalog_reason: "a cache replay opened a socket",
            generation: Some(generated),
            cache_replay: "miss",
        }),
    }
}

fn builder_fault(error: &PgBuilderError) -> ConfigError {
    ConfigError::new(static_phase(&error.code), error.message)
}

fn static_phase(code: &str) -> &'static str {
    let name = code.strip_prefix("oo.pg-builder/").unwrap_or(code);
    match name {
        "compiler.seal" => "compiler.seal",
        "compiler.request" => "compiler.request",
        "compiler.model" => "compiler.model",
        "compiler.samples" => "compiler.samples",
        "compiler.columns" => "compiler.columns",
        "catalog.schema" => "catalog.schema",
        "catalog.stale" => "catalog.stale",
        "profile.samples" => "profile.samples",
        "profile.schema" => "profile.schema",
        "comment.schema" => "comment.schema",
        "budget.tables" => "budget.tables",
        "budget.request_bytes" => "budget.request_bytes",
        "approval.schema" => "approval.schema",
        "approval.required" => "approval.required",
        "approval.authentication" => "approval.authentication",
        "approval.stale" => "approval.stale",
        "config.secret" => "config.secret",
        "config.budget" => "config.budget",
        "provider.runtime" => "provider.runtime",
        "cache.socket" => "cache.replay",
        _ => "selfbuild",
    }
}

struct AlignRelay {
    origin: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AlignRelay {
    fn start(config: &Config) -> Result<Self, ConfigError> {
        let approved = loopback_port(&config.llm_origin)?;
        if approved != forward::MODEL_PORT {
            return Err(ConfigError::new("config.endpoint", "the origin is not the loopback builder endpoint"));
        }
        let listener = TcpListener::bind(("127.0.0.1", approved)).map_err(|_| ConfigError::new("forward", "the model relay could not bind"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let connect_port = forward::MODEL_SIDECAR_PORT;
        let thread = std::thread::spawn(move || forward::serve_aligned_ports(listener, connect_port, approved, thread_stop));
        Ok(Self {
            origin: format!("http://127.0.0.1:{approved}"),
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for AlignRelay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn loopback_port(origin: &str) -> Result<u16, ConfigError> {
    let rest = origin.strip_prefix("http://127.0.0.1:").ok_or_else(|| ConfigError::new("config.endpoint", "the origin is not the loopback builder endpoint"))?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ConfigError::new("config.endpoint", "the origin is not the loopback builder endpoint"));
    }
    let port = rest.parse::<u16>().map_err(|_| ConfigError::new("config.endpoint", "the origin is not the loopback builder endpoint"))?;
    if port == 0 {
        return Err(ConfigError::new("config.endpoint", "the origin is not the loopback builder endpoint"));
    }
    Ok(port)
}

fn builder_config(config: &Config, origin: &str, selected: &[(String, String)]) -> String {
    let tables: Vec<String> = selected
        .iter()
        .map(|(schema, table)| format!("{{\"schema\":{schema},\"table\":{table}}}", schema = json_string(schema), table = json_string(table)))
        .collect();
    format!(
        "{{\"protocol\":\"oo.pg-builder.config/v1\",\"adapter\":\"loopback_http_local_dev\",\"namespace\":\"pg-builder\",\"purpose\":\"generate\",\"sample_policy\":\"none\",\"temperature\":0,\"endpoint\":{{\"origin\":{origin},\"path\":\"/v1/chat/completions\",\"model_id\":\"grok-4.7\"}},\"secret\":{{\"bearer_file\":{bearer}}},\"declared_price\":{{\"status\":\"unknown\"}},\"budget\":{{\"deadline_ms\":{deadline},\"max_request_bytes\":262144,\"max_response_bytes\":262144,\"max_prompt_tokens\":100000,\"max_completion_tokens\":100000,\"max_total_tokens\":200000,\"max_tables\":16,\"max_in_flight\":1}},\"postgres\":{{\"host\":{host},\"port\":{port},\"database\":{database},\"username\":{username},\"password_file\":{password},\"ca_file\":{ca},\"catalog_bin\":{catalog},\"tables\":[{tables}]}}}}",
        origin = json_string(origin),
        bearer = json_string(&config.bearer_file.display().to_string()),
        deadline = LLM_DEADLINE_MS,
        host = json_string(&config.host),
        port = config.port,
        database = json_string(&config.database),
        username = json_string(&config.username),
        password = json_string(&config.password_file.display().to_string()),
        ca = json_string(&config.ca_file.display().to_string()),
        catalog = json_string(&config.catalog_bin.display().to_string()),
        tables = tables.join(",")
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{acceptance_question_set, loop_status, refuse_unvalidated};
    use crate::bind::BULK_IMPORT_HASH;
    use serde_json::json;

    fn batch() -> Vec<(String, String)> {
        vec![
            ("public".to_owned(), "batch_a".to_owned()),
            ("public".to_owned(), "batch_b".to_owned()),
            ("public".to_owned(), "batch_c".to_owned()),
        ]
    }

    #[test]
    fn table_names_do_not_select_the_sealed_branch() {
        let shop = vec![
            ("shop".to_owned(), "party".to_owned()),
            ("shop".to_owned(), "account".to_owned()),
            ("shop".to_owned(), "entry".to_owned()),
        ];
        let other = vec![("finance".to_owned(), "ledger".to_owned())];
        let batch = batch();
        assert_eq!(acceptance_question_set(false, &batch, BULK_IMPORT_HASH), "configured");
        assert_eq!(acceptance_question_set(true, &batch, BULK_IMPORT_HASH), "bulk");
        assert_eq!(acceptance_question_set(true, &batch, "other"), "sealed");
        assert_eq!(acceptance_question_set(false, &shop, ""), "shop");
        assert_eq!(acceptance_question_set(true, &shop, BULK_IMPORT_HASH), "sealed");
        assert_eq!(acceptance_question_set(false, &other, ""), "configured");
    }

    #[test]
    fn a_refused_catalog_is_blocked() {
        assert_eq!(loop_status(true), "draft_ready");
        assert_eq!(loop_status(false), "blocked");
        assert_eq!(refuse_unvalidated("catalog_refused").code, "catalog.refused");
        assert_eq!(refuse_unvalidated("catalog.absent").code, "catalog.absent");
        assert_eq!(refuse_unvalidated("not_validated").code, "selfbuild");
    }

    #[test]
    fn a_fresh_grant_keeps_only_admitted_columns() {
        use crate::source::{LiveColumn, LiveTable, MappingRead};
        let mapping = MappingRead {
            config_digest: "digest".to_owned(),
            version: "17.10".to_owned(),
            role_state: "present".to_owned(),
            tables: vec![LiveTable {
                logical_schema: "public".to_owned(),
                logical_table: "batch_b".to_owned(),
                physical_schema: "_oo".to_owned(),
                physical_table: "pt_batch_b".to_owned(),
                rows: 8,
                columns: vec![
                    LiveColumn { name: "id".to_owned(), physical_name: "pc_id".to_owned(), type_name: "int8".to_owned(), typmod: -1, type_oid: 20 },
                    LiveColumn { name: "at_time".to_owned(), physical_name: "pc_at".to_owned(), type_name: "time".to_owned(), typmod: -1, type_oid: 1083 },
                    LiveColumn { name: "blob".to_owned(), physical_name: "pc_blob".to_owned(), type_name: "bytea".to_owned(), typmod: -1, type_oid: 17 },
                ],
            }],
        };
        let (statements, columns) = super::admitted_grant_sql(&mapping).expect("sql");
        assert_eq!(columns, 1);
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0], "GRANT USAGE ON SCHEMA \"_oo\" TO \"oo_r_devread\"");
        assert_eq!(statements[1], "GRANT SELECT (\"pc_id\") ON TABLE \"_oo\".\"pt_batch_b\" TO \"oo_r_devread\"");
        assert!(!statements[1].contains("pc_at"));
        assert!(!statements[1].contains("pc_blob"));
    }

    #[test]
    fn pack_profile_replaces_only_a_non_digit_reltuples() {
        use crate::source::{LiveColumn, LiveTable, MappingRead};
        let column = LiveColumn {
            name: "id".to_owned(),
            physical_name: "pc_id".to_owned(),
            type_name: "int8".to_owned(),
            typmod: -1,
            type_oid: 20,
        };
        let table = |name: &str, rows: i64| LiveTable {
            logical_schema: "public".to_owned(),
            logical_table: name.to_owned(),
            physical_schema: "_oo".to_owned(),
            physical_table: format!("pt_{name}"),
            rows,
            columns: vec![column.clone()],
        };
        let mapping = MappingRead {
            config_digest: "digest".to_owned(),
            version: "17.10".to_owned(),
            role_state: "present".to_owned(),
            tables: vec![table("tracking_events", 90000), table("hubs", 2000), table("shipments", 8000)],
        };
        let profile = json!({
            "protocol": "oo.pg-builder.profile/v1",
            "sample_policy": "none",
            "tables": [
                {"schema": "public", "name": "tracking_events", "reltuples": "90000.0"},
                {"schema": "public", "name": "hubs", "reltuples": "-1"},
                {"schema": "public", "name": "shipments", "reltuples": "8000"}
            ]
        });
        let out = super::pack_profile(&profile, &mapping);
        let tables = out.get("tables").and_then(|value| value.as_array()).expect("tables");
        assert_eq!(tables[0].get("reltuples").and_then(|value| value.as_str()), Some("90000"));
        assert_eq!(tables[1].get("reltuples").and_then(|value| value.as_str()), Some("2000"));
        assert_eq!(tables[2].get("reltuples").and_then(|value| value.as_str()), Some("8000"));
        assert!(super::is_pack_row_count("0"));
        assert!(!super::is_pack_row_count("01"));
        assert!(!super::is_pack_row_count("-1"));
    }
}
