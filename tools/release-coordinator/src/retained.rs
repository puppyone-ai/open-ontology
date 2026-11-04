//! D15: publish an owner-reviewed registry for one oo-load retained generation.
//!
//! The legacy publisher proves imports/ACKs and stays unchanged. This path
//! accepts only a sealed `_oo_load.gateway_publications` cut, binds the
//! owner self-review (D6, `mode=self`) to the cut's exact relations and column
//! types, opens their fences and calls `_oo.native_library_mark_ready` in one
//! target transaction. It never writes import/ACK rows and never skips a fence.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use tokio_postgres::Transaction;

use crate::error::{StoreError, query_error};
use crate::store::{Receipt, begin_for, open, refused, require_tls, rollback, stamped};
use crate::target::TargetConn;

pub const PROTOCOL: &str = "oo.release-coordinator/retained-publication/v1";
pub const RECEIPT_PROTOCOL: &str = "oo.load.per-table-receipt/v1";
const REVIEWED: &str = "oo.owner-reviewed-registry/v1";
const SHARD_KEYS: [&str; 13] = [
    "applied_pos",
    "consistency",
    "consistency_grade",
    "cross_table_consistent",
    "position_codec",
    "position_lineage",
    "protocol",
    "run_id",
    "run_ids",
    "source_generation",
    "source_id",
    "tables",
    "target_generation",
];
const TABLE_KEYS: [&str; 14] = [
    "accepted_rows",
    "applied_pos",
    "capture_method",
    "consistency_grade",
    "delete_detection",
    "physical_name",
    "recipe_sha256",
    "run_id",
    "schema",
    "source_generation",
    "storage_schema",
    "storage_table",
    "table",
    "target_generation",
];

macro_rules! p {
    ($($value:expr),* $(,)?) => {
        &[$(&$value as &(dyn postgres_types::ToSql + Sync)),*]
    };
}

/// One request: the reviewed registry and the generation it is published for.
pub struct RetainedRequest<'a> {
    pub library_id: &'a str,
    /// The oo-load run that sealed the candidate. Not required to be a UUID.
    pub generation: &'a str,
    /// Gateway data login named in the generated reviewed document.
    pub login: &'a str,
    /// `oo.owner-reviewed-registry/v1`, exactly as `oo review` wrote it.
    pub reviewed: &'a Value,
}

/// oo-load run and library identifiers: `[A-Za-z0-9._-]{1,128}`.
pub fn run_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn closed(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|v| v.len() == keys.len() && keys.iter().all(|k| v.contains_key(*k)))
}

fn hex64(value: &Value) -> bool {
    value.as_str().is_some_and(crate::seal::hex64)
}

fn ident(value: &str) -> Result<String, StoreError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 63
        || !(bytes[0] == b'_' || bytes[0].is_ascii_lowercase())
        || !bytes[1..]
            .iter()
            .all(|b| *b == b'_' || b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err(StoreError::Input);
    }
    Ok(format!("\"{value}\""))
}

fn physical(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 63
            && s.starts_with("pt_")
            && s[3..]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

/// Gateway's document hash: serde_json bytes of the value, as `library_policy::sha`.
fn gateway_sha(value: &Value) -> String {
    crate::sha256::sha256_hex(&serde_json::to_vec(value).unwrap_or_default())
}

/// Closed D15 cut contract: one oo-load shard, snapshot-zero, grade B, per-table.
/// Generation identifiers are oo-load run tokens, not UUIDs.
pub fn validate_cut(cut: &Value, generation: &str) -> Result<(), StoreError> {
    let library = &cut["library"];
    let manifest = &library["manifest"];
    if !run_token(generation)
        || !closed(cut, &["library", "shards"])
        || !closed(
            library,
            &[
                "protocol",
                "library_id",
                "manifest",
                "manifest_sha256",
                "expected_tables",
                "shards",
            ],
        )
        || library["protocol"] != crate::native_library::PROTOCOL
        || !library["library_id"].as_str().is_some_and(run_token)
        || !closed(manifest, &["protocol", "library_id", "sources"])
        || manifest["protocol"] != "oo.library.membership/v1"
        || manifest["library_id"] != library["library_id"]
        || library["manifest_sha256"].as_str()
            != Some(crate::canon::canonical_sha(manifest).as_str())
    {
        return Err(StoreError::Input);
    }
    let shards = cut["shards"].as_array().ok_or(StoreError::Input)?;
    let members = manifest["sources"].as_array().ok_or(StoreError::Input)?;
    if shards.len() != 1
        || members.len() != 1
        || library["shards"] != json!([shards[0]["source_id"]])
    {
        return Err(StoreError::Input);
    }
    let shard = &shards[0];
    let receipt = &shard["receipt"];
    let source = shard["source_id"].as_str().ok_or(StoreError::Input)?;
    let tables = receipt["tables"]
        .as_array()
        .filter(|t| !t.is_empty())
        .ok_or(StoreError::Input)?;
    let generation_number = receipt["source_generation"]
        .as_i64()
        .filter(|n| *n >= 1)
        .ok_or(StoreError::Input)?;
    if !closed(
        shard,
        &["source_id", "anchor_run", "receipt", "receipt_sha256"],
    ) || shard["anchor_run"] != generation
        || shard["receipt_sha256"].as_str() != Some(crate::canon::canonical_sha(receipt).as_str())
        || !closed(receipt, &SHARD_KEYS)
        || receipt["protocol"] != RECEIPT_PROTOCOL
        || receipt["source_id"] != source
        || receipt["run_id"] != generation
        || receipt["run_ids"] != json!([generation])
        || receipt["target_generation"].as_i64() != Some(generation_number)
        || receipt["position_codec"] != "snapshot-zero/v1"
        || receipt["position_lineage"] != "oo-load.retained"
        || receipt["applied_pos"] != "0"
        || receipt["consistency"] != "retained_generation"
        || receipt["consistency_grade"] != "B"
        || receipt["cross_table_consistent"] != false
        || !closed(&members[0], &["source_id", "schema", "tables"])
        || members[0]["source_id"] != source
        || library["expected_tables"].as_u64() != Some(tables.len() as u64)
    {
        return Err(StoreError::Input);
    }
    let schema = members[0]["schema"].as_str().ok_or(StoreError::Input)?;
    let names = members[0]["tables"].as_array().ok_or(StoreError::Input)?;
    let (mut seen, mut physical_names) = (BTreeSet::new(), BTreeSet::new());
    if names.len() != tables.len() {
        return Err(StoreError::Input);
    }
    for table in tables {
        let (storage_schema, name) = crate::native_publication::storage_names(table)?;
        if !closed(table, &TABLE_KEYS)
            || storage_schema != schema
            || names.iter().filter(|n| n.as_str() == Some(name)).count() != 1
            || !seen.insert(name)
            || !physical(&table["physical_name"])
            || !physical_names.insert(table["physical_name"].as_str().unwrap_or(""))
            || table["run_id"] != generation
            || table["accepted_rows"].as_i64().is_none_or(|n| n < 0)
            || !hex64(&table["recipe_sha256"])
            || table["source_generation"].as_i64() != Some(generation_number)
            || table["target_generation"].as_i64() != Some(generation_number)
            || table["applied_pos"] != "0"
            || table["capture_method"] != "c5_diff"
            || table["delete_detection"] != "reconcile"
            || table["consistency_grade"] != "B"
        {
            return Err(StoreError::Input);
        }
    }
    Ok(())
}

/// True only for the D15 shard shape, so callers can route it away from legacy ACK proofs.
pub fn is_retained(cut: &Value) -> bool {
    cut["shards"].as_array().is_some_and(|s| {
        s.iter()
            .any(|s| s["receipt"]["consistency"] == "retained_generation")
    })
}

/// Owner self-review contract (D6). The compiled Pack is recomputed, never trusted.
pub struct Reviewed {
    pub effective: Value,
    pub visibility: Value,
    pub ceilings: Value,
    pub producer: Value,
    pub pack_digest: String,
    /// `dataset.column` -> explicit owner classification decision present.
    pub decided: BTreeSet<String>,
    pub registry_sha256: String,
}

pub fn reviewed(value: &Value) -> Result<Reviewed, &'static str> {
    let decision = &value["review_decision"];
    let session = &decision["decisions"];
    let command = &value["command"];
    if value["protocol"] != REVIEWED
        || value["status"] != "reviewed"
        || value["published"] != false
        || value["authentication"] != "not_claimed"
        || decision["reviewer"] != "owner"
        || decision["mode"] != "self"
        || session["protocol"] != "oo.owner-review/v1"
        || session["g14"]["action"] != "accept"
        || command["review"]["mode"] != "self"
        || command["review"]["reviewer"] != "owner"
        || command["review"]["review_origin"] != "operator_pinned_file"
    {
        return Err("review_mode_self_required");
    }
    let items = session["items"]
        .as_object()
        .ok_or("review_mode_self_required")?;
    if items
        .values()
        .any(|d| !matches!(d["action"].as_str(), Some("accept" | "edit")))
    {
        return Err("review_rejected_item");
    }
    let compiled =
        oo_builder::compile_reviewed_community(command).map_err(|_| "review_compile_refused")?;
    if compiled != value["compiled"]
        || compiled["protocol"] != "oo.builder.reviewed-community-result/v1"
        || compiled["published"] != false
        || compiled["producer"]["published"] != false
        || compiled["producer"]["business_owner_signed"] != false
    {
        return Err("review_compiled_mismatch");
    }
    let mut decided = BTreeSet::new();
    for id in items.keys() {
        if let Some(rest) = id.strip_prefix("classification/") {
            let (dataset, column) = rest.rsplit_once('/').ok_or("review_mode_self_required")?;
            decided.insert(format!("{dataset}.{column}"));
        }
    }
    Ok(Reviewed {
        effective: compiled["effective"].clone(),
        visibility: command["visibility"].clone(),
        ceilings: command["ceilings"].clone(),
        pack_digest: compiled["producer"]["pack_digest"]
            .as_str()
            .ok_or("review_compiled_mismatch")?
            .to_owned(),
        producer: compiled["producer"].clone(),
        decided,
        registry_sha256: gateway_sha(value),
    })
}

/// A table of the cut and the reviewed columns that its fence opens.
#[derive(Debug, PartialEq)]
pub struct Binding {
    pub dataset: String,
    pub physical: String,
    pub logical_columns: Vec<String>,
    pub expected: Vec<Value>,
}

/// Visibility must cover exactly the cut, with the cut's exact column types.
pub fn bind(review: &Reviewed, vector: &Value) -> Result<Vec<Binding>, &'static str> {
    let entries = vector.as_array().ok_or("retained_candidate_shape")?;
    let visible = review.visibility["datasets"]
        .as_array()
        .ok_or("retained_visibility_coverage")?;
    let datasets: BTreeSet<_> = visible
        .iter()
        .filter_map(|d| d["dataset"].as_str())
        .collect();
    let cut: BTreeSet<_> = entries
        .iter()
        .filter_map(|d| d["dataset"].as_str())
        .collect();
    if datasets != cut || datasets.len() != visible.len() || cut.len() != entries.len() {
        return Err("retained_visibility_coverage");
    }
    let mut out = Vec::new();
    for entry in entries {
        let dataset = entry["dataset"]
            .as_str()
            .ok_or("retained_candidate_shape")?;
        let shown = visible
            .iter()
            .find(|d| d["dataset"] == dataset)
            .ok_or("retained_visibility_coverage")?;
        let definition: Vec<_> = review.effective["datasets"]
            .as_array()
            .ok_or("retained_column_binding")?
            .iter()
            .filter(|d| d["dataset"] == dataset)
            .collect();
        let expected = entry["expected_columns"]
            .as_array()
            .ok_or("retained_candidate_shape")?;
        if definition.len() != 1 || !physical(&entry["physical_name"]) {
            return Err("retained_column_binding");
        }
        let mut columns = Vec::new();
        let mut typed = Vec::new();
        for column in shown["columns"]
            .as_array()
            .filter(|c| !c.is_empty())
            .ok_or("retained_column_binding")?
        {
            let name = column.as_str().ok_or("retained_column_binding")?;
            let defined: Vec<_> = definition[0]["columns"]
                .as_array()
                .ok_or("retained_column_binding")?
                .iter()
                .filter(|c| c["name"] == name)
                .collect();
            let stored: Vec<_> = expected.iter().filter(|c| c["name"] == name).collect();
            if defined.len() != 1
                || stored.len() != 1
                || columns.iter().any(|c| c == name)
                || defined[0]["data_type"] != stored[0]["data_type"]
                || !stored[0]["data_type"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            {
                return Err("retained_column_binding");
            }
            if defined[0]["pii"] != "none" && !review.decided.contains(&format!("{dataset}.{name}"))
            {
                return Err("retained_classification_decision");
            }
            columns.push(name.to_owned());
            typed.push(json!({"name":name,"data_type":stored[0]["data_type"]}));
        }
        // Every identity column (scalar or ordered composite) must be opened by this fence.
        for entity in review.effective["entities"]
            .as_array()
            .ok_or("retained_entity_key_binding")?
            .iter()
            .filter(|e| e["primary_dataset"] == dataset)
        {
            let key: oo_core::oos::EntityKey = serde_json::from_value(entity["key"].clone())
                .map_err(|_| "retained_entity_key_binding")?;
            if !key
                .columns()
                .iter()
                .all(|c| columns.iter().any(|o| o == c.as_str()))
            {
                return Err("retained_entity_key_binding");
            }
        }
        out.push(Binding {
            dataset: dataset.to_owned(),
            physical: entry["physical_name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            logical_columns: columns,
            expected: typed,
        });
    }
    Ok(out)
}

/// The Gateway's operator-owned reviewed document for this cut (local operator review).
pub fn gateway_document(
    review: &Reviewed,
    publication: &Value,
    vector: &Value,
    config_digest: &str,
    source: &str,
    login: &str,
) -> Result<Value, &'static str> {
    let mut columns = serde_json::Map::new();
    for dataset in review.visibility["datasets"]
        .as_array()
        .ok_or("retained_visibility_coverage")?
    {
        let name = dataset["dataset"]
            .as_str()
            .ok_or("retained_visibility_coverage")?;
        let definition = review.effective["datasets"]
            .as_array()
            .ok_or("retained_column_binding")?
            .iter()
            .find(|d| d["dataset"] == name)
            .ok_or("retained_column_binding")?;
        for column in dataset["columns"]
            .as_array()
            .ok_or("retained_column_binding")?
        {
            let column_name = column.as_str().ok_or("retained_column_binding")?;
            let c = definition["columns"]
                .as_array()
                .ok_or("retained_column_binding")?
                .iter()
                .find(|c| c["name"] == column_name)
                .ok_or("retained_column_binding")?;
            let mut decision = json!({"pii":c["pii"],"semantic_type":c["semantic_type"],
                "reviewer":c["classification"]["by"]});
            // The explicit owner classification decision on a visible column is the allow.
            if c["pii"] != "none" {
                decision["explicit_allow"] = json!(true);
            }
            columns.insert(format!("{name}.{column_name}"), decision);
        }
    }
    let local = json!({"review_origin":"operator_pinned_file","project":review.effective["project"],
        "role":review.visibility["role"],"principal":review.visibility["principal"],
        "columns":columns,"hidden_columns":[]});
    let producer = json!({"protocol":"oo.selfbuild.library-pack/v1",
        "artifact":review.producer["artifact"],"pack_digest":review.pack_digest,
        "effective_sha256":gateway_sha(&review.effective),"visibility_sha256":gateway_sha(&review.visibility),
        "review_sha256":gateway_sha(&local),"reviewed_registry_sha256":review.registry_sha256,
        "published":false,"business_owner_signed":false});
    let bindings: Vec<Value> = vector.as_array().ok_or("retained_candidate_shape")?.iter().map(|e| json!({
        "dataset":e["dataset"],"source":source,"config_digest":config_digest,"generation":e["generation"],
        "type_digest":e["recipe_sha256"],"input_digest":e["recipe_sha256"]})).collect();
    Ok(json!({"protocol":"oo.gateway.library-reviewed/v1",
        "envelope":{"contract":"oo.operator.library-pack/v1","operation":"prepareLocalBuildPack",
            "management_identity":"operator_local","producer_authenticated":false,"published":false,
            "active":false,"compiled_pack":"not_published","data_grants":"none",
            "producer":producer,"output_sha256":gateway_sha(&producer),"review":local},
        "effective":review.effective,"visibility":review.visibility,"publication":publication,
        "bindings":bindings,"login":login,"ceilings":review.ceilings}))
}

struct Candidate {
    generation: i64,
    receipt_sha256: String,
    cut: Value,
    vector: Value,
    config_digest: String,
    source: String,
}

async fn json_text(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn postgres_types::ToSql + Sync)],
) -> Result<Option<Vec<String>>, StoreError> {
    let Some(row) = tx.query_opt(sql, params).await.map_err(query_error)? else {
        return Ok(None);
    };
    (0..row.len())
        .map(|i| row.try_get::<_, String>(i).map_err(|_| StoreError::Input))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn parse(text: &str) -> Result<Value, StoreError> {
    serde_json::from_str(text).map_err(|_| StoreError::Input)
}

async fn candidate(
    tx: &Transaction<'_>,
    library: &str,
    generation: &str,
) -> Result<Option<Candidate>, StoreError> {
    let present: bool = tx
        .query_one(
            "SELECT to_regclass('_oo_load.gateway_publications') IS NOT NULL \
         AND to_regprocedure('_oo.native_library_check_retained(jsonb)') IS NOT NULL \
         AND to_regprocedure('_oo.native_library_mark_ready(text,text)') IS NOT NULL",
            &[],
        )
        .await
        .map_err(query_error)?
        .get(0);
    if !present {
        return Ok(None);
    }
    let Some(row) = json_text(tx,
        "SELECT g.generation::text,g.receipt_sha256,p.library::text,p.shards::text,g.shard::text,\
         g.data_vector::text,g.data_vector_sha256,g.request::text,g.source \
         FROM _oo_load.gateway_publications g JOIN _oo.native_library_publication p USING(receipt_sha256) \
         WHERE g.library_id=$1 AND g.run=$2 AND g.sealed_at IS NOT NULL AND p.library_id=$1 FOR SHARE OF g",
        p!(library, generation)).await? else { return Ok(None) };
    let cut = json!({"library":parse(&row[2])?,"shards":parse(&row[3])?});
    let vector = parse(&row[5])?;
    let request = parse(&row[7])?;
    if crate::canon::canonical_sha(&cut) != row[1]
        || cut["shards"][0] != parse(&row[4])?
        || crate::canon::canonical_sha(&vector) != row[6]
        || request["source_id"] != row[8]
        || !hex64(&request["binding_sha256"])
    {
        return Err(StoreError::Input);
    }
    validate_cut(&cut, generation)?;
    Ok(Some(Candidate {
        generation: row[0].parse().map_err(|_| StoreError::Input)?,
        receipt_sha256: row[1].clone(),
        config_digest: request["binding_sha256"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        source: row[8].clone(),
        cut,
        vector,
    }))
}

/// Physical column names of the retained view, through the registered mappings.
async fn physical_columns(
    tx: &Transaction<'_>,
    c: &Candidate,
    b: &Binding,
) -> Result<Option<Vec<String>>, StoreError> {
    let (schema, table) = b.dataset.split_once('.').ok_or(StoreError::Input)?;
    let mut out = Vec::new();
    for (column, typed) in b.logical_columns.iter().zip(&b.expected) {
        let row = tx.query_opt(
            "SELECT m.physical_name,format_type(a.atttypid,a.atttypmod) FROM _oo.physical_mappings m \
             JOIN pg_catalog.pg_class r ON r.relname=m.scope_table_physical \
             JOIN pg_catalog.pg_namespace n ON n.oid=r.relnamespace AND n.nspname='_oo' \
             JOIN pg_catalog.pg_attribute a ON a.attrelid=r.oid AND a.attname=m.physical_name AND a.attnum>0 AND NOT a.attisdropped \
             WHERE m.source=$1 AND m.config_digest=$2 AND m.run_generation=$3 AND m.level='c' \
             AND m.logical_namespace=$4 AND m.logical_table=$5 AND m.logical_column=$6 AND m.scope_table_physical=$7",
            p!(c.source, c.config_digest, c.generation, schema, table, column, b.physical),
        ).await.map_err(query_error)?;
        let Some(row) = row else { return Ok(None) };
        let (name, data_type): (String, String) = (row.get(0), row.get(1));
        let normalized = match data_type.as_str() {
            "timestamp without time zone" => "timestamp",
            "timestamp with time zone" => "timestamptz",
            other => other,
        };
        if Some(normalized) != typed["data_type"].as_str()
            && Some(data_type.as_str()) != typed["data_type"].as_str()
        {
            return Ok(None);
        }
        ident(&name)?;
        out.push(name);
    }
    Ok(Some(out))
}

async fn role_guard(
    tx: &Transaction<'_>,
    role: &str,
    login: &str,
) -> Result<Option<&'static str>, StoreError> {
    let Some(row) = tx.query_opt(
        "SELECT rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolinherit OR rolbypassrls OR rolreplication \
         FROM pg_catalog.pg_roles WHERE rolname=$1", p!(role),
    ).await.map_err(query_error)? else { return Ok(Some("retained_role_missing")) };
    if row.get::<_, bool>(0) {
        return Ok(Some("retained_role_unsafe"));
    }
    // The Gateway data login switches to the reviewed role; it is never the role itself.
    let Some(row) = tx.query_opt(
        "SELECT l.rolcanlogin AND NOT l.rolsuper AND NOT l.rolbypassrls AND pg_catalog.pg_has_role(l.oid,r.oid,'SET') \
         FROM pg_catalog.pg_roles l, pg_catalog.pg_roles r WHERE l.rolname=$1 AND r.rolname=$2", p!(login, role),
    ).await.map_err(query_error)? else { return Ok(Some("retained_login_binding")) };
    if !row.get::<_, bool>(0) {
        return Ok(Some("retained_login_binding"));
    }
    Ok(None)
}

/// Open each reviewed fence closed -> granted -> open, then activate the head.
/// One transaction: a failed semantic admission leaves every fence and the head unchanged.
pub async fn publish_reviewed_registry(
    conn: &TargetConn<'_>,
    request: &RetainedRequest<'_>,
) -> Result<Receipt, StoreError> {
    if !run_token(request.library_id) || !run_token(request.generation) {
        return Err(StoreError::Input);
    }
    ident(request.login)?;
    let review = match reviewed(request.reviewed) {
        Ok(review) => review,
        Err(reason) => {
            return Ok(refused(
                reason,
                json!({"fence_touched":false,"database_used":false}),
            ));
        }
    };
    let role = review.visibility["role"]
        .as_str()
        .ok_or(StoreError::Input)?
        .to_owned();
    if !role.starts_with("oo_r_") || role == request.login {
        return Ok(refused(
            "retained_role_unsafe",
            json!({"fence_touched":false,"database_used":false}),
        ));
    }
    ident(&role)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    match body(&tx, request, &review, &role).await {
        Ok(receipt) if receipt.code == 0 => {
            tx.commit().await.map_err(|e| match query_error(e) {
                StoreError::Query { sqlstate: Some(s) } => StoreError::Query { sqlstate: Some(s) },
                _ => StoreError::CommitUnknown,
            })?;
            Ok(receipt)
        }
        Ok(receipt) => {
            let _ = tx.rollback().await;
            Ok(receipt)
        }
        Err(error) => Err(rollback(tx, error).await),
    }
}

fn refusal(reason: &'static str) -> Receipt {
    refused(
        reason,
        json!({"fence_touched":false,"database_used":true,"head_changed":false}),
    )
}

async fn body(
    tx: &Transaction<'_>,
    request: &RetainedRequest<'_>,
    review: &Reviewed,
    role: &str,
) -> Result<Receipt, StoreError> {
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        p!(format!("oo-load-library:{}", request.library_id)),
    )
    .await
    .map_err(query_error)?;
    let Some(c) = candidate(tx, request.library_id, request.generation).await? else {
        return Ok(refusal("retained_candidate_missing"));
    };
    let bindings = match bind(review, &c.vector) {
        Ok(bindings) => bindings,
        Err(reason) => return Ok(refusal(reason)),
    };
    let fenced: bool = tx
        .query_one(
            "SELECT to_regclass('oo_publish_target.fence') IS NOT NULL",
            &[],
        )
        .await
        .map_err(query_error)?
        .get(0);
    if !fenced {
        return Ok(refusal("retained_fence_schema_missing"));
    }
    if let Some(reason) = role_guard(tx, role, request.login).await? {
        return Ok(refusal(reason));
    }
    let newest: i64 = tx
        .query_one(
            "SELECT coalesce(max(g.generation),0) FROM _oo_load.gateway_publications g \
         JOIN _oo.native_library_head h ON h.receipt_sha256=g.receipt_sha256 \
         WHERE h.library_id=$1 AND h.state='ready'",
            p!(request.library_id),
        )
        .await
        .map_err(query_error)?
        .get(0);
    if c.generation < newest {
        return Ok(refusal("publication_superseded"));
    }
    let mut physical = BTreeMap::new();
    for b in &bindings {
        let owned: Option<bool> = tx.query_opt(
            "SELECT pg_catalog.pg_has_role(current_user,c.relowner,'USAGE') FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname=$1 AND c.relkind='v'",
            p!(b.physical)).await.map_err(query_error)?.map(|r| r.get(0));
        match owned {
            None => return Ok(refusal("retained_relation_missing")),
            Some(false) => return Ok(refusal("retained_owner_required")),
            Some(true) => {}
        }
        let Some(columns) = physical_columns(tx, &c, b).await? else {
            return Ok(refusal("retained_column_binding"));
        };
        physical.insert(b.physical.clone(), columns);
    }
    let version = format!("{}.{}", request.generation, &review.pack_digest[..16]);
    let candidate_id = format!("retained-{}", &c.receipt_sha256[..32]);
    let usage: bool = tx
        .query_one(
            "SELECT pg_catalog.has_schema_privilege($1::text::name,'_oo','USAGE')",
            p!(role),
        )
        .await
        .map_err(query_error)?
        .get(0);
    if !usage {
        tx.batch_execute(&format!("GRANT USAGE ON SCHEMA _oo TO {}", ident(role)?))
            .await
            .map_err(query_error)?;
    }
    let mut fences = Vec::new();
    let mut replay = true;
    for b in &bindings {
        let columns = &physical[&b.physical];
        let relation = format!("_oo.{}", ident(&b.physical)?);
        tx.execute(
            "INSERT INTO oo_publish_target.fence(singleton,state,role_name,schema_name,relation_name,opened_columns,streams_stopped) \
             VALUES(true,'closed',$1,'_oo',$2,'{}',0) ON CONFLICT DO NOTHING", p!(role, b.physical),
        ).await.map_err(query_error)?;
        let row = tx
            .query_one(
                "SELECT state,opened_columns,opened_pack_digest FROM oo_publish_target.fence \
             WHERE role_name=$1 AND schema_name='_oo' AND relation_name=$2 FOR UPDATE",
                p!(role, b.physical),
            )
            .await
            .map_err(query_error)?;
        let (state, opened, pack): (String, Vec<String>, Option<String>) =
            (row.get(0), row.get(1), row.get(2));
        let mut sorted = columns.clone();
        sorted.sort();
        if state == "open"
            && pack.as_deref() == Some(review.pack_digest.as_str())
            && opened == sorted
        {
            fences.push(json!({"dataset":b.dataset,"relation":b.physical,"fence_state":"open","outcome":"replay"}));
            continue;
        }
        replay = false;
        // Views take the same exclusion as the legacy overlay fence.
        tx.batch_execute(&format!("LOCK TABLE {relation} IN ACCESS EXCLUSIVE MODE"))
            .await
            .map_err(query_error)?;
        let quoted = sorted
            .iter()
            .map(|c| ident(c))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        let all = tx.query(
            "SELECT attname FROM pg_catalog.pg_attribute WHERE attrelid=$1::text::regclass AND attnum>0 AND NOT attisdropped",
            p!(relation)).await.map_err(query_error)?.iter().map(|r| ident(&r.get::<_, String>(0)))
            .collect::<Result<Vec<_>, _>>()?.join(",");
        for grantee in [ident(role)?, "PUBLIC".to_owned()] {
            tx.batch_execute(&format!("REVOKE ALL ON TABLE {relation} FROM {grantee}; REVOKE ALL ({all}) ON TABLE {relation} FROM {grantee}"))
                .await.map_err(query_error)?;
        }
        tx.execute(
            "UPDATE oo_publish_target.fence SET state='closed',opened_columns='{}',opened_pack_digest=NULL,opened_version=NULL,candidate_id=NULL \
             WHERE role_name=$1 AND schema_name='_oo' AND relation_name=$2", p!(role, b.physical),
        ).await.map_err(query_error)?;
        tx.batch_execute(&format!(
            "GRANT SELECT ({quoted}) ON TABLE {relation} TO {}",
            ident(role)?
        ))
        .await
        .map_err(query_error)?;
        tx.execute("UPDATE oo_publish_target.fence SET state='granted' WHERE role_name=$1 AND schema_name='_oo' AND relation_name=$2",
            p!(role, b.physical)).await.map_err(query_error)?;
        tx.execute(
            "UPDATE oo_publish_target.fence SET state='open',opened_columns=$3,opened_pack_digest=$4,opened_version=$5,candidate_id=$6 \
             WHERE role_name=$1 AND schema_name='_oo' AND relation_name=$2",
            p!(role, b.physical, sorted, review.pack_digest, version, candidate_id),
        ).await.map_err(query_error)?;
        fences.push(json!({"dataset":b.dataset,"relation":b.physical,"fence_state":"open","outcome":"opened",
            "opened_columns":b.logical_columns}));
    }
    let head: Option<(String, String)> = tx.query_opt(
        "SELECT receipt_sha256,state FROM _oo.native_library_head WHERE library_id=$1 FOR UPDATE", p!(request.library_id),
    ).await.map_err(query_error)?.map(|r| (r.get(0), r.get(1)));
    let Some(head) = head else {
        return Ok(refusal("retained_candidate_missing"));
    };
    let ready_before = head == (c.receipt_sha256.clone(), "ready".to_owned());
    tx.execute(
        "UPDATE _oo.native_library_head SET receipt_sha256=$2,state='pending' WHERE library_id=$1",
        p!(request.library_id, c.receipt_sha256),
    )
    .await
    .map_err(query_error)?;
    if let Err(error) = tx
        .execute(
            "SELECT _oo.native_library_mark_ready($1,$2)",
            p!(request.library_id, c.receipt_sha256),
        )
        .await
    {
        let sqlstate = error.code().map(|c| c.code().to_owned());
        return Ok(refused(
            "semantic_admission_refused",
            json!({"sqlstate":sqlstate,"fence_touched":false,"head_changed":false,"database_used":true}),
        ));
    }
    tx.execute("UPDATE _oo_load.gateway_publications SET activated_at=coalesce(activated_at,clock_timestamp()) WHERE receipt_sha256=$1",
        p!(c.receipt_sha256)).await.map_err(query_error)?;
    let publication = json!({"protocol":crate::native_library::PROTOCOL,"run_id":request.generation,
        "receipt_sha256":c.receipt_sha256,"receipt":c.cut});
    let document = gateway_document(
        review,
        &publication,
        &c.vector,
        &c.config_digest,
        &c.source,
        request.login,
    )
    .map_err(|_| StoreError::Input)?;
    Ok(stamped(
        if replay && ready_before {
            "replay"
        } else {
            "activated"
        },
        None,
        json!({
            "protocol":PROTOCOL,"library_id":request.library_id,"generation":request.generation,
            "source_id":c.source,"source_generation":c.generation,"config_digest":c.config_digest,
            "receipt_sha256":c.receipt_sha256,"data_vector":c.vector,
            "data_vector_sha256":crate::canon::canonical_sha(&c.vector),
            "pack_digest":review.pack_digest,"reviewed_registry_sha256":review.registry_sha256,
            "review_mode":"self","reviewer":"owner","role":role,"login":request.login,
            "fences":fences,"state":"ready","gateway_published":true,
            "legacy_import_written":false,"legacy_ack_written":false,"database_used":true,
            "gateway_document":document,"gateway_document_sha256":gateway_sha(&document),
        }),
        "not_issued",
    ))
}

#[cfg(test)]
#[path = "../tests/observations/retained.rs"]
mod tests;
