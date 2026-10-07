use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oo_connectors::{AdmittedCatalog, PgConnector, PgTypePolicy};
use oo_pipeline::target_installer::sha256_hex;

use crate::error::MirrorError;
use crate::pg::map_source;
use crate::plan::{
    parse_config_text, read_ca, read_secret, read_text, seal_plan, source_config, store_plan, ColumnShape,
    Plan, TableShape,
};

pub struct DiscoverRequest {
    pub config_path: PathBuf,
    pub plan_path: PathBuf,
    pub source_password: PathBuf,
    pub source_ca: PathBuf,
}

pub async fn discover(request: DiscoverRequest) -> Result<Plan, MirrorError> {
    if exists(&request.plan_path)? {
        return Err(MirrorError::new("plan_exists", "exists"));
    }
    let text = read_text(&request.config_path, "config", "config_path")?;
    let config = parse_config_text(&text)?;
    let password = read_secret(&request.source_password)?;
    let ca = read_ca(&request.source_ca)?;
    if sha256_hex(&ca) != config.ca_sha256 {
        return Err(MirrorError::new("ca_drift", "ca"));
    }
    let source = source_config(&config, password.as_str(), &ca)?;
    let deadline = Instant::now() + Duration::from_millis(config.statement_timeout_ms);
    let connector = PgConnector::new(source);
    let prepared = connector
        .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, config.generation, deadline)
        .await
        .map_err(|err| map_source(&err))?;
    let admitted = shapes_from_admitted(&prepared.admitted);
    let plan = seal_plan(
        config,
        admitted,
        &prepared.expected_principal,
        &prepared.expected_database,
        prepared.expected_in_recovery,
        &prepared.expected_postmaster_start,
    )?;
    store_plan(&request.plan_path, &plan)?;
    Ok(plan)
}

pub fn shapes_from_admitted(admitted: &AdmittedCatalog) -> Vec<TableShape> {
    admitted
        .tables
        .iter()
        .map(|table| TableShape {
            schema: table.schema.clone(),
            name: table.name.clone(),
            columns: table
                .columns
                .iter()
                .map(|column| ColumnShape {
                    attnum: column.attnum,
                    name: column.name.clone(),
                    oid: column.type_oid,
                    typmod: column.typmod,
                    not_null: column.not_null,
                })
                .collect(),
            primary_key: table.primary_key.clone(),
        })
        .collect()
}

fn exists(path: &Path) -> Result<bool, MirrorError> {
    if !path.is_absolute() {
        return Err(MirrorError::new("config", "path"));
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(MirrorError::new("plan_exists", "symlink")),
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(MirrorError::new("config", "path")),
    }
}
