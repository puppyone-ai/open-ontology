//! Local SourceEngine target adapter, pending shared legacy admission support.
//! Private chunk commits and receipt-keyed COPY are followed by one atomic view
//! replacement. Published storage is immutable. This is NOT Gateway publication.
use crate::{
    engine_load::{Result, Target},
    native_arrow::{DecodedField, DecodedStream, NativeCell},
    native_c1_apply::cell_text,
};
use oo_connectors::engine::{ChunkReceipt, TableChunkPlan};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};
use sqlx_core::sql_str::AssertSqlSafe;

#[path = "engine_load_pg_c5.rs"]
mod c5;
pub use c5::{C5Baseline, C5Part, same_ranges};

fn db(_: sqlx::Error) -> &'static str {
    "load_target"
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn quote(name: &str) -> Result<String> {
    if name.is_empty() || name.len() > 63 || name.contains('\0') {
        return Err("load_identifier");
    }
    Ok(format!("\"{}\"", name.replace('"', "\"\"")))
}

/// Closed, lossless target types. Unsupported types fail before COPY.
pub(crate) fn column(field: &DecodedField) -> Result<(i64, &'static str)> {
    Ok(match field.arrow_type.as_str() {
        "int8" | "int16" => (21, "smallint"),
        "int32" => (23, "integer"),
        "int64" => (20, "bigint"),
        "uint8" => (21, "smallint"),
        "uint64" => (1700, "numeric"),
        "bool" => (16, "boolean"),
        "utf8" => (25, "text"),
        "list(utf8)" => (114, "json"),
        "binary" => (17, "bytea"),
        "float64" => (701, "double precision"),
        "decimal128" if field.scale.is_some_and(|s| s >= 0) => (1700, "numeric"),
        "date32" => (1082, "date"),
        "duration_micros" => (1186, "interval"),
        "timestamp_micros" if field.timezone.is_none() => (1114, "timestamp"),
        "timestamp_micros_utc" if field.timezone.as_deref() == Some("UTC") => (1184, "timestamptz"),
        _ => return Err("load_unsupported_type"),
    })
}

pub struct PgTarget<'a> {
    conn: &'a mut PgConnection,
    identity: String,
    run: String,
    stage: String,
    view: String,
    plan: String,
    fields: Option<Vec<DecodedField>>,
}

impl<'a> PgTarget<'a> {
    /// Caller must use a dedicated connection and close it after the load. The
    /// session advisory lock fences concurrent writers including process death.
    pub async fn new(
        conn: &'a mut PgConnection,
        source: &str,
        run: &str,
        table: &oo_connectors::engine::TableRef,
    ) -> Result<Self> {
        if source.is_empty() || run.is_empty() || run.len() > 128 {
            return Err("load_identity");
        }
        let identity = hash(&serde_json::to_vec(&(source, table)).map_err(|_| "load_identity")?);
        let locked: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
                .bind(&identity)
                .fetch_one(&mut *conn)
                .await
                .map_err(db)?;
        if !locked {
            return Err("load_target_busy");
        }
        // Private metadata never grants readers access to incomplete generations.
        sqlx::raw_sql(
            "CREATE SCHEMA IF NOT EXISTS _oo_load;
             REVOKE ALL ON SCHEMA _oo_load FROM PUBLIC;
             CREATE SCHEMA IF NOT EXISTS oo_load_data;
             CREATE TABLE IF NOT EXISTS _oo_load.chunks (
               stage text NOT NULL, chunk bigint NOT NULL, receipt jsonb NOT NULL,
               write_bytes bigint NOT NULL CHECK(write_bytes>=0), PRIMARY KEY(stage,chunk));
             CREATE TABLE IF NOT EXISTS _oo_load.schemas (
               stage text PRIMARY KEY, identity text NOT NULL, run text NOT NULL, definition text NOT NULL);
             CREATE TABLE IF NOT EXISTS _oo_load.publications (
               identity text NOT NULL, run text NOT NULL, plan jsonb NOT NULL,
               stage text NOT NULL UNIQUE, PRIMARY KEY(identity,run));
             CREATE TABLE IF NOT EXISTS _oo_load.current (
               identity text PRIMARY KEY, run text NOT NULL);",
        )
        .execute(&mut *conn)
        .await
        .map_err(db)?;
        Ok(Self {
            conn,
            view: format!("t_{}", &identity[..40]),
            identity,
            run: run.into(),
            stage: String::new(),
            plan: String::new(),
            fields: None,
        })
    }

    pub fn relation(&self) -> String {
        format!("oo_load_data.{}", self.view)
    }

    /// A completed run is immutable. Reusing its ID is resume, not refresh.
    pub async fn completed(&mut self) -> Result<Option<Vec<ChunkReceipt>>> {
        let stage: Option<String> = sqlx::query_scalar(
            "SELECT stage FROM _oo_load.publications WHERE identity=$1 AND run=$2",
        )
        .bind(&self.identity)
        .bind(&self.run)
        .fetch_optional(&mut *self.conn)
        .await
        .map_err(db)?;
        match stage {
            None => Ok(None),
            Some(stage) => {
                let current: Option<String> =
                    sqlx::query_scalar("SELECT run FROM _oo_load.current WHERE identity=$1")
                        .bind(&self.identity)
                        .fetch_optional(&mut *self.conn)
                        .await
                        .map_err(db)?;
                if current.as_deref() != Some(&self.run) {
                    return Err("load_run_superseded");
                }
                let rows: Vec<String> = sqlx::query_scalar(
                    "SELECT receipt::text FROM _oo_load.chunks WHERE stage=$1 ORDER BY chunk",
                )
                .bind(stage)
                .fetch_all(&mut *self.conn)
                .await
                .map_err(db)?;
                rows.into_iter()
                    .map(|v| serde_json::from_str(&v).map_err(|_| "load_receipt"))
                    .collect::<Result<Vec<_>>>()
                    .map(Some)
            }
        }
    }

    pub async fn write_bytes(&mut self) -> Result<i64> {
        sqlx::query_scalar(
            "SELECT COALESCE(sum(c.write_bytes),0)::bigint FROM _oo_load.chunks c
             JOIN _oo_load.schemas p ON p.stage=c.stage WHERE p.identity=$1 AND p.run=$2",
        )
        .bind(&self.identity)
        .bind(&self.run)
        .fetch_one(&mut *self.conn)
        .await
        .map_err(db)
    }
}

impl Target for PgTarget<'_> {
    async fn begin(&mut self, plan: &TableChunkPlan) -> Result<()> {
        let installed: bool =
            sqlx::query_scalar("SELECT to_regclass('_oo_load.sync_bindings') IS NOT NULL")
                .fetch_one(&mut *self.conn)
                .await
                .map_err(db)?;
        if installed {
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM _oo_load.sync_bindings WHERE identity=$1)",
            )
            .bind(&self.identity)
            .fetch_one(&mut *self.conn)
            .await
            .map_err(db)?;
            if active {
                return Err("load_rebaseline_required_active_sync");
            }
        }
        if self.completed().await?.is_some() {
            return Err("load_already_published");
        }
        self.plan = serde_json::to_string(plan).map_err(|_| "load_plan")?;
        // Includes source, run, complete bounds and revision. No old-plan chunk
        // can enter a replacement plan merely because its ordinal is reused.
        let identity = serde_json::to_vec(&(&self.identity, &self.run, &self.plan))
            .map_err(|_| "load_identity")?;
        self.stage = format!("s_{}", &hash(&identity)[..40]);
        self.fields = None;
        Ok(())
    }

    async fn stage(&mut self, receipt: &ChunkReceipt, decoded: &DecodedStream) -> Result<()> {
        if self.fields.as_ref().is_some_and(|f| f != &decoded.fields) {
            return Err("load_schema_drift");
        }
        let mut names = std::collections::BTreeSet::new();
        let mut definitions = Vec::new();
        let mut types = Vec::new();
        for f in &decoded.fields {
            if !names.insert(&f.name) || f.name == "__oo_chunk" {
                return Err("load_schema");
            }
            let (oid, sql) = column(f)?;
            types.push(oid);
            definitions.push(format!(
                "{} {sql}{}",
                quote(&f.name)?,
                if f.nullable { "" } else { " NOT NULL" }
            ));
        }
        if definitions.is_empty() {
            return Err("load_schema");
        }
        let mut tx = self.conn.begin().await.map_err(db)?;
        let definition = definitions.join(",");
        sqlx::query("INSERT INTO _oo_load.schemas VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(&self.stage)
            .bind(&self.identity)
            .bind(&self.run)
            .bind(&definition)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let prior_definition: String =
            sqlx::query_scalar("SELECT definition FROM _oo_load.schemas WHERE stage=$1")
                .bind(&self.stage)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if prior_definition != definition {
            return Err("load_schema_drift");
        }

        sqlx::query(AssertSqlSafe(format!(
            "CREATE TABLE IF NOT EXISTS _oo_load.{} (__oo_chunk bigint NOT NULL,{})",
            quote(&self.stage)?,
            definitions.join(",")
        )))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        let encoded = serde_json::to_string(receipt).map_err(|_| "load_receipt")?;
        let prior: Option<String> = sqlx::query_scalar(
            "SELECT receipt::text FROM _oo_load.chunks WHERE stage=$1 AND chunk=$2",
        )
        .bind(&self.stage)
        .bind(i64::from(receipt.chunk_id))
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let same = prior
            .as_deref()
            .map(serde_json::from_str::<ChunkReceipt>)
            .transpose()
            .map_err(|_| "load_receipt")?
            .as_ref()
            == Some(receipt);
        if !same {
            sqlx::query(AssertSqlSafe(format!(
                "DELETE FROM _oo_load.{} WHERE __oo_chunk=$1",
                quote(&self.stage)?
            )))
            .bind(i64::from(receipt.chunk_id))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            let statement = format!(
                "COPY _oo_load.{} FROM STDIN WITH (FORMAT text)",
                quote(&self.stage)?
            );
            let mut copy = tx.copy_in_raw(&statement).await.map_err(db)?;
            let mut buffer = Vec::new();
            for row in 0..decoded.rows {
                buffer.extend_from_slice(receipt.chunk_id.to_string().as_bytes());
                for (index, oid) in types.iter().enumerate() {
                    buffer.push(b'\t');
                    let cell = decoded
                        .columns
                        .get(index)
                        .and_then(|c| c.get(row))
                        .ok_or("load_schema")?;
                    // PostgreSQL has no unsigned bigint. Widen without passing
                    // through signed i64 or floating point.
                    if let NativeCell::UInt64(value) = cell {
                        cell_text(
                            &mut buffer,
                            *oid,
                            -1,
                            &NativeCell::Decimal {
                                precision: 20,
                                scale: 0,
                                coefficient: i128::from(*value),
                            },
                        )?;
                    } else {
                        cell_text(&mut buffer, *oid, -1, cell)?;
                    }
                }
                buffer.push(b'\n');
                if buffer.len() >= 65_536 {
                    copy.send(std::mem::take(&mut buffer)).await.map_err(db)?;
                }
            }
            if !buffer.is_empty() {
                copy.send(buffer).await.map_err(db)?;
            }
            if copy.finish().await.map_err(db)? != receipt.rows {
                return Err("load_copy_rows");
            }
            let bytes: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
                "SELECT COALESCE(sum(pg_column_size(t)),0)::bigint FROM _oo_load.{} t WHERE __oo_chunk=$1", quote(&self.stage)?
            ))).bind(i64::from(receipt.chunk_id)).fetch_one(&mut *tx).await.map_err(db)?;
            sqlx::query(
                "INSERT INTO _oo_load.chunks VALUES($1,$2,$3::jsonb,$4)
                 ON CONFLICT(stage,chunk) DO UPDATE SET receipt=EXCLUDED.receipt,
                 write_bytes=_oo_load.chunks.write_bytes+EXCLUDED.write_bytes",
            )
            .bind(&self.stage)
            .bind(i64::from(receipt.chunk_id))
            .bind(encoded)
            .bind(bytes)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        self.fields = Some(decoded.fields.clone());
        Ok(())
    }

    async fn publish(&mut self, plan: &TableChunkPlan) -> Result<()> {
        if serde_json::to_string(plan).map_err(|_| "load_plan")? != self.plan {
            return Err("load_plan_changed");
        }
        let fields = self
            .fields
            .as_ref()
            .ok_or("load_schema")?
            .iter()
            .map(|f| quote(&f.name))
            .collect::<Result<Vec<_>>>()?
            .join(",");
        let mut tx = self.conn.begin().await.map_err(db)?;
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT receipt::text FROM _oo_load.chunks WHERE stage=$1")
                .bind(&self.stage)
                .fetch_all(&mut *tx)
                .await
                .map_err(db)?;
        if rows.len() != plan.chunks.len() {
            return Err("load_incomplete");
        }
        for row in rows {
            let receipt: ChunkReceipt = serde_json::from_str(&row).map_err(|_| "load_receipt")?;
            if !plan.chunks.iter().any(|c| {
                c.chunk_id == receipt.chunk_id
                    && c.plan_revision == receipt.plan_revision
                    && c.table == receipt.table
            }) {
                return Err("load_incomplete");
            }
        }
        sqlx::query(AssertSqlSafe(format!(
            "CREATE OR REPLACE VIEW oo_load_data.{} AS SELECT {fields} FROM _oo_load.{}",
            quote(&self.view)?,
            quote(&self.stage)?
        )))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        sqlx::query("INSERT INTO _oo_load.publications VALUES($1,$2,$3::jsonb,$4)")
            .bind(&self.identity)
            .bind(&self.run)
            .bind(&self.plan)
            .bind(&self.stage)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("INSERT INTO _oo_load.current VALUES($1,$2) ON CONFLICT(identity) DO UPDATE SET run=EXCLUDED.run")
            .bind(&self.identity).bind(&self.run).execute(&mut *tx).await.map_err(db)?;
        tx.commit().await.map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_loads_as_interval_microseconds() {
        let field = DecodedField {
            name: "elapsed".into(),
            nullable: true,
            arrow_type: "duration_micros".into(),
            precision: None,
            scale: None,
            timezone: None,
        };
        assert_eq!(column(&field), Ok((1186, "interval")));
        let mut out = Vec::new();
        cell_text(
            &mut out,
            1186,
            -1,
            &NativeCell::Int {
                bit_width: 64,
                value: -3_723_000_001,
            },
        )
        .unwrap();
        assert_eq!(out, b"-3723000001 microseconds");
    }
}
