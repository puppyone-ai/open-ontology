//! Sealed Agent DB catalog capture.
//!
//! This reads `pg_catalog` through the trusted TLS reader. It does not call
//! `catalog-pg`, `set_config`, or the source database. Logical names come from
//! the sealed mapping. `_oo_pos` is excluded. Histograms and raw values stay
//! withheld. The wire document is `oo.source.pgsql/v1` so the existing compiler
//! can hash it. That serialization does not claim a source-preflight pass.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::bind::SealedCopy;
use crate::config::Config;
use crate::source::{MappingRead, LiveColumn, LiveTable};

const METADATA_COLUMN: &str = "_oo_pos";
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub struct PhaseFault {
    pub phase: &'static str,
    pub reason: &'static str,
}

pub struct LocalCapture {
    pub catalog_text: String,
    pub catalog: Value,
    pub profile: Value,
    pub comments: Value,
    pub provenance: Value,
    pub schema_hash: String,
}

#[derive(Clone)]
struct HashColumn {
    attnum: i32,
    name: String,
    type_oid: i64,
    typmod: i32,
    not_null: bool,
    collation_oid: i64,
    typtype: String,
    base_type_oid: Option<i64>,
    identity: String,
    generated: String,
    default_sha256: Option<String>,
    enum_labels: Vec<String>,
    domain_checks: Vec<String>,
}

#[derive(Clone)]
struct HashKey {
    attnum: i32,
    name: String,
}

#[derive(Clone)]
struct HashUnique {
    name: String,
    columns: Vec<HashKey>,
    nulls_not_distinct: bool,
}

#[derive(Clone)]
struct HashForeign {
    name: String,
    columns: Vec<HashForeignColumn>,
}

#[derive(Clone)]
struct HashForeignColumn {
    attnum: i32,
    name: String,
    ref_schema: String,
    ref_table: String,
    ref_attnum: i32,
    ref_name: String,
}

#[derive(Clone)]
struct HashPolicy {
    name: String,
    command: String,
    permissive: bool,
    using_sha256: Option<String>,
    check_sha256: Option<String>,
}

#[derive(Clone)]
struct HashTable {
    schema: String,
    name: String,
    relkind: String,
    replica_identity: String,
    rls: bool,
    force_rls: bool,
    columns: Vec<HashColumn>,
    primary_key: Vec<HashKey>,
    primary_key_nulls_not_distinct: Option<bool>,
    unique_keys: Vec<HashUnique>,
    foreign_keys: Vec<HashForeign>,
    policies: Vec<HashPolicy>,
}

pub fn capture(config: &Config, sealed: &SealedCopy, mapping: &MappingRead) -> Result<LocalCapture, PhaseFault> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| fault("catalog.runtime", "unavailable"))?;
    runtime.block_on(capture_task(config, sealed, mapping))
}

async fn capture_task(config: &Config, sealed: &SealedCopy, mapping: &MappingRead) -> Result<LocalCapture, PhaseFault> {
    if mapping.tables.len() != sealed.tables.len() || mapping.tables.is_empty() {
        return Err(fault("catalog.reconcile", "table_count"));
    }
    let client = crate::source::connect(config, "minimal-product-loop-local-catalog", Duration::from_secs(30))
        .await
        .map_err(connect_fault)?;
    client
        .batch_execute("SET statement_timeout = '30s'")
        .await
        .map_err(|error| fault("catalog.session", sql_reason(&error)))?;
    let mut tables = Vec::new();
    let mut wire_tables = Vec::new();
    let mut profile_tables = Vec::new();
    let mut comment_tables = Vec::new();
    let mut metadata_keys_omitted = 0_u32;
    for live in &mapping.tables {
        if !sealed.tables.iter().any(|table| table.schema == live.logical_schema && table.name == live.logical_table) {
            return Err(fault("catalog.reconcile", "table_absent"));
        }
        let read = read_table(&client, live, &mapping.tables).await?;
        metadata_keys_omitted = metadata_keys_omitted.saturating_add(read.metadata_keys_omitted);
        tables.push(read.hash);
        profile_tables.push(read.profile);
        comment_tables.push(read.comments);
        wire_tables.push(read.wire);
    }
    tables.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    let schema_hash = sha256_hex(schema_preimage(&tables).as_bytes());
    let captured_at = captured_at()?;
    if !token_ok(&sealed.source_id) || !name_ok(&config.database) {
        return Err(fault("catalog.identity", "token"));
    }
    let catalog = json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": sealed.source_id,
        "database": config.database,
        "captured_at": captured_at,
        "schema_hash": schema_hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": wire_tables,
        "type_conversion": "unsupported"
    });
    let catalog_text = serde_json::to_string(&catalog).map_err(|_| fault("catalog.encode", "json"))?;
    let names: Vec<String> = tables.iter().map(|table| format!("{}.{}", table.schema, table.name)).collect();
    let provenance = json!({
        "protocol": "oo.minimal-product-loop.local-catalog/v1",
        "capture": "local_agent_db_capture",
        "source_preflight": "source_preflight_not_invoked",
        "catalog_pg_execution": "not_claimed",
        "sample_policy": "none",
        "histogram_bounds": "withheld",
        "most_common_vals": "withheld",
        "attnum": "agent_db_attribute_number",
        "names": "logical_original",
        "metadata_column": METADATA_COLUMN,
        "metadata_keys_omitted": metadata_keys_omitted,
        "schema_hash_algorithm": "pg-catalog-hash/v1",
        "schema_hash": schema_hash,
        "source_copy_receipt": {
            "manifest_import_hash": sealed.import_hash,
            "snapshot_id": sealed.snapshot_id,
            "source_id": sealed.source_id,
            "generation": sealed.generation,
            "plan_sha256": sealed.plan_sha256
        },
        "config_digest": mapping.config_digest,
        "server_version": mapping.version,
        "tables": names
    });
    Ok(LocalCapture {
        catalog_text,
        catalog,
        profile: json!({
            "protocol": "oo.pg-builder.profile/v1",
            "sample_policy": "none",
            "tables": profile_tables
        }),
        comments: json!({
            "protocol": "oo.pg-builder.comments/v1",
            "authority": "untrusted_data",
            "tables": comment_tables
        }),
        provenance,
        schema_hash,
    })
}

struct TableRead {
    hash: HashTable,
    wire: Value,
    profile: Value,
    comments: Value,
    metadata_keys_omitted: u32,
}

async fn read_table(client: &tokio_postgres::Client, live: &LiveTable, all: &[LiveTable]) -> Result<TableRead, PhaseFault> {
    require_ident(&live.logical_schema, "catalog.ident")?;
    require_ident(&live.logical_table, "catalog.ident")?;
    let relation = client
        .query(
            "SELECT c.relkind::text, c.relreplident::text, c.relrowsecurity, c.relforcerowsecurity, \
             pg_catalog.pg_get_userbyid(c.relowner)::text, pg_catalog.obj_description(c.oid, 'pg_class'), c.reltuples::text \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("catalog.table", sql_reason(&error)))?;
    if relation.len() != 1 {
        return Err(fault("catalog.table", "relation_absent"));
    }
    let relkind: String = relation[0].get(0);
    let replica: String = relation[0].get(1);
    let rls: bool = relation[0].get(2);
    let force_rls: bool = relation[0].get(3);
    let owner: String = relation[0].get(4);
    let table_comment: Option<String> = relation[0].get(5);
    let reltuples: Option<String> = relation[0].get(6);
    if relkind.len() != 1 || replica.len() != 1 || !name_ok(&owner) {
        return Err(fault("catalog.table", "marker"));
    }
    let attributes = client
        .query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull, \
             a.attcollation::int8, coll.collname::text, a.attidentity::text, a.attgenerated::text, \
             pg_catalog.pg_get_expr(ad.adbin, ad.adrelid), pg_catalog.col_description(a.attrelid, a.attnum), \
             t.typname::text, tn.nspname::text, t.typtype::text, t.typbasetype::int8, \
             pg_catalog.has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'SELECT'), \
             pg_catalog.has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'INSERT'), \
             pg_catalog.has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'UPDATE') \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
             JOIN pg_catalog.pg_type AS t ON t.oid = a.atttypid \
             JOIN pg_catalog.pg_namespace AS tn ON tn.oid = t.typnamespace \
             LEFT JOIN pg_catalog.pg_collation AS coll ON coll.oid = a.attcollation AND a.attcollation <> 0 \
             LEFT JOIN pg_catalog.pg_attrdef AS ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             WHERE n.nspname = $1 AND c.relname = $2 \
             ORDER BY a.attnum",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("catalog.column", sql_reason(&error)))?;
    let stats = client
        .query(
            "SELECT a.attname::text, s.null_frac::text, s.n_distinct::text, s.correlation::text \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
             LEFT JOIN pg_catalog.pg_stats AS s \
               ON s.schemaname = n.nspname AND s.tablename = c.relname AND s.attname = a.attname \
             WHERE n.nspname = $1 AND c.relname = $2 \
             ORDER BY a.attnum",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("profile.query", sql_reason(&error)))?;
    let mut by_physical: BTreeMap<&str, &LiveColumn> = BTreeMap::new();
    for column in &live.columns {
        if column.name == METADATA_COLUMN || column.physical_name == METADATA_COLUMN {
            return Err(fault("catalog.reconcile", "metadata_selected"));
        }
        require_ident(&column.name, "catalog.ident")?;
        if by_physical.insert(column.physical_name.as_str(), column).is_some() {
            return Err(fault("catalog.reconcile", "duplicate_physical"));
        }
    }
    let mut seen = BTreeMap::<String, HashColumn>::new();
    let mut wire_columns = Vec::new();
    let mut profile_columns = Vec::new();
    let mut comment_columns = Vec::new();
    let mut stat_by_name = BTreeMap::new();
    for row in &stats {
        let name: String = row.get(0);
        stat_by_name.insert(name, (optional_text(row.get(1)), optional_text(row.get(2)), optional_text(row.get(3))));
    }
    for row in &attributes {
        let attnum: i32 = row.get(0);
        let physical: String = row.get(1);
        let type_oid: i64 = row.get(2);
        let typmod: i32 = row.get(3);
        let not_null: bool = row.get(4);
        let collation_oid: i64 = row.get(5);
        let collation_name: Option<String> = row.get(6);
        let identity: String = row.get(7);
        let generated: String = row.get(8);
        let default_expr: Option<String> = row.get(9);
        let comment: Option<String> = row.get(10);
        let type_name: String = row.get(11);
        let type_schema: String = row.get(12);
        let typtype: String = row.get(13);
        let base_type: i64 = row.get(14);
        let select_priv: Option<bool> = row.get(15);
        let insert_priv: Option<bool> = row.get(16);
        let update_priv: Option<bool> = row.get(17);
        if physical == METADATA_COLUMN {
            continue;
        }
        let Some(mapped) = by_physical.get(physical.as_str()) else {
            return Err(fault("catalog.reconcile", "unmapped_column"));
        };
        if mapped.type_oid != type_oid || mapped.typmod != typmod || mapped.type_name != type_name {
            return Err(fault("catalog.reconcile", "type_mismatch"));
        }
        if !(1..=1600).contains(&attnum) || typtype.len() != 1 || !name_ok(&type_name) || !name_ok(&type_schema) {
            return Err(fault("catalog.column", "marker"));
        }
        if !matches!(identity.as_str(), "" | "a" | "d") || !matches!(generated.as_str(), "" | "s") {
            return Err(fault("catalog.column", "identity"));
        }
        match (collation_oid == 0, collation_name.as_deref()) {
            (true, None) => {}
            (false, Some(name)) if name_ok(name) => {}
            _ => return Err(fault("catalog.column", "collation")),
        }
        let base_type_oid = if typtype == "d" {
            if !(1..=i64::from(u32::MAX)).contains(&base_type) {
                return Err(fault("catalog.column", "domain_base"));
            }
            Some(base_type)
        } else if base_type == 0 {
            None
        } else {
            return Err(fault("catalog.column", "base_type"));
        };
        if typmod < -1 {
            return Err(fault("catalog.column", "typmod"));
        }
        let (numeric_precision, numeric_scale) = if type_name == "numeric" {
            match numeric_parts(typmod) {
                Some((precision, scale)) => (Some(precision), Some(scale)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        let enum_labels = if typtype == "e" {
            enum_labels(client, type_oid).await?
        } else {
            Vec::new()
        };
        let domain_checks = if typtype == "d" {
            domain_checks(client, type_oid).await?
        } else {
            Vec::new()
        };
        let default_evidence = evidence_value(default_expr.as_deref())?;
        let comment_evidence = evidence_value(comment.as_deref())?;
        let hash_column = HashColumn {
            attnum,
            name: mapped.name.clone(),
            type_oid,
            typmod,
            not_null,
            collation_oid,
            typtype: typtype.clone(),
            base_type_oid,
            identity: identity.clone(),
            generated: generated.clone(),
            default_sha256: evidence_sha(&default_evidence),
            enum_labels: enum_labels.clone(),
            domain_checks: domain_checks.iter().filter_map(|item| item.get("sha256").and_then(Value::as_str).map(str::to_owned)).collect(),
        };
        if seen.insert(mapped.name.clone(), hash_column).is_some() {
            return Err(fault("catalog.reconcile", "duplicate_logical"));
        }
        wire_columns.push(json!({
            "attnum": attnum,
            "name": mapped.name,
            "name_ident": mapped.name,
            "type_oid": type_oid,
            "type_schema": type_schema,
            "type_name": type_name,
            "typtype": typtype,
            "typmod": typmod,
            "base_type_oid": base_type_oid,
            "numeric_precision": numeric_precision,
            "numeric_scale": numeric_scale,
            "not_null": not_null,
            "collation_oid": collation_oid,
            "collation_name": collation_name,
            "identity": identity,
            "generated": generated,
            "default_expr": default_evidence,
            "comment": comment_evidence,
            "enum_labels": enum_labels,
            "domain_checks": domain_checks,
            "type_support": {"converted": false, "reason": type_reason(&typtype, &type_name, typmod)},
            "current_privileges": {"select": select_priv, "insert": insert_priv, "update": update_priv}
        }));
        let (null_frac, n_distinct, correlation) = stat_by_name.get(&physical).cloned().unwrap_or((Value::Null, Value::Null, Value::Null));
        profile_columns.push(json!({
            "name": mapped.name,
            "attnum": attnum,
            "null_frac": null_frac,
            "n_distinct": n_distinct,
            "correlation": correlation
        }));
        comment_columns.push(json!({"name": mapped.name, "text": optional_text(comment)}));
    }
    if seen.len() != live.columns.len() {
        return Err(fault("catalog.reconcile", "column_absent"));
    }
    let mut columns: Vec<HashColumn> = seen.into_values().collect();
    columns.sort_by_key(|column| column.attnum);
    wire_columns.sort_by_key(|column| column.get("attnum").and_then(Value::as_i64).unwrap_or(0));
    let keys = read_keys(client, live, all).await?;
    let privileges = read_privileges(client, live).await?;
    let policies = read_policies(client, live).await?;
    let hash = HashTable {
        schema: live.logical_schema.clone(),
        name: live.logical_table.clone(),
        relkind,
        replica_identity: replica.clone(),
        rls,
        force_rls,
        columns,
        primary_key: keys.primary.clone(),
        primary_key_nulls_not_distinct: keys.primary_nulls,
        unique_keys: keys.uniques.clone(),
        foreign_keys: keys.foreign.clone(),
        policies: policies.hash.clone(),
    };
    let profile = json!({
        "schema": live.logical_schema,
        "name": live.logical_table,
        "reltuples": optional_text(reltuples),
        "columns": profile_columns
    });
    let comments = json!({
        "schema": live.logical_schema,
        "name": live.logical_table,
        "text": optional_text(table_comment.clone()),
        "columns": comment_columns
    });
    let table_comment_evidence = evidence_value(table_comment.as_deref())?;
    let mut wire = json!({
        "schema": live.logical_schema,
        "schema_ident": live.logical_schema,
        "name": live.logical_table,
        "name_ident": live.logical_table,
        "relkind": hash.relkind,
        "replica_identity": replica,
        "rls_enabled": rls,
        "rls_forced": force_rls,
        "owner": owner,
        "comment": table_comment_evidence,
        "columns": wire_columns,
        "primary_key": keys.primary.iter().map(|column| json!({"attnum": column.attnum, "name": column.name})).collect::<Vec<_>>(),
        "primary_key_nulls_not_distinct": keys.primary_nulls,
        "unique_keys": keys.unique_wire,
        "foreign_keys": keys.foreign_wire,
        "policies": policies.wire,
        "privileges": privileges
    });
    if keys.primary.is_empty() && let Some(object) = wire.as_object_mut() {
        object.insert("primary_key_nulls_not_distinct".to_owned(), Value::Null);
    }
    Ok(TableRead {
        hash,
        wire,
        profile,
        comments,
        metadata_keys_omitted: keys.metadata_keys_omitted,
    })
}

struct KeyRead {
    primary: Vec<HashKey>,
    primary_nulls: Option<bool>,
    uniques: Vec<HashUnique>,
    unique_wire: Vec<Value>,
    foreign: Vec<HashForeign>,
    foreign_wire: Vec<Value>,
    metadata_keys_omitted: u32,
}

async fn read_keys(client: &tokio_postgres::Client, live: &LiveTable, all: &[LiveTable]) -> Result<KeyRead, PhaseFault> {
    let primary_rows = client
        .query(
            "SELECT ic.relname::text, i.indnullsnotdistinct, (i.indpred IS NULL AND i.indexprs IS NULL), \
             a.attnum::int4, a.attname::text \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_index AS i ON i.indrelid = c.oid AND i.indisprimary \
             JOIN pg_catalog.pg_class AS ic ON ic.oid = i.indexrelid \
             JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
             LEFT JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid AND a.attnum = u.attnum \
             WHERE n.nspname = $1 AND c.relname = $2 \
             ORDER BY ic.relname, u.ord",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("catalog.primary_key", sql_reason(&error)))?;
    let primary = one_key(&primary_rows, live, "catalog.primary_key")?;
    let (primary_columns, primary_nulls, primary_omitted) = match primary {
        KeyChoice::Absent => (Vec::new(), None, 0),
        KeyChoice::OmitMetadata => (Vec::new(), None, 1),
        KeyChoice::Keep { columns, nulls_not_distinct, .. } => (columns, Some(nulls_not_distinct), 0),
    };
    let unique_rows = client
        .query(
            "SELECT ic.relname::text, i.indnullsnotdistinct, (i.indpred IS NULL AND i.indexprs IS NULL), \
             a.attnum::int4, a.attname::text \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_index AS i ON i.indrelid = c.oid AND i.indisunique AND NOT i.indisprimary \
             JOIN pg_catalog.pg_class AS ic ON ic.oid = i.indexrelid \
             JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
             LEFT JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid AND a.attnum = u.attnum \
             WHERE n.nspname = $1 AND c.relname = $2 \
             ORDER BY ic.relname, u.ord",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("catalog.unique", sql_reason(&error)))?;
    let mut uniques = Vec::new();
    let mut unique_wire = Vec::new();
    let mut omitted: u32 = primary_omitted;
    for group in group_keys(&unique_rows) {
        match one_grouped(&group, live, "catalog.unique")? {
            KeyChoice::Absent => {}
            KeyChoice::OmitMetadata => omitted = omitted.saturating_add(1),
            KeyChoice::Keep { name, columns, nulls_not_distinct } => {
                unique_wire.push(json!({
                    "name": name,
                    "columns": columns.iter().map(|column| json!({"attnum": column.attnum, "name": column.name})).collect::<Vec<_>>(),
                    "nulls_not_distinct": nulls_not_distinct
                }));
                uniques.push(HashUnique { name, columns, nulls_not_distinct });
            }
        }
    }
    let foreign_rows = client
        .query(
            "SELECT con.conname::text, fn.nspname::text, fc.relname::text, \
             sa.attnum::int4, sa.attname::text, da.attnum::int4, da.attname::text \
             FROM pg_catalog.pg_class AS src \
             JOIN pg_catalog.pg_namespace AS sn ON sn.oid = src.relnamespace \
             JOIN pg_catalog.pg_constraint AS con ON con.conrelid = src.oid AND con.contype = 'f' \
             JOIN pg_catalog.pg_class AS fc ON fc.oid = con.confrelid \
             JOIN pg_catalog.pg_namespace AS fn ON fn.oid = fc.relnamespace \
             JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS u(src_att, dst_att, ord) ON true \
             JOIN pg_catalog.pg_attribute AS sa ON sa.attrelid = src.oid AND sa.attnum = u.src_att \
             JOIN pg_catalog.pg_attribute AS da ON da.attrelid = fc.oid AND da.attnum = u.dst_att \
             WHERE sn.nspname = $1 AND src.relname = $2 \
             ORDER BY con.conname, u.ord",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("catalog.foreign_key", sql_reason(&error)))?;
    let (foreign, foreign_wire) = foreign_keys(&foreign_rows, live, all)?;
    Ok(KeyRead {
        primary: primary_columns,
        primary_nulls,
        uniques,
        unique_wire,
        foreign,
        foreign_wire,
        metadata_keys_omitted: omitted,
    })
}

enum KeyChoice {
    Absent,
    OmitMetadata,
    Keep { name: String, columns: Vec<HashKey>, nulls_not_distinct: bool },
}

fn one_key(rows: &[tokio_postgres::Row], live: &LiveTable, phase_name: &'static str) -> Result<KeyChoice, PhaseFault> {
    let groups = group_keys(rows);
    if groups.len() > 1 {
        return Err(fault(phase_name, "not_one"));
    }
    let Some(group) = groups.first() else {
        return Ok(KeyChoice::Absent);
    };
    one_grouped(group, live, phase_name)
}

struct KeyGroup {
    name: String,
    plain: bool,
    nulls_not_distinct: bool,
    members: Vec<(i32, Option<String>)>,
}

fn group_keys(rows: &[tokio_postgres::Row]) -> Vec<KeyGroup> {
    let mut groups = Vec::<KeyGroup>::new();
    for row in rows {
        let name: String = row.get(0);
        let nulls_not_distinct: bool = row.get(1);
        let plain: bool = row.get(2);
        let attnum: i32 = row.get(3);
        let attname: Option<String> = row.get(4);
        if let Some(group) = groups.last_mut().filter(|group| group.name == name) {
            group.members.push((attnum, attname));
        } else {
            groups.push(KeyGroup { name, plain, nulls_not_distinct, members: vec![(attnum, attname)] });
        }
    }
    groups
}

fn one_grouped(group: &KeyGroup, live: &LiveTable, phase_name: &'static str) -> Result<KeyChoice, PhaseFault> {
    if !group.plain || group.members.is_empty() || !name_ok(&group.name) || group.name == "primary_key" {
        return Err(fault(phase_name, "unsupported_key"));
    }
    let mut logical = Vec::new();
    let mut metadata = 0_u32;
    for (attnum, physical) in &group.members {
        let Some(physical) = physical else {
            return Err(fault(phase_name, "expression_index"));
        };
        if physical == METADATA_COLUMN {
            metadata = metadata.saturating_add(1);
            continue;
        }
        let Some(column) = live.columns.iter().find(|column| column.physical_name == *physical) else {
            return Err(fault(phase_name, "unmapped_column"));
        };
        logical.push(HashKey { attnum: *attnum, name: column.name.clone() });
    }
    if metadata > 0 && !logical.is_empty() {
        return Err(fault(phase_name, "mixed_metadata_key"));
    }
    if metadata > 0 {
        return Ok(KeyChoice::OmitMetadata);
    }
    Ok(KeyChoice::Keep { name: group.name.clone(), columns: logical, nulls_not_distinct: group.nulls_not_distinct })
}

struct ForeignGroup {
    name: String,
    columns: Vec<HashForeignColumn>,
}

fn foreign_keys(rows: &[tokio_postgres::Row], live: &LiveTable, all: &[LiveTable]) -> Result<(Vec<HashForeign>, Vec<Value>), PhaseFault> {
    let mut grouped: Vec<ForeignGroup> = Vec::new();
    for row in rows {
        let name: String = row.get(0);
        let ref_schema: String = row.get(1);
        let ref_table: String = row.get(2);
        let attnum: i32 = row.get(3);
        let local_physical: String = row.get(4);
        let ref_attnum: i32 = row.get(5);
        let ref_physical: String = row.get(6);
        if local_physical == METADATA_COLUMN || ref_physical == METADATA_COLUMN {
            return Err(fault("catalog.foreign_key", "metadata_column"));
        }
        let Some(local) = live.columns.iter().find(|column| column.physical_name == local_physical) else {
            return Err(fault("catalog.foreign_key", "unmapped_column"));
        };
        let Some(target) = mapped_reference(all, &ref_schema, &ref_table, &ref_physical) else {
            return Err(fault("catalog.foreign_key", "unmapped_reference"));
        };
        let member = HashForeignColumn {
            attnum,
            name: local.name.clone(),
            ref_schema: target.0,
            ref_table: target.1,
            ref_attnum,
            ref_name: target.2,
        };
        if let Some(group) = grouped.last_mut().filter(|group| group.name == name) {
            group.columns.push(member);
        } else {
            grouped.push(ForeignGroup { name, columns: vec![member] });
        }
    }
    let mut hash = Vec::new();
    let mut wire = Vec::new();
    for group in grouped {
        if !name_ok(&group.name) {
            return Err(fault("catalog.foreign_key", "name"));
        }
        wire.push(json!({
            "name": group.name,
            "columns": group.columns.iter().map(|column| json!({
                "attnum": column.attnum,
                "name": column.name,
                "ref_schema": column.ref_schema,
                "ref_table": column.ref_table,
                "ref_attnum": column.ref_attnum,
                "ref_name": column.ref_name
            })).collect::<Vec<_>>()
        }));
        hash.push(HashForeign { name: group.name, columns: group.columns });
    }
    Ok((hash, wire))
}

fn mapped_reference(all: &[LiveTable], schema: &str, table: &str, column: &str) -> Option<(String, String, String)> {
    for live in all {
        if live.physical_schema == schema && live.physical_table == table {
            let found = live.columns.iter().find(|item| item.physical_name == column)?;
            return Some((live.logical_schema.clone(), live.logical_table.clone(), found.name.clone()));
        }
    }
    None
}

struct PolicyRead {
    hash: Vec<HashPolicy>,
    wire: Vec<Value>,
}

async fn read_policies(client: &tokio_postgres::Client, live: &LiveTable) -> Result<PolicyRead, PhaseFault> {
    let rows = client
        .query(
            "SELECT pol.polname::text, pol.polcmd::text, pol.polpermissive, \
             pg_catalog.pg_get_expr(pol.polqual, pol.polrelid), \
             pg_catalog.pg_get_expr(pol.polwithcheck, pol.polrelid) \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_policy AS pol ON pol.polrelid = c.oid \
             WHERE n.nspname = $1 AND c.relname = $2 \
             ORDER BY pol.polname",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("catalog.policy", sql_reason(&error)))?;
    let mut hash = Vec::new();
    let mut wire = Vec::new();
    for row in &rows {
        let name: String = row.get(0);
        let command: String = row.get(1);
        let permissive: bool = row.get(2);
        let using_expr: Option<String> = row.get(3);
        let check_expr: Option<String> = row.get(4);
        if !name_ok(&name) || command.len() != 1 {
            return Err(fault("catalog.policy", "marker"));
        }
        let using_evidence = evidence_value(using_expr.as_deref())?;
        let check_evidence = evidence_value(check_expr.as_deref())?;
        hash.push(HashPolicy {
            name: name.clone(),
            command: command.clone(),
            permissive,
            using_sha256: evidence_sha(&using_evidence),
            check_sha256: evidence_sha(&check_evidence),
        });
        wire.push(json!({
            "name": name,
            "command": command,
            "permissive": permissive,
            "using_expr": using_evidence,
            "check_expr": check_evidence
        }));
    }
    Ok(PolicyRead { hash, wire })
}

async fn read_privileges(client: &tokio_postgres::Client, live: &LiveTable) -> Result<Vec<Value>, PhaseFault> {
    let rows = client
        .query(
            "SELECT CASE WHEN priv.grantee = 0 THEN 'public' ELSE pg_catalog.pg_get_userbyid(priv.grantee)::text END, \
             priv.privilege_type::text, priv.is_grantable \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS priv \
             WHERE n.nspname = $1 AND c.relname = $2 \
             ORDER BY 1, 2, 3",
            &[&live.physical_schema, &live.physical_table],
        )
        .await
        .map_err(|error| fault("catalog.privilege", sql_reason(&error)))?;
    if rows.len() > 64 {
        return Err(fault("catalog.privilege", "budget"));
    }
    let mut privileges = Vec::new();
    for row in &rows {
        let grantee: String = row.get(0);
        let privilege: String = row.get(1);
        let grantable: bool = row.get(2);
        if !name_ok(&grantee) || !name_ok(&privilege) {
            return Err(fault("catalog.privilege", "name"));
        }
        privileges.push(json!({"grantee": grantee, "privilege": privilege, "grantable": grantable}));
    }
    Ok(privileges)
}

async fn enum_labels(client: &tokio_postgres::Client, type_oid: i64) -> Result<Vec<String>, PhaseFault> {
    let rows = client
        .query(
            "SELECT e.enumlabel::text FROM pg_catalog.pg_enum AS e WHERE e.enumtypid = $1 ORDER BY e.enumsortorder",
            &[&type_oid],
        )
        .await
        .map_err(|error| fault("catalog.enum", sql_reason(&error)))?;
    if rows.len() > 64 {
        return Err(fault("catalog.enum", "budget"));
    }
    let mut labels = Vec::new();
    for row in &rows {
        let label: String = row.get(0);
        if !name_ok(&label) {
            return Err(fault("catalog.enum", "name"));
        }
        labels.push(label);
    }
    Ok(labels)
}

async fn domain_checks(client: &tokio_postgres::Client, type_oid: i64) -> Result<Vec<Value>, PhaseFault> {
    let rows = client
        .query(
            "SELECT pg_catalog.pg_get_constraintdef(con.oid) \
             FROM pg_catalog.pg_constraint AS con \
             WHERE con.contype = 'c' AND con.contypid = $1 \
             ORDER BY con.oid",
            &[&type_oid],
        )
        .await
        .map_err(|error| fault("catalog.domain", sql_reason(&error)))?;
    if rows.len() > 16 {
        return Err(fault("catalog.domain", "budget"));
    }
    let mut checks = Vec::new();
    for row in &rows {
        let text: Option<String> = row.get(0);
        let Some(text) = text else {
            return Err(fault("catalog.domain", "empty"));
        };
        let value = evidence_value(Some(text.as_str()))?;
        if value.is_null() {
            return Err(fault("catalog.domain", "empty"));
        }
        checks.push(value);
    }
    Ok(checks)
}

fn fault(phase: &'static str, reason: &'static str) -> PhaseFault {
    PhaseFault { phase, reason }
}

fn connect_fault(reason: &'static str) -> PhaseFault {
    let mapped = match reason {
        "tls" => "tls",
        "connect" => "connect",
        "file" => "secret_file",
        _ => "connect",
    };
    fault("catalog.connect", mapped)
}

fn sql_reason(error: &tokio_postgres::Error) -> &'static str {
    match error.code().map(|code| code.code()) {
        Some("42501") => "permission_denied",
        Some("42P01") => "relation_absent",
        Some("42703") => "column_absent",
        _ => "query_refused",
    }
}

fn require_ident(name: &str, phase_name: &'static str) -> Result<(), PhaseFault> {
    if ident_shape(name) { Ok(()) } else { Err(fault(phase_name, "stable_ident_adapter_required")) }
}

fn ident_shape(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || name.starts_with("q_") {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_lowercase()) && chars.all(|ch| ch == '_' || ch.is_ascii_lowercase() || ch.is_ascii_digit())
}

fn name_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= 63 && !value.as_bytes().contains(&0) && value.chars().all(|ch| (ch as u32) >= 0x20 && ch != '\u{7f}')
}

fn token_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
}

fn optional_text(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

fn evidence_value(text: Option<&str>) -> Result<Value, PhaseFault> {
    let Some(text) = text else {
        return Ok(Value::Null);
    };
    if text.len() > 65_536 {
        return Err(fault("catalog.evidence", "too_long"));
    }
    let sha256 = sha256_hex(text.as_bytes());
    if text.is_empty() && sha256 != EMPTY_SHA256 {
        return Err(fault("catalog.evidence", "empty_hash"));
    }
    Ok(json!({"byte_len": text.len(), "sha256": sha256}))
}

fn numeric_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod - 4;
    let precision = (adjusted >> 16) & 0xFFFF;
    let scale = adjusted & 0xFFFF;
    Some((precision, scale))
}

fn type_reason(typtype: &str, type_name: &str, typmod: i32) -> &'static str {
    if typtype == "d" {
        return "domain_not_converted";
    }
    if typtype == "e" {
        return "enum_not_converted";
    }
    if type_name == "numeric" {
        match numeric_parts(typmod) {
            None => return "unbounded_or_wide_numeric",
            Some((precision, _)) if precision > 38 => return "unbounded_or_wide_numeric",
            Some(_) => {}
        }
    }
    if matches!(typtype, "b" | "p" | "r" | "m" | "c") {
        return "known_builtin_not_converted";
    }
    "unknown_type_oid"
}

fn captured_at() -> Result<String, PhaseFault> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| fault("catalog.clock", "unavailable"))?;
    Ok(utc_stamp(since.as_secs(), since.subsec_micros()))
}

/// The seal request hash includes `captured_at`. A later capture of the same
/// catalog body reuses the prior stamp so the validated draft stays a cache hit.
pub(crate) fn reuse_captured_at(prior_text: &str, fresh: &mut LocalCapture) -> bool {
    let Ok(prior) = serde_json::from_str::<Value>(prior_text) else {
        return false;
    };
    let Some(prior_at) = prior.get("captured_at").and_then(Value::as_str).map(str::to_owned) else {
        return false;
    };
    if !stable_stamp(&prior_at) || prior.get("schema_hash") != fresh.catalog.get("schema_hash") {
        return false;
    }
    let mut left = prior;
    let mut right = fresh.catalog.clone();
    let Some(left_map) = left.as_object_mut() else {
        return false;
    };
    let Some(right_map) = right.as_object_mut() else {
        return false;
    };
    left_map.insert("captured_at".to_owned(), Value::Null);
    right_map.insert("captured_at".to_owned(), Value::Null);
    if left != right {
        return false;
    }
    let Some(fresh_map) = fresh.catalog.as_object_mut() else {
        return false;
    };
    fresh_map.insert("captured_at".to_owned(), Value::String(prior_at));
    let Ok(text) = serde_json::to_string(&fresh.catalog) else {
        return false;
    };
    fresh.catalog_text = text;
    true
}

fn stable_stamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 27 && bytes[10] == b'T' && bytes[26] == b'Z' && value.is_ascii()
}

fn utc_stamp(secs: u64, micros: u32) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let tod = secs % 86_400;
    let (year, month, day) = ymd(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}Z",
        hour = tod / 3600,
        minute = (tod % 3600) / 60,
        second = tod % 60
    )
}

fn ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year as i32, month as u32, day)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn schema_preimage(tables: &[HashTable]) -> String {
    let mut ordered: Vec<&HashTable> = tables.iter().collect();
    ordered.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_table(&mut buf, table);
    }
    buf.push_str("]}");
    buf
}

fn write_table(buf: &mut String, table: &HashTable) {
    buf.push('{');
    write_str(buf, "schema", &table.schema);
    buf.push(',');
    write_str(buf, "name", &table.name);
    buf.push(',');
    write_str(buf, "relkind", &table.relkind);
    buf.push(',');
    write_str(buf, "replica_identity", &table.replica_identity);
    buf.push(',');
    write_bool(buf, "rls", table.rls);
    buf.push(',');
    write_bool(buf, "force_rls", table.force_rls);
    buf.push_str(",\"columns\":[");
    let mut columns = table.columns.clone();
    columns.sort_by_key(|column| column.attnum);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_column(buf, column);
    }
    buf.push_str("],\"primary_key\":[");
    write_key_columns(buf, &table.primary_key);
    buf.push_str("],\"primary_key_nulls_not_distinct\":");
    write_opt_bool(buf, table.primary_key_nulls_not_distinct);
    buf.push_str(",\"unique_keys\":[");
    let mut uniques = table.unique_keys.clone();
    uniques.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, key) in uniques.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(buf, "name", &key.name);
        buf.push_str(",\"columns\":[");
        write_key_columns(buf, &key.columns);
        buf.push_str("],");
        write_bool(buf, "nulls_not_distinct", key.nulls_not_distinct);
        buf.push('}');
    }
    buf.push_str("],\"foreign_keys\":[");
    let mut foreign = table.foreign_keys.clone();
    foreign.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, key) in foreign.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(buf, "name", &key.name);
        buf.push_str(",\"columns\":[");
        for (col_index, column) in key.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            write_i32(buf, "attnum", column.attnum);
            buf.push(',');
            write_str(buf, "name", &column.name);
            buf.push(',');
            write_str(buf, "ref_schema", &column.ref_schema);
            buf.push(',');
            write_str(buf, "ref_table", &column.ref_table);
            buf.push(',');
            write_i32(buf, "ref_attnum", column.ref_attnum);
            buf.push(',');
            write_str(buf, "ref_name", &column.ref_name);
            buf.push('}');
        }
        buf.push_str("]}");
    }
    buf.push_str("],\"policies\":[");
    let mut policies = table.policies.clone();
    policies.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, policy) in policies.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(buf, "name", &policy.name);
        buf.push(',');
        write_str(buf, "command", &policy.command);
        buf.push(',');
        write_bool(buf, "permissive", policy.permissive);
        buf.push(',');
        write_evidence(buf, "using_sha256", policy.using_sha256.as_deref());
        buf.push(',');
        write_evidence(buf, "check_sha256", policy.check_sha256.as_deref());
        buf.push('}');
    }
    buf.push_str("]}");
}

fn write_column(buf: &mut String, column: &HashColumn) {
    buf.push('{');
    write_i32(buf, "attnum", column.attnum);
    buf.push(',');
    write_str(buf, "name", &column.name);
    buf.push(',');
    write_i64(buf, "type_oid", column.type_oid);
    buf.push(',');
    write_i32(buf, "typmod", column.typmod);
    buf.push(',');
    write_bool(buf, "not_null", column.not_null);
    buf.push(',');
    write_i64(buf, "collation_oid", column.collation_oid);
    buf.push(',');
    write_str(buf, "typtype", &column.typtype);
    buf.push(',');
    write_opt_i64(buf, "base_type_oid", column.base_type_oid);
    buf.push(',');
    write_str(buf, "identity", &column.identity);
    buf.push(',');
    write_str(buf, "generated", &column.generated);
    buf.push(',');
    write_evidence(buf, "default_sha256", column.default_sha256.as_deref());
    buf.push_str(",\"enum_labels\":[");
    for (index, label) in column.enum_labels.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_json_string(buf, label);
    }
    buf.push_str("],\"domain_checks\":[");
    for (index, check) in column.domain_checks.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_json_string(buf, check);
    }
    buf.push_str("]}");
}

fn write_key_columns(buf: &mut String, columns: &[HashKey]) {
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_i32(buf, "attnum", column.attnum);
        buf.push(',');
        write_str(buf, "name", &column.name);
        buf.push('}');
    }
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_json_string(buf, name);
    buf.push(':');
    push_json_string(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_json_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_json_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_json_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_opt_i64(buf: &mut String, name: &str, value: Option<i64>) {
    push_json_string(buf, name);
    buf.push(':');
    match value {
        Some(inner) => buf.push_str(&inner.to_string()),
        None => buf.push_str("null"),
    }
}

fn write_opt_bool(buf: &mut String, value: Option<bool>) {
    match value {
        Some(true) => buf.push_str("true"),
        Some(false) => buf.push_str("false"),
        None => buf.push_str("null"),
    }
}

fn write_evidence(buf: &mut String, name: &str, value: Option<&str>) {
    push_json_string(buf, name);
    buf.push(':');
    match value {
        Some(inner) => push_json_string(buf, inner),
        None => buf.push_str("null"),
    }
}

fn evidence_sha(value: &Value) -> Option<String> {
    value.get("sha256").and_then(Value::as_str).map(str::to_owned)
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::{schema_preimage, sha256_hex, utc_stamp, HashColumn, HashTable, EMPTY_SHA256};

    fn customer() -> HashTable {
        HashTable {
            schema: "public".to_owned(),
            name: "customer".to_owned(),
            relkind: "r".to_owned(),
            replica_identity: "d".to_owned(),
            rls: false,
            force_rls: false,
            columns: vec![HashColumn {
                attnum: 1,
                name: "id".to_owned(),
                type_oid: 23,
                typmod: -1,
                not_null: true,
                collation_oid: 0,
                typtype: "b".to_owned(),
                base_type_oid: None,
                identity: String::new(),
                generated: String::new(),
                default_sha256: None,
                enum_labels: Vec::new(),
                domain_checks: Vec::new(),
            }],
            primary_key: vec![super::HashKey { attnum: 1, name: "id".to_owned() }],
            primary_key_nulls_not_distinct: Some(false),
            unique_keys: Vec::new(),
            foreign_keys: Vec::new(),
            policies: Vec::new(),
        }
    }

    #[test]
    fn schema_preimage_matches_the_compiler_vector() {
        let expected = r#"{"tables":[{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{"attnum":1,"name":"id","type_oid":23,"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}],"primary_key":[{"attnum":1,"name":"id"}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}]}"#;
        assert_eq!(schema_preimage(&[customer()]), expected);
        assert_eq!(sha256_hex(schema_preimage(&[customer()]).as_bytes()), "60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499");
        assert_eq!(sha256_hex(schema_preimage(&[]).as_bytes()), "cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc");
        assert_eq!(sha256_hex(b""), EMPTY_SHA256);
    }

    #[test]
    fn utc_stamp_formats_the_epoch() {
        assert_eq!(utc_stamp(0, 0), "1970-01-01T00:00:00.000000Z");
    }

    fn sample_capture(stamp: &str, table: &str) -> super::LocalCapture {
        let catalog = serde_json::json!({
            "protocol": "oo.source.pgsql/v1",
            "captured_at": stamp,
            "schema_hash": "ab".repeat(32),
            "tables": [{"name": table}]
        });
        super::LocalCapture {
            catalog_text: serde_json::to_string(&catalog).expect("catalog"),
            catalog,
            profile: serde_json::Value::Null,
            comments: serde_json::Value::Null,
            provenance: serde_json::Value::Null,
            schema_hash: "ab".repeat(32),
        }
    }

    #[test]
    fn same_catalog_reuses_the_prior_capture_stamp() {
        let prior = sample_capture("2026-10-08T23:11:06.057053Z", "hubs");
        let mut fresh = sample_capture("2026-10-08T23:20:00.000000Z", "hubs");
        assert!(super::reuse_captured_at(&prior.catalog_text, &mut fresh));
        assert_eq!(fresh.catalog.get("captured_at").and_then(|value| value.as_str()), Some("2026-10-08T23:11:06.057053Z"));
        assert!(fresh.catalog_text.contains("2026-10-08T23:11:06.057053Z"));
    }

    #[test]
    fn changed_catalog_keeps_the_fresh_capture_stamp() {
        let prior = sample_capture("2026-10-08T23:11:06.057053Z", "hubs");
        let mut fresh = sample_capture("2026-10-08T23:20:00.000000Z", "shipments");
        assert!(!super::reuse_captured_at(&prior.catalog_text, &mut fresh));
        assert_eq!(fresh.catalog.get("captured_at").and_then(|value| value.as_str()), Some("2026-10-08T23:20:00.000000Z"));
        assert!(!super::reuse_captured_at("not-json", &mut fresh));
        assert!(!super::reuse_captured_at("{\"captured_at\":\"now\",\"schema_hash\":\"ab\"}", &mut fresh));
    }
}


