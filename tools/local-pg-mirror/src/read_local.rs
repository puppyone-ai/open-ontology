use std::path::PathBuf;
use std::time::Duration;

use sqlx::{Connection, Row};

use crate::error::MirrorError;
use crate::manifest::{load_manifest, load_target_config};
use crate::pg::{self, SessionKind};
use crate::plan::{read_ca, read_secret};
use oo_pipeline::target_installer::sha256_hex;

pub struct ReadRequest {
    pub target_config: PathBuf,
    pub manifest: PathBuf,
    pub reader_password: PathBuf,
    pub ca: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnView {
    pub name: String,
    pub oid: i64,
    pub typmod: i32,
}

#[derive(Debug)]
pub struct LocalTable {
    pub schema: String,
    pub name: String,
    pub rows: Vec<Vec<Option<String>>>,
    pub catalog: Vec<ColumnView>,
}

#[derive(Debug)]
pub struct LocalRead {
    pub tables: Vec<LocalTable>,
}

pub async fn read_local(request: ReadRequest) -> Result<LocalRead, MirrorError> {
    let manifest = load_manifest(&request.manifest)?.ok_or_else(|| MirrorError::new("config", "manifest_missing"))?;
    if manifest.mirror_state != "complete" || manifest.customer_complete {
        return Err(MirrorError::new("refused", "not_complete"));
    }
    if request.target_config.display().to_string() != manifest.target_config_path {
        return Err(MirrorError::new("config", "target_config_path"));
    }
    let target = load_target_config(&request.target_config, &manifest.target_config_sha256)?;
    let host = crate::jsonutil::take_str(&target, "host")?;
    let port = u16::try_from(crate::jsonutil::take_u64(&target, "port")?).map_err(|_| MirrorError::new("config", "port"))?;
    let database = crate::jsonutil::take_str(&target, "database")?;
    let user = crate::jsonutil::take_str(&target, "user")?;
    let ca_sha = crate::jsonutil::take_str(&target, "ca_sha256")?;
    let password = read_secret(&request.reader_password)?;
    let ca = read_ca(&request.ca)?;
    if sha256_hex(&ca) != ca_sha {
        return Err(MirrorError::new("ca_drift", "ca"));
    }
    let mut conn = pg::dial(
        host,
        port,
        user,
        password.as_str(),
        database,
        &ca,
        Duration::from_secs(30),
        SessionKind::Runtime,
    )
    .await?;
    let mut tx = conn.begin().await.map_err(pg::map_sqlx)?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(pg::map_sqlx)?;
    sqlx::query("SET LOCAL statement_timeout = '120s'")
        .execute(&mut *tx)
        .await
        .map_err(pg::map_sqlx)?;
    sqlx::query("SET LOCAL lock_timeout = '1s'")
        .execute(&mut *tx)
        .await
        .map_err(pg::map_sqlx)?;
    sqlx::query("SET LOCAL DateStyle TO 'ISO, YMD'")
        .execute(&mut *tx)
        .await
        .map_err(pg::map_sqlx)?;
    let mut tables = Vec::new();
    for table in &manifest.tables_completed {
        let physical = table
            .physical_name
            .as_deref()
            .ok_or_else(|| MirrorError::new("unsupported_read", "physical_name"))?;
        let quoted = oo_pipeline::physical_mapping::quote_physical_name(physical)
            .map_err(|_| MirrorError::new("unsupported_read", "physical_name"))?;
        let mut select = String::from("SELECT ");
        if table.columns.is_empty() {
            return Err(MirrorError::new("unsupported_read", "columns"));
        }
        for (index, column) in table.columns.iter().enumerate() {
            if index > 0 {
                select.push(',');
            }
            select.push_str(&select_expr(&column.physical_name, column.oid)?);
        }
        select.push_str(" FROM \"_oo\".");
        select.push_str(&quoted);
        select.push_str(" ORDER BY \"_oo_pos\"");
        let fetched = sqlx::query(sqlx::AssertSqlSafe(select))
            .fetch_all(&mut *tx)
            .await
            .map_err(pg::map_sqlx)?;
        if fetched.len() as u64 != table.rows {
            return Err(MirrorError::new("local_count", "rows"));
        }
        let mut rows = Vec::new();
        for row in &fetched {
            let mut cells = Vec::new();
            for index in 0..table.columns.len() {
                let cell: Option<String> = row.try_get(index).map_err(|_| MirrorError::new("unsupported_read", "cell"))?;
                cells.push(cell);
            }
            rows.push(cells);
        }
        let catalog = read_catalog(&mut tx, physical, &table.columns).await?;
        tables.push(LocalTable {
            schema: table.schema.clone(),
            name: table.name.clone(),
            rows,
            catalog,
        });
    }
    tx.commit().await.map_err(pg::map_sqlx)?;
    Ok(LocalRead { tables })
}

async fn read_catalog(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    physical: &str,
    columns: &[crate::manifest::ColumnRecord],
) -> Result<Vec<ColumnView>, MirrorError> {
    let attrs = sqlx::query(
        "SELECT a.attname::text, a.atttypid::int8, a.atttypmod::int4 \
         FROM pg_catalog.pg_attribute AS a \
         JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
         WHERE n.nspname = '_oo' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(physical)
    .fetch_all(&mut **tx)
    .await
    .map_err(pg::map_sqlx)?;
    if attrs.len() != columns.len() + 1 {
        return Err(MirrorError::new("refused", "type_mismatch"));
    }
    let mut catalog = Vec::with_capacity(attrs.len());
    for attr in &attrs {
        catalog.push(ColumnView {
            name: attr.try_get(0).map_err(pg::map_sqlx)?,
            oid: attr.try_get(1).map_err(pg::map_sqlx)?,
            typmod: attr.try_get(2).map_err(pg::map_sqlx)?,
        });
    }
    if catalog[0].name != "_oo_pos" || catalog[0].oid != 1700 {
        return Err(MirrorError::new("refused", "type_mismatch"));
    }
    for (view, column) in catalog.iter().skip(1).zip(columns.iter()) {
        if view.oid != column.oid || view.typmod != column.typmod || view.name != column.physical_name {
            return Err(MirrorError::new("refused", "type_mismatch"));
        }
    }
    Ok(catalog)
}

fn select_expr(physical: &str, oid: i64) -> Result<String, MirrorError> {
    let quoted = oo_pipeline::physical_mapping::quote_physical_name(physical)
        .map_err(|_| MirrorError::new("unsupported_read", "physical_name"))?;
    let expr = match oid {
        16 => format!("CASE WHEN {quoted} THEN 'true' WHEN NOT {quoted} THEN 'false' ELSE NULL END"),
        17 => format!("encode({quoted}, 'hex')"),
        20 | 21 | 23 | 25 | 114 | 700 | 701 | 1042 | 1043 | 1082 | 1083 | 1114 | 1184 | 1700 | 2950 => {
            format!("{quoted}::text")
        }
        _ => return Err(MirrorError::new("unsupported_read", "oid")),
    };
    Ok(expr)
}
