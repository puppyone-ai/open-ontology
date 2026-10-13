//! `oo-pipeline catalog-import` and `oo-pipeline install-sealed-catalog`.
//!
//! `catalog-import` reads one sealed full import from the PostgreSQL target.
//! The target is runtime storage. It is not opened as a PostgreSQL source,
//! and `_oo` shadow tables are not profiled.
//! The caller supplies source identity and a tool manifest. A physical name
//! or a SQL string is not a request field.
//!
//! `install-sealed-catalog` is the administrator path for migration `0006`.
//! The read command does not install it.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

use crate::import_cli::{Stop, read_bounded_stdin, read_ca, read_password};
use crate::physical_mapping::LOOKUP_SQL;
use crate::target_installer::{
    self, InstallError, ReadinessError, compiled_sealed_catalog_digest, install_sealed_catalog,
    verify_full_import_readiness, verify_sealed_catalog_readiness,
};

/// Request protocol for the read command.
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.catalog-import/v1";

/// Stdout protocol. This is not `oo.source.pgsql/v1`.
pub const RESULT_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";

/// Tool manifest this command accepts.
pub const MANIFEST_PROTOCOL: &str = "oo.mysql-preview.manifest/v1";

/// PostgreSQL sealed-import request. This is not `catalog-import`.
pub const PG_REQUEST_PROTOCOL: &str = "oo.pipeline.catalog-import-pg/v1";

/// Tool manifest for a sealed PostgreSQL source. Not the MySQL manifest.
pub const PG_MANIFEST_PROTOCOL: &str = "oo.pipeline.pg-sealed-manifest/v1";

/// Administrator install request.
pub const INSTALL_PROTOCOL: &str = "oo.pipeline.install-sealed-catalog/v1";

const INSTALL_RESULT: &str = "oo.pipeline.install-sealed-catalog-result/v1";

/// Append one logical table header to the schema preimage.
pub fn push_schema_table(out: &mut String, logical_schema: &str, logical_table: &str) {
    out.push_str(logical_schema);
    out.push('.');
    out.push_str(logical_table);
    out.push('\n');
}

/// Append one logical column to the schema preimage.
///
/// A missing caveat is an empty field. The line is `name`, oid, typmod, caveat.
pub fn push_schema_column(
    out: &mut String,
    logical_name: &str,
    oid: i64,
    typmod: i32,
    caveat: Option<&str>,
) {
    out.push_str(logical_name);
    out.push('\t');
    out.push_str(&oid.to_string());
    out.push('\t');
    out.push_str(&typmod.to_string());
    out.push('\t');
    out.push_str(caveat.unwrap_or(""));
    out.push('\n');
}

/// Read one sealed import and write the catalog document.
pub async fn catalog_import() -> Result<(), u8> {
    match execute().await {
        Ok(document) => write_stdout(&document),
        Err(stop) => {
            stop.emit();
            Err(1)
        }
    }
}

/// Read one sealed PostgreSQL import. The stored engine stays `postgresql`.
pub async fn catalog_import_pg() -> Result<(), u8> {
    match execute_pg().await {
        Ok(document) => write_stdout(&document),
        Err(stop) => {
            stop.emit();
            Err(1)
        }
    }
}

/// Install migration `0006` on an administrator connection.
pub async fn install_sealed_catalog_cli() -> Result<(), u8> {
    match install_execute().await {
        Ok(document) => match io::stdout().write_all(document.as_bytes()) {
            Ok(()) => Ok(()),
            Err(_) => {
                eprintln!("oo-pipeline: catalog output");
                Err(1)
            }
        },
        Err(stop) => {
            stop.emit();
            Err(1)
        }
    }
}

async fn execute() -> Result<Vec<u8>, Stop> {
    let text = read_bounded_stdin("catalog input")?;
    let request = parse_request(&text)?;
    let password = read_password(&request.target.password_file, "target password file")?;
    let ca = read_ca(&request.target.ca_file, "target ca file")?;
    let mut conn = connect(
        &request.target.host,
        request.target.port,
        &request.target.database,
        &request.target.runtime_principal,
        &password,
        &ca,
    )
    .await?;
    expect_principal(&mut conn, &request.target.runtime_principal).await?;
    set_timeout(&mut conn).await?;
    verify_ready(&mut conn).await?;
    let mut tx = conn
        .begin()
        .await
        .map_err(|_| Stop::Fixed("target database"))?;
    if sqlx::raw_sql("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return Err(Stop::Fixed("target database"));
    }
    let identity = match read_identity(&mut tx, &request).await {
        Ok(identity) => identity,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    let document = match bind_catalog(&mut tx, &request, &identity, CatalogKind::Mysql).await {
        Ok(document) => document,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    tx.commit()
        .await
        .map_err(|_| Stop::Fixed("target database"))?;
    Ok(document)
}

async fn execute_pg() -> Result<Vec<u8>, Stop> {
    let text = read_bounded_stdin("catalog input")?;
    let (request, owner) = parse_pg_request(&text)?;
    let password = read_password(&request.target.password_file, "target password file")?;
    let ca = read_ca(&request.target.ca_file, "target ca file")?;
    let mut conn = connect(
        &request.target.host,
        request.target.port,
        &request.target.database,
        &request.target.runtime_principal,
        &password,
        &ca,
    )
    .await?;
    expect_principal(&mut conn, &request.target.runtime_principal).await?;
    set_timeout(&mut conn).await?;
    verify_ready(&mut conn).await?;
    let mut tx = conn
        .begin()
        .await
        .map_err(|_| Stop::Fixed("target database"))?;
    if sqlx::raw_sql("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return Err(Stop::Fixed("target database"));
    }
    let identity = match read_identity(&mut tx, &request).await {
        Ok(identity) => identity,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    if let Err(err) = assert_owner(&mut tx, &request, &owner).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let document = match bind_catalog(&mut tx, &request, &identity, CatalogKind::Postgresql).await {
        Ok(document) => document,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    tx.commit()
        .await
        .map_err(|_| Stop::Fixed("target database"))?;
    Ok(document)
}

async fn install_execute() -> Result<String, Stop> {
    let text = read_bounded_stdin("catalog input")?;
    let value = oo_core::parse_json_value(&text, INSTALL_PROTOCOL)
        .map_err(|_| Stop::Fixed("invalid input"))?;
    let request: InstallRequest =
        serde_json::from_value(value).map_err(|_| Stop::Fixed("invalid input"))?;
    if request.protocol != INSTALL_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    let password = read_password(&request.target.password_file, "target password file")?;
    let ca = read_ca(&request.target.ca_file, "target ca file")?;
    let mut conn = connect(
        &request.target.host,
        request.target.port,
        &request.target.database,
        &request.target.administrator,
        &password,
        &ca,
    )
    .await?;
    expect_principal(&mut conn, &request.target.administrator).await?;
    if !administrator(&mut conn).await? {
        return Err(Stop::Fixed("target administrator"));
    }
    let outcome = install_sealed_catalog(&mut conn)
        .await
        .map_err(install_stop)?;
    let digest = compiled_sealed_catalog_digest();
    Ok(format!(
        "{}\n",
        serde_json::to_string(&json!({
            "digest": digest,
            "outcome": outcome.to_string(),
            "protocol": INSTALL_RESULT
        }))
        .map_err(|_| Stop::Fixed("catalog output"))?
    ))
}

fn parse_request(text: &str) -> Result<Request, Stop> {
    let value = oo_core::parse_json_value(text, REQUEST_PROTOCOL)
        .map_err(|_| Stop::Fixed("invalid input"))?;
    let request: Request =
        serde_json::from_value(value).map_err(|_| Stop::Fixed("invalid input"))?;
    if request.protocol != REQUEST_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    if request.manifest.protocol != MANIFEST_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    if request.manifest.engine != "mysql" {
        return Err(Stop::Fixed("source engine"));
    }
    validate_closed(&request)?;
    Ok(request)
}

fn parse_pg_request(text: &str) -> Result<(Request, String), Stop> {
    let value = oo_core::parse_json_value(text, PG_REQUEST_PROTOCOL)
        .map_err(|_| Stop::Fixed("invalid input"))?;
    let protocol = value
        .get("protocol")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let manifest_protocol = value
        .pointer("/manifest/protocol")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if protocol != PG_REQUEST_PROTOCOL || manifest_protocol != PG_MANIFEST_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    let parsed: PgRequest =
        serde_json::from_value(value).map_err(|_| Stop::Fixed("invalid input"))?;
    if parsed.protocol != PG_REQUEST_PROTOCOL || parsed.manifest.protocol != PG_MANIFEST_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    if parsed.manifest.engine != "postgresql" {
        return Err(Stop::Fixed("source engine"));
    }
    if !closed_owner(&parsed.owner) {
        return Err(Stop::Fixed("invalid input"));
    }
    let request = Request {
        protocol: parsed.protocol,
        source_id: parsed.source_id,
        config_digest: parsed.config_digest,
        source_generation: parsed.source_generation,
        manifest: parsed.manifest,
        target: parsed.target,
    };
    validate_closed(&request)?;
    Ok((request, parsed.owner))
}

fn validate_closed(request: &Request) -> Result<(), Stop> {
    if !closed_source(&request.source_id)
        || request.source_id != request.manifest.source_id
        || !closed_digest(&request.config_digest)
        || request.config_digest != request.manifest.config_digest
        || request.source_generation == 0
        || request.source_generation != request.manifest.source_generation
        || !closed_digest(&request.manifest.schema_digest)
        || !closed_digest(&request.manifest.type_digest)
        || !closed_digest(&request.manifest.input_digest)
        || !closed_digest(&request.manifest.hook_digest)
        || request.manifest.tables.is_empty()
        || request.manifest.tables.len() > 16
    {
        return Err(Stop::Fixed("invalid input"));
    }
    for table in &request.manifest.tables {
        if table.logical_schema.is_empty()
            || table.logical_table.is_empty()
            || table.columns.is_empty()
            || table.columns.len() > 64
            || !closed_digest(&table.canonical_digest)
        {
            return Err(Stop::Fixed("invalid input"));
        }
        for column in &table.columns {
            if column.logical_name.is_empty() {
                return Err(Stop::Fixed("invalid input"));
            }
        }
        for reject in &table.rejects {
            if !closed_reason(&reject.reason) {
                return Err(Stop::Fixed("invalid input"));
            }
            if request.manifest.payload_includes_reject_lines
                && (reject.attnum.is_none() || reject.value_len.is_none())
            {
                return Err(Stop::Fixed("quality claim"));
            }
        }
    }
    Ok(())
}

async fn verify_ready(conn: &mut sqlx::postgres::PgConnection) -> Result<(), Stop> {
    verify_full_import_readiness(&mut *conn)
        .await
        .map_err(ready_stop)?;
    let sealed = verify_sealed_catalog_readiness(&mut *conn)
        .await
        .map_err(ready_stop)?;
    if sealed.digest() != compiled_sealed_catalog_digest() {
        return Err(Stop::Fixed("migration checksum"));
    }
    Ok(())
}

async fn read_identity(conn: &mut sqlx::PgConnection, request: &Request) -> Result<Identity, Stop> {
    let generation =
        i64::try_from(request.source_generation).map_err(|_| Stop::Fixed("invalid input"))?;
    let text: String =
        sqlx::query_scalar("SELECT _oo.full_import_sealed_identity($1, $2, $3)::text")
            .bind(&request.source_id)
            .bind(&request.config_digest)
            .bind(generation)
            .fetch_one(&mut *conn)
            .await
            .map_err(map_db)?;
    let identity: Identity =
        serde_json::from_str(&text).map_err(|_| Stop::Fixed("catalog output"))?;
    if identity.state != "awaiting_publish"
        || identity.source_id != request.source_id
        || identity.config_digest != request.config_digest
        || identity.source_generation != generation
        || identity.run_id != request.manifest.run_id
        || identity.target_generation != request.manifest.target_generation
        || identity.schema_digest != request.manifest.schema_digest
        || identity.type_digest != request.manifest.type_digest
        || identity.input_digest != request.manifest.input_digest
    {
        return Err(Stop::Fixed("sealed import is absent"));
    }
    Ok(identity)
}

enum CatalogKind {
    Mysql,
    Postgresql,
}

async fn assert_owner(
    conn: &mut sqlx::PgConnection,
    request: &Request,
    owner: &str,
) -> Result<(), Stop> {
    let generation =
        i64::try_from(request.source_generation).map_err(|_| Stop::Fixed("invalid input"))?;
    let matched: bool =
        sqlx::query_scalar("SELECT _oo.full_import_sealed_owner($1, $2, $3, $4::uuid)")
            .bind(&request.source_id)
            .bind(&request.config_digest)
            .bind(generation)
            .bind(owner)
            .fetch_one(&mut *conn)
            .await
            .map_err(map_db)?;
    if matched {
        Ok(())
    } else {
        Err(Stop::Fixed("sealed import owner"))
    }
}

async fn stored_columns(
    conn: &mut sqlx::PgConnection,
    request: &Request,
) -> Result<Vec<StoredColumn>, Stop> {
    let generation =
        i64::try_from(request.source_generation).map_err(|_| Stop::Fixed("invalid input"))?;
    let text: String =
        sqlx::query_scalar("SELECT _oo.full_import_sealed_columns($1, $2, $3)::text")
            .bind(&request.source_id)
            .bind(&request.config_digest)
            .bind(generation)
            .fetch_one(&mut *conn)
            .await
            .map_err(map_db)?;
    serde_json::from_str(&text).map_err(|_| Stop::Fixed("catalog output"))
}

async fn bind_catalog(
    conn: &mut sqlx::PgConnection,
    request: &Request,
    identity: &Identity,
    kind: CatalogKind,
) -> Result<Vec<u8>, Stop> {
    let preimage = manifest_preimage(&request.manifest);
    let line_digest = target_installer::sha256_hex(preimage.as_bytes());
    let mut pg_columns = None;
    let (engine, schema_digest) = match kind {
        CatalogKind::Mysql => {
            if line_digest != request.manifest.schema_digest
                || line_digest != identity.schema_digest
            {
                return Err(Stop::Fixed("schema digest"));
            }
            ("mysql", line_digest)
        }
        CatalogKind::Postgresql => {
            if request.manifest.schema_digest != identity.schema_digest {
                return Err(Stop::Fixed("schema digest"));
            }
            let stored = stored_columns(conn, request).await?;
            if !column_set_matches(&request.manifest, &stored) {
                return Err(Stop::Fixed("table coverage"));
            }
            pg_columns = Some(stored);
            // The sealed run stores the PostgreSQL connector schema hash.
            // sealedBuilder requires schema_digest to be this logical preimage.
            ("postgresql", line_digest)
        }
    };
    if request.manifest.tables.len() != identity.tables.len() {
        return Err(Stop::Fixed("sealed import is partial"));
    }
    let mut previous_index = -1_i32;
    for stored in &identity.tables {
        if stored.table_index <= previous_index {
            return Err(Stop::Fixed("catalog output"));
        }
        previous_index = stored.table_index;
    }
    let generation = identity.source_generation;
    let mut tables = Vec::new();
    for (manifest_table, stored) in request.manifest.tables.iter().zip(&identity.tables) {
        if manifest_table.logical_schema != stored.schema_ident
            || manifest_table.logical_table != stored.table_ident
            || manifest_table.canonical_digest != stored.canonical_digest
            || i64::try_from(manifest_table.accepted_rows).ok().as_ref()
                != Some(&stored.applied_rows)
        {
            return Err(Stop::Fixed("table coverage"));
        }
        let namespace = lookup(
            conn,
            &request.source_id,
            &request.config_digest,
            generation,
            "namespace",
            &manifest_table.logical_schema,
            "",
            "",
        )
        .await?;
        let table_physical = lookup(
            conn,
            &request.source_id,
            &request.config_digest,
            generation,
            "table",
            &manifest_table.logical_schema,
            &manifest_table.logical_table,
            "",
        )
        .await?;
        if table_physical != stored.physical_name {
            return Err(Stop::Fixed("physical mapping"));
        }
        let mut columns = Vec::new();
        for column in &manifest_table.columns {
            let physical = lookup(
                conn,
                &request.source_id,
                &request.config_digest,
                generation,
                "column",
                &manifest_table.logical_schema,
                &manifest_table.logical_table,
                &column.logical_name,
            )
            .await?;
            if let Some(stored_cols) = pg_columns.as_ref() {
                let bound = stored_cols.iter().find(|item| {
                    item.logical_namespace == manifest_table.logical_schema
                        && item.logical_table == manifest_table.logical_table
                        && item.logical_column == column.logical_name
                });
                if bound.map(|item| item.physical_name.as_str()) != Some(physical.as_str()) {
                    return Err(Stop::Fixed("physical mapping"));
                }
            }
            columns.push(json!({
                "caveat": column.caveat,
                "key": column.key,
                "logical_name": column.logical_name,
                "oid": column.oid,
                "source_type": column.source_type,
                "stored_physical": physical,
                "typmod": column.typmod
            }));
        }
        tables.push(json!({
            "accepted_rows": manifest_table.accepted_rows,
            "canonical_digest": stored.canonical_digest,
            "columns": columns,
            "logical_schema": manifest_table.logical_schema,
            "logical_table": manifest_table.logical_table,
            "rejects": manifest_table.rejects,
            "stored_namespace": namespace,
            "stored_physical": table_physical
        }));
    }
    let quality = quality_view(identity, &request.manifest)?;
    let body = json!({
        "capture": "sealed_import",
        "config_digest": identity.config_digest,
        "engine": engine,
        "hook_checked": false,
        "input_digest": identity.input_digest,
        "protocol": RESULT_PROTOCOL,
        "quality": quality,
        "run_id": identity.run_id,
        "schema_digest": schema_digest,
        "shadows_queried": false,
        "source_generation": identity.source_generation,
        "source_id": identity.source_id,
        "source_introspection": "not_used",
        "state": identity.state,
        "tables": tables,
        "target_generation": identity.target_generation,
        "tool_hook_digest": request.manifest.hook_digest,
        "type_digest": identity.type_digest
    });
    let bytes = serde_json::to_vec(&body).map_err(|_| Stop::Fixed("catalog output"))?;
    if bytes.len() > oo_core::limits::DOCUMENT_MAX_BYTES {
        return Err(Stop::Fixed("catalog output"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Stop::Fixed("catalog output"))?;
    if !text.contains(RESULT_PROTOCOL) || text.contains("oo.source.pgsql/v1") {
        return Err(Stop::Fixed("catalog output"));
    }
    Ok(bytes)
}

fn manifest_preimage(manifest: &Manifest) -> String {
    let mut out = String::new();
    for table in &manifest.tables {
        push_schema_table(&mut out, &table.logical_schema, &table.logical_table);
        for column in &table.columns {
            push_schema_column(
                &mut out,
                &column.logical_name,
                column.oid,
                column.typmod,
                column.caveat.as_deref(),
            );
        }
    }
    out
}

fn quality_view(identity: &Identity, manifest: &Manifest) -> Result<Value, Stop> {
    let physicals: Vec<&str> = identity
        .tables
        .iter()
        .map(|table| table.physical_name.as_str())
        .collect();
    let mut stored_rejected = 0_i64;
    let mut lines = Vec::new();
    for row in &identity.quality {
        if !physicals.contains(&row.physical_name.as_str()) {
            return Err(Stop::Fixed("quality identity"));
        }
        if row.kind == "batch" {
            stored_rejected = stored_rejected.saturating_add(row.rejected_rows.unwrap_or(0));
        } else if row.kind == "reject" {
            lines.push(row);
        } else {
            return Err(Stop::Fixed("quality identity"));
        }
    }
    let manifest_count: usize = manifest
        .tables
        .iter()
        .map(|table| table.rejects.len())
        .sum();
    if manifest.payload_includes_reject_lines {
        if stored_rejected != i64::try_from(manifest_count).unwrap_or(i64::MAX)
            || lines.len() != manifest_count
        {
            return Err(Stop::Fixed("quality claim"));
        }
        for table in &manifest.tables {
            let physical = identity
                .tables
                .iter()
                .find(|stored| {
                    stored.schema_ident == table.logical_schema
                        && stored.table_ident == table.logical_table
                })
                .map(|stored| stored.physical_name.as_str())
                .ok_or(Stop::Fixed("quality claim"))?;
            for reject in &table.rejects {
                let found = lines.iter().any(|row| {
                    row.physical_name == physical
                        && row.reason.as_deref() == Some(reject.reason.as_str())
                        && row.attnum == reject.attnum
                        && row.value_len == reject.value_len
                        && row.row_index
                            == Some(i64::try_from(reject.row_index).unwrap_or(i64::MAX))
                        && key_same(row.key_digest.as_deref(), reject.key_digest.as_deref())
                });
                if !found {
                    return Err(Stop::Fixed("quality claim"));
                }
            }
        }
    } else if stored_rejected != 0 || !lines.is_empty() {
        return Err(Stop::Fixed("quality claim"));
    }
    // The compiler accepts one batch row per sealed table. accepted_rows is the
    // sum of the stored chunk batches and must equal applied_rows. A multi-chunk
    // table uses t{table_index}.b0 as that table slot. Chunk rows stay in
    // _oo.full_import_quality.
    let mut projected = Vec::new();
    for stored in &identity.tables {
        let batches: Vec<&QualityRow> = identity
            .quality
            .iter()
            .filter(|row| row.kind == "batch" && row.physical_name == stored.physical_name)
            .collect();
        if batches.is_empty() {
            return Err(Stop::Fixed("quality identity"));
        }
        let mut accepted = 0_i64;
        let mut rejected = 0_i64;
        for row in &batches {
            if row.reason.is_some()
                || row.attnum.is_some()
                || row.value_len.is_some()
                || row.row_index.is_some()
                || row
                    .key_digest
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
            {
                return Err(Stop::Fixed("quality identity"));
            }
            let row_accepted = row.accepted_rows.ok_or(Stop::Fixed("quality identity"))?;
            let row_rejected = row.rejected_rows.unwrap_or(0);
            if row_accepted < 0 || row_rejected < 0 {
                return Err(Stop::Fixed("quality identity"));
            }
            accepted = accepted
                .checked_add(row_accepted)
                .ok_or(Stop::Fixed("quality identity"))?;
            rejected = rejected
                .checked_add(row_rejected)
                .ok_or(Stop::Fixed("quality identity"))?;
        }
        if accepted != stored.applied_rows {
            return Err(Stop::Fixed("quality claim"));
        }
        if batches.len() == 1 {
            projected.push((*batches[0]).clone());
        } else if (0..1_000_000).contains(&stored.table_index) {
            projected.push(QualityRow {
                physical_name: stored.physical_name.clone(),
                batch_id: format!("t{}.b0", stored.table_index),
                kind: "batch".to_string(),
                reason: None,
                value_len: None,
                attnum: None,
                row_index: None,
                key_digest: None,
                accepted_rows: Some(accepted),
                rejected_rows: Some(rejected),
            });
        } else {
            return Err(Stop::Fixed("quality identity"));
        }
    }
    for row in &lines {
        projected.push((*row).clone());
    }
    let manifest_rejects: usize = manifest_count;
    Ok(json!({
        "loss_concealed": false,
        "manifest_reject_count": manifest_rejects,
        "payload_includes_reject_lines": manifest.payload_includes_reject_lines,
        "stored_omits_manifest_rejects": !manifest.payload_includes_reject_lines && manifest_rejects > 0,
        "stored_rejected_rows": stored_rejected,
        "stored_rows": projected
    }))
}

fn column_set_matches(manifest: &Manifest, stored: &[StoredColumn]) -> bool {
    let mut expected = BTreeSet::new();
    for column in stored {
        if column.logical_namespace.is_empty()
            || column.logical_table.is_empty()
            || column.logical_column.is_empty()
            || !expected.insert((
                column.logical_namespace.clone(),
                column.logical_table.clone(),
                column.logical_column.clone(),
            ))
        {
            return false;
        }
    }
    if expected.is_empty() {
        return false;
    }
    let mut seen = BTreeSet::new();
    for table in &manifest.tables {
        for column in &table.columns {
            if !seen.insert((
                table.logical_schema.clone(),
                table.logical_table.clone(),
                column.logical_name.clone(),
            )) {
                return false;
            }
        }
    }
    seen == expected
}

fn key_same(stored: Option<&str>, manifest: Option<&str>) -> bool {
    let stored = stored.filter(|value| !value.is_empty());
    let manifest = manifest.filter(|value| !value.is_empty());
    stored == manifest
}

#[allow(clippy::too_many_arguments)]
async fn lookup(
    conn: &mut sqlx::PgConnection,
    source: &str,
    config_digest: &str,
    generation: i64,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> Result<String, Stop> {
    sqlx::query_scalar(LOOKUP_SQL)
        .bind(source)
        .bind(config_digest)
        .bind(generation)
        .bind(level)
        .bind(namespace)
        .bind(table)
        .bind(column)
        .fetch_one(&mut *conn)
        .await
        .map_err(map_db)
}

async fn connect(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ca: &[u8],
) -> Result<sqlx::postgres::PgConnection, Stop> {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(username)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    if options.get_options().is_some() {
        return Err(Stop::Fixed("target connect"));
    }
    let options = options.disable_statement_logging();
    match tokio::time::timeout(Duration::from_secs(30), options.connect()).await {
        Ok(Ok(conn)) => Ok(conn),
        _ => Err(Stop::Fixed("target connect")),
    }
}

async fn expect_principal(
    conn: &mut sqlx::postgres::PgConnection,
    principal: &str,
) -> Result<(), Stop> {
    let row = sqlx::query("SELECT CURRENT_USER::text, SESSION_USER::text")
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| Stop::Fixed("target principal"))?;
    let current: String = row
        .try_get(0)
        .map_err(|_| Stop::Fixed("target principal"))?;
    let session: String = row
        .try_get(1)
        .map_err(|_| Stop::Fixed("target principal"))?;
    if current == principal && session == principal {
        Ok(())
    } else {
        Err(Stop::Fixed("target principal"))
    }
}

async fn set_timeout(conn: &mut sqlx::postgres::PgConnection) -> Result<(), Stop> {
    sqlx::raw_sql("SET statement_timeout = '30s'")
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|_| Stop::Fixed("target database"))
}

async fn administrator(conn: &mut sqlx::postgres::PgConnection) -> Result<bool, Stop> {
    let row = sqlx::query("SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user")
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| Stop::Fixed("target database"))?;
    let superuser: bool = row.try_get(0).map_err(|_| Stop::Fixed("target database"))?;
    if superuser {
        return Ok(true);
    }
    let member: bool = sqlx::query_scalar(
        "SELECT pg_catalog.pg_has_role(current_user, 'oo_target_owner', 'MEMBER')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| Stop::Fixed("target administrator"))?;
    Ok(member)
}

fn map_db(err: sqlx::Error) -> Stop {
    let text = err.to_string();
    if text.contains("sealed import is absent") {
        Stop::Fixed("sealed import is absent")
    } else if text.contains("sealed import is ambiguous") {
        Stop::Fixed("sealed import is ambiguous")
    } else if text.contains("sealed import is partial") {
        Stop::Fixed("sealed import is partial")
    } else if text.contains("physical mapping is absent") || text.contains("OO024") {
        Stop::Fixed("physical mapping is absent")
    } else if text.contains("sealed import identity is closed") {
        Stop::Fixed("invalid input")
    } else if text.contains("sealed import owner") {
        Stop::Fixed("sealed import owner")
    } else if text.contains("full_import_sealed_identity") && text.contains("does not exist") {
        Stop::Fixed("sealed catalog is not installed")
    } else if text.contains("full_import_sealed_owner") && text.contains("does not exist")
        || text.contains("full_import_sealed_columns") && text.contains("does not exist")
    {
        Stop::Fixed("sealed owner check is not installed")
    } else {
        Stop::Fixed("target database")
    }
}

fn ready_stop(err: ReadinessError) -> Stop {
    match err {
        ReadinessError::NotInstalled | ReadinessError::Incomplete => {
            Stop::Fixed("sealed catalog is not installed")
        }
        ReadinessError::ChecksumMismatch { .. } => Stop::Fixed("migration checksum"),
        ReadinessError::Database { .. } => Stop::Fixed("target database"),
    }
}

fn install_stop(err: InstallError) -> Stop {
    match err {
        InstallError::NotAdministrator => Stop::Fixed("target administrator"),
        InstallError::ChecksumMismatch { .. } => Stop::Fixed("migration checksum"),
        InstallError::MissingRelation { .. } | InstallError::Refused { .. } => {
            Stop::Fixed("target catalog refused")
        }
        InstallError::Database { sqlstate, .. } if sqlstate == "OO001" => {
            Stop::Fixed("migration checksum")
        }
        InstallError::Database { .. } => Stop::Fixed("target database"),
    }
}

fn write_stdout(document: &[u8]) -> Result<(), u8> {
    match io::stdout().write_all(document) {
        Ok(()) => Ok(()),
        Err(_) => {
            eprintln!("oo-pipeline: catalog output");
            Err(1)
        }
    }
}

fn closed_owner(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        let hyphen = matches!(index, 8 | 13 | 18 | 23);
        (hyphen && *byte == b'-') || (!hyphen && byte.is_ascii_hexdigit())
    })
}

fn closed_source(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn closed_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn closed_reason(value: &str) -> bool {
    (1..=80).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PgRequest {
    protocol: String,
    source_id: String,
    config_digest: String,
    source_generation: u64,
    owner: String,
    manifest: Manifest,
    target: TargetIn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredColumn {
    logical_namespace: String,
    logical_table: String,
    logical_column: String,
    physical_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    source_id: String,
    config_digest: String,
    source_generation: u64,
    manifest: Manifest,
    target: TargetIn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    protocol: String,
    engine: String,
    source_id: String,
    config_digest: String,
    source_generation: u64,
    target_generation: i64,
    run_id: String,
    schema_digest: String,
    type_digest: String,
    input_digest: String,
    hook_digest: String,
    payload_includes_reject_lines: bool,
    tables: Vec<ManifestTable>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestTable {
    logical_schema: String,
    logical_table: String,
    accepted_rows: u64,
    canonical_digest: String,
    columns: Vec<ManifestColumn>,
    rejects: Vec<ManifestReject>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestColumn {
    logical_name: String,
    oid: i64,
    typmod: i32,
    caveat: Option<String>,
    #[serde(default)]
    key: Option<bool>,
    #[serde(default)]
    source_type: Option<String>,
}

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestReject {
    column: String,
    reason: String,
    row_index: u64,
    #[serde(default)]
    attnum: Option<i32>,
    #[serde(default)]
    value_len: Option<i64>,
    #[serde(default)]
    key_digest: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetIn {
    host: String,
    port: u16,
    database: String,
    runtime_principal: String,
    password_file: String,
    ca_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallRequest {
    protocol: String,
    target: InstallTarget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallTarget {
    host: String,
    port: u16,
    database: String,
    administrator: String,
    password_file: String,
    ca_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    state: String,
    run_id: String,
    source_id: String,
    source_generation: i64,
    target_generation: i64,
    config_digest: String,
    type_digest: String,
    input_digest: String,
    schema_digest: String,
    tables: Vec<IdentityTable>,
    quality: Vec<QualityRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityTable {
    table_index: i32,
    schema_ident: String,
    table_ident: String,
    applied_rows: i64,
    canonical_digest: String,
    physical_name: String,
}

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct QualityRow {
    physical_name: String,
    batch_id: String,
    kind: String,
    reason: Option<String>,
    value_len: Option<i64>,
    attnum: Option<i32>,
    row_index: Option<i64>,
    key_digest: Option<String>,
    accepted_rows: Option<i64>,
    rejected_rows: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_and_sql_fields_are_rejected() {
        let body = r#"{"protocol":"oo.pipeline.catalog-import/v1","source_id":"mysqlpreview","config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_generation":1,"manifest":{"protocol":"oo.mysql-preview.manifest/v1","engine":"mysql","physical_name":"pt_x"},"target":{"host":"127.0.0.1","port":1,"database":"db","runtime_principal":"oo_runtime","password_file":"pw","ca_file":"ca"}}"#;
        assert!(matches!(
            parse_request(body),
            Err(Stop::Fixed("invalid input"))
        ));
        let sql = body.replace("physical_name", "sql");
        assert!(parse_request(&sql).is_err());
    }

    #[test]
    fn pg_protocol_rejects_the_mysql_document_and_a_mysql_engine() {
        let mysql = r#"{"protocol":"oo.pipeline.catalog-import/v1","source_id":"oo_dl01w1_46","config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_generation":1,"owner":"b5ac4bc8-1872-4fe4-a56a-04a70f7103a2","manifest":{"protocol":"oo.mysql-preview.manifest/v1","engine":"postgresql","source_id":"oo_dl01w1_46","config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_generation":1,"target_generation":1,"run_id":"e8908a10-4cfc-4aa4-b64c-bc45999cd715","schema_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","type_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","input_digest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","hook_digest":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","payload_includes_reject_lines":false,"tables":[{"logical_schema":"public","logical_table":"hubs","accepted_rows":1,"canonical_digest":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","columns":[{"logical_name":"hub_id","oid":20,"typmod":-1,"caveat":null,"key":false}],"rejects":[]}]},"target":{"host":"127.0.0.1","port":1,"database":"db","runtime_principal":"oo_admin","password_file":"/pw","ca_file":"/ca"}}"#;
        assert!(matches!(
            parse_pg_request(mysql),
            Err(Stop::Fixed("invalid version"))
        ));
        let bare = mysql.replace(",\"owner\":\"b5ac4bc8-1872-4fe4-a56a-04a70f7103a2\"", "");
        assert!(matches!(
            parse_pg_request(&bare),
            Err(Stop::Fixed("invalid version"))
        ));
        let missing_owner = bare
            .replace(
                "oo.pipeline.catalog-import/v1",
                "oo.pipeline.catalog-import-pg/v1",
            )
            .replace(
                "oo.mysql-preview.manifest/v1",
                "oo.pipeline.pg-sealed-manifest/v1",
            );
        assert!(matches!(
            parse_pg_request(&missing_owner),
            Err(Stop::Fixed("invalid input"))
        ));
        let wrong_engine = mysql
            .replace(
                "oo.pipeline.catalog-import/v1",
                "oo.pipeline.catalog-import-pg/v1",
            )
            .replace(
                "oo.mysql-preview.manifest/v1",
                "oo.pipeline.pg-sealed-manifest/v1",
            )
            .replace("\"engine\":\"postgresql\"", "\"engine\":\"mysql\"");
        assert!(matches!(
            parse_pg_request(&wrong_engine),
            Err(Stop::Fixed("source engine"))
        ));
        let accepted = mysql
            .replace(
                "oo.pipeline.catalog-import/v1",
                "oo.pipeline.catalog-import-pg/v1",
            )
            .replace(
                "oo.mysql-preview.manifest/v1",
                "oo.pipeline.pg-sealed-manifest/v1",
            );
        let (request, owner) = match parse_pg_request(&accepted) {
            Ok(parsed) => parsed,
            Err(_) => panic!("pg request refused"),
        };
        assert_eq!(request.manifest.engine, "postgresql");
        assert_eq!(owner, "b5ac4bc8-1872-4fe4-a56a-04a70f7103a2");
        assert!(matches!(
            parse_request(&accepted),
            Err(Stop::Fixed("invalid input"))
        ));
    }

    #[test]
    fn postgresql_engine_is_refused() {
        let body = r#"{"protocol":"oo.pipeline.catalog-import/v1","source_id":"mysqlpreview","config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_generation":1,"manifest":{"protocol":"oo.mysql-preview.manifest/v1","engine":"postgresql","source_id":"mysqlpreview","config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_generation":1,"target_generation":1,"run_id":"de8276fc-8634-4543-994f-ce82a46a3c18","schema_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","type_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","input_digest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","hook_digest":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","payload_includes_reject_lines":false,"tables":[]},"target":{"host":"127.0.0.1","port":1,"database":"db","runtime_principal":"oo_runtime","password_file":"pw","ca_file":"ca"}}"#;
        assert!(matches!(
            parse_request(body),
            Err(Stop::Fixed("source engine"))
        ));
    }

    #[test]
    fn schema_preimage_uses_logical_names_and_caveats() {
        let mut out = String::new();
        push_schema_table(&mut out, "erp", "parcel");
        push_schema_column(&mut out, "day_on", 1082, -1, None);
        push_schema_column(
            &mut out,
            "clock_at",
            1083,
            -1,
            Some("mysql_time_not_interval"),
        );
        assert_eq!(
            out,
            "erp.parcel\nday_on\t1082\t-1\t\nclock_at\t1083\t-1\tmysql_time_not_interval\n"
        );
    }

    #[test]
    fn quality_projection_sums_chunk_batches_per_sealed_table() {
        let digest = "a".repeat(64);
        let identity = Identity {
            state: "awaiting_publish".to_string(),
            run_id: "de8276fc-8634-4543-994f-ce82a46a3c18".to_string(),
            source_id: "mysqlscale".to_string(),
            source_generation: 1,
            target_generation: 4,
            config_digest: digest.clone(),
            type_digest: digest.clone(),
            input_digest: digest.clone(),
            schema_digest: digest.clone(),
            tables: vec![
                IdentityTable {
                    table_index: 0,
                    schema_ident: "logistics".to_string(),
                    table_ident: "hubs".to_string(),
                    applied_rows: 2000,
                    canonical_digest: digest.clone(),
                    physical_name: "pt_hubs".to_string(),
                },
                IdentityTable {
                    table_index: 3,
                    schema_ident: "logistics".to_string(),
                    table_ident: "value_probe".to_string(),
                    applied_rows: 6,
                    canonical_digest: digest.clone(),
                    physical_name: "pt_probe".to_string(),
                },
            ],
            quality: vec![
                batch_row("pt_hubs", "t0.b0", 1000, 0),
                batch_row("pt_hubs", "t0.b1", 1000, 0),
                batch_row("pt_probe", "t3.b0", 6, 2),
                QualityRow {
                    physical_name: "pt_probe".to_string(),
                    batch_id: "t3.b0".to_string(),
                    kind: "reject".to_string(),
                    reason: Some("zero_date".to_string()),
                    value_len: Some(8),
                    attnum: Some(6),
                    row_index: Some(4),
                    key_digest: Some("c".repeat(64)),
                    accepted_rows: None,
                    rejected_rows: None,
                },
                QualityRow {
                    physical_name: "pt_probe".to_string(),
                    batch_id: "t3.b0".to_string(),
                    kind: "reject".to_string(),
                    reason: Some("time_range".to_string()),
                    value_len: Some(8),
                    attnum: Some(7),
                    row_index: Some(5),
                    key_digest: Some("d".repeat(64)),
                    accepted_rows: None,
                    rejected_rows: None,
                },
            ],
        };
        let manifest = Manifest {
            protocol: "oo.mysql-preview.manifest/v1".to_string(),
            engine: "mysql".to_string(),
            source_id: "mysqlscale".to_string(),
            config_digest: digest.clone(),
            source_generation: 1,
            target_generation: 4,
            run_id: identity.run_id.clone(),
            schema_digest: digest.clone(),
            type_digest: digest.clone(),
            input_digest: digest.clone(),
            hook_digest: digest,
            payload_includes_reject_lines: true,
            tables: vec![
                ManifestTable {
                    logical_schema: "logistics".to_string(),
                    logical_table: "hubs".to_string(),
                    accepted_rows: 2000,
                    canonical_digest: "b".repeat(64),
                    columns: Vec::new(),
                    rejects: Vec::new(),
                },
                ManifestTable {
                    logical_schema: "logistics".to_string(),
                    logical_table: "value_probe".to_string(),
                    accepted_rows: 6,
                    canonical_digest: "b".repeat(64),
                    columns: Vec::new(),
                    rejects: vec![
                        ManifestReject {
                            column: "opened_on".to_string(),
                            reason: "zero_date".to_string(),
                            row_index: 4,
                            attnum: Some(6),
                            value_len: Some(8),
                            key_digest: Some("c".repeat(64)),
                        },
                        ManifestReject {
                            column: "clock_at".to_string(),
                            reason: "time_range".to_string(),
                            row_index: 5,
                            attnum: Some(7),
                            value_len: Some(8),
                            key_digest: Some("d".repeat(64)),
                        },
                    ],
                },
            ],
        };
        let view = match quality_view(&identity, &manifest) {
            Ok(value) => value,
            Err(Stop::Fixed(message)) => panic!("projection stopped: {message}"),
            Err(_) => panic!("projection stopped"),
        };
        let rows = view
            .get("stored_rows")
            .and_then(|value| value.as_array())
            .expect("rows");
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0].get("batch_id").and_then(|value| value.as_str()),
            Some("t0.b0")
        );
        assert_eq!(
            rows[0]
                .get("accepted_rows")
                .and_then(|value| value.as_i64()),
            Some(2000)
        );
        assert_eq!(
            rows[0]
                .get("rejected_rows")
                .and_then(|value| value.as_i64()),
            Some(0)
        );
        assert_eq!(
            rows[1].get("batch_id").and_then(|value| value.as_str()),
            Some("t3.b0")
        );
        assert_eq!(
            rows[1]
                .get("accepted_rows")
                .and_then(|value| value.as_i64()),
            Some(6)
        );
        assert_eq!(
            rows[2].get("kind").and_then(|value| value.as_str()),
            Some("reject")
        );
        assert_eq!(
            view.get("stored_rejected_rows")
                .and_then(|value| value.as_i64()),
            Some(2)
        );
    }

    #[test]
    fn quality_projection_refuses_a_chunk_sum_that_misses_the_sealed_table() {
        let digest = "d".repeat(64);
        let identity = Identity {
            state: "awaiting_publish".to_string(),
            run_id: "de8276fc-8634-4543-994f-ce82a46a3c18".to_string(),
            source_id: "mysqlscale".to_string(),
            source_generation: 1,
            target_generation: 4,
            config_digest: digest.clone(),
            type_digest: digest.clone(),
            input_digest: digest.clone(),
            schema_digest: digest.clone(),
            tables: vec![IdentityTable {
                table_index: 0,
                schema_ident: "logistics".to_string(),
                table_ident: "hubs".to_string(),
                applied_rows: 2000,
                canonical_digest: digest.clone(),
                physical_name: "pt_hubs".to_string(),
            }],
            quality: vec![batch_row("pt_hubs", "t0.b0", 1000, 0)],
        };
        let manifest = Manifest {
            protocol: "oo.mysql-preview.manifest/v1".to_string(),
            engine: "mysql".to_string(),
            source_id: "mysqlscale".to_string(),
            config_digest: digest.clone(),
            source_generation: 1,
            target_generation: 4,
            run_id: identity.run_id.clone(),
            schema_digest: digest.clone(),
            type_digest: digest,
            input_digest: "e".repeat(64),
            hook_digest: "f".repeat(64),
            payload_includes_reject_lines: false,
            tables: vec![ManifestTable {
                logical_schema: "logistics".to_string(),
                logical_table: "hubs".to_string(),
                accepted_rows: 2000,
                canonical_digest: "b".repeat(64),
                columns: Vec::new(),
                rejects: Vec::new(),
            }],
        };
        match quality_view(&identity, &manifest) {
            Err(Stop::Fixed("quality claim")) => {}
            Ok(_) => panic!("mismatched chunk sum was accepted"),
            Err(Stop::Fixed(message)) => panic!("unexpected stop {message}"),
            Err(_) => panic!("unexpected stop"),
        }
    }

    fn batch_row(physical: &str, batch_id: &str, accepted: i64, rejected: i64) -> QualityRow {
        QualityRow {
            physical_name: physical.to_string(),
            batch_id: batch_id.to_string(),
            kind: "batch".to_string(),
            reason: None,
            value_len: None,
            attnum: None,
            row_index: None,
            key_digest: None,
            accepted_rows: Some(accepted),
            rejected_rows: Some(rejected),
        }
    }

    #[test]
    fn command_source_does_not_introspect_shadows() {
        let src = include_str!("catalog_import.rs");
        let forbidden = [
            concat!("pg_attr", "ibute"),
            concat!("pg_st", "ats"),
            concat!("obj_desc", "ription"),
        ];
        for word in forbidden {
            assert!(!src.contains(word));
        }
    }
}
