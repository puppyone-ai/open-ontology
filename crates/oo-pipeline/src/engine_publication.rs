//! Prepare retained SourceEngine cuts under the native-library contract.
//!
//! This is an owner-side operation on the Agent DB, never a source connection.
//! It deliberately leaves semantic authorization pending: storage verification
//! cannot manufacture a reviewed policy, legacy task ACK, or open fence.
use crate::engine_load_pg::C5Part;
use crate::physical_mapping::{Level, MappingRequest, REGISTER_SQL};
use oo_connectors::engine::{ChunkReceipt, TableChunkPlan, TableRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection, Postgres, Transaction};
use sqlx_core::sql_str::AssertSqlSafe;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationRequest {
    pub library_id: String,
    pub source_id: String,
    /// The exact non-secret source binding used by PgTarget::new.
    pub source_binding: String,
    pub generation: String,
    pub tables: Vec<TableRef>,
}

fn digest(v: &Value) -> String {
    hash(&serde_json::to_vec(v).expect("JSON value"))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn db(_: sqlx::Error) -> &'static str {
    "publication_target"
}

fn quote(s: &str) -> Result<String> {
    if s.is_empty() || s.len() > 63 || s.contains('\0') {
        return Err("publication_identifier");
    }
    Ok(format!("\"{}\"", s.replace('"', "\"\"")))
}

fn logical(s: &str) -> Result<String> {
    crate::native_arrow::mapping_ident(s).map_err(|_| "publication_mapping")
}

fn validate(r: &PublicationRequest) -> Result<()> {
    for value in [&r.library_id, &r.generation] {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err("publication_identity");
        }
    }
    if r.source_binding.is_empty()
        || r.source_binding.len() > 65536
        || r.tables.is_empty()
        || r.tables.len() > 4096
    {
        return Err("publication_scope");
    }
    let source: (String, String, String, u16, String, String) =
        serde_json::from_str(&r.source_binding).map_err(|_| "publication_source_binding")?;
    if source.0 != r.source_id
        || source.1.is_empty()
        || source.2.is_empty()
        || source.3 == 0
        || source.4.is_empty()
        || source.5.is_empty()
    {
        return Err("publication_source_binding");
    }
    let mut names = BTreeSet::new();
    let schema = &r.tables[0].schema;
    for table in &r.tables {
        if table.schema != *schema {
            return Err("publication_single_schema_required");
        }
        if !names.insert((logical(&table.schema)?, logical(&table.name)?)) {
            return Err("publication_scope");
        }
        crate::physical_mapping::plan_physical_mapping(&MappingRequest {
            source: r.source_id.clone(),
            config_digest: "0".repeat(64),
            run_generation: 1,
            level: Level::Table,
            logical_namespace: logical(&table.schema)?,
            logical_table: logical(&table.name)?,
            logical_column: String::new(),
        })
        .map_err(|_| "publication_mapping")?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    protocol: String,
    plan: TableChunkPlan,
    parts: Vec<C5Part>,
    definition: String,
}

struct Verified {
    relation: String,
    recipe: Value,
    columns: Vec<(String, String)>,
    rows: u64,
}

async fn verify(
    tx: &mut Transaction<'_, Postgres>,
    request: &PublicationRequest,
    table: &TableRef,
) -> Result<Verified> {
    let identity = hash(
        &serde_json::to_vec(&(&request.source_binding, table))
            .map_err(|_| "publication_identity")?,
    );
    // Use the same fence as the Loader. It also excludes concurrent reclamation.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(&identity)
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    let (relation, raw, plan): (String, String, String) = sqlx::query_as(
        "SELECT g.relation_name,g.recipe::text,p.plan::text
         FROM _oo_load.generations g JOIN _oo_load.publications p USING(identity,run)
         WHERE g.identity=$1 AND g.run=$2 FOR SHARE OF g,p",
    )
    .bind(&identity)
    .bind(&request.generation)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?
    .ok_or("publication_unverified")?;
    let value: Value = serde_json::from_str(&raw).map_err(|_| "publication_recipe")?;
    let recipe: Recipe = serde_json::from_value(value.clone()).map_err(|_| "publication_recipe")?;
    let expected = format!(
        "g_{}",
        &hash(
            &serde_json::to_vec(&(&identity, &request.generation))
                .map_err(|_| "publication_identity")?
        )[..40]
    );
    if relation != expected
        || recipe.protocol != "oo.load.storage-generation/v1"
        || value["plan"] != serde_json::from_str::<Value>(&plan).map_err(|_| "publication_plan")?
        || recipe.plan.table != *table
        || recipe.parts.len() != recipe.plan.chunks.len()
    {
        return Err("publication_seal_mismatch");
    }
    crate::engine_load::validate_plan(&recipe.plan, table)?;
    let mut ids = BTreeSet::new();
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    let mut rows = 0_u64;
    for part in &recipe.parts {
        let receipt = &part.receipt;
        if !ids.insert(receipt.chunk_id)
            || !recipe.plan.chunks.iter().any(|c| {
                c.chunk_id == receipt.chunk_id
                    && c.plan_revision == receipt.plan_revision
                    && c.table == receipt.table
            })
        {
            return Err("publication_seal_mismatch");
        }
        let (stored, definition): (String, String) = sqlx::query_as(
            "SELECT c.receipt::text,s.definition FROM _oo_load.chunks c
             JOIN _oo_load.schemas s USING(stage)
             WHERE c.stage=$1 AND c.chunk=$2 AND s.identity=$3 FOR SHARE OF c,s",
        )
        .bind(&part.stage)
        .bind(i64::from(receipt.chunk_id))
        .bind(&identity)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db)?
        .ok_or("publication_part_missing")?;
        if serde_json::from_str::<ChunkReceipt>(&stored).map_err(|_| "publication_receipt")?
            != *receipt
            || definition != recipe.definition
        {
            return Err("publication_seal_mismatch");
        }
        if !groups.contains_key(&part.stage) {
            let fields: Vec<(String, String, bool)> = sqlx::query_as(
                "SELECT a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull
                 FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid
                 WHERE c.oid=to_regclass($1) AND c.relkind='r'
                 AND a.attnum>0 AND NOT a.attisdropped AND a.attname<>'__oo_chunk'
                 ORDER BY a.attnum",
            )
            .bind(format!("_oo_load.{}", quote(&part.stage)?))
            .fetch_all(&mut **tx)
            .await
            .map_err(db)?;
            let actual = fields
                .iter()
                .map(|(name, ty, required)| {
                    let ty = match ty.as_str() {
                        "timestamp without time zone" => "timestamp",
                        "timestamp with time zone" => "timestamptz",
                        ty => ty,
                    };
                    Ok(format!(
                        "{} {ty}{}",
                        quote(name)?,
                        if *required { " NOT NULL" } else { "" }
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .join(",");
            if actual != recipe.definition {
                return Err("publication_schema_mismatch");
            }
        }
        rows = rows.checked_add(receipt.rows).ok_or("publication_rows")?;
        groups
            .entry(part.stage.clone())
            .or_default()
            .push(receipt.chunk_id.to_string());
    }
    let qualified = format!("_oo_load.{}", quote(&relation)?);
    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT attname,format_type(atttypid,atttypmod) FROM pg_attribute
         WHERE attrelid=to_regclass($1) AND attnum>0 AND NOT attisdropped ORDER BY attnum",
    )
    .bind(&qualified)
    .fetch_all(&mut **tx)
    .await
    .map_err(db)?;
    if columns.is_empty() {
        return Err("publication_columns");
    }
    let names = columns
        .iter()
        .map(|c| quote(&c.0))
        .collect::<Result<Vec<_>>>()?
        .join(",");
    let selects = groups
        .into_iter()
        .map(|(stage, chunks)| {
            Ok(format!(
                "SELECT {names} FROM _oo_load.{} WHERE __oo_chunk IN ({})",
                quote(&stage)?,
                chunks.join(",")
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .join(" UNION ALL ");
    // Ask PostgreSQL to normalize both view definitions, rather than accepting
    // the name of a retained view as proof that it still references its recipe.
    sqlx::query(AssertSqlSafe(format!(
        "CREATE TEMP VIEW oo_load_verify AS {selects}"
    )))
    .execute(&mut **tx)
    .await
    .map_err(db)?;
    let same: bool = sqlx::query_scalar(
        "SELECT pg_get_viewdef(to_regclass($1),false)=pg_get_viewdef('pg_temp.oo_load_verify'::regclass,false)")
        .bind(&qualified).fetch_one(&mut **tx).await.map_err(db)?;
    sqlx::query("DROP VIEW pg_temp.oo_load_verify")
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    if !same {
        return Err("publication_view_mismatch");
    }
    Ok(Verified {
        relation,
        recipe: value,
        columns,
        rows,
    })
}

async fn register(
    tx: &mut Transaction<'_, Postgres>,
    r: &PublicationRequest,
    config: &str,
    generation: i64,
    table: &TableRef,
    level: Level,
    column: &str,
) -> Result<String> {
    let namespace = logical(&table.schema)?;
    let name = if level == Level::Namespace {
        String::new()
    } else {
        logical(&table.name)?
    };
    let column = if level == Level::Column {
        logical(column)?
    } else {
        String::new()
    };
    let planned = crate::physical_mapping::plan_physical_mapping(&MappingRequest {
        source: r.source_id.clone(),
        config_digest: config.into(),
        run_generation: generation,
        level,
        logical_namespace: namespace.clone(),
        logical_table: name.clone(),
        logical_column: column.clone(),
    })
    .map_err(|_| "publication_mapping")?;
    sqlx::query(REGISTER_SQL)
        .bind(&r.source_id)
        .bind(config)
        .bind(generation)
        .bind(level.as_str())
        .bind(&namespace)
        .bind(&name)
        .bind(&column)
        .bind(planned.identity_digest.to_vec())
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    Ok(planned.physical_name)
}

/// Prepare one complete, retained source cut in a single Agent DB transaction.
///
/// Replays verify the sealed recipes again, return the same receipt, and never
/// rewind a newer head. No grants change. `state=pending` is not Gateway success.
/// The native contract must already be installed by the target administrator.
pub async fn publish_generation(
    conn: &mut PgConnection,
    request: &PublicationRequest,
) -> Result<Value> {
    validate(request)?;
    let mut request = request.clone();
    request
        .tables
        .sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
    let r = &request;
    let mut tx = conn.begin().await.map_err(db)?;
    sqlx::query(
        "SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','120s',true)",
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("oo-load-library:{}", r.library_id))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let mut verified = Vec::new();
    for table in &r.tables {
        verified.push(verify(&mut tx, r, table).await?);
    }
    sqlx::raw_sql(include_str!("../migrations/0123_engine_publication.sql"))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    sqlx::raw_sql(include_str!("../migrations/0124_retained_generation.sql"))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    sqlx::raw_sql(include_str!(
        "../migrations/0125_retained_generation_read.sql"
    ))
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let config = hash(r.source_binding.as_bytes());
    let descriptor = json!({"source_id":r.source_id,"binding_sha256":config,
        "tables":r.tables,"recipes":verified.iter().map(|v| &v.recipe).collect::<Vec<_>>()});
    sqlx::query(
        "INSERT INTO _oo_load.gateway_publications(library_id,source,run,request)
         VALUES($1,$2,$3,$4::jsonb) ON CONFLICT DO NOTHING",
    )
    .bind(&r.library_id)
    .bind(&r.source_id)
    .bind(&r.generation)
    .bind(descriptor.to_string())
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let (generation, stored, old): (i64, String, Option<String>) = sqlx::query_as(
        "SELECT generation,request::text,receipt_sha256 FROM _oo_load.gateway_publications
         WHERE library_id=$1 AND source=$2 AND run=$3 FOR UPDATE",
    )
    .bind(&r.library_id)
    .bind(&r.source_id)
    .bind(&r.generation)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if serde_json::from_str::<Value>(&stored).map_err(|_| "publication_request")? != descriptor {
        return Err("publication_replay_mismatch");
    }
    let mut tables = Vec::new();
    let mut vector = Vec::new();
    let mut proofs = Vec::<Value>::new();
    for (table, cut) in r.tables.iter().zip(&verified) {
        register(&mut tx, r, &config, generation, table, Level::Namespace, "").await?;
        let physical = register(&mut tx, r, &config, generation, table, Level::Table, "").await?;
        let mut projections = Vec::new();
        for (column, _) in &cut.columns {
            let mapped = register(
                &mut tx,
                r,
                &config,
                generation,
                table,
                Level::Column,
                column,
            )
            .await?;
            projections.push(format!("{} AS {}", quote(column)?, quote(&mapped)?));
        }
        let select = format!(
            "SELECT {} FROM _oo_load.{}",
            projections.join(","),
            quote(&cut.relation)?
        );
        if old.is_none() {
            sqlx::query(AssertSqlSafe(format!(
                "CREATE VIEW _oo.{} AS {select}",
                quote(&physical)?
            )))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        } else {
            sqlx::query(AssertSqlSafe(format!(
                "CREATE TEMP VIEW oo_load_verify AS {select}"
            )))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            let same: bool = sqlx::query_scalar(
                "SELECT pg_get_viewdef(to_regclass($1),false)=pg_get_viewdef('pg_temp.oo_load_verify'::regclass,false)")
                .bind(format!("_oo.{}",quote(&physical)?))
                .fetch_one(&mut *tx).await.map_err(db)?;
            sqlx::query("DROP VIEW pg_temp.oo_load_verify")
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            if !same {
                return Err("publication_mapped_view_mismatch");
            }
        }
        let recipe_sha = digest(&cut.recipe);
        let mut relations = vec![
            ("_oo", physical.clone()),
            ("_oo_load", cut.relation.clone()),
        ];
        for part in cut.recipe["parts"].as_array().ok_or("publication_recipe")? {
            relations.push((
                "_oo_load",
                part["stage"].as_str().ok_or("publication_recipe")?.into(),
            ));
        }
        for (schema, relation) in relations {
            let proof: String =
                sqlx::query_scalar("SELECT _oo.native_library_relation_proof($1,$2)::text")
                    .bind(schema)
                    .bind(relation)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db)?;
            let proof = serde_json::from_str(&proof).map_err(|_| "publication_proof")?;
            if !proofs.contains(&proof) {
                proofs.push(proof);
            }
        }
        tables.push(json!({"schema":table.schema,"table":table.name,
            "storage_schema":logical(&table.schema)?,"storage_table":logical(&table.name)?,
            "run_id":r.generation,"physical_name":physical,"accepted_rows":cut.rows,
            "recipe_sha256":recipe_sha,"source_generation":generation,
            "target_generation":generation,"applied_pos":"0","capture_method":"c5_diff",
            "delete_detection":"reconcile","consistency_grade":"B"}));
        vector.push(
            json!({"dataset":format!("{}.{}",logical(&table.schema)?,logical(&table.name)?),
            "source_id":r.source_id,"generation":generation,"recipe_sha256":recipe_sha,
            "storage_relation":cut.relation,"physical_name":physical,
            "expected_columns":cut.columns.iter().map(|(name,ty)|
                Ok(json!({"name":logical(name)?,"data_type":ty}))).collect::<Result<Vec<_>>>()?,
            "source_committed_at":Value::Null}),
        );
        sqlx::query(
            "INSERT INTO _oo.table_freshness(schema_name,table_name,source,
             source_generation,target_generation,physical_name,last_pos,
             source_committed_at,applied_at,mode,state)
             VALUES($1,$2,$3,$4,$4,$5,0,NULL,clock_timestamp(),'full','awaiting_publish')
             ON CONFLICT DO NOTHING",
        )
        .bind(logical(&table.schema)?)
        .bind(logical(&table.name)?)
        .bind(&r.source_id)
        .bind(generation)
        .bind(&physical)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    sqlx::query(
        "INSERT INTO _oo.source_watermark(source,consistency,consistency_grade,applied_pos,
         source_committed_at,applied_at,state,cross_table_consistent,source_generation,
         target_generation,position_codec,position_lineage)
         VALUES($1,'per_table','B',0,NULL,clock_timestamp(),'lagging',false,$2,$2,
         'snapshot-zero/v1','oo-load.retained') ON CONFLICT DO NOTHING",
    )
    .bind(&r.source_id)
    .bind(generation)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let mut schemas = BTreeMap::<String, Vec<String>>::new();
    for table in &r.tables {
        schemas
            .entry(logical(&table.schema)?)
            .or_default()
            .push(logical(&table.name)?);
    }
    // Native library membership admits one schema per source.
    if schemas.len() != 1 {
        return Err("publication_single_schema_required");
    }
    let (schema, names) = schemas.into_iter().next().ok_or("publication_scope")?;
    let manifest = json!({"protocol":"oo.library.membership/v1","library_id":r.library_id,
        "sources":[{"source_id":r.source_id,"schema":schema,"tables":names}]});
    let receipt = json!({"protocol":"oo.load.per-table-receipt/v1",
        "source_id":r.source_id,"run_id":r.generation,"run_ids":[r.generation],
        "source_generation":generation,"target_generation":generation,
        "position_codec":"snapshot-zero/v1","position_lineage":"oo-load.retained",
        "applied_pos":"0","consistency":"retained_generation","consistency_grade":"B",
        "cross_table_consistent":false,"tables":tables});
    let library = json!({"protocol":"oo.pipeline.native-library-publication/v1",
        "library_id":r.library_id,"manifest_sha256":digest(&manifest),"manifest":manifest,
        "expected_tables":tables.len(),"shards":[r.source_id]});
    let shards = json!([{"source_id":r.source_id,"anchor_run":r.generation,
        "receipt_sha256":digest(&receipt),"receipt":receipt}]);
    let cut = json!({"library":library,"shards":shards});
    let sha = digest(&cut);
    if old.as_ref().is_some_and(|old| old != &sha) {
        return Err("publication_replay_mismatch");
    }
    sqlx::query(
        "INSERT INTO _oo.native_library_publication(receipt_sha256,library_id,library,shards,publication_xid)
         VALUES($1,$2,$3::jsonb,$4::jsonb,pg_current_xact_id()) ON CONFLICT DO NOTHING")
        .bind(&sha).bind(&r.library_id).bind(library.to_string()).bind(shards.to_string())
        .execute(&mut *tx).await.map_err(db)?;
    let stored: (String, String) = sqlx::query_as(
        "SELECT library::text,shards::text FROM _oo.native_library_publication WHERE receipt_sha256=$1")
        .bind(&sha).fetch_one(&mut *tx).await.map_err(db)?;
    if serde_json::from_str::<Value>(&stored.0).map_err(|_| "publication_receipt")? != library
        || serde_json::from_str::<Value>(&stored.1).map_err(|_| "publication_receipt")? != shards
    {
        return Err("publication_receipt_mismatch");
    }
    // A candidate must never displace a serving cut before semantic admission.
    sqlx::query(
        "INSERT INTO _oo.native_library_head(library_id,manifest_sha256,receipt_sha256,state)
         VALUES($1,$2,$3,'pending') ON CONFLICT DO NOTHING",
    )
    .bind(&r.library_id)
    .bind(digest(&manifest))
    .bind(&sha)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let head_manifest: String = sqlx::query_scalar(
        "SELECT manifest_sha256 FROM _oo.native_library_head WHERE library_id=$1",
    )
    .bind(&r.library_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if head_manifest != digest(&manifest) {
        return Err("publication_membership_mismatch");
    }
    sqlx::query(
        "INSERT INTO _oo.native_library_member(source_id,library_id)
         VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(&r.source_id)
    .bind(&r.library_id)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let owner: String =
        sqlx::query_scalar("SELECT library_id FROM _oo.native_library_member WHERE source_id=$1")
            .bind(&r.source_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    if owner != r.library_id {
        return Err("publication_membership_mismatch");
    }
    sqlx::query(
        "UPDATE _oo_load.gateway_publications SET receipt_sha256=$4,shard=$5::jsonb,
         data_vector=$6::jsonb,data_vector_sha256=$7,relation_proofs=$8::jsonb,sealed_at=clock_timestamp()
         WHERE library_id=$1 AND source=$2 AND run=$3 AND receipt_sha256 IS NULL",
    )
    .bind(&r.library_id)
    .bind(&r.source_id)
    .bind(&r.generation)
    .bind(&sha)
    .bind(shards[0].to_string())
    .bind(json!(vector).to_string())
    .bind(digest(&json!(vector)))
    .bind(json!(proofs).to_string())
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(
        json!({"protocol":"oo.load.publication-candidate/v1","state":"pending",
        "gateway_published":false,"receipt_sha256":sha,"receipt":cut,
        "data_vector":vector,"data_vector_sha256":digest(&json!(vector)),
        "generation":generation,"config_digest":config}),
    )
}

/// Atomically switch only to a fenced candidate. Failure preserves the old head.
pub async fn activate_generation(conn: &mut PgConnection, candidate: &Value) -> Result<Value> {
    let library = candidate["receipt"]["library"]["library_id"]
        .as_str()
        .ok_or("publication_identity")?;
    let sha = candidate["receipt_sha256"]
        .as_str()
        .ok_or("publication_identity")?;
    let mut tx = conn.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("oo-load-library:{library}"))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let newest: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(g.generation),0) FROM _oo_load.gateway_publications g
         JOIN _oo.native_library_head h ON h.receipt_sha256=g.receipt_sha256
         WHERE h.library_id=$1 AND h.state='ready'",
    )
    .bind(library)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if candidate["generation"]
        .as_i64()
        .ok_or("publication_identity")?
        < newest
    {
        return Err("publication_superseded");
    }
    sqlx::query(
        "UPDATE _oo.native_library_head SET receipt_sha256=$2,state='pending' WHERE library_id=$1",
    )
    .bind(library)
    .bind(sha)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    sqlx::query("SELECT _oo.native_library_mark_ready($1,$2)")
        .bind(library)
        .bind(sha)
        .execute(&mut *tx)
        .await
        .map_err(|_| "publication_semantic_admission_required")?;
    sqlx::query("UPDATE _oo_load.gateway_publications SET activated_at=coalesce(activated_at,clock_timestamp()) WHERE receipt_sha256=$1")
        .bind(sha).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    let mut result = candidate.clone();
    result["state"] = json!("ready");
    result["gateway_published"] = json!(true);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> PublicationRequest {
        PublicationRequest {
            library_id: "owned".into(),
            source_id: "fixture".into(),
            source_binding: serde_json::to_string(&(
                "fixture",
                "postgres",
                "127.0.0.1",
                1_u16,
                "unused",
                "reader",
            ))
            .unwrap(),
            generation: "cycle-1".into(),
            tables: vec![TableRef {
                schema: "app".into(),
                name: "items".into(),
            }],
        }
    }
    #[test]
    fn publication_scope_is_closed_before_database_access() {
        assert!(validate(&request()).is_ok());
        for bad in ["", "../run", "run\n", &"x".repeat(129)] {
            let mut r = request();
            r.generation = bad.into();
            assert!(validate(&r).is_err());
        }
        let mut r = request();
        r.tables.push(r.tables[0].clone());
        assert_eq!(validate(&r), Err("publication_scope"));
        let mut r = request();
        r.source_id = "not a source".into();
        assert_eq!(validate(&r), Err("publication_source_binding"));
        let mut value = serde_json::to_value(request()).unwrap();
        value["verified"] = json!(true);
        assert!(serde_json::from_value::<PublicationRequest>(value).is_err());
        let mut oracle = request();
        oracle.tables[0].schema = "APP".into();
        oracle.tables[0].name = "ITEMS".into();
        assert!(validate(&oracle).is_ok());
        assert_eq!(logical("ITEMS"), Ok("q_4954454d53".into()));
    }
}
