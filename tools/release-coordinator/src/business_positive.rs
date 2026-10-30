//! Bounded same-database test authority, using real TLS logins and immutable rows.
//! SQL is generated from pinned COUNT/SUM plans, never submitted receipt claims.
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::{Value, json};
use tokio_postgres::Transaction;

use crate::canon::{canonical, canonical_sha};
use crate::error::{StoreError, query_error};
use crate::maintain::ControlConn;
use crate::store::{self, Receipt};

const MIGRATION: &str = include_str!("../migrations/oo_release_business_v1.sql");
const KINDS: &[&str] = &["executor", "oracle", "evaluator", "reviewer"];
const GATES: &[&str] = &["G5a", "G5b", "G6", "G10", "G11"];
const PRESENT: &str = "SELECT to_regclass('oo_release_control.business_policy')::text";

pub fn business_execution_capability() -> String {
    format!(
        "count-sum-test/v1:{}",
        canonical_sha(&json!([
            crate::sha256_hex(include_bytes!("business_positive.rs")),
            crate::sha256_hex(include_bytes!("business_native.rs"))
        ]))
    )
}
pub(crate) fn require(ok: bool) -> Result<(), StoreError> {
    if ok { Ok(()) } else { Err(StoreError::Input) }
}
pub(crate) fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str, StoreError> {
    v[k].as_str()
        .filter(|v| !v.is_empty())
        .ok_or(StoreError::Input)
}
fn fields(v: &Value, names: &[&str]) -> Result<(), StoreError> {
    require(
        v.as_object()
            .is_some_and(|o| o.len() == names.len() && names.iter().all(|n| o.contains_key(*n))),
    )
}
fn ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .enumerate()
            .all(|(i, c)| c.is_ascii_lowercase() || c == b'_' || (i > 0 && c.is_ascii_digit()))
}
fn relation(s: &str) -> bool {
    s.split_once('.').is_some_and(|(a, b)| ident(a) && ident(b))
}
fn query_sql(q: &Value, policy: &Value) -> Result<String, StoreError> {
    fields(q, &["aggregate", "column", "relation"])?;
    let op = text(q, "aggregate")?;
    let column = text(q, "column")?;
    let table = text(q, "relation")?;
    require(["count", "sum"].contains(&op) && ident(column) && relation(table))?;
    require(policy["relations"].as_array().is_some_and(|rs| {
        rs.iter().any(|r| {
            r["name"] == table
                && r["columns"]
                    .as_array()
                    .is_some_and(|cs| cs.contains(&json!(column)))
        })
    }))?;
    Ok(format!("SELECT {op}({column}) FROM {table}"))
}
async fn now(tx: &Transaction<'_>) -> Result<i64, StoreError> {
    tx.query_one(
        "SELECT floor(extract(epoch FROM clock_timestamp()))::bigint",
        &[],
    )
    .await
    .map_err(query_error)?
    .try_get(0)
    .map_err(query_error)
}
async fn lock(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.query_one(crate::sql::LOCK_INSTALL, &[&crate::sql::INSTALL_LOCK])
        .await
        .map_err(query_error)?;
    Ok(())
}
async fn session(conn: &ControlConn<'_>) -> Result<store::Session, StoreError> {
    let s = store::open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    store::require_tls(&s.client).await?;
    let row = s
        .client
        .query_one("SELECT session_user,current_user", &[])
        .await
        .map_err(query_error)?;
    require(row.get::<_, String>(0) == conn.user && row.get::<_, String>(1) == conn.user)?;
    Ok(s)
}

fn policy_shape(p: &Value) -> Result<(), StoreError> {
    let native = crate::business_native::enabled(p);
    let mut keys = vec![
        "protocol",
        "scope",
        "project_id",
        "candidate_id",
        "candidate_digest",
        "visibility_sha256",
        "effective_sha256",
        "actors",
        "role",
        "relations",
        "checks",
        "expires_at",
        "receipt_ttl_seconds",
        "policy_revision",
        "execution_capability_version",
    ];
    if native {
        keys.extend([
            "native_mapping",
            "statement_timeout_ms",
            "execution_deadline_ms",
        ]);
    } else {
        keys.push("generation_relation");
    }
    fields(p, &keys)?;
    require(
        (p["protocol"] == "oo.release-coordinator/business-policy/v1" || native)
            && p["scope"] == "test_scope",
    )?;
    require(p["execution_capability_version"] == business_execution_capability())?;
    require(
        crate::seal::token_ok(text(p, "policy_revision")?)
            && (native || relation(text(p, "generation_relation")?)),
    )?;
    for key in ["project_id", "candidate_id"] {
        require(crate::seal::token_ok(text(p, key)?))?;
    }
    for key in ["candidate_digest", "visibility_sha256", "effective_sha256"] {
        require(crate::seal::hex64(text(p, key)?))?;
    }
    require(ident(text(p, "role")?) && text(p, "role")?.starts_with("oo_r_"))?;
    fields(&p["actors"], KINDS)?;
    let actors: BTreeSet<_> = KINDS
        .iter()
        .map(|k| text(&p["actors"], k))
        .collect::<Result<_, _>>()?;
    if native {
        crate::business_native::validate(p)?;
    }
    require(
        actors.len() == 4
            && actors.iter().all(|a| {
                ident(a) && *a != crate::sql::ADMIN_ROLE && *a != text(p, "role").unwrap_or("")
            }),
    )?;
    let relations = p["relations"].as_array().ok_or(StoreError::Input)?;
    require(!relations.is_empty() && relations.len() <= 8)?;
    let mut seen = BTreeSet::new();
    for r in relations {
        fields(r, &["name", "role", "columns"])?;
        require(relation(text(r, "name")?) && seen.insert(text(r, "name")?))?;
        require(r["role"] == p["role"] || r["role"] == p["actors"]["oracle"])?;
        let columns = r["columns"].as_array().ok_or(StoreError::Input)?;
        let names: BTreeSet<_> = columns.iter().filter_map(Value::as_str).collect();
        require(
            !names.is_empty()
                && names.len() == columns.len()
                && names.len() <= 16
                && names.iter().all(|n| ident(n)),
        )?;
    }
    let checks = p["checks"].as_array().ok_or(StoreError::Input)?;
    require(!checks.is_empty() && checks.len() <= 32)?;
    let mut ids = BTreeSet::new();
    for c in checks {
        fields(c, &["asset", "query", "oracle"])?;
        require(ids.insert(text(c, "asset")?))?;
        query_sql(&c["query"], p)?;
        query_sql(&c["oracle"], p)?;
        let qr = relations
            .iter()
            .find(|r| r["name"] == c["query"]["relation"])
            .ok_or(StoreError::Input)?;
        let or = relations
            .iter()
            .find(|r| r["name"] == c["oracle"]["relation"])
            .ok_or(StoreError::Input)?;
        require(
            qr["role"] == p["role"]
                && or["role"] == p["actors"]["oracle"]
                && qr["name"] != or["name"],
        )?;
    }
    require(
        p["receipt_ttl_seconds"]
            .as_i64()
            .is_some_and(|n| (1..=3600).contains(&n))
            && p["expires_at"].as_i64().is_some(),
    )
}

async fn load_policy(tx: &Transaction<'_>, sha: &str) -> Result<Option<Value>, StoreError> {
    let present: Option<String> = tx
        .query_one(PRESENT, &[])
        .await
        .map_err(query_error)?
        .get(0);
    if present.is_none() {
        return Ok(None);
    }
    let hash: String = tx
        .query_one(
            "SELECT sql_sha FROM oo_release_control.business_manifest WHERE singleton",
            &[],
        )
        .await
        .map_err(query_error)?
        .get(0);
    require(hash == crate::sha256_hex(MIGRATION.as_bytes()))?;
    let row=tx.query_opt("SELECT document FROM oo_release_control.business_policy p WHERE policy_sha=$1 \
        AND NOT EXISTS(SELECT 1 FROM oo_release_control.business_revocation r WHERE r.policy_sha=p.policy_sha)",&[&sha])
        .await.map_err(query_error)?;
    let Some(row) = row else { return Ok(None) };
    let raw: String = row.get(0);
    require(crate::sha256_hex(raw.as_bytes()) == sha)?;
    let policy: Value = serde_json::from_str(&raw).map_err(|_| StoreError::Input)?;
    policy_shape(&policy)?;
    if policy["expires_at"].as_i64().ok_or(StoreError::Input)? <= now(tx).await? {
        return Ok(None);
    }
    Ok(Some(policy))
}

async fn candidate(tx: &Transaction<'_>, p: &Value) -> Result<(Value, Value), StoreError> {
    let row=tx.query_one("SELECT source_hash,project_id,effective_oos,visibility_json,effective_sha256,visibility_sha256 FROM oo_release_control.candidate WHERE candidate_id=$1",
        &[&text(p,"candidate_id")?]).await.map_err(query_error)?;
    require(
        row.get::<_, String>(0) == text(p, "candidate_digest")?
            && row.get::<_, String>(1) == text(p, "project_id")?,
    )?;
    let effective: Value =
        serde_json::from_str(&row.get::<_, String>(2)).map_err(|_| StoreError::Input)?;
    let visibility: Value =
        serde_json::from_str(&row.get::<_, String>(3)).map_err(|_| StoreError::Input)?;
    require(
        canonical_sha(&effective) == text(p, "effective_sha256")?
            && canonical_sha(&effective) == row.get::<_, String>(4)
            && canonical_sha(&visibility) == row.get::<_, String>(5),
    )?;
    require(
        canonical_sha(&visibility) == text(p, "visibility_sha256")?
            && visibility["role"] == p["role"],
    )?;
    let mut expected = BTreeMap::new();
    for m in effective["metrics"].as_array().ok_or(StoreError::Input)? {
        if m["certified"] == true {
            require(m["tests"].as_array().is_some_and(|ts| {
                !ts.is_empty()
                    && ts.iter().all(|t| {
                        t["params"]
                            .as_object()
                            .is_some_and(serde_json::Map::is_empty)
                            && t["expect"] == json!({"rows":1})
                    })
            }))?;
            expected.insert(
                format!("metric:{}", text(m, "metric")?),
                (
                    text(m, "sql")?.to_owned(),
                    Some(text(&m["control_total"], "sql")?.to_owned()),
                ),
            );
        }
    }
    for e in effective["examples"].as_array().ok_or(StoreError::Input)? {
        expected.insert(
            format!("example:{}", text(e, "id")?),
            (text(e, "sql")?.to_owned(), None),
        );
    }
    for e in effective["ontology"]["publish"]["require_eval"]
        .as_array()
        .ok_or(StoreError::Input)?
    {
        expected.insert(
            format!("eval:{}", e.as_str().ok_or(StoreError::Input)?),
            (String::new(), None),
        );
    }
    let checks = p["checks"].as_array().ok_or(StoreError::Input)?;
    require(expected.len() == checks.len())?;
    for c in checks {
        let (sql, oracle) = expected.get(text(c, "asset")?).ok_or(StoreError::Input)?;
        let query = query_sql(&c["query"], p)?;
        require(sql.is_empty() || sql == &query)?;
        if let Some(oracle) = oracle {
            require(oracle == &query_sql(&c["oracle"], p)?)?;
        }
        // Target SQL cannot use columns outside the sealed target role projection.
        let visible = visibility["datasets"]
            .as_array()
            .ok_or(StoreError::Input)?
            .iter()
            .find(|d| d["dataset"] == c["query"]["relation"])
            .ok_or(StoreError::Input)?;
        require(
            visible["columns"]
                .as_array()
                .is_some_and(|cs| cs.contains(&c["query"]["column"])),
        )?;
    }
    for r in p["relations"].as_array().ok_or(StoreError::Input)? {
        if r["role"] == p["role"] {
            let visible = visibility["datasets"]
                .as_array()
                .ok_or(StoreError::Input)?
                .iter()
                .find(|d| d["dataset"] == r["name"])
                .ok_or(StoreError::Input)?;
            require(
                r["columns"]
                    .as_array()
                    .ok_or(StoreError::Input)?
                    .iter()
                    .all(|column| {
                        visible["columns"]
                            .as_array()
                            .is_some_and(|columns| columns.contains(column))
                    }),
            )?;
        }
    }
    Ok((effective, visibility))
}

async fn role_state(tx: &Transaction<'_>, p: &Value) -> Result<Value, StoreError> {
    let mut names: Vec<String> = KINDS
        .iter()
        .map(|k| text(&p["actors"], k).map(str::to_owned))
        .collect::<Result<_, _>>()?;
    names.push(text(p, "role")?.into());
    names.sort();
    let mut roles = Vec::new();
    for name in names {
        let row=tx.query_one("SELECT oid::bigint,rolsuper,rolcreaterole,rolcreatedb,rolreplication,rolbypassrls,rolcanlogin,\
            (rolvaliduntil IS NULL OR rolvaliduntil>clock_timestamp()),coalesce(rolvaliduntil::text,''),\
            coalesce((SELECT string_agg(roleid::text||':'||xmin::text,',' ORDER BY roleid) FROM pg_auth_members WHERE member=r.oid),''),\
            CASE WHEN rolvaliduntil IS NULL OR rolvaliduntil='infinity'::timestamptz THEN NULL \
            ELSE floor(extract(epoch FROM rolvaliduntil))::bigint END \
            FROM pg_roles r WHERE rolname=$1",&[&name]).await.map_err(query_error)?;
        require(!(1..=5).any(|i| row.get::<_, bool>(i)) && row.get::<_, bool>(7))?;
        let privileged: bool = tx.query_one(
            "SELECT pg_has_role($1,'oo_release_admin','MEMBER') OR EXISTS(SELECT 1 FROM pg_roles \
             WHERE (rolsuper OR rolcreaterole OR rolbypassrls OR rolreplication) AND pg_has_role($1,oid,'MEMBER'))",
            &[&name],
        ).await.map_err(query_error)?.get(0);
        require(!privileged)?;
        if name != text(p, "role")? {
            require(row.get::<_, bool>(6))?;
        }
        roles.push(
            json!({"login":name,"oid":row.get::<_,i64>(0),"can_login":row.get::<_,bool>(6),
            "valid_until":row.get::<_,String>(8),"memberships":row.get::<_,String>(9),
            "expires_at":row.get::<_,Option<i64>>(10)}),
        );
    }
    Ok(json!(roles))
}

async fn vector(tx: &Transaction<'_>, p: &Value, effective: &Value) -> Result<Value, StoreError> {
    let roles = role_state(tx, p).await?;
    if crate::business_native::enabled(p) {
        return crate::business_native::vector(tx, p, roles, effective).await;
    }
    let mut relations = p["relations"].as_array().ok_or(StoreError::Input)?.clone();
    relations.sort_by_key(|r| r["name"].as_str().unwrap_or("").to_owned());
    // The locks are held through plan/CAS commit, not just through the hash query.
    for r in &relations {
        tx.batch_execute(&format!("LOCK TABLE {} IN SHARE MODE", text(r, "name")?))
            .await
            .map_err(query_error)?;
    }
    let generation_relation = text(p, "generation_relation")?;
    require(!relations.iter().any(|r| r["name"] == generation_relation))?;
    tx.batch_execute(&format!("LOCK TABLE {generation_relation} IN SHARE MODE"))
        .await
        .map_err(query_error)?;
    for kind in KINDS {
        for table in relations
            .iter()
            .map(|r| text(r, "name"))
            .chain(std::iter::once(Ok(generation_relation)))
        {
            let writable: bool = tx
                .query_one(
                    "SELECT has_table_privilege($1,$2,'INSERT,UPDATE,DELETE,TRUNCATE') \
            OR has_any_column_privilege($1,$2,'UPDATE')",
                    &[&text(&p["actors"], kind)?, &table?],
                )
                .await
                .map_err(query_error)?
                .get(0);
            require(!writable)?;
        }
    }
    let mut entries = Vec::new();
    for r in &relations {
        let name = text(r, "name")?;
        let generations = tx
            .query(
                &format!("SELECT generation::bigint FROM {generation_relation} WHERE dataset=$1"),
                &[&name],
            )
            .await
            .map_err(query_error)?;
        require(generations.len() == 1)?;
        let generation: i64 = generations[0].try_get(0).map_err(query_error)?;
        require(generation > 0)?;
        let row=tx.query_one("SELECT c.oid::bigint,c.relkind::text,c.relrowsecurity,c.relforcerowsecurity,\
            coalesce(c.relacl::text,''),coalesce((SELECT string_agg(a.attname||':'||a.atttypid||':'||a.attnotnull||':'||coalesce(a.attacl::text,''),',' ORDER BY a.attnum) \
            FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),'') \
            FROM pg_class c WHERE c.oid=to_regclass($1)",&[&name]).await.map_err(query_error)?;
        require(
            row.get::<_, String>(1) == "r" && !row.get::<_, bool>(2) && !row.get::<_, bool>(3),
        )?;
        let column_names: Vec<String> = r["columns"]
            .as_array()
            .ok_or(StoreError::Input)?
            .iter()
            .map(|c| c.as_str().map(str::to_owned).ok_or(StoreError::Input))
            .collect::<Result<_, _>>()?;
        let types = tx.query("SELECT attname,atttypid::bigint FROM pg_attribute \
            WHERE attrelid=to_regclass($1) AND attname=ANY($2::text[]) AND attnum>0 AND NOT attisdropped",
            &[&name,&column_names]).await.map_err(query_error)?;
        require(
            types.len() == column_names.len()
                && types
                    .iter()
                    .all(|r| [20_i64, 21, 23, 1700].contains(&r.get::<_, i64>(1))),
        )?;
        tx.batch_execute(&format!("SET LOCAL ROLE {}", text(r, "role")?))
            .await
            .map_err(query_error)?;
        let columns = r["columns"]
            .as_array()
            .ok_or(StoreError::Input)?
            .iter()
            .map(|v| v.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(",");
        let rows = tx
            .query(
                &format!("SELECT json_build_array({columns})::text FROM {name} LIMIT 1001"),
                &[],
            )
            .await
            .map_err(query_error)?;
        require(rows.len() <= 1000)?;
        let mut data = rows
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect::<Vec<_>>();
        data.sort();
        require(data.iter().map(String::len).sum::<usize>() <= 1_048_576)?;
        tx.batch_execute("RESET ROLE").await.map_err(query_error)?;
        entries.push(json!({"relation":name,"role":r["role"],"columns":r["columns"],"oid":row.get::<_,i64>(0),
            "generation":generation,"acl":row.get::<_,String>(4),"schema":row.get::<_,String>(5),"rows":data.len(),"rows_sha256":canonical_sha(&json!(data))}));
    }
    Ok(
        json!({"protocol":"oo.release-coordinator/test-data-vector/v1","relations":entries,"roles":roles}),
    )
}

pub async fn install_business_policy(
    control: &ControlConn<'_>,
    path: &Path,
    pin: &str,
) -> Result<Receipt, StoreError> {
    require(control.user == crate::sql::ADMIN_ROLE && crate::seal::hex64(pin))?;
    let meta = std::fs::symlink_metadata(path).map_err(|_| StoreError::Input)?;
    require(meta.is_file() && meta.len() <= 1_048_576 && meta.permissions().mode() & 0o077 == 0)?;
    let raw = std::fs::read_to_string(path).map_err(|_| StoreError::Input)?;
    require(crate::sha256_hex(raw.as_bytes()) == pin)?;
    let p: Value = serde_json::from_str(&raw).map_err(|_| StoreError::Input)?;
    policy_shape(&p)?;
    let mut s = session(control).await?;
    let tx = store::begin(&mut s).await?;
    lock(&tx).await?;
    candidate(&tx, &p).await?;
    role_state(&tx, &p).await?;
    require(p["expires_at"].as_i64().unwrap() > now(&tx).await?)?;
    if tx
        .query_one(PRESENT, &[])
        .await
        .map_err(query_error)?
        .get::<_, Option<String>>(0)
        .is_none()
    {
        tx.batch_execute(MIGRATION).await.map_err(query_error)?;
        tx.execute(
            "INSERT INTO oo_release_control.business_manifest VALUES(true,$1)",
            &[&crate::sha256_hex(MIGRATION.as_bytes())],
        )
        .await
        .map_err(query_error)?;
    }
    let hash: String = tx
        .query_one(
            "SELECT sql_sha FROM oo_release_control.business_manifest WHERE singleton",
            &[],
        )
        .await
        .map_err(query_error)?
        .get(0);
    require(hash == crate::sha256_hex(MIGRATION.as_bytes()))?;
    for kind in KINDS {
        let role = text(&p["actors"], kind)?;
        tx.batch_execute(&format!(
            "GRANT USAGE ON SCHEMA oo_release_control TO {role};\
            GRANT SELECT ON oo_release_control.business_policy,oo_release_control.business_receipt,\
            oo_release_control.business_revocation,oo_release_control.business_manifest TO {role};\
            GRANT INSERT ON oo_release_control.business_receipt TO {role}"
        ))
        .await
        .map_err(query_error)?;
    }
    tx.execute("INSERT INTO oo_release_control.business_policy(policy_sha,candidate_id,source_hash,document) \
        VALUES($1,$2,$3,$4) ON CONFLICT(policy_sha) DO NOTHING",&[&pin,&text(&p,"candidate_id")?,&text(&p,"candidate_digest")?,&raw]).await.map_err(query_error)?;
    tx.commit().await.map_err(query_error)?;
    Ok(store::stamped(
        "installed",
        None,
        json!({"policy_sha256":pin,"scope":"test_scope","database_used":true}),
        "not_issued",
    ))
}

pub async fn revoke_business_policy(
    control: &ControlConn<'_>,
    pin: &str,
) -> Result<Receipt, StoreError> {
    require(control.user == crate::sql::ADMIN_ROLE && crate::seal::hex64(pin))?;
    let mut s = session(control).await?;
    let tx = store::begin(&mut s).await?;
    lock(&tx).await?;
    tx.execute("INSERT INTO oo_release_control.business_revocation(policy_sha) VALUES($1) ON CONFLICT DO NOTHING",&[&pin]).await.map_err(query_error)?;
    tx.commit().await.map_err(query_error)?;
    Ok(store::stamped(
        "revoked",
        None,
        json!({"policy_sha256":pin,"database_used":true}),
        "not_issued",
    ))
}

async fn execute_actor(
    conn: &ControlConn<'_>,
    p: &Value,
    pin: &str,
    run: &str,
    kind: &str,
    binding: &Value,
    snapshot: &str,
    deadline: tokio::time::Instant,
) -> Result<String, StoreError> {
    require(conn.user == text(&p["actors"], kind)?)?;
    let mut s = session(conn).await?;
    let cancel = s.client.cancel_token();
    let tx = s
        .client
        .build_transaction()
        .read_only(true)
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await
        .map_err(query_error)?;
    require(!snapshot.is_empty() && snapshot.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))?;
    tx.batch_execute(&format!("SET TRANSACTION SNAPSHOT '{snapshot}'"))
        .await
        .map_err(query_error)?;
    let timeout = p["statement_timeout_ms"].as_u64().unwrap_or(5000);
    tx.batch_execute(&format!("SET LOCAL search_path=pg_catalog; SET LOCAL statement_timeout='{timeout}ms'; SET LOCAL lock_timeout='{timeout}ms'"))
        .await
        .map_err(query_error)?;
    let started = now(&tx).await?;
    if kind != "oracle" {
        tx.batch_execute(&format!("SET LOCAL ROLE {}", text(p, "role")?))
            .await
            .map_err(query_error)?;
    }
    let mut results = BTreeMap::new();
    for c in p["checks"].as_array().ok_or(StoreError::Input)? {
        let asset = text(c, "asset")?;
        if (kind == "executor" && asset.starts_with("eval:"))
            || (kind == "evaluator" && !asset.starts_with("eval:"))
        {
            continue;
        }
        let query = &c[if kind == "oracle" { "oracle" } else { "query" }];
        let sql = query_sql(query, p)?;
        let physical_sql = if crate::business_native::enabled(p) {
            crate::business_native::physical_sql(query, binding)?
        } else {
            sql.clone()
        };
        let statement = format!("SELECT value::text FROM ({physical_sql}) q(value)");
        let work = async {
            let stmt = tx.prepare(&statement).await.map_err(query_error)?;
            tx.query(&stmt, &[]).await.map_err(query_error)
        };
        let rows = match tokio::time::timeout_at(deadline, work).await {
            Ok(result) => result?,
            Err(_) => {
                let tls = crate::rustls_connector(conn.ca_pem)?;
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    cancel.cancel_query(tls),
                )
                .await;
                return Err(StoreError::Input);
            }
        };
        require(rows.len() == 1)?;
        let value: Option<String> = rows[0].try_get(0).map_err(query_error)?;
        let value = value.ok_or(StoreError::Input)?;
        require(value.len() <= 128)?;
        results.insert(
            asset.to_owned(),
            json!({"sql_sha256":crate::sha256_hex(sql.as_bytes()),"executed_sql_sha256":crate::sha256_hex(physical_sql.as_bytes()),
                "returned_rows":1,"scalar":value}),
        );
    }
    let completed = now(&tx).await?;
    tx.rollback().await.map_err(query_error)?;
    let body = json!({"protocol":"oo.release-coordinator/business-observation/v1","policy_sha":pin,
        "execution_capability_version":business_execution_capability(),
        "database_snapshot":snapshot,
        "run_id":run,"kind":kind,"actor":conn.user,"binding":binding,"results":results,
        "started_at":started,"completed_at":completed,
        "expires_at":p["expires_at"].as_i64().unwrap().min(started+p["receipt_ttl_seconds"].as_i64().unwrap())});
    persist(&s.client, pin, run, kind, &body).await
}

async fn persist(
    client: &tokio_postgres::Client,
    pin: &str,
    id: &str,
    kind: &str,
    body: &Value,
) -> Result<String, StoreError> {
    let raw = canonical(body);
    let hash = crate::sha256_hex(raw.as_bytes());
    client.execute("INSERT INTO oo_release_control.business_receipt(policy_sha,kind,receipt_id,actor_oid,body,body_sha) \
        VALUES($1,$2,$3,0,$4,$5)",&[&pin,&kind,&id,&raw,&hash]).await.map_err(query_error)?;
    Ok(hash)
}

pub async fn execute_business_checks(
    control: &ControlConn<'_>,
    executor: &ControlConn<'_>,
    oracle: &ControlConn<'_>,
    evaluator: &ControlConn<'_>,
    pin: &str,
    run: &str,
) -> Result<Receipt, StoreError> {
    require(
        control.user == crate::sql::ADMIN_ROLE
            && crate::seal::token_ok(run)
            && [executor, oracle, evaluator]
                .iter()
                .all(|c| c.endpoint == control.endpoint),
    )?;
    let mut s = session(control).await?;
    let tx = store::begin_for(&mut s, "60s", "60s").await?;
    tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .await
        .map_err(query_error)?;
    lock(&tx).await?;
    let p = load_policy(&tx, pin).await?.ok_or(StoreError::Input)?;
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_millis(p["execution_deadline_ms"].as_u64().unwrap_or(60000));
    let (effective, visibility) = candidate(&tx, &p).await?;
    let data = vector(&tx, &p, &effective).await?;
    let binding = json!({"candidate_digest":p["candidate_digest"],"definition_digest":canonical_sha(&effective),
        "policy_digest":canonical_sha(&visibility),"data_vector":data,"data_vector_digest":canonical_sha(&data)});
    let snapshot: String = tx
        .query_one("SELECT pg_export_snapshot()", &[])
        .await
        .map_err(query_error)?
        .get(0);
    let mut refs = BTreeMap::new();
    for (kind, conn) in [
        ("executor", executor),
        ("oracle", oracle),
        ("evaluator", evaluator),
    ] {
        refs.insert(
            kind,
            execute_actor(conn, &p, pin, run, kind, &binding, &snapshot, deadline).await?,
        );
    }
    tx.commit().await.map_err(query_error)?;
    Ok(store::stamped(
        "recorded",
        None,
        json!({"run_id":run,"policy_sha256":pin,"receipt_refs":refs,
        "execution_bundle_sha256":canonical_sha(&json!(refs)),"binding":binding,
        "test_admissible":false,"production_admissible":false,"database_used":true}),
        "not_issued",
    ))
}

async fn observations(
    tx: &Transaction<'_>,
    pin: &str,
    run: &str,
    p: &Value,
) -> Result<BTreeMap<String, (Value, String)>, StoreError> {
    let rows = tx
        .query(
            "SELECT kind,actor,actor_oid::bigint,body,body_sha,\
        floor(extract(epoch FROM recorded_at))::bigint FROM oo_release_control.business_receipt \
        WHERE policy_sha=$1 AND receipt_id=$2 ORDER BY kind",
            &[&pin, &run],
        )
        .await
        .map_err(query_error)?;
    let mut result = BTreeMap::new();
    let time = now(tx).await?;
    for row in rows {
        let kind: String = row.get(0);
        let actor: String = row.get(1);
        let recorded: i64 = row.get(5);
        let raw: String = row.get(3);
        let hash: String = row.get(4);
        require(
            KINDS.contains(&kind.as_str())
                && actor == text(&p["actors"], &kind)?
                && hash == crate::sha256_hex(raw.as_bytes()),
        )?;
        let oid: i64 = tx
            .query_one(
                "SELECT oid::bigint FROM pg_roles WHERE rolname=$1",
                &[&actor],
            )
            .await
            .map_err(query_error)?
            .get(0);
        require(row.get::<_, i64>(2) == oid)?;
        let body: Value = serde_json::from_str(&raw).map_err(|_| StoreError::Input)?;
        require(body["execution_capability_version"] == business_execution_capability())?;
        require(
            body["policy_sha"] == pin
                && body["run_id"] == run
                && body["kind"] == kind
                && body["actor"] == actor,
        )?;
        let start = body["started_at"].as_i64().ok_or(StoreError::Input)?;
        let end = body["completed_at"].as_i64().ok_or(StoreError::Input)?;
        let expires = body["expires_at"].as_i64().ok_or(StoreError::Input)?;
        require(
            start <= end
                && end <= time
                && expires > time
                && expires <= p["expires_at"].as_i64().unwrap()
                && expires - start <= p["receipt_ttl_seconds"].as_i64().unwrap()
                && recorded >= end
                && row.get::<_, i64>(5) < expires,
        )?;
        result.insert(kind, (body, hash));
    }
    Ok(result)
}

pub async fn review_business_checks(
    reviewer: &ControlConn<'_>,
    pin: &str,
    run: &str,
    expected_bundle: &str,
) -> Result<Receipt, StoreError> {
    require(crate::seal::hex64(expected_bundle) && crate::seal::token_ok(run))?;
    let mut s = session(reviewer).await?;
    let tx = store::begin(&mut s).await?;
    let p = load_policy(&tx, pin).await?.ok_or(StoreError::Input)?;
    require(reviewer.user == text(&p["actors"], "reviewer")?)?;
    let found = observations(&tx, pin, run, &p).await?;
    require(
        found.len() == 3
            && ["executor", "oracle", "evaluator"]
                .iter()
                .all(|k| found.contains_key(*k)),
    )?;
    let refs: BTreeMap<_, _> = found
        .iter()
        .map(|(k, (_, hash))| (k.clone(), hash.clone()))
        .collect();
    require(canonical_sha(&json!(refs)) == expected_bundle)?;
    require(matches_results(&p, &found)?)?;
    let time = now(&tx).await?;
    let deadline = found
        .values()
        .map(|(b, _)| b["expires_at"].as_i64().unwrap_or(0))
        .min()
        .ok_or(StoreError::Input)?;
    let body = json!({"protocol":"oo.release-coordinator/business-review/v1","policy_sha":pin,"run_id":run,
        "execution_capability_version":business_execution_capability(),
        "kind":"reviewer","actor":reviewer.user,"binding":found["executor"].0["binding"],
        "receipt_refs":refs,"decision":"approve","started_at":time,"completed_at":time,"expires_at":deadline,
        "business_owner_signed":false,"scope":"test_scope"});
    tx.rollback().await.map_err(query_error)?;
    let hash = persist(&s.client, pin, run, "reviewer", &body).await?;
    Ok(store::stamped(
        "recorded",
        None,
        json!({"receipt_sha256":hash,"business_owner_signed":false,
        "production_admissible":false,"database_used":true}),
        "not_issued",
    ))
}

fn matches_results(
    p: &Value,
    found: &BTreeMap<String, (Value, String)>,
) -> Result<bool, StoreError> {
    let Some((oracle, _)) = found.get("oracle") else {
        return Ok(false);
    };
    let mut expected = BTreeMap::<&str, BTreeSet<String>>::from([
        ("executor", BTreeSet::new()),
        ("oracle", BTreeSet::new()),
        ("evaluator", BTreeSet::new()),
    ]);
    for c in p["checks"].as_array().ok_or(StoreError::Input)? {
        let asset = text(c, "asset")?;
        let kind = if asset.starts_with("eval:") {
            "evaluator"
        } else {
            "executor"
        };
        expected.get_mut(kind).unwrap().insert(asset.into());
        expected.get_mut("oracle").unwrap().insert(asset.into());
        let Some((observed, _)) = found.get(kind) else {
            return Ok(false);
        };
        if observed["binding"] != oracle["binding"]
            || observed["database_snapshot"] != oracle["database_snapshot"]
            || observed["results"][asset]["scalar"] != oracle["results"][asset]["scalar"]
            || !observed["results"][asset]["scalar"].is_string()
            || observed["results"][asset]["sql_sha256"]
                != crate::sha256_hex(query_sql(&c["query"], p)?.as_bytes())
            || oracle["results"][asset]["sql_sha256"]
                != crate::sha256_hex(query_sql(&c["oracle"], p)?.as_bytes())
        {
            return Ok(false);
        }
        if crate::business_native::enabled(p) {
            let executed = crate::business_native::physical_sql(&c["query"], &observed["binding"])?;
            let independent =
                crate::business_native::physical_sql(&c["oracle"], &oracle["binding"])?;
            if observed["results"][asset]["executed_sql_sha256"]
                != crate::sha256_hex(executed.as_bytes())
                || oracle["results"][asset]["executed_sql_sha256"]
                    != crate::sha256_hex(independent.as_bytes())
            {
                return Ok(false);
            }
        }
    }
    for (kind, keys) in expected {
        let Some((body, _)) = found.get(kind) else {
            return Ok(false);
        };
        let actual = body["results"]
            .as_object()
            .ok_or(StoreError::Input)?
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual != keys {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) struct VerifiedBusiness {
    pub(crate) certificates: Vec<String>,
    pub(crate) deadline: i64,
    gates: BTreeSet<String>,
    receipts: BTreeMap<String, Vec<Value>>,
    native_mapping_sha256: Option<String>,
}
impl VerifiedBusiness {
    pub(crate) fn matches_native(&self, mapping: Option<&Value>) -> bool {
        self.native_mapping_sha256 == mapping.map(canonical_sha)
    }
    pub(crate) fn receipts(&self, gate: &str) -> Option<&Vec<Value>> {
        self.receipts.get(gate)
    }
    pub(crate) fn covers(&self, gate: &str) -> bool {
        self.gates.contains(gate)
    }
    pub(crate) fn overlay(&self, gates: &mut [crate::Summary]) {
        for gate in gates {
            if self.covers(&gate.gate_id) && gate.disposition == "blocked" {
                gate.disposition = "admissible".into();
                gate.test_admissible = true;
                gate.production_admissible = false;
            }
        }
    }
}

fn receipt_views(
    p: &Value,
    pin: &str,
    effective: &Value,
    visibility: &Value,
    found: &BTreeMap<String, (Value, String)>,
) -> Result<BTreeMap<String, Vec<Value>>, StoreError> {
    let mut out = BTreeMap::<String, Vec<Value>>::new();
    let linked: BTreeMap<_, _> = found
        .iter()
        .map(|(k, (_, h))| (k.clone(), h.clone()))
        .collect();
    for check in p["checks"].as_array().ok_or(StoreError::Input)? {
        let asset = text(check, "asset")?;
        let (category, id) = asset.split_once(':').ok_or(StoreError::Input)?;
        let (definition, gates) = match category {
            "metric" => (
                effective["metrics"]
                    .as_array()
                    .ok_or(StoreError::Input)?
                    .iter()
                    .find(|m| m["metric"] == id)
                    .ok_or(StoreError::Input)?
                    .clone(),
                vec!["G5a", "G5b", "G11"],
            ),
            "example" => (
                effective["examples"]
                    .as_array()
                    .ok_or(StoreError::Input)?
                    .iter()
                    .find(|e| e["id"] == id)
                    .ok_or(StoreError::Input)?
                    .clone(),
                vec!["G6"],
            ),
            "eval" => (check.clone(), vec!["G10"]),
            _ => return Err(StoreError::Input),
        };
        for gate in gates {
            let actor_kind = match gate {
                "G10" => "evaluator",
                "G11" => "reviewer",
                _ => "executor",
            };
            let (body, body_sha) = &found[actor_kind];
            let (kind, expected, observed, plan) = if gate == "G11" {
                let expected = json!({"decision":"approve","receipt_refs":body["receipt_refs"]});
                (
                    "semantic_review",
                    expected.clone(),
                    json!({"decision":body["decision"],"receipt_refs":body["receipt_refs"]}),
                    json!({"candidate_digest":p["candidate_digest"],"definition":definition,"receipt_refs":body["receipt_refs"]}),
                )
            } else {
                let kind = match gate {
                    "G5a" => "metric_definition",
                    "G5b" => "metric_control_total",
                    "G6" => "example_execution",
                    _ => "business_eval",
                };
                (
                    kind,
                    json!({"scalar":found["oracle"].0["results"][asset]["scalar"]}),
                    json!({"scalar":body["results"][asset]["scalar"]}),
                    json!({"query":query_sql(&check["query"],p)?,"oracle":query_sql(&check["oracle"],p)?}),
                )
            };
            let receipt = json!({
                "evidence_schema":"oo.release-coordinator/authenticated-business-evidence/v1",
                "evidence_id":canonical_sha(&json!([pin,gate,asset,body_sha])),
                "gate_id":gate,"asset_id":asset,"evidence_kind":kind,
                "issuer":body["actor"],"issuer_scope":{"scope":"test_scope","project_id":p["project_id"],
                    "candidate_id":p["candidate_id"],"role":p["role"],"policy_sha":pin},
                "candidate_digest":p["candidate_digest"],"semantic_definition_digest":canonical_sha(&definition),
                "dependency_digest":canonical_sha(effective),
                "data_vector_digest":body["binding"]["data_vector_digest"],
                "source_coverage":{"relations":p["relations"],"generation_relation":p["generation_relation"],
                    "native_mapping":p["native_mapping"]},
                "role":p["role"],"policy_digest":canonical_sha(visibility),"sql_or_plan_digest":canonical_sha(&plan),
                "execution_capability_version":business_execution_capability(),
                "expected_result_digest":canonical_sha(&expected),"observed_result_digest":canonical_sha(&observed),
                "started_at":body["started_at"],"completed_at":body["completed_at"],"expires_at":body["expires_at"],
                "outcome":"pass","unresolved_count":0,
                "authenticated_receipt_reference":{"policy_sha":pin,"run_id":body["run_id"],
                    "body_sha256":body_sha,"linked_receipts":linked},
                "test_admissible":true,"production_admissible":false,"business_owner_signed":false
            });
            out.entry(gate.into()).or_default().push(receipt);
        }
    }
    Ok(out)
}

pub(crate) async fn verify(
    tx: &Transaction<'_>,
    candidate_id: &str,
    source: &str,
    effective: &Value,
    visibility: &Value,
) -> Result<Option<VerifiedBusiness>, StoreError> {
    if tx
        .query_one(PRESENT, &[])
        .await
        .map_err(query_error)?
        .get::<_, Option<String>>(0)
        .is_none()
    {
        return Ok(None);
    }
    let policies=tx.query("SELECT p.policy_sha FROM oo_release_control.business_policy p WHERE candidate_id=$1 AND source_hash=$2 \
        AND NOT EXISTS(SELECT 1 FROM oo_release_control.business_revocation r WHERE r.policy_sha=p.policy_sha)",&[&candidate_id,&source]).await.map_err(query_error)?;
    if policies.len() != 1 {
        return Ok(None);
    }
    let pin: String = policies[0].get(0);
    let Some(p) = load_policy(tx, &pin).await? else {
        return Ok(None);
    };
    let (current, current_vis) = candidate(tx, &p).await?;
    require(&current == effective && &current_vis == visibility)?;
    let data = vector(tx, &p, effective).await?;
    let binding = json!({"candidate_digest":source,"definition_digest":canonical_sha(effective),
        "policy_digest":canonical_sha(visibility),"data_vector":data,"data_vector_digest":canonical_sha(&data)});
    let rows=tx.query("SELECT receipt_id FROM oo_release_control.business_receipt WHERE policy_sha=$1 AND kind='reviewer' ORDER BY recorded_at DESC LIMIT 33",&[&pin]).await.map_err(query_error)?;
    require(rows.len() <= 32)?;
    let mut valid = Vec::new();
    for row in rows {
        let run: String = row.get(0);
        // Expired/malformed persisted receipts never become a pass.
        let found = match observations(tx, &pin, &run, &p).await {
            Ok(v) => v,
            Err(StoreError::Input) => continue,
            Err(e) => return Err(e),
        };
        if found.len() != 4 || !matches_results(&p, &found)? {
            continue;
        }
        if found.values().any(|(b, _)| b["binding"] != binding) {
            continue;
        }
        let refs: BTreeMap<_, _> = found
            .iter()
            .filter(|(k, _)| k.as_str() != "reviewer")
            .map(|(k, (_, h))| (k.clone(), h.clone()))
            .collect();
        let review = &found["reviewer"].0;
        if review["receipt_refs"] != json!(refs)
            || review["decision"] != "approve"
            || review["business_owner_signed"] != false
        {
            continue;
        }
        valid.push(found);
    }
    if valid.len() != 1 {
        return Ok(None);
    }
    let found = valid.remove(0);
    let gates = crate::apply::preconditions(effective)
        .map_err(|_| StoreError::Input)?
        .iter()
        .filter(|g| g.disposition == "blocked" && GATES.contains(&g.gate_id))
        .map(|g| g.gate_id.to_owned())
        .collect();
    let receipts = receipt_views(&p, &pin, effective, visibility, &found)?;
    let certificates = found
        .values()
        .map(|(_, h)| h.clone())
        .chain(std::iter::once(pin))
        .collect();
    let mut deadline = found
        .values()
        .map(|(b, _)| b["expires_at"].as_i64().unwrap_or(0))
        .min()
        .ok_or(StoreError::Input)?;
    for role in binding["data_vector"]["roles"]
        .as_array()
        .ok_or(StoreError::Input)?
    {
        if let Some(expiry) = role["expires_at"].as_i64() {
            deadline = deadline.min(expiry);
        }
    }
    Ok(Some(VerifiedBusiness {
        certificates,
        deadline,
        gates,
        receipts,
        native_mapping_sha256: crate::business_native::enabled(&p)
            .then(|| canonical_sha(&p["native_mapping"])),
    }))
}

pub(crate) async fn preflight(
    session: &mut store::Session,
    candidate_id: &str,
    source: &str,
    effective: &Value,
    visibility: &Value,
) -> Result<Option<VerifiedBusiness>, StoreError> {
    let tx = store::begin(session).await?;
    lock(&tx).await?;
    let result = verify(&tx, candidate_id, source, effective, visibility).await?;
    tx.commit().await.map_err(query_error)?;
    Ok(result)
}
