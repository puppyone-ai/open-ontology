//! Typed CDC overlay over immutable snapshot storage. The entire source batch
//! and its durable position commit together, across all selected tables.
use crate::{
    engine_load::Result,
    engine_load_pg::column,
    native_arrow::{DecodedStream, NativeCell},
    native_c1_apply::cell_text,
};
use oo_connectors::engine::{
    ChangeBatch, ChunkStrategy, KeyValue, RowChange, TableChunkPlan, TableRef, Watermark,
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection, Postgres, Transaction};
use sqlx_core::sql_str::AssertSqlSafe;

fn db(_: sqlx::Error) -> &'static str {
    "sync_target"
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn quote(value: &str) -> Result<String> {
    if value.is_empty() || value.len() > 63 || value.contains('\0') {
        return Err("sync_identifier");
    }
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}

struct Binding {
    table: TableRef,
    identity: String,
    run: String,
    columns: Vec<(String, String)>,
    keys: Vec<String>,
    delta: String,
    tombs: String,
}

pub struct PgSync<'a> {
    conn: &'a mut PgConnection,
    source: String,
    lineage: String,
    bindings: Vec<Binding>,
}

impl<'a> PgSync<'a> {
    /// `lineage` must be obtained from the live source, not a display name.
    /// `initial` is an already aligned baseline position, never "now".
    /// Caller owns and closes this dedicated connection, releasing its locks.
    pub async fn open(
        conn: &'a mut PgConnection,
        source: &str,
        lineage: &str,
        tables: &[TableRef],
        initial: &Watermark,
    ) -> Result<Self> {
        if source.is_empty() || lineage.is_empty() || tables.is_empty() || tables.len() > 128 {
            return Err("sync_scope");
        }
        let mut tables = tables.to_vec();
        tables.sort();
        if tables.windows(2).any(|p| p[0] == p[1]) {
            return Err("sync_scope");
        }
        let endpoint_binding = source;
        let source = hash(source.as_bytes());
        let mut tx = conn.begin().await.map_err(db)?;
        sqlx::raw_sql(include_str!("../migrations/0120_engine_sync.sql"))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        // The source-wide lock serializes multi-table transactions; table locks
        // additionally fence snapshot loaders using their existing identities.
        lock(&mut tx, &source).await?;
        let scope = serde_json::to_string(&tables).map_err(|_| "sync_scope")?;
        let previous: Option<(String, String)> =
            sqlx::query_as("SELECT lineage,scope::text FROM _oo_load.sync_state WHERE source=$1")
                .bind(&source)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
        if let Some((old_lineage, old_scope)) = &previous {
            let old_scope: Vec<TableRef> =
                serde_json::from_str(old_scope).map_err(|_| "sync_scope")?;
            if old_lineage != lineage || old_scope != tables {
                return Err("sync_rebaseline_required_lineage_or_scope");
            }
        } else {
            sqlx::query("INSERT INTO _oo_load.sync_state(source,lineage,scope,position) VALUES($1,$2,$3::jsonb,$4::jsonb)")
                .bind(&source).bind(lineage).bind(&scope)
                .bind(serde_json::to_string(initial).map_err(|_| "sync_position")?)
                .execute(&mut *tx).await.map_err(db)?;
        }
        let mut bindings = Vec::new();
        for table in tables {
            let identity =
                hash(&serde_json::to_vec(&(endpoint_binding, &table)).map_err(|_| "sync_scope")?);
            lock(&mut tx, &identity).await?;
            let (run, stage, plan): (String, String, String) = sqlx::query_as(
                "SELECT p.run,p.stage,p.plan::text FROM _oo_load.publications p
                 JOIN _oo_load.current c USING(identity,run) WHERE p.identity=$1",
            )
            .bind(&identity)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            let c5_installed: bool =
                sqlx::query_scalar("SELECT to_regclass('_oo_load.c5_state') IS NOT NULL")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db)?;
            if c5_installed {
                let c5: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM _oo_load.c5_state WHERE identity=$1 AND run=$2)",
                )
                .bind(&identity)
                .bind(&run)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
                if c5 {
                    return Err("sync_c5_handoff_required");
                }
            }
            let plan: TableChunkPlan = serde_json::from_str(&plan).map_err(|_| "sync_plan")?;
            let ChunkStrategy::Keyset { columns: keys } = plan.strategy else {
                return Err("sync_requires_stable_key");
            };
            if keys.is_empty() {
                return Err("sync_requires_stable_key");
            }
            let baseline = format!("_oo_load.{}", quote(&stage)?);
            let columns: Vec<(String, String)> = sqlx::query_as(
                "SELECT attname::text,format_type(atttypid,atttypmod)
                 FROM pg_attribute WHERE attrelid=$1::regclass AND attnum>0
                 AND NOT attisdropped AND attname<>'__oo_chunk' ORDER BY attnum",
            )
            .bind(&baseline)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
            if columns.is_empty() || keys.iter().any(|k| !columns.iter().any(|(n, _)| n == k)) {
                return Err("sync_schema");
            }
            let names = columns
                .iter()
                .map(|(n, _)| quote(n))
                .collect::<Result<Vec<_>>>()?
                .join(",");
            let key_names = keys
                .iter()
                .map(|n| quote(n))
                .collect::<Result<Vec<_>>>()?
                .join(",");
            let delta = format!("d_{}", &identity[..40]);
            let tombs = format!("x_{}", &identity[..40]);
            let saved: Option<(String, String)> = sqlx::query_as(
                "SELECT source,baseline_run FROM _oo_load.sync_bindings WHERE identity=$1",
            )
            .bind(&identity)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
            if let Some((old_source, old_run)) = saved {
                if old_source != source || old_run != run || previous.is_none() {
                    return Err("sync_rebaseline_required_binding");
                }
            } else {
                if previous.is_some() {
                    return Err("sync_incomplete_binding");
                }
                let predicate = keys
                    .iter()
                    .map(|k| quote(k).map(|q| format!("b.{q}=x.{q}")))
                    .collect::<Result<Vec<_>>>()?
                    .join(" AND ");
                let delta_q = quote(&delta)?;
                let tombs_q = quote(&tombs)?;
                sqlx::raw_sql(AssertSqlSafe(format!(
                    "CREATE TABLE _oo_load.{delta_q} AS SELECT {names} FROM {baseline} WITH NO DATA;
                     ALTER TABLE _oo_load.{delta_q} ADD PRIMARY KEY ({key_names});
                     CREATE TABLE _oo_load.{tombs_q} AS SELECT {key_names} FROM {baseline} WITH NO DATA;
                     ALTER TABLE _oo_load.{tombs_q} ADD PRIMARY KEY ({key_names});
                     CREATE OR REPLACE VIEW oo_load_data.{} AS
                     SELECT {names} FROM {baseline} b WHERE NOT EXISTS
                       (SELECT 1 FROM _oo_load.{tombs_q} x WHERE {predicate})
                     UNION ALL SELECT {names} FROM _oo_load.{delta_q};",
                    quote(&format!("t_{}", &identity[..40]))?
                ))).execute(&mut *tx).await.map_err(db)?;
                sqlx::query("INSERT INTO _oo_load.sync_bindings VALUES($1,$2,$3)")
                    .bind(&identity)
                    .bind(&source)
                    .bind(&run)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
            }
            bindings.push(Binding {
                table,
                identity,
                run,
                columns,
                keys,
                delta,
                tombs,
            });
        }
        tx.commit().await.map_err(db)?;
        Ok(Self {
            conn,
            source,
            lineage: lineage.into(),
            bindings,
        })
    }

    pub async fn position(&mut self) -> Result<Watermark> {
        let raw: String = sqlx::query_scalar(
            "SELECT position::text FROM _oo_load.sync_state WHERE source=$1 AND lineage=$2",
        )
        .bind(&self.source)
        .bind(&self.lineage)
        .fetch_one(&mut *self.conn)
        .await
        .map_err(db)?;
        serde_json::from_str(&raw).map_err(|_| "sync_position")
    }

    pub async fn metrics(&mut self) -> Result<(i64, i64)> {
        sqlx::query_as("SELECT generation,write_bytes FROM _oo_load.sync_state WHERE source=$1")
            .bind(&self.source)
            .fetch_one(&mut *self.conn)
            .await
            .map_err(db)
    }

    /// Images must be independently decoded from the batch's canonical IPC.
    /// No image is written for deletes; tombstones prevent baseline resurrection.
    pub async fn apply(
        &mut self,
        from: &Watermark,
        batch: &ChangeBatch,
        images: &[DecodedStream],
    ) -> Result<()> {
        self.apply_with_comparator(from, batch, images, crate::engine_sync::at_or_after)
            .await
    }

    /// The caller supplies an epoch-bound comparator for opaque source positions.
    pub async fn apply_with_comparator(
        &mut self,
        from: &Watermark,
        batch: &ChangeBatch,
        images: &[DecodedStream],
        compare: impl Fn(&Watermark, &Watermark) -> Result<bool>,
    ) -> Result<()> {
        if images.len() != batch.changes.len() || self.bindings.is_empty() {
            return Err("sync_batch_shape");
        }
        if !compare(&batch.position, from)? || compare(from, &batch.position)? {
            return Err("sync_position_not_advanced");
        }
        let mut tx = self.conn.begin().await.map_err(db)?;
        lock(&mut tx, &self.source).await?;
        let current: String = sqlx::query_scalar(
            "SELECT position::text FROM _oo_load.sync_state WHERE source=$1 AND lineage=$2 FOR UPDATE")
            .bind(&self.source).bind(&self.lineage)
            .fetch_one(&mut *tx).await.map_err(db)?;
        if serde_json::from_str::<Watermark>(&current).map_err(|_| "sync_position")? != *from {
            return Err("sync_stale_position");
        }
        let mut bytes = 0i64;
        for (change, image) in batch.changes.iter().zip(images) {
            let (table, key, old, deleted) = match change {
                RowChange::Insert { table, key } => (table, key, None, false),
                RowChange::Update {
                    table,
                    key,
                    old_key,
                } => (table, key, old_key.as_ref(), false),
                RowChange::Delete { table, key } => (table, key, None, true),
            };
            let b = self
                .bindings
                .iter()
                .find(|b| &b.table == table)
                .ok_or("sync_table_outside_scope")?;
            lock(&mut tx, &b.identity).await?;
            let current_run: String =
                sqlx::query_scalar("SELECT run FROM _oo_load.current WHERE identity=$1")
                    .bind(&b.identity)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db)?;
            if current_run != b.run {
                return Err("sync_rebaseline_required_baseline_changed");
            }
            if let Some(old) = old {
                bytes = bytes
                    .checked_add(remove(&mut tx, b, old).await?)
                    .ok_or("sync_byte_overflow")?;
            }
            bytes = bytes
                .checked_add(remove(&mut tx, b, key).await?)
                .ok_or("sync_byte_overflow")?;
            if !deleted {
                let row = encode(b, image)?;
                let columns = b
                    .columns
                    .iter()
                    .map(|(name, _)| quote(name))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                let mut copy = tx
                    .copy_in_raw(&format!(
                        "COPY _oo_load.{} ({columns}) FROM STDIN",
                        quote(&b.delta)?
                    ))
                    .await
                    .map_err(db)?;
                copy.send(row).await.map_err(db)?;
                if copy.finish().await.map_err(db)? != 1 {
                    return Err("sync_image_rows");
                }
                let (predicate, values) = key_predicate(b, key)?;
                let mut query = sqlx::query_scalar::<_, i64>(AssertSqlSafe(format!(
                    "SELECT pg_column_size(d)::bigint FROM _oo_load.{} d WHERE {predicate}",
                    quote(&b.delta)?
                )));
                for value in values {
                    query = query.bind(value);
                }
                let row_bytes = query
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(db)?
                    .ok_or("sync_image_key_mismatch")?;
                bytes = bytes.checked_add(row_bytes).ok_or("sync_byte_overflow")?;
            }
        }
        sqlx::query("UPDATE _oo_load.sync_state SET position=$2::jsonb,generation=generation+$3,write_bytes=write_bytes+$4 WHERE source=$1")
            .bind(&self.source).bind(serde_json::to_string(&batch.position).map_err(|_| "sync_position")?)
            .bind(i64::from(!batch.changes.is_empty())).bind(bytes)
            .execute(&mut *tx).await.map_err(db)?;
        tx.commit().await.map_err(db)
    }
}

fn key_predicate(b: &Binding, key: &[KeyValue]) -> Result<(String, Vec<String>)> {
    if key.len() != b.keys.len() {
        return Err("sync_key");
    }
    let values = key.iter().map(key_text).collect::<Result<Vec<_>>>()?;
    let predicates = b
        .keys
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let (_, kind) = b
                .columns
                .iter()
                .find(|(n, _)| n == name)
                .ok_or("sync_key")?;
            Ok(format!("{}=${}::text::{kind}", quote(name)?, i + 1))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((predicates.join(" AND "), values))
}

async fn lock(tx: &mut Transaction<'_, Postgres>, identity: &str) -> Result<()> {
    let locked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(identity)
            .fetch_one(&mut **tx)
            .await
            .map_err(db)?;
    if !locked {
        return Err("sync_target_busy");
    }
    Ok(())
}

fn key_text(key: &KeyValue) -> Result<String> {
    match key {
        KeyValue::Int(v) => Ok(v.to_string()),
        KeyValue::Text(v) | KeyValue::Decimal(v) => Ok(v.clone()),
        KeyValue::Bytes(v)
            if v.len().is_multiple_of(2) && v.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            Ok(format!("\\x{v}"))
        }
        _ => Err("sync_key"),
    }
}

async fn remove(tx: &mut Transaction<'_, Postgres>, b: &Binding, key: &[KeyValue]) -> Result<i64> {
    if key.len() != b.keys.len() {
        return Err("sync_key");
    }
    let values = key.iter().map(key_text).collect::<Result<Vec<_>>>()?;
    let mut predicates = Vec::new();
    let mut casts = Vec::new();
    for (index, name) in b.keys.iter().enumerate() {
        let (_, kind) = b
            .columns
            .iter()
            .find(|(n, _)| n == name)
            .ok_or("sync_key")?;
        casts.push(format!("${}::text::{kind}", index + 1));
        predicates.push(format!("{}={}", quote(name)?, casts.last().unwrap()));
    }
    let statement = format!(
        "DELETE FROM _oo_load.{} WHERE {}",
        quote(&b.delta)?,
        predicates.join(" AND ")
    );
    let mut query = sqlx::query(AssertSqlSafe(statement));
    for value in &values {
        query = query.bind(value);
    }
    query.execute(&mut **tx).await.map_err(db)?;
    let statement = format!(
        "INSERT INTO _oo_load.{} AS t VALUES({}) ON CONFLICT DO NOTHING RETURNING pg_column_size(t)::bigint",
        quote(&b.tombs)?,
        casts.join(",")
    );
    let mut query = sqlx::query_scalar::<_, i64>(AssertSqlSafe(statement));
    for value in &values {
        query = query.bind(value);
    }
    Ok(query
        .fetch_optional(&mut **tx)
        .await
        .map_err(db)?
        .unwrap_or(0))
}

fn encode(binding: &Binding, image: &DecodedStream) -> Result<Vec<u8>> {
    if image.rows != 1
        || image.fields.len() != binding.columns.len()
        || image.columns.len() != image.fields.len()
    {
        return Err("sync_image_shape");
    }
    let mut row = Vec::new();
    for (i, (field, (name, kind))) in image.fields.iter().zip(&binding.columns).enumerate() {
        let (oid, expected) = column(field)?;
        let kind = match kind.as_str() {
            "timestamp without time zone" => "timestamp",
            "timestamp with time zone" => "timestamptz",
            other => other,
        };
        if &field.name != name || expected != kind {
            return Err("sync_rebaseline_required_schema");
        }
        if i > 0 {
            row.push(b'\t');
        }
        let cell = image.columns[i].first().ok_or("sync_image_shape")?;
        if let NativeCell::UInt64(v) = cell {
            cell_text(
                &mut row,
                oid,
                -1,
                &NativeCell::Decimal {
                    precision: 20,
                    scale: 0,
                    coefficient: i128::from(*v),
                },
            )?;
        } else {
            cell_text(&mut row, oid, -1, cell)?;
        }
    }
    row.push(b'\n');
    Ok(row)
}
