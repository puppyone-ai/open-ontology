//! F22: a content identity, not an authority token or a retained PG snapshot.
//! Every use is authenticated again. An unavailable old version fails closed.

use axum::http::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use tokio_postgres::Transaction;

use crate::policy::{
    CompiledPolicy,
    catalog::{CatalogError, CatalogLimits, load_policy_catalog_in_transaction},
};
use crate::query_executor::ExecError;
use crate::sql_guard::{PolicyCatalog, PolicyIdentity};

#[cfg(test)]
#[path = "publication_binding_reference_tests.rs"]
mod reference_tests;

#[cfg(test)]
#[path = "publication_identity_pg_tests.rs"]
mod identity_tests;

pub(crate) const EXPECTED: &str = "x-oo-expected-publication-id";
pub(crate) const EXPECTED_VECTOR: &str = "x-oo-expected-data-vector-sha256";
pub(crate) const ACTUAL: &str = "x-oo-publication-id";
pub(crate) const ACTUAL_VECTOR: &str = "x-oo-data-vector-sha256";
pub(crate) const DRIFT_HINT: &str = "reason=PUBLICATION_DRIFT; action=RECOMPUTE_WHOLE_QUESTION; discard all intermediate results; do not combine releases or retry only this SQL";

/// The installed immutable receipt, not the mutable ingestion high watermark.
pub(crate) async fn library_cut(tx: &Transaction<'_>, catalog: &Value) -> Result<Value, ExecError> {
    let library = catalog["library_id"].as_str().ok_or_else(drift)?;
    let sha = catalog["receipt_sha256"]
        .as_str()
        .filter(|s| hex64(s))
        .ok_or_else(drift)?;
    let mut row = tx.query_opt(
        "SELECT p.library::text,p.shards::text FROM _oo.native_library_publication p \
         JOIN _oo.native_library_head h ON h.library_id=p.library_id AND h.receipt_sha256=p.receipt_sha256 \
         WHERE p.library_id=$1 AND p.receipt_sha256=$2 AND h.state='ready' \
         AND h.manifest_sha256=p.library->>'manifest_sha256'",
        &[&library,&sha],
    ).await.map_err(database)?;
    if row.is_none() {
        // A reviewed-file refresh and the DB head cannot be one transaction.
        // Already activated retained cuts remain usable while their fences
        // stay open; pending cuts and mutable C1 never take this path.
        let installed: bool = tx
            .query_one(
                "SELECT coalesce(has_function_privilege(current_user,
                 to_regprocedure('_oo.native_library_retained_table(text)'),'EXECUTE'),false)",
                &[],
            )
            .await
            .map_err(database)?
            .get(0);
        if installed {
            row = tx.query_opt(
                "SELECT p.library::text,p.shards::text FROM _oo.native_library_publication p
                 JOIN _oo.native_library_head h ON h.library_id=p.library_id AND h.state='ready'
                 WHERE p.library_id=$1 AND p.receipt_sha256=$2
                  AND h.manifest_sha256=p.library->>'manifest_sha256'
                  AND jsonb_array_length(p.shards)>0
                  AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s
                   WHERE s->'receipt'->>'consistency' IS DISTINCT FROM 'retained_generation'
                    OR jsonb_array_length(s->'receipt'->'tables')=0
                    OR EXISTS(SELECT 1 FROM jsonb_array_elements(s->'receipt'->'tables') t
                     WHERE _oo.native_library_retained_table(t->>'physical_name')->>'library_receipt_sha256'
                      IS DISTINCT FROM p.receipt_sha256))",
                &[&library,&sha],
            ).await.map_err(database)?;
        }
    }
    let row = row.ok_or_else(drift)?;
    let library: Value = serde_json::from_str(&row.get::<_, String>(0)).map_err(|_| drift())?;
    let shards: Value = serde_json::from_str(&row.get::<_, String>(1)).map_err(|_| drift())?;
    let cut = json!({"library":library,"shards":shards});
    if digest(&cut) != sha || cut["library"]["library_id"] != catalog["library_id"] {
        return Err(drift());
    }
    // C1 can mutate its storage in place; only C5/per-table retained cuts are
    // independent of ingestion. Do not relax C1's current-cut checks.
    for shard in cut["shards"].as_array().ok_or_else(drift)? {
        if !immutable_library_shard(shard) {
            let source = shard["source_id"].as_str().ok_or_else(drift)?;
            let receipt = shard["receipt_sha256"].as_str().ok_or_else(drift)?;
            if !tx
                .query_one(
                    "SELECT _oo.native_library_source_ready($1,$2)",
                    &[&source, &receipt],
                )
                .await
                .map_err(database)?
                .get::<_, bool>(0)
            {
                return Err(drift());
            }
        }
    }
    Ok(cut)
}

fn immutable_library_shard(shard: &Value) -> bool {
    matches!(
        shard["receipt"]["consistency"].as_str(),
        Some("per_table" | "retained_generation")
    )
}

#[test]
fn retained_generation_is_independent_of_ingestion() {
    for consistency in ["per_table", "retained_generation"] {
        assert!(immutable_library_shard(
            &json!({"receipt":{"consistency":consistency}})
        ));
    }
    for consistency in ["transactional", "unknown", ""] {
        assert!(!immutable_library_shard(
            &json!({"receipt":{"consistency":consistency}})
        ));
    }
}

pub(crate) fn library_table<'a>(
    cut: &'a Value,
    source: &str,
    schema: &str,
    table: &str,
) -> Result<(&'a Value, &'a Value), ExecError> {
    let mut found = None;
    for shard in cut["shards"].as_array().ok_or_else(drift)? {
        if shard["source_id"] != source {
            continue;
        }
        for entry in shard["receipt"]["tables"].as_array().ok_or_else(drift)? {
            if entry.get("storage_schema").unwrap_or(&entry["schema"]) == schema
                && entry.get("storage_table").unwrap_or(&entry["table"]) == table
            {
                if found.is_some() {
                    return Err(drift());
                }
                found = Some((&shard["receipt"], entry));
            }
        }
    }
    found.ok_or_else(drift)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Expected {
    pub publication_id: String,
    pub data_vector_sha256: Option<String>,
}

impl Expected {
    pub fn from_headers(headers: &HeaderMap) -> Result<Option<Self>, ()> {
        let one = |name| -> Result<Option<&str>, ()> {
            let mut values = headers.get_all(name).iter();
            let text = values
                .next()
                .map(|v| v.to_str().map_err(|_| ()))
                .transpose()?;
            if values.next().is_some() {
                return Err(());
            }
            Ok(text)
        };
        let id = one(EXPECTED)?;
        let vector = one(EXPECTED_VECTOR)?;
        if id.is_none() && vector.is_none() {
            return Ok(None);
        }
        let id = id.ok_or(())?;
        if id != "current" && !id.strip_prefix("pub1_").is_some_and(hex64) {
            return Err(());
        }
        if vector.is_some_and(|v| !hex64(v)) || (id == "current" && vector.is_some()) {
            return Err(());
        }
        Ok(Some(Self {
            publication_id: id.into(),
            data_vector_sha256: vector.map(str::to_owned),
        }))
    }

    fn matches(&self, actual: &BindingReceipt) -> bool {
        (self.publication_id == "current" || self.publication_id == actual.publication_id)
            && self
                .data_vector_sha256
                .as_ref()
                .is_none_or(|v| v == &actual.data_vector_sha256)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingReceipt {
    pub publication_id: String,
    pub data_vector_sha256: String,
}

impl BindingReceipt {
    pub(crate) fn headers(&self, headers: &mut HeaderMap) {
        // Both values are generated lowercase hex, never caller text.
        headers.insert(
            ACTUAL,
            HeaderValue::from_str(&self.publication_id).expect("binding hash"),
        );
        headers.insert(
            ACTUAL_VECTOR,
            HeaderValue::from_str(&self.data_vector_sha256).expect("vector hash"),
        );
    }
}

pub(crate) struct BindingCheck {
    pub expected: Expected,
    pub identity: PolicyIdentity,
    pub principal: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub pack_digest: String,
    pub source: Option<String>,
    pub policies: Vec<CompiledPolicy>,
    pub limits: CatalogLimits,
    pub vector: Option<Value>,
    pub native_catalog: Option<Value>,
    pub admitted_catalog: Option<PolicyCatalog>,
}

impl BindingCheck {
    /// Acquire transaction-owned relation locks through a zero-row projection.
    /// LOCK TABLE needs table-wide SELECT, which column-only roles do NOT have.
    /// Never broaden their grants just to pin. The probe is fixed platform SQL,
    /// not the user statement; verify runs before any user prepare/execute.
    pub(crate) async fn lock_relations(&self, tx: &Transaction<'_>) -> Result<(), ExecError> {
        self.lock_catalog(tx, self.admitted_catalog.as_ref().ok_or_else(drift)?)
            .await
    }

    async fn lock_catalog(
        &self,
        tx: &Transaction<'_>,
        catalog: &PolicyCatalog,
    ) -> Result<(), ExecError> {
        let role = ident(&self.identity.role);
        tx.batch_execute(&format!("SET LOCAL ROLE {role}"))
            .await
            .map_err(database)?;
        let mut relations: Vec<_> = catalog.relations.iter().collect();
        relations.sort_by_key(|r| (&r.physical_schema, &r.physical_name));
        if relations.is_empty() {
            return Err(drift());
        }
        let mut projections = Vec::new();
        for relation in relations {
            let columns = relation
                .columns
                .iter()
                .map(|c| ident(&c.physical_name))
                .collect::<Vec<_>>()
                .join(",");
            if columns.is_empty() {
                return Err(drift());
            }
            projections.push(format!(
                "SELECT {columns} FROM {}.{} LIMIT 0;",
                ident(&relation.physical_schema),
                ident(&relation.physical_name)
            ));
        }
        // Fixed platform SQL only, bounded to 32 zero-row statements per batch.
        // User statements still use the prepared portal path, never batch_execute.
        for chunk in projections.chunks(32) {
            tx.batch_execute(&chunk.join("\n"))
                .await
                .map_err(database)?;
        }
        tx.batch_execute("SET LOCAL ROLE NONE; SET LOCAL search_path TO pg_catalog")
            .await
            .map_err(database)?;
        Ok(())
    }

    async fn load_catalog(&self, tx: &Transaction<'_>) -> Result<PolicyCatalog, ExecError> {
        load_policy_catalog_in_transaction(
            tx,
            &self.identity,
            &self.policies,
            &self.limits,
            self.source.as_deref(),
            self.vector.as_ref(),
            self.native_catalog.as_ref(),
        )
        .await
        .map_err(|error| match error {
            CatalogError::Database { sqlstate } if sqlstate == "57014" => ExecError::Timeout {
                sqlstate: Some(sqlstate),
            },
            _ => drift(),
        })
    }

    /// Metadata has no user SQL planning catalog to revalidate. Load and pin
    /// its complete authorized catalog once, inside the response's snapshot.
    pub(crate) async fn metadata_scope(
        &self,
        tx: &Transaction<'_>,
    ) -> Result<(BindingReceipt, PolicyCatalog), ExecError> {
        if self.admitted_catalog.is_some() {
            return Err(drift());
        }
        let settings = tx.query_one(
            "SELECT current_setting('transaction_isolation'),current_setting('transaction_read_only')", &[],
        ).await.map_err(database)?;
        if settings.get::<_, String>(0) != "repeatable read" || settings.get::<_, String>(1) != "on"
        {
            return Err(drift());
        }
        let current = self.load_catalog(tx).await?;
        self.lock_catalog(tx, &current).await?;
        let receipt = self.receipt_in_snapshot(tx, &current).await?;
        Ok((receipt, current))
    }

    pub(crate) async fn verify(&self, tx: &Transaction<'_>) -> Result<BindingReceipt, ExecError> {
        let current = self.load_catalog(tx).await?;
        if self.admitted_catalog.as_ref() != Some(&current) {
            return Err(drift());
        }
        self.receipt_in_snapshot(tx, &current).await
    }

    async fn receipt_in_snapshot(
        &self,
        tx: &Transaction<'_>,
        current: &PolicyCatalog,
    ) -> Result<BindingReceipt, ExecError> {
        let mut data = Vec::new();
        let mut permissions = Vec::new();
        let mut source_set = BTreeSet::new();
        let mut sources = Vec::new();
        let mut schemas = Vec::new();
        let mut names = Vec::new();
        let mut physical_schemas = Vec::new();
        let mut physical_names = Vec::new();
        let cut = if let Some(catalog) = self
            .native_catalog
            .as_ref()
            .filter(|c| c["protocol"] == "oo.gateway.library-binding/v1")
        {
            if catalog["pack_digest"] != self.pack_digest {
                return Err(drift());
            }
            Some(library_cut(tx, catalog).await?)
        } else {
            None
        };
        let vector = self
            .vector
            .as_ref()
            .map(crate::policy::catalog::index_vector)
            .transpose()
            .map_err(|_| drift())?;
        for relation in &current.relations {
            let dataset = format!("{}.{}", relation.schema, relation.name);
            let source = vector
                .as_ref()
                .and_then(|v| v.get(dataset.as_str()))
                .and_then(|v| v["source_id"].as_str())
                .or(self.source.as_deref())
                .unwrap_or(&self.identity.project);
            source_set.insert(source.to_owned());
            sources.push(source.to_owned());
            schemas.push(relation.schema.clone());
            names.push(relation.name.clone());
            physical_schemas.push(relation.physical_schema.clone());
            physical_names.push(relation.physical_name.clone());
        }
        // Migration 0135 is an explicit, publisher-controlled upgrade. Never
        // manufacture identities on a read path or fall back after installation.
        // 0136 binds routine dependencies of views; an identity digest without
        // it cannot see a replaced function, so its absence is drift too.
        let installation = tx
            .query_one(
                "SELECT to_regclass('_oo.publication_identity') IS NOT NULL
                    OR to_regprocedure('_oo.publication_relation_digest(text,text)') IS NOT NULL,
                    (SELECT count(*)=2 FROM pg_catalog.pg_event_trigger
                     WHERE evtname IN ('oo_publication_identity_ddl','oo_publication_identity_drop')
                     AND evtenabled='O'
                     AND evtfoid=to_regprocedure('_oo.publication_identity_invalidate()'))
                    AND to_regprocedure('_oo.publication_routine_identity(oid[])') IS NOT NULL",
                &[],
            )
            .await
            .map_err(database)?;
        let logical: bool = installation.get(0);
        if logical && !installation.get::<_, bool>(1) {
            return Err(drift());
        }
        let sql = if logical {
            "SELECT w.ordinality::int8,to_jsonb(f)::text,to_jsonb(pf)::text,
                jsonb_build_array(i.publication_id,i.generation,i.relation_identity_sha256)::text
             FROM unnest($1::text[],$2::text[],$3::text[],$4::text[],$5::text[]) WITH ORDINALITY
                w(source,schema_name,table_name,physical_schema,physical_name,ordinality)
             LEFT JOIN _oo.table_freshness f ON NOT $8 AND f.source=w.source AND f.schema_name=w.schema_name
                AND f.table_name=w.table_name AND f.physical_name=w.physical_name
             JOIN oo_publish_target.fence pf ON pf.role_name=$6 AND pf.schema_name=w.physical_schema
                AND pf.relation_name=w.physical_name AND pf.state='open' AND pf.opened_pack_digest=$7
             JOIN _oo.publication_identity i ON i.role_name=pf.role_name
                AND i.schema_name=pf.schema_name AND i.relation_name=pf.relation_name
                AND i.valid AND i.publication_id=pf.opened_pack_digest AND i.generation=pf.opened_version
                AND i.relation_identity_sha256=_oo.publication_relation_digest(i.schema_name,i.relation_name)
             WHERE $8 OR f.source IS NOT NULL"
        } else {
            "SELECT w.ordinality::int8,to_jsonb(f)::text,to_jsonb(pf)::text,
                jsonb_build_array(c.oid,c.relacl,c.relrowsecurity,c.relforcerowsecurity,c.relkind,c.reloptions,
                 CASE WHEN c.relkind='v' THEN pg_catalog.pg_get_viewdef(c.oid,true) ELSE c.relfilenode::text END,
                 (SELECT jsonb_agg(jsonb_build_array(a.attnum,a.attname,a.atttypid,a.atttypmod,
                     a.attnotnull,a.attcollation,a.attgenerated,a.attidentity,a.attisdropped,a.attacl) ORDER BY a.attnum)
                  FROM pg_catalog.pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0),
                 (SELECT jsonb_agg(to_jsonb(p) ORDER BY p.oid) FROM pg_catalog.pg_policy p WHERE p.polrelid=c.oid))::text
             FROM unnest($1::text[],$2::text[],$3::text[],$4::text[],$5::text[]) WITH ORDINALITY
                w(source,schema_name,table_name,physical_schema,physical_name,ordinality)
             LEFT JOIN _oo.table_freshness f ON NOT $8 AND f.source=w.source AND f.schema_name=w.schema_name
                AND f.table_name=w.table_name AND f.physical_name=w.physical_name
             JOIN oo_publish_target.fence pf ON pf.role_name=$6 AND pf.schema_name=w.physical_schema
                AND pf.relation_name=w.physical_name AND pf.state='open' AND pf.opened_pack_digest=$7
             JOIN pg_catalog.pg_namespace n ON n.nspname=w.physical_schema
             JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=w.physical_name
                AND c.oid=pg_catalog.to_regclass(pg_catalog.quote_ident(w.physical_schema)||'.'||pg_catalog.quote_ident(w.physical_name))
                AND (c.relkind='v' OR c.relfilenode=pg_catalog.pg_relation_filenode(c.oid))
             WHERE $8 OR f.source IS NOT NULL"
        };
        let rows = tx
            .query(
                sql,
                &[
                    &sources,
                    &schemas,
                    &names,
                    &physical_schemas,
                    &physical_names,
                    &self.identity.role,
                    &self.pack_digest,
                    &cut.is_some(),
                ],
            )
            .await
            .map_err(database)?;
        let mut seen = BTreeSet::new();
        for row in rows {
            let idx = usize::try_from(row.get::<_, i64>(0))
                .ok()
                .and_then(|i| i.checked_sub(1))
                .filter(|i| *i < current.relations.len())
                .ok_or_else(drift)?;
            if !seen.insert(idx) {
                return Err(drift());
            }
            if cut.is_none() {
                data.push(json!([schemas[idx], names[idx], row.get::<_, String>(1)]));
            }
            permissions.push(row.get::<_, String>(2));
            permissions.push(row.get::<_, String>(3));
        }
        if seen.len() != current.relations.len() {
            return Err(drift());
        }
        // Native C5 can publish data while retaining all semantic/Pack labels.
        // Bind the actual source positions as well as per-table generations.
        let has_watermarks: bool = tx
            .query_one(
                "SELECT pg_catalog.to_regclass('_oo.source_watermark') IS NOT NULL",
                &[],
            )
            .await
            .map_err(database)?
            .get(0);
        if let Some(cut) = cut {
            data.push(json!(["published_library", cut]));
        } else if has_watermarks {
            let sources: Vec<_> = source_set.iter().cloned().collect();
            let rows=tx.query("SELECT source,to_jsonb(w)::text FROM _oo.source_watermark w WHERE source=ANY($1::text[])",
                &[&sources]).await.map_err(database)?;
            let mut seen = BTreeSet::new();
            for row in rows {
                let source: String = row.get(0);
                if !source_set.contains(&source) || !seen.insert(source.clone()) {
                    return Err(drift());
                }
                data.push(json!(["source_watermark", source, row.get::<_, String>(1)]));
            }
            if seen != source_set {
                return Err(drift());
            }
        } else if self.vector.is_some() {
            return Err(drift());
        }
        data.sort_by_key(Value::to_string);
        permissions.sort();
        let mut schema = json!({
            "relations": current.relations.iter().map(|r| json!([
                r.schema, r.name, r.physical_schema, r.physical_name,
                r.columns.iter().map(|c| json!([c.name, c.physical_name, c.type_oid])).collect::<Vec<_>>()
            ])).collect::<Vec<_>>(),
            "functions": current.functions.iter().map(|f| json!([
                f.oid, f.schema, f.name, f.argument_type_oids, f.aggregate_star
            ])).collect::<Vec<_>>(),
            "operators": current.operators.iter().map(|o| json!([
                o.oid, o.procedure_oid, o.schema, o.name, o.left_type_oid, o.right_type_oid, o.result_type_oid
            ])).collect::<Vec<_>>(),
            "literal_types": [current.literal_types.integer, current.literal_types.text, current.literal_types.boolean],
            "null_test_allowed": current.null_test_allowed,
        });
        if logical {
            // Parser/catalog OIDs are transaction-local handles, not receipt identity.
            // Use PostgreSQL's qualified symbolic signatures for the allowed surface.
            let types: Vec<u32> = current
                .relations
                .iter()
                .flat_map(|r| r.columns.iter().map(|c| c.type_oid))
                .collect();
            let functions: Vec<u32> = current.functions.iter().map(|f| f.oid).collect();
            let operators: Vec<u32> = current.operators.iter().map(|o| o.oid).collect();
            let literals = vec![
                current.literal_types.integer,
                current.literal_types.text,
                current.literal_types.boolean,
            ];
            let symbols: String = tx.query_one(
                "SELECT jsonb_build_array(
                  (SELECT jsonb_agg(format_type(t,NULL) ORDER BY ord) FROM unnest($1::oid[]) WITH ORDINALITY w(t,ord)),
                  (SELECT jsonb_agg(p::regprocedure::text ORDER BY ord) FROM unnest($2::oid[]) WITH ORDINALITY w(p,ord)),
                  (SELECT jsonb_agg(jsonb_build_array(o::regoperator::text,
                    op.oprcode::regprocedure::text,format_type(op.oprresult,NULL)) ORDER BY ord)
                   FROM unnest($3::oid[]) WITH ORDINALITY w(o,ord) JOIN pg_catalog.pg_operator op ON op.oid=w.o),
                  (SELECT jsonb_agg(format_type(t,NULL) ORDER BY ord) FROM unnest($4::oid[]) WITH ORDINALITY w(t,ord)))::text",
                &[&types,&functions,&operators,&literals],
            ).await.map_err(database)?.get(0);
            schema = json!({
                "protocol":"oo.logical-publication/v1",
                "relations":current.relations.iter().map(|r| json!([
                    r.schema,r.name,r.physical_schema,r.physical_name,
                    r.columns.iter().map(|c| json!([c.name,c.physical_name])).collect::<Vec<_>>()
                ])).collect::<Vec<_>>(),
                "symbols":symbols,
                "aggregate_star":current.functions.iter().map(|f|f.aggregate_star).collect::<Vec<_>>(),
                "null_test_allowed":current.null_test_allowed,
            });
        }
        let receipt = receipt(
            &json!([
                self.identity.project,
                self.principal,
                self.identity.role,
                self.identity.policy_id,
                self.policy_version,
                self.knowledge_version,
                self.pack_digest,
                self.vector,
                self.native_catalog,
                permissions,
                schema
            ]),
            &json!(data),
        );
        if !self.expected.matches(&receipt) {
            return Err(drift());
        }
        Ok(receipt)
    }
}

fn receipt(scope: &Value, data: &Value) -> BindingReceipt {
    let data_vector_sha256 = digest(data);
    let publication_id = format!("pub1_{}", digest(&json!([scope, data_vector_sha256])));
    BindingReceipt {
        publication_id,
        data_vector_sha256,
    }
}

fn digest(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON value"))
    )
}

fn ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn drift() -> ExecError {
    ExecError::Protocol {
        step: "publication_drift",
    }
}
fn database(error: tokio_postgres::Error) -> ExecError {
    if error.code().is_some_and(|code| code.code() == "57014") {
        ExecError::Timeout {
            sqlstate: Some("57014".into()),
        }
    } else {
        drift()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_includes_data_semantics_permissions_and_principal() {
        let base = receipt(
            &json!(["role", "principal", "kv1", "acl1"]),
            &json!(["generation1"]),
        );
        assert_ne!(
            base,
            receipt(
                &json!(["role", "principal", "kv1", "acl1"]),
                &json!(["generation2"])
            )
        );
        for scope in [
            json!(["role", "other", "kv1", "acl1"]),
            json!(["role", "principal", "kv2", "acl1"]),
            json!(["role", "principal", "kv1", "acl2"]),
        ] {
            let changed = receipt(&scope, &json!(["generation1"]));
            assert_ne!(base.publication_id, changed.publication_id);
            assert_eq!(base.data_vector_sha256, changed.data_vector_sha256);
        }
        let forged = Expected {
            publication_id: format!("pub1_{}", "0".repeat(64)),
            data_vector_sha256: None,
        };
        assert!(!forged.matches(&base));
    }

    #[test]
    fn headers_are_optional_but_never_ambiguous() {
        let mut headers = HeaderMap::new();
        assert_eq!(Expected::from_headers(&headers), Ok(None));
        headers.insert(EXPECTED_VECTOR, HeaderValue::from_static("bad"));
        assert!(Expected::from_headers(&headers).is_err());
        headers.clear();
        headers.insert(EXPECTED, HeaderValue::from_static("current"));
        assert!(Expected::from_headers(&headers).unwrap().is_some());
        headers.append(EXPECTED, HeaderValue::from_static("current"));
        assert!(Expected::from_headers(&headers).is_err());
    }
}
