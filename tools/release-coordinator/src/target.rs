//! Target-database fence. Privileges are separate transactions from the control CAS.

use serde_json::{Value, json};
use tokio_postgres::Transaction;

use crate::error::{Phase, StoreError, query_error};
use crate::sql::{self, TARGET_ROLE};
use crate::store::{
    Endpoint, Receipt, begin_for, commit, open, refused, require_tls, rollback, stamped,
};

macro_rules! p {
    ($($value:expr),* $(,)?) => {
        &[$(&$value as &(dyn postgres_types::ToSql + Sync)),*]
    };
}

const POLICY: &str = "oo_fence";

#[derive(Clone, Copy)]
pub struct TargetConn<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
}

/// Observe the configured physical standby after all fence-closing commits.
pub async fn replica_barrier(primary: &TargetConn<'_>, replica: &TargetConn<'_>) -> Result<Value, StoreError> {
    if primary.endpoint == replica.endpoint || primary.endpoint.database != replica.endpoint.database {
        return Err(StoreError::Input);
    }
    let primary_session = open(primary.endpoint, primary.user, primary.password, primary.ca_pem).await?;
    require_tls(&primary_session.client).await?;
    let cut = primary_session.client.query_one("SELECT pg_is_in_recovery(), pg_current_wal_flush_lsn()::text", &[]).await.map_err(query_error)?;
    if cut.get::<_, bool>(0) { return Err(StoreError::Input); }
    let required: String = cut.get(1);
    let standby = open(replica.endpoint, replica.user, replica.password, replica.ca_pem).await?;
    require_tls(&standby.client).await?;
    standby.client.batch_execute("SET statement_timeout = '5s'").await.map_err(query_error)?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let row = standby.client.query_one("SELECT pg_is_in_recovery(), current_setting('transaction_read_only') = 'on', pg_last_wal_replay_lsn()::text, COALESCE(pg_last_wal_replay_lsn() >= $1::text::pg_lsn, false)", p!(&required)).await.map_err(query_error)?;
        let recovery: bool = row.get(0); let readonly: bool = row.get(1);
        if !recovery || !readonly { return Err(StoreError::Input); }
        if row.get::<_, bool>(3) {
            return Ok(json!({"barrier":"replayed", "primary_flush_lsn":required,
                "replica_replay_lsn":row.get::<_, Option<String>>(2), "in_recovery":recovery,
                "transaction_read_only":readonly, "replica_host":replica.endpoint.host,
                "database":replica.endpoint.database}));
        }
        if tokio::time::Instant::now() >= deadline { return Err(StoreError::Input); }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

pub struct FenceRef<'a> {
    pub schema_name: &'a str,
    pub relation_name: &'a str,
    pub role_name: &'a str,
}

struct FenceRow {
    state: String,
    opened: Vec<String>,
    pack: Option<String>,
    version: Option<String>,
    candidate: Option<String>,
    streams: i32,
}

pub async fn install_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let receipt = match install_body(&tx, fence).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn close_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let receipt = match close_body(&tx, fence).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn grant_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
    columns: &[String],
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    let columns = clean_columns(columns)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let receipt = match grant_body(&tx, fence, &columns).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn revoke_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let receipt = match revoke_body(&tx, fence).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn open_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
    columns: &[String],
    pack_digest: &str,
    version: &str,
    candidate_id: &str,
) -> Result<Receipt, StoreError> {
    open_target_inner(
        conn,
        fence,
        columns,
        pack_digest,
        version,
        candidate_id,
        None,
    )
    .await
}

pub async fn open_native_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
    columns: &[String],
    pack_digest: &str,
    version: &str,
    candidate_id: &str,
    publication: &crate::native_publication::Publication,
) -> Result<Receipt, StoreError> {
    open_target_inner(
        conn,
        fence,
        columns,
        pack_digest,
        version,
        candidate_id,
        Some(publication),
    )
    .await
}

/// One source cut opens every reviewed table fence in the same target transaction.
pub async fn open_native_collection(conn:&TargetConn<'_>,bindings:&[(String,String,String,Vec<String>)],
 pack_digest:&str,version:&str,candidate_id:&str,publication:&crate::native_publication::Publication)->Result<Vec<Receipt>,StoreError> {
 if bindings.is_empty() || (!crate::native_library::is_library(&publication.receipt) && bindings.len()>32) || !crate::seal::hex64(pack_digest)
  || !crate::seal::token_ok(version) || !crate::seal::token_ok(candidate_id) {return Err(StoreError::Input);}
 for (schema,table,role,columns) in bindings {
  checked(&FenceRef {schema_name:schema,relation_name:table,role_name:role})?;clean_columns(columns)?;
 }
 let mut session=open(conn.endpoint,conn.user,conn.password,conn.ca_pem).await?;require_tls(&session.client).await?;
 let tx=begin_for(&mut session,"60s","60s").await?;
 if crate::native_library::is_library(&publication.receipt) {
  let ceiling:i32=tx.query_one("SELECT max_tables FROM _oo.native_library_limits WHERE singleton",&[]).await.map_err(query_error)?.get(0);
  let tables=crate::native_library::tables(&publication.receipt)?;
  let actual:std::collections::BTreeSet<_>=bindings.iter().map(|b|b.1.as_str()).collect();
  let expected:std::collections::BTreeSet<_>=tables.iter().filter_map(|t|t["physical_name"].as_str()).collect();
  if bindings.len()>ceiling as usize || actual!=expected || actual.len()!=bindings.len() || tables.len()!=bindings.len()
   || bindings.iter().any(|b|b.0!="_oo" || b.2!=bindings[0].2) {return Err(StoreError::Input);}
  tx.batch_execute("GRANT USAGE ON SCHEMA oo_publish_target TO oo_target_owner; GRANT SELECT,UPDATE ON oo_publish_target.fence TO oo_target_owner").await.map_err(query_error)?;
 }
 assert_source_not_adopted(&tx,publication).await?;
 let text=serde_json::to_string(&publication.receipt).map_err(|_|StoreError::Input)?;
 let raw:String=tx.query_one(crate::native_publication::publication_sql(&publication.receipt),p!(&publication.run_id,&publication.receipt_sha256,&text)).await.map_err(query_error)?.try_get(0).map_err(|_|StoreError::Input)?;
 let published:Value=serde_json::from_str(&raw).map_err(|_|StoreError::Input)?;
 let mut receipts=Vec::with_capacity(bindings.len());
 for (schema,table,role,columns) in bindings {
  let fence=FenceRef {schema_name:schema,relation_name:table,role_name:role};
  let mut receipt=match open_body(&tx,&fence,columns,pack_digest,version,candidate_id).await {
   Ok(receipt)=>receipt,Err(err)=>return Err(rollback(tx,err).await),
  };
  if receipt.code!=0 || receipt.body["fence_state"]!="open" {let _=tx.rollback().await;return Ok(vec![receipt]);}
  receipt.body["native_publication"]=published.clone();receipts.push(receipt);
 }
 tx.commit().await.map_err(query_error)?;Ok(receipts)
}

async fn open_target_inner(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
    columns: &[String],
    pack_digest: &str,
    version: &str,
    candidate_id: &str,
    publication: Option<&crate::native_publication::Publication>,
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    if !crate::seal::hex64(pack_digest)
        || !crate::seal::token_ok(version)
        || !crate::seal::token_ok(candidate_id)
    {
        return Err(StoreError::Input);
    }
    let columns = clean_columns(columns)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let published = if let Some(publication) = publication {
        assert_source_not_adopted(&tx,publication).await?;
        let receipt = serde_json::to_string(&publication.receipt).map_err(|_| StoreError::Input)?;
        let row = tx
            .query_one(
                crate::native_publication::publication_sql(&publication.receipt),
                p!(&publication.run_id, &publication.receipt_sha256, &receipt),
            )
            .await
            .map_err(query_error)?;
        let text: String = row.try_get(0).map_err(|_| StoreError::Input)?;
        Some(serde_json::from_str::<Value>(&text).map_err(|_| StoreError::Input)?)
    } else {
        None
    };
    let mut receipt =
        match open_body(&tx, fence, &columns, pack_digest, version, candidate_id).await {
            Ok(receipt) => receipt,
            Err(err) => return Err(rollback(tx, err).await),
        };
    if publication.is_some() && (receipt.code != 0 || receipt.body["fence_state"] != "open") {
        let _ = tx.rollback().await;
        return Ok(receipt);
    }
    if let Some(published) = published {
        receipt.body["native_publication"] = published;
    }
    commit(tx, receipt).await
}

pub async fn observe_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
    columns: &[String],
    reader_user: &str,
    reader_password: &str,
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    let columns = clean_columns(columns)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let catalog = match catalog_body(&tx, fence).await {
        Ok(catalog) => catalog,
        Err(err) => return Err(rollback(tx, err).await),
    };
    let policy = match policy_expr(&tx, fence).await {
        Ok(policy) => policy,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(
        tx,
        stamped(
            "observed_admin",
            None,
            json!({"database_used": true}),
            "not_issued",
        ),
    )
    .await?;
    let reader = open(conn.endpoint, reader_user, reader_password, conn.ca_pem).await?;
    require_tls(&reader.client).await?;
    if reader_user != fence.role_name {
        let role = reader.client.query_one(
            "SELECT r.rolcanlogin, r.rolsuper, pg_catalog.pg_has_role(current_user, r.oid, 'SET') FROM pg_catalog.pg_roles r WHERE r.rolname = $1",
            p!(fence.role_name),
        ).await.map_err(query_error)?;
        if role.get::<_, bool>(0) || role.get::<_, bool>(1) || !role.get::<_, bool>(2) {
            return Err(StoreError::LoginRole);
        }
        reader
            .client
            .batch_execute(&format!("SET ROLE {}", quote_ident(fence.role_name)?))
            .await
            .map_err(query_error)?;
    }
    reader_observe(&reader.client, fence, &columns, &catalog, policy.as_deref()).await
}

pub async fn status_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
    pointer_pack: Option<&str>,
    visible: Option<&[String]>,
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let receipt = match status_body(&tx, fence, pointer_pack, visible).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn column_types(
    conn: &TargetConn<'_>,
    schema_name: &str,
    relation_name: &str,
) -> Result<Vec<(String, String)>, StoreError> {
    column_types_inner(conn,schema_name,relation_name,false).await
}

pub(crate) async fn library_column_types(conn:&TargetConn<'_>,schema_name:&str,relation_name:&str)->Result<Vec<(String,String)>,StoreError> {
    column_types_inner(conn,schema_name,relation_name,true).await
}

async fn column_types_inner(conn:&TargetConn<'_>,schema_name:&str,relation_name:&str,formatted:bool)->Result<Vec<(String,String)>,StoreError> {
    sql_ident(schema_name)?;
    sql_ident(relation_name)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let rows = match tx.query(if formatted {LIBRARY_COLUMN_TYPES} else {COLUMN_TYPES}, p!(schema_name, relation_name)).await {
        Ok(rows) => rows,
        Err(err) => return Err(rollback(tx, query_error(err)).await),
    };
    let mut out = Vec::new();
    for row in rows {
        let name: String = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let typname: String = row
            .try_get(1)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        out.push((name, typname));
    }
    commit(
        tx,
        stamped("types", None, json!({"database_used": true}), "not_issued"),
    )
    .await?;
    Ok(out)
}

async fn install_body(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<Receipt, StoreError> {
    install_body_for(tx, fence, false).await
}

async fn install_body_for(tx: &Transaction<'_>, fence: &FenceRef<'_>, library: bool) -> Result<Receipt, StoreError> {
    require_target(tx).await?;
    let relation = tx
        .query_opt(RELATION_EXISTS, p!(fence.schema_name, fence.relation_name))
        .await
        .map_err(query_error)?;
    if relation.is_none() {
        return Ok(refused("relation", json!({"database_used": true})));
    }
    relation_is_overlay(tx, fence).await?;
    let role = tx
        .query_opt(ROLE_EXISTS, p!(fence.role_name))
        .await
        .map_err(query_error)?;
    if role.is_none() {
        return Ok(refused("role", json!({"database_used": true})));
    }
    let hash = crate::sha256::sha256_hex(sql::TARGET_SQL.as_bytes());
    let exists = tx
        .query_opt(TARGET_MANIFEST_TABLE, &[])
        .await
        .map_err(query_error)?
        .is_some();
    if !exists {
        tx.batch_execute(sql::TARGET_SQL)
            .await
            .map_err(query_error)?;
        one(tx, "INSERT INTO oo_publish_target.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)", p!(hash)).await?;
        one(
            tx,
            "INSERT INTO oo_publish_target.fence (singleton, state, role_name, schema_name, relation_name, opened_columns, opened_pack_digest, opened_version, candidate_id, epoch, streams_stopped) VALUES (TRUE, 'closed', $1, $2, $3, '{}', NULL, NULL, NULL, NULL, 0)",
            p!(fence.role_name, fence.schema_name, fence.relation_name),
        )
        .await?;
        install_native_reads(tx, fence).await?;
        return Ok(fence_receipt(
            "installed",
            None,
            "closed",
            &[],
            &[],
            unpublished(hash),
            0,
        ));
    }
    let stored = one_text(
        tx,
        "SELECT sql_sha256 FROM oo_publish_target.install_manifest WHERE singleton",
    )
    .await?;
    if stored != hash {
        return Ok(refused(
            "install_drift",
            json!({"check": "target_hash", "database_used": true}),
        ));
    }
    if !library && tx
        .query_opt(
            // The fixed RLS policy is relation-local; other source relations have independent fences.
            "SELECT 1 FROM oo_publish_target.fence WHERE role_name <> $1 AND schema_name = $2 AND relation_name = $3 LIMIT 1",
            p!(fence.role_name, fence.schema_name, fence.relation_name),
        )
        .await
        .map_err(query_error)?
        .is_some()
    {
        return Err(StoreError::Input);
    }
    tx.execute("INSERT INTO oo_publish_target.fence (singleton,state,role_name,schema_name,relation_name,opened_columns,streams_stopped) VALUES (TRUE,'closed',$1,$2,$3,'{}',0) ON CONFLICT DO NOTHING", p!(fence.role_name, fence.schema_name, fence.relation_name)).await.map_err(query_error)?;
    install_native_reads(tx, fence).await?;
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    let revoked = revoked(tx, fence).await?;
    let published = columns_published(&row.state, &row.opened, &revoked, None)
        && matches!(policy_expr(tx, fence).await?.as_deref(), Some("true" | "(true)"));
    Ok(fence_receipt(
        "replay",
        None,
        &row.state,
        &row.opened,
        &revoked,
        Shown {
            pack: stored,
            published,
        },
        row.streams,
    ))
}

async fn lock_relation(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<(), StoreError> {
    // Views need the same exclusion that heap policy DDL supplies, including
    // prepared readers and replay conflicts with streaming standby queries.
    let mode = if relation_is_overlay(tx, fence).await? { "ACCESS EXCLUSIVE" } else { "SHARE ROW EXCLUSIVE" };
    exec_ident(
        tx,
        &format!("LOCK TABLE {} IN {mode} MODE", table(fence)?),
        &[],
    )
    .await
}

async fn close_body(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<Receipt, StoreError> {
    require_target(tx).await?;
    relation_is_overlay(tx, fence).await?;
    bind_row(tx, fence).await?;
    // Stop old snapshots (including SET ROLE logins) before waiting on their locks.
    let streams = terminate_role(tx, fence.role_name).await?;
    lock_relation(tx, fence).await?;
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    revoke_acl(tx, fence, false).await?;
    revoke_acl(tx, fence, true).await?;
    set_row_fence(tx, fence, false).await?;
    write_fence(tx, fence, "closed", &row.opened, closed_pointer(), streams).await?;
    let revoked = revoked(tx, fence).await?;
    Ok(fence_receipt(
        "closed",
        None,
        "closed",
        &row.opened,
        &revoked,
        unpublished(String::new()),
        streams,
    ))
}

async fn grant_body(
    tx: &Transaction<'_>,
    fence: &FenceRef<'_>,
    columns: &[String],
) -> Result<Receipt, StoreError> {
    require_target(tx).await?;
    lock_relation(tx, fence).await?;
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    if row.state == "open" {
        return Ok(refused("fence_open", json!({"database_used": true})));
    }
    let known = names(tx, fence).await?;
    if columns
        .iter()
        .any(|column| !known.iter().any(|name| name == column))
    {
        return Ok(refused("column", json!({"database_used": true})));
    }
    let revoked = revoked(tx, fence).await?;
    let granted: Vec<String> = columns
        .iter()
        .filter(|column| !revoked.iter().any(|item| item == *column))
        .cloned()
        .collect();
    if granted.is_empty() {
        return Ok(refused(
            "column",
            json!({"revoked_columns": revoked, "database_used": true}),
        ));
    }
    set_row_fence(tx, fence, false).await?;
    exec_ident(tx, &grant_sql(fence, &granted)?, &[]).await?;
    write_fence(
        tx,
        fence,
        "granted",
        &row.opened,
        FencePointer {
            pack: None,
            version: None,
            candidate: row.candidate.as_deref(),
            epoch: None,
        },
        row.streams,
    )
    .await?;
    Ok(fence_receipt(
        "granted",
        None,
        "granted",
        &granted,
        &revoked,
        unpublished(String::new()),
        row.streams,
    ))
}

async fn revoke_body(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<Receipt, StoreError> {
    require_target(tx).await?;
    lock_relation(tx, fence).await?;
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    if row.state == "open" {
        return Ok(refused("fence_open", json!({"database_used": true})));
    }
    revoke_acl(tx, fence, false).await?;
    set_row_fence(tx, fence, false).await?;
    write_fence(
        tx,
        fence,
        "closed",
        &row.opened,
        closed_pointer(),
        row.streams,
    )
    .await?;
    let revoked = revoked(tx, fence).await?;
    Ok(fence_receipt(
        "revoked",
        None,
        "closed",
        &row.opened,
        &revoked,
        unpublished(String::new()),
        row.streams,
    ))
}

async fn open_body(
    tx: &Transaction<'_>,
    fence: &FenceRef<'_>,
    columns: &[String],
    pack_digest: &str,
    version: &str,
    candidate_id: &str,
) -> Result<Receipt, StoreError> {
    require_target(tx).await?;
    lock_relation(tx, fence).await?;
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    if std::env::var("OO_W73_OPEN_FAULT").ok().as_deref() == Some("1") {
        tx.query_one("SELECT 1/0", &[]).await.map_err(query_error)?;
    }
    if row.state == "open"
        && row.pack.as_deref() == Some(pack_digest)
        && row.version.as_deref() == Some(version)
    {
        let policy = policy_expr(tx, fence).await?;
        if !matches!(policy.as_deref(), Some("true" | "(true)")) {
            return Ok(refused("fence_mismatch", json!({"policy": policy, "database_used": true})));
        }
        let revoked = revoked(tx, fence).await?;
        let published = columns_published("open", &row.opened, &revoked, Some(columns));
        let mut receipt = fence_receipt(
            "replay",
            None,
            "open",
            &row.opened,
            &revoked,
            Shown {
                pack: pack_digest.to_owned(),
                published,
            },
            row.streams,
        );
        receipt.body["policy"] = json!(policy);
        return Ok(receipt);
    }
    let known = names(tx, fence).await?;
    if columns
        .iter()
        .any(|column| !known.iter().any(|name| name == column))
    {
        return Ok(refused("column", json!({"database_used": true})));
    }
    let mut revoked = revoked(tx, fence).await?;
    for previous in &row.opened {
        if !columns.iter().any(|column| column == previous)
            && !revoked.iter().any(|column| column == previous)
        {
            tx.execute(
                "INSERT INTO oo_publish_target.revoked_column (role_name, column_name, revoked_at_version, schema_name, relation_name) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                p!(fence.role_name, previous, version, fence.schema_name, fence.relation_name),
            )
            .await
            .map_err(query_error)?;
            revoked.push(previous.clone());
        }
    }
    revoked.sort();
    let grant_cols: Vec<String> = columns
        .iter()
        .filter(|column| !revoked.iter().any(|item| item == *column))
        .cloned()
        .collect();
    revoke_acl(tx, fence, false).await?;
    revoke_acl(tx, fence, true).await?;
    set_row_fence(tx, fence, !grant_cols.is_empty()).await?;
    if grant_cols.is_empty() {
        let empty: Vec<String> = Vec::new();
        write_fence(tx, fence, "closed", &empty, closed_pointer(), row.streams).await?;
        return Ok(fence_receipt(
            "closed",
            None,
            "closed",
            &[],
            &revoked,
            unpublished(String::new()),
            row.streams,
        ));
    }
    exec_ident(tx, &grant_sql(fence, &grant_cols)?, &[]).await?;
    write_fence(
        tx,
        fence,
        "open",
        &grant_cols,
        FencePointer {
            pack: Some(pack_digest),
            version: Some(version),
            candidate: Some(candidate_id),
            epoch: None,
        },
        row.streams,
    )
    .await?;
    let published = columns_published("open", &grant_cols, &revoked, Some(columns));
    let mut receipt = fence_receipt(
        "opened",
        None,
        "open",
        &grant_cols,
        &revoked,
        Shown {
            pack: pack_digest.to_owned(),
            published,
        },
        row.streams,
    );
    receipt.body["policy"] = json!(policy_expr(tx, fence).await?);
    Ok(receipt)
}

async fn status_body(
    tx: &Transaction<'_>,
    fence: &FenceRef<'_>,
    pointer_pack: Option<&str>,
    visible: Option<&[String]>,
) -> Result<Receipt, StoreError> {
    require_target(tx).await?;
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    let revoked = revoked(tx, fence).await?;
    let policy = policy_expr(tx, fence).await?;
    let pack_matches =
        row.state == "open" && row.pack.as_deref().is_some() && row.pack.as_deref() == pointer_pack;
    let published = pack_matches && columns_published(&row.state, &row.opened, &revoked, visible)
        && matches!(policy.as_deref(), Some("true" | "(true)"));
    let mut receipt = fence_receipt(
        "status",
        None,
        &row.state,
        &row.opened,
        &revoked,
        Shown {
            pack: row.pack.clone().unwrap_or_default(),
            published,
        },
        row.streams,
    );
    receipt.body["policy"] = json!(policy);
    Ok(receipt)
}

fn columns_published(
    state: &str,
    opened: &[String],
    revoked: &[String],
    visible: Option<&[String]>,
) -> bool {
    let Some(visible) = visible else {
        return false;
    };
    state == "open"
        && opened.len() == visible.len()
        && visible
            .iter()
            .all(|column| opened.iter().any(|opened| opened == column))
        && visible
            .iter()
            .all(|column| !revoked.iter().any(|revoked| revoked == column))
}

async fn catalog_body(
    tx: &Transaction<'_>,
    fence: &FenceRef<'_>,
) -> Result<Vec<String>, StoreError> {
    require_target(tx).await?;
    bind_row(tx, fence).await?;
    names(tx, fence).await
}

async fn reader_observe(
    client: &tokio_postgres::Client,
    fence: &FenceRef<'_>,
    sealed: &[String],
    catalog: &[String],
    policy: Option<&str>,
) -> Result<Receipt, StoreError> {
    if !matches!(policy, Some("false" | "(false)")) {
        return Ok(refused(
            "fence_open",
            json!({"policy": policy, "database_used": true}),
        ));
    }
    // A qualified name in an expression is table.column, and a bound `$1::regclass` is typed regclass. The checked identifier is a string literal.
    let privilege_sql = format!(
        "SELECT has_column_privilege(current_user, '{}'::regclass, $1, 'SELECT')",
        table(fence)?
    );
    let mut sealed_ok = Vec::new();
    for column in sealed {
        let row = client
            .query_one(&privilege_sql, p!(column))
            .await
            .map_err(query_error)?;
        let allowed: bool = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        if !allowed {
            return Ok(refused(
                "fence_mismatch",
                json!({"column": column, "database_used": true}),
            ));
        }
        sealed_ok.push(column.clone());
    }
    let mut hidden_denied = Vec::new();
    for column in catalog {
        if sealed.iter().any(|item| item == column) {
            continue;
        }
        let row = client
            .query_one(&privilege_sql, p!(column))
            .await
            .map_err(query_error)?;
        let allowed: bool = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        if allowed {
            return Ok(refused(
                "fence_mismatch",
                json!({"hidden": column, "database_used": true}),
            ));
        }
        hidden_denied.push(column.clone());
    }
    let count_sql = format!("SELECT count(*) FROM {}", table(fence)?);
    let count = match client.query_one(&count_sql, &[]).await {
        Ok(row) => row
            .try_get::<_, i64>(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        Err(err) => {
            let mapped = query_error(err);
            if matches!(&mapped, StoreError::Query { sqlstate: Some(state) } if state == "42501") {
                return Ok(refused("privilege_absent", json!({"database_used": true})));
            }
            return Err(mapped);
        }
    };
    if count != 0 {
        return Ok(refused(
            "fence_mismatch",
            json!({"row_count": count, "database_used": true}),
        ));
    }
    Ok(stamped(
        "observed",
        None,
        json!({
            "policy": "false",
            "row_count": count,
            "sealed_columns": sealed_ok,
            "hidden_denied": hidden_denied,
            "reader": fence.role_name,
            "compiled_pack": "not_published",
            "knowledge_published": false,
            "database_used": true,
            "standby_configured": false,
        }),
        "not_issued",
    ))
}

async fn bind_row(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<(), StoreError> {
    let row = tx.query_one(
        "SELECT role_name, schema_name, relation_name FROM oo_publish_target.fence WHERE role_name = $1 AND schema_name = $2 AND relation_name = $3",
        p!(fence.role_name, fence.schema_name, fence.relation_name),
    )
    .await
    .map_err(query_error)?;
    let role: String = row
        .try_get(0)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let schema: String = row
        .try_get(1)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let relation: String = row
        .try_get(2)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    if role != fence.role_name || schema != fence.schema_name || relation != fence.relation_name {
        return Err(StoreError::Input);
    }
    Ok(())
}

async fn lock_fence(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<FenceRow, StoreError> {
    let row = tx
        .query_one(
            "SELECT state, opened_columns, opened_pack_digest, opened_version, candidate_id, streams_stopped FROM oo_publish_target.fence WHERE role_name = $1 AND schema_name = $2 AND relation_name = $3 FOR UPDATE",
            p!(fence.role_name, fence.schema_name, fence.relation_name),
        )
        .await
        .map_err(query_error)?;
    Ok(FenceRow {
        state: row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        opened: row
            .try_get(1)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        pack: row
            .try_get(2)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        version: row
            .try_get(3)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        candidate: row
            .try_get(4)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        streams: row
            .try_get(5)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
    })
}

struct FencePointer<'a> {
    pack: Option<&'a str>,
    version: Option<&'a str>,
    candidate: Option<&'a str>,
    epoch: Option<&'a str>,
}

struct Shown {
    pack: String,
    published: bool,
}

fn closed_pointer() -> FencePointer<'static> {
    FencePointer {
        pack: None,
        version: None,
        candidate: None,
        epoch: None,
    }
}

fn unpublished(pack: String) -> Shown {
    Shown {
        pack,
        published: false,
    }
}

async fn write_fence(
    tx: &Transaction<'_>,
    fence: &FenceRef<'_>,
    state: &str,
    opened: &[String],
    pointer: FencePointer<'_>,
    streams: i32,
) -> Result<(), StoreError> {
    one(
        tx,
        "UPDATE oo_publish_target.fence SET state = $1, opened_columns = $2, opened_pack_digest = $3, opened_version = $4, candidate_id = $5, epoch = $6, streams_stopped = $7 WHERE role_name = $8 AND schema_name = $9 AND relation_name = $10",
        p!(state, opened, pointer.pack, pointer.version, pointer.candidate, pointer.epoch, streams, fence.role_name, fence.schema_name, fence.relation_name),
    )
    .await
}

async fn revoked(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<Vec<String>, StoreError> {
    let rows = tx.query("SELECT column_name FROM oo_publish_target.revoked_column WHERE role_name = $1 AND schema_name = $2 AND relation_name = $3 ORDER BY column_name", p!(fence.role_name, fence.schema_name, fence.relation_name)).await.map_err(query_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(
            row.try_get(0)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
        );
    }
    Ok(out)
}

async fn names(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<Vec<String>, StoreError> {
    let rows = tx
        .query(COLUMN_TYPES, p!(fence.schema_name, fence.relation_name))
        .await
        .map_err(query_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(
            row.try_get(0)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
        );
    }
    Ok(out)
}

async fn policy_expr(
    tx: &Transaction<'_>,
    fence: &FenceRef<'_>,
) -> Result<Option<String>, StoreError> {
    if relation_is_overlay(tx, fence).await? {
        // Observe the real gate, not a fabricated pg_policy row for a view.
        let visible: bool = tx.query_one(
            "SELECT _oo.c5_overlay_visible($1,$2::text::name)",
            p!(fence.relation_name, fence.role_name),
        ).await.map_err(query_error)?.get(0);
        return Ok(Some(visible.to_string()));
    }
    let row = tx
        .query_opt(
            POLICY_EXPR,
            p!(fence.schema_name, fence.relation_name, POLICY),
        )
        .await
        .map_err(query_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let expr: Option<String> = row
        .try_get(0)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    Ok(expr.map(|text| text.trim().to_owned()))
}

async fn terminate_role(tx: &Transaction<'_>, role: &str) -> Result<i32, StoreError> {
    // A prepared transaction has no backend to kill; never attest it was closed.
    let prepared: bool = tx.query_one(
        "SELECT EXISTS(SELECT FROM pg_catalog.pg_prepared_xacts \
         WHERE database=current_database() AND pg_catalog.pg_has_role(owner,$1::text,'SET'))",
        p!(role),
    ).await.map_err(query_error)?.get(0);
    if prepared { return Err(StoreError::Input); }
    let rows = tx
        .query(
            "SELECT a.pid FROM pg_catalog.pg_stat_activity a JOIN pg_catalog.pg_roles r ON r.rolname=a.usename \
             WHERE (a.usename=$1 OR (NOT r.rolsuper AND a.usename NOT IN ('oo_target_admin','oo_target_owner') \
             AND pg_catalog.pg_has_role(a.usename,$1::text,'SET'))) \
             AND a.datname=current_database() AND a.pid<>pg_catalog.pg_backend_pid()",
            p!(role),
        )
        .await
        .map_err(query_error)?;
    let mut stopped = 0_i32;
    for row in rows {
        let pid: i32 = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let killed = tx
            .query_one("SELECT pg_catalog.pg_terminate_backend($1, 5000)", p!(pid))
            .await
            .map_err(query_error)?;
        let ok: bool = killed
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        if ok {
            stopped += 1;
        } else {
            return Err(StoreError::Input);
        }
    }
    Ok(stopped)
}

async fn require_target(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let user = one_text(tx, "SELECT current_user").await?;
    if user == TARGET_ROLE {
        Ok(())
    } else {
        Err(StoreError::LoginRole)
    }
}

fn fence_receipt(
    outcome: &str,
    reason: Option<&str>,
    state: &str,
    opened: &[String],
    revoked: &[String],
    shown: Shown,
    streams: i32,
) -> Receipt {
    let published = shown.published && state == "open";
    let class = if published {
        "test_scope_not_production"
    } else {
        "not_issued"
    };
    stamped(
        outcome,
        reason,
        json!({
            "fence_state": state,
            "opened_columns": opened,
            "revoked_columns": revoked,
            "opened_pack_digest": if shown.pack.is_empty() { Value::Null } else { Value::String(shown.pack) },
            "compiled_pack": "not_published",
            "knowledge_published": published,
            "streams_stopped": streams,
            "database_used": true,
        }),
        class,
    )
}

fn checked(fence: &FenceRef<'_>) -> Result<(), StoreError> {
    sql_ident(fence.schema_name)?;
    sql_ident(fence.relation_name)?;
    sql_ident(fence.role_name)?;
    if !fence.role_name.starts_with("oo_r_") {
        return Err(StoreError::Input);
    }
    Ok(())
}

fn clean_columns(columns: &[String]) -> Result<Vec<String>, StoreError> {
    let mut out = Vec::new();
    for column in columns {
        sql_ident(column)?;
        if !out.iter().any(|item: &String| item == column) {
            out.push(column.clone());
        }
    }
    out.sort();
    if out.is_empty() {
        Err(StoreError::Input)
    } else {
        Ok(out)
    }
}

fn quote_ident(value: &str) -> Result<String, StoreError> {
    sql_ident(value)?;
    Ok(format!("\"{value}\""))
}

fn sql_ident(value: &str) -> Result<(), StoreError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return Err(StoreError::Input);
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return Err(StoreError::Input);
    }
    if bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        Ok(())
    } else {
        Err(StoreError::Input)
    }
}

fn table(fence: &FenceRef<'_>) -> Result<String, StoreError> {
    Ok(format!(
        "{}.{}",
        quote_ident(fence.schema_name)?,
        quote_ident(fence.relation_name)?
    ))
}

fn revoke_all(fence: &FenceRef<'_>) -> Result<String, StoreError> {
    Ok(format!(
        "REVOKE ALL ON TABLE {} FROM {}",
        table(fence)?,
        quote_ident(fence.role_name)?
    ))
}

fn revoke_public(fence: &FenceRef<'_>) -> Result<String, StoreError> {
    Ok(format!("REVOKE ALL ON TABLE {} FROM PUBLIC", table(fence)?))
}

async fn revoke_acl(tx: &Transaction<'_>, fence: &FenceRef<'_>, public: bool) -> Result<(), StoreError> {
    exec_ident(tx, &if public { revoke_public(fence)? } else { revoke_all(fence)? }, &[]).await?;
    // Table-level REVOKE does not remove independent column grants.
    let columns = names(tx, fence).await?.iter().map(|c| quote_ident(c))
        .collect::<Result<Vec<_>, _>>()?.join(", ");
    if !columns.is_empty() {
        let role = if public { "PUBLIC".to_owned() } else { quote_ident(fence.role_name)? };
        exec_ident(tx, &format!("REVOKE ALL ({columns}) ON TABLE {} FROM {role}", table(fence)?), &[]).await?;
    }
    Ok(())
}

async fn relation_is_overlay(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<bool, StoreError> {
    let row = tx.query_opt(RELATION_EXISTS, p!(fence.schema_name, fence.relation_name))
        .await.map_err(query_error)?.ok_or(StoreError::Input)?;
    if row.get::<_, String>(0) == "r" { return Ok(false); }
    if fence.schema_name != "_oo" || !native_shadow(fence.relation_name, "pt_")
        || !tx.query_one("SELECT to_regprocedure('_oo.c5_overlay_visible(text,name)') IS NOT NULL", &[])
            .await.map_err(query_error)?.get::<_, bool>(0)
        || !tx.query_one("SELECT _oo.c5_overlay_is_valid($1)", p!(fence.relation_name))
            .await.map_err(query_error)?.get::<_, bool>(0)
    {
        return Err(StoreError::Input);
    }
    Ok(true)
}

async fn set_row_fence(tx: &Transaction<'_>, fence: &FenceRef<'_>, visible: bool) -> Result<(), StoreError> {
    if relation_is_overlay(tx, fence).await? {
        // A boolean-only ACL latch also fences snapshots born during close.
        let gate = quote_ident(&format!("pv_{}", &fence.relation_name[3..]))?;
        let role = quote_ident(fence.role_name)?;
        exec_ident(tx, &format!("REVOKE ALL ON FUNCTION _oo.{gate}() FROM PUBLIC"), &[]).await?;
        let statement = if visible {
            format!("GRANT EXECUTE ON FUNCTION _oo.{gate}() TO {role}")
        } else {
            format!("REVOKE ALL ON FUNCTION _oo.{gate}() FROM {role}")
        };
        return exec_ident(tx, &statement, &[]).await;
    }
    exec_ident(tx, &format!("ALTER TABLE {} ENABLE ROW LEVEL SECURITY", table(fence)?), &[]).await?;
    exec_ident(tx, &format!("ALTER TABLE {} FORCE ROW LEVEL SECURITY", table(fence)?), &[]).await?;
    exec_ident(tx, &format!("DROP POLICY IF EXISTS {POLICY} ON {}", table(fence)?), &[]).await?;
    exec_ident(tx, &format!("CREATE POLICY {POLICY} ON {} FOR SELECT TO {} USING ({visible})",
        table(fence)?, quote_ident(fence.role_name)?), &[]).await
}

async fn install_native_reads(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<(), StoreError> {
    if fence.schema_name != "_oo" || !native_shadow(fence.relation_name, "pt_") { return Ok(()); }
    let overlay = relation_is_overlay(tx, fence).await?;
    let relations: Vec<String> = if overlay {
        tx.query(
            "SELECT root_name FROM _oo.c5_overlay WHERE physical_name=$1 AND root_name IS NOT NULL \
             UNION SELECT d.delta_name FROM _oo.c5_overlay_component c JOIN _oo.c5_overlay d \
             ON d.physical_name=c.delta_owner WHERE c.physical_name=$1",
            p!(fence.relation_name),
        ).await.map_err(query_error)?.iter().map(|r| r.get(0)).collect()
    } else { vec![fence.relation_name.to_owned()] };
    for relation in relations {
        let owned: bool = tx.query_one(
            "SELECT EXISTS(SELECT FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname='_oo' AND c.relname=$1 AND c.relkind='r' AND c.relowner='oo_target_owner'::regrole)",
            p!(&relation),
        ).await.map_err(query_error)?.get(0);
        if !owned { return Err(StoreError::Input); }
        let relation = format!("_oo.{}", quote_ident(&relation)?);
        exec_ident(tx, &format!("DROP POLICY IF EXISTS oo_internal_read ON {relation}"), &[]).await?;
        exec_ident(tx, &format!(
            "CREATE POLICY oo_internal_read ON {relation} FOR SELECT TO oo_target_owner USING (true)"
        ), &[]).await?;
    }
    // All table workers share these ACL rows; avoid concurrent catalog rewrites.
    const INTERNAL_ACCESS: &str = "SELECT \
        pg_catalog.has_schema_privilege('oo_target_owner','oo_publish_target','USAGE') \
        AND pg_catalog.has_table_privilege('oo_target_owner','oo_publish_target.fence','SELECT')";
    if tx.query_one(INTERNAL_ACCESS, &[]).await.map_err(query_error)?.get::<_,bool>(0) {
        return Ok(());
    }
    tx.query_one("SELECT sql_sha256 FROM oo_publish_target.install_manifest WHERE singleton FOR UPDATE", &[])
        .await.map_err(query_error)?;
    if !tx.query_one(INTERNAL_ACCESS, &[]).await.map_err(query_error)?.get::<_,bool>(0) {
        // The helper owns the read; end users only receive its boolean EXECUTE.
        tx.batch_execute("GRANT USAGE ON SCHEMA oo_publish_target TO oo_target_owner; \
            GRANT SELECT ON oo_publish_target.fence TO oo_target_owner").await.map_err(query_error)?;
    }
    Ok(())
}

fn grant_sql(fence: &FenceRef<'_>, columns: &[String]) -> Result<String, StoreError> {
    let cols = columns
        .iter()
        .map(|column| quote_ident(column))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!(
        "GRANT SELECT ({cols}) ON TABLE {} TO {}",
        table(fence)?,
        quote_ident(fence.role_name)?
    ))
}

async fn exec_ident(
    tx: &Transaction<'_>,
    statement: &str,
    params: &[&(dyn postgres_types::ToSql + Sync)],
) -> Result<(), StoreError> {
    tx.execute(statement, params).await.map_err(query_error)?;
    Ok(())
}

async fn one(
    tx: &Transaction<'_>,
    statement: &str,
    params: &[&(dyn postgres_types::ToSql + Sync)],
) -> Result<(), StoreError> {
    let count = tx.execute(statement, params).await.map_err(query_error)?;
    if count == 1 {
        Ok(())
    } else {
        Err(StoreError::Query { sqlstate: None })
    }
}

async fn one_text(tx: &Transaction<'_>, statement: &str) -> Result<String, StoreError> {
    let row = tx.query_one(statement, &[]).await.map_err(query_error)?;
    row.try_get(0)
        .map_err(|_| StoreError::Query { sqlstate: None })
}

const COLUMN_TYPES: &str = "\
SELECT a.attname, t.typname \
FROM pg_catalog.pg_attribute a \
JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";
const LIBRARY_COLUMN_TYPES: &str = "\
SELECT a.attname, pg_catalog.format_type(a.atttypid,a.atttypmod) \
FROM pg_catalog.pg_attribute a \
JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";

const RELATION_EXISTS: &str = "\
SELECT c.relkind::text FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r','v')";

const ROLE_EXISTS: &str = "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1";

const TARGET_MANIFEST_TABLE: &str = "\
SELECT 1 FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'oo_publish_target' AND c.relname = 'install_manifest'";

const POLICY_EXPR: &str = "\
SELECT pg_catalog.pg_get_expr(p.polqual, p.polrelid) \
FROM pg_catalog.pg_policy p \
JOIN pg_catalog.pg_class c ON c.oid = p.polrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND p.polname = $3";

/// Permanently retain removal history before revoking a dataset.
pub async fn retire_target(
    conn: &TargetConn<'_>,
    fence: &FenceRef<'_>,
    version: &str,
) -> Result<Receipt, StoreError> {
    checked(fence)?;
    if !crate::seal::token_ok(version) {
        return Err(StoreError::Input);
    }
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    bind_row(&tx, fence).await?;
    if lock_fence(&tx, fence).await?.state != "closed" {
        // Retrying retirement must not kill readers of the new generation.
        let closed = close_body(&tx, fence).await?;
        if closed.code != 0 {
            return commit(tx, closed).await;
        }
    }
    let row = lock_fence(&tx, fence).await?;
    for column in &row.opened {
        tx.execute("INSERT INTO oo_publish_target.revoked_column(role_name,schema_name,relation_name,column_name,revoked_at_version) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",p!(fence.role_name,fence.schema_name,fence.relation_name,column,version)).await.map_err(query_error)?;
    }
    let receipt = revoke_body(&tx, fence).await?;
    commit(tx, receipt).await
}

pub fn validate_native_mapping(value: &Value) -> Result<(&str, &str, i64), StoreError> {
    let o = value.as_object().ok_or(StoreError::Input)?;
    if o.len() != 3 {
        return Err(StoreError::Input);
    }
    let source = o
        .get("source")
        .and_then(Value::as_str)
        .ok_or(StoreError::Input)?;
    let digest = o
        .get("config_digest")
        .and_then(Value::as_str)
        .ok_or(StoreError::Input)?;
    let generation = o
        .get("generation")
        .and_then(Value::as_i64)
        .ok_or(StoreError::Input)?;
    if !crate::seal::token_ok(source) || !crate::seal::hex64(digest) || generation < 1 {
        return Err(StoreError::Input);
    }
    Ok((source, digest, generation))
}

pub fn validate_native_context(value: &Value) -> Result<(), StoreError> {
    if let Some(entries) = value.get("datasets") {
        if value.as_object().is_none_or(|o| {
            o.keys()
                .any(|key| !["datasets", "publication"].contains(&key.as_str()))
        }) {
            return Err(StoreError::Input);
        }
        let entries = entries.as_array().ok_or(StoreError::Input)?;
        if entries.is_empty() || (entries.len() > 256 && value.get("publication").is_none_or(|p|!crate::native_library::is_library(&p["receipt"]))) {
            return Err(StoreError::Input);
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in entries {
            let mut context = entry.as_object().ok_or(StoreError::Input)?.clone();
            let dataset = context
                .remove("dataset")
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or(StoreError::Input)?;
            if dataset.split_once('.').is_none() || !seen.insert(dataset) {
                return Err(StoreError::Input);
            }
            validate_native_mapping(&Value::Object(context))?;
        }
        crate::native_publication::validate_context(value)?;
        Ok(())
    } else {
        validate_native_mapping(value).map(|_| ())
    }
}

pub(crate) struct ValidatedNativeContext<'a> {
    mappings: std::collections::BTreeMap<&'a str, (&'a str, &'a str, i64)>,
    default_mapping: Option<(&'a str, &'a str, i64)>,
    publication: Option<crate::native_publication::Publication>,
    pending_runs: std::collections::BTreeSet<(String, i64)>,
}

impl<'a> ValidatedNativeContext<'a> {
    pub(crate) fn new(mapping: &'a Value) -> Result<Self, StoreError> {
        validate_native_context(mapping)?;
        let mut mappings = std::collections::BTreeMap::new();
        let default_mapping = if let Some(entries) = mapping["datasets"].as_array() {
            for entry in entries {
                mappings.insert(
                    entry["dataset"].as_str().ok_or(StoreError::Input)?,
                    (entry["source"].as_str().ok_or(StoreError::Input)?,
                     entry["config_digest"].as_str().ok_or(StoreError::Input)?,
                     entry["generation"].as_i64().ok_or(StoreError::Input)?),
                );
            }
            None
        } else { Some(validate_native_mapping(mapping)?) };
        // The full validator above already verified all receipt hashes and cross-links.
        // Retain that immutable publication instead of revalidating it for each table.
        let publication = mapping.get("publication").map(|p| -> Result<_,StoreError> { Ok(crate::native_publication::Publication {
            run_id:p["run_id"].as_str().ok_or(StoreError::Input)?.to_owned(),
            receipt_sha256:p["receipt_sha256"].as_str().ok_or(StoreError::Input)?.to_owned(),
            receipt:p["receipt"].clone(),
        }) }).transpose()?;
        let mut pending_runs = std::collections::BTreeSet::new();
        if let Some(publication) = &publication {
            if crate::native_library::is_library(&publication.receipt)
                || crate::native_publication::per_table_collection(&publication.receipt) {
                for table in crate::native_library::tables(&publication.receipt)? {
                    pending_runs.insert((
                        table["run_id"].as_str().ok_or(StoreError::Input)?.to_owned(),
                        table["target_generation"].as_i64().ok_or(StoreError::Input)?,
                    ));
                }
            } else {
                let generation=publication.receipt["target_generation"].as_i64().ok_or(StoreError::Input)?;
                if let Some(runs)=publication.receipt["run_ids"].as_array() {
                    for run in runs {
                        pending_runs.insert((run.as_str().ok_or(StoreError::Input)?.to_owned(),generation));
                    }
                } else { pending_runs.insert((publication.run_id.clone(),generation)); }
            }
        }
        Ok(Self {mappings,default_mapping,publication,pending_runs})
    }

    fn mapping_for(&self, namespace:&str, table:&str)->Result<(&'a str,&'a str,i64),StoreError> {
        self.default_mapping.or_else(||self.mappings.get(format!("{namespace}.{table}").as_str()).copied())
            .ok_or(StoreError::Input)
    }

    pub(crate) fn publication(&self)->Option<&crate::native_publication::Publication> {
        self.publication.as_ref()
    }

    fn permits_pending(&self,run:&str,generation:i64)->bool {
        self.pending_runs.contains(&(run.to_owned(),generation))
    }
}

/// Resolve logical policy fields through the actual imported mapping, never a client-provided physical name.
pub async fn resolve_native_binding(
    conn: &TargetConn<'_>,
    mapping: &Value,
    b: &crate::apply::VisibilityBinding,
) -> Result<crate::apply::VisibilityBinding, StoreError> {
    let validated = ValidatedNativeContext::new(mapping)?;
    resolve_validated_native_binding(conn,&validated,b).await
}

pub(crate) async fn resolve_validated_native_binding(
    conn:&TargetConn<'_>,
    context:&ValidatedNativeContext<'_>,
    b:&crate::apply::VisibilityBinding,
)->Result<crate::apply::VisibilityBinding,StoreError> {
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await.map_err(|e|e.at(Phase::NativeConnect))?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let resolved = resolve_native_binding_body(&tx, context, b).await?;
    tx.commit().await.map_err(|e|query_error(e).at(Phase::NativeCommit))?;
    Ok(resolved)
}

pub(crate) async fn resolve_native_binding_body(
    tx: &Transaction<'_>,
    context: &ValidatedNativeContext<'_>,
    b: &crate::apply::VisibilityBinding,
) -> Result<crate::apply::VisibilityBinding, StoreError> {
    let (source,digest,generation)=context.mapping_for(&b.0,&b.1)?;
    let pending: Option<(String, i64)> = tx
        .query_opt(
            "SELECT run_id::text, target_generation FROM _oo.full_import_run \
         WHERE source_id = $1 AND source_generation = $2 AND state = 'awaiting_publish' \
         ORDER BY target_generation DESC LIMIT 1",
            p!(source, generation),
        )
        .await
        .map_err(|e|query_error(e).at(Phase::NativePending))?
        .map(|row| (row.get(0), row.get(1)));
    if let Some((run_id, target_generation)) = pending {
        if !context.permits_pending(&run_id,target_generation) {
            return Err(StoreError::Input);
        }
    }
    let empty = "";
    let namespace = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1,$2,$3,'namespace',$4,$5,$6)",
            p!(source, digest, generation, b.0, empty, empty),
        )
        .await
        .map_err(|e|query_error(e).at(Phase::NativeNamespace))?
        .try_get::<_, String>(0)
        .map_err(|e|query_error(e).at(Phase::NativeNamespace))?;
    if !native_shadow(&namespace, "pn_") {
        return Err(StoreError::Input);
    }
    let table = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1,$2,$3,'table',$4,$5,$6)",
            p!(source, digest, generation, b.0, b.1, empty),
        )
        .await
        .map_err(|e|query_error(e).at(Phase::NativeTable))?
        .try_get::<_, String>(0)
        .map_err(|e|query_error(e).at(Phase::NativeTable))?;
    if !native_shadow(&table, "pt_") {
        return Err(StoreError::Input);
    }
    let stored = tx.query(
        "SELECT n.nspname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE c.relname=$1 AND c.relkind IN ('r','v') AND (n.nspname=$2 OR n.nspname='_oo')",
        p!(table, namespace),
    ).await.map_err(|e|query_error(e).at(Phase::NativeRelation))?;
    if stored.len() != 1 {
        return Err(StoreError::Input);
    }
    let namespace: String = stored[0].try_get(0).map_err(|e|query_error(e).at(Phase::NativeRelation))?;
    let fence = FenceRef { schema_name: &namespace, relation_name: &table, role_name: &b.2 };
    if relation_is_overlay(tx, &fence).await? {
        let exact: bool = tx.query_one(
            "SELECT EXISTS(SELECT FROM _oo.c5_overlay o JOIN _oo.full_import_run r USING(run_id) \
             WHERE o.physical_name=$1 AND r.source_id=$2 AND r.config_digest=$3 AND r.source_generation=$4 \
             AND o.scope->>'schema'=$5 AND o.scope->>'table'=$6)",
            p!(table, source, digest, generation, b.0, b.1),
        ).await.map_err(|e|query_error(e).at(Phase::NativeRelation))?.get(0);
        if !exact { return Err(StoreError::Input); }
    }
    // Keep the authoritative lookup for every column, but issue one database
    // round trip per table and preserve the caller's column order.
    let mapped = tx.query(
        "SELECT _oo.lookup_physical_mapping($1,$2,$3,'column',$4,$5,n.name) \
         FROM unnest($6::text[]) WITH ORDINALITY AS n(name,ordinal) ORDER BY n.ordinal",
        p!(source, digest, generation, b.0, b.1, b.3),
    ).await.map_err(|e|query_error(e).at(Phase::NativeColumns))?;
    if mapped.len() != b.3.len() {
        return Err(StoreError::Input);
    }
    let mut columns = Vec::with_capacity(mapped.len());
    for row in mapped {
        let column = row.try_get::<_, String>(0).map_err(|e|query_error(e).at(Phase::NativeColumns))?;
        if !native_shadow(&column, "pc_") || columns.contains(&column) {
            return Err(StoreError::Input);
        }
        columns.push(column);
    }
    Ok((namespace, table, b.2.clone(), columns))
}
fn native_shadow(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|tail| {
        tail.len() == 60
            && tail
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

#[cfg(test)]
mod replica_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "owned physical standby and private credentials required"]
    async fn owned_replica_replays_and_rejects_primary() {
        let path = std::env::var("OO_NATIVE_STANDBY_TEST_CONFIG").expect("private config path");
        let config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let endpoints = ["primary", "replica"].map(|name| Endpoint {
            pack_ceilings: None,
            host: config[name]["host"].as_str().unwrap().to_owned(), port: 5432,
            database: config["database"].as_str().unwrap().to_owned(),
        });
        let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
        let cas = ["primary", "replica"].map(|name| std::fs::read(config[name]["ca_file"].as_str().unwrap()).unwrap());
        let primary = TargetConn {endpoint:&endpoints[0], user:"oo_target_admin", password:password.trim(), ca_pem:&cas[0]};
        let replica = TargetConn {endpoint:&endpoints[1], user:"oo_target_admin", password:password.trim(), ca_pem:&cas[1]};
        if std::env::var("OO_EXPECT_REPLICA_BEHIND").as_deref()==Ok("1") {
            assert!(replica_barrier(&primary,&replica).await.is_err());
            println!("OWNED_REPLICA_RAW paused behind standby refused after bounded actual replay wait");
            return;
        }
        let observed = replica_barrier(&primary, &replica).await.expect("real standby replay");
        assert_eq!(observed["barrier"], "replayed");
        assert_eq!(observed["in_recovery"], true);
        assert_eq!(observed["transaction_read_only"], true);
        assert!(replica_barrier(&primary, &primary).await.is_err());
        assert!(replica_barrier(&replica, &primary).await.is_err());
        println!("OWNED_REPLICA_RAW {}", observed);
    }
}

#[cfg(test)]
mod multi_role_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "owned target with two prepared roles and relations required"]
    async fn owned_distinct_relations_keep_role_fences_and_acl_isolated() {
        let path = std::env::var("OO_NATIVE_MULTI_ROLE_TEST_CONFIG").expect("private config");
        let cfg: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let endpoint = Endpoint { pack_ceilings:None,host:cfg["host"].as_str().unwrap().into(), port:5432,
            database:cfg["database"].as_str().unwrap().into() };
        let password=std::fs::read_to_string(cfg["password_file"].as_str().unwrap()).unwrap();
        let ca=std::fs::read(cfg["ca_file"].as_str().unwrap()).unwrap();
        let conn=TargetConn {endpoint:&endpoint,user:cfg["user"].as_str().unwrap(),password:password.trim(),ca_pem:&ca};
        let fences:Vec<FenceRef<'_>>=cfg["bindings"].as_array().unwrap().iter().map(|b|FenceRef {
            schema_name:b["schema"].as_str().unwrap(),relation_name:b["table"].as_str().unwrap(),role_name:b["role"].as_str().unwrap()}).collect();
        assert_eq!(fences.len(),2);
        assert_ne!(fences[0].role_name,fences[1].role_name);
        assert_ne!((fences[0].schema_name,fences[0].relation_name),(fences[1].schema_name,fences[1].relation_name));
        assert_eq!(install_target(&conn,&fences[0]).await.unwrap().code,0);
        let session=open(conn.endpoint,conn.user,conn.password,conn.ca_pem).await.unwrap();
        let query="SELECT jsonb_build_object('fences',(SELECT jsonb_agg(to_jsonb(f) ORDER BY schema_name,relation_name) FROM oo_publish_target.fence f WHERE role_name=$1),'acl',(SELECT relacl::text FROM pg_class WHERE oid=to_regclass(format('%I.%I',$2::text,$3::text))))::text";
        let first=&fences[0];
        let before:String=session.client.query_one(query,p!(first.role_name,first.schema_name,first.relation_name)).await.unwrap().get(0);
        assert_eq!(install_target(&conn,&fences[1]).await.unwrap().code,0);
        let after:String=session.client.query_one(query,p!(first.role_name,first.schema_name,first.relation_name)).await.unwrap().get(0);
        assert_eq!(before,after,"unrelated role fence/ACL changed");
        let conflicting=FenceRef {schema_name:first.schema_name,relation_name:first.relation_name,role_name:fences[1].role_name};
        assert!(install_target(&conn,&conflicting).await.is_err());
        let final_state:String=session.client.query_one(query,p!(first.role_name,first.schema_name,first.relation_name)).await.unwrap().get(0);
        assert_eq!(before,final_state,"rejected role changed relation state");
        println!("OWNED_MULTI_ROLE_RAW distinct relation installed; original fence/ACL unchanged; same relation foreign role refused unchanged");
    }
}

pub async fn install_visibility_reader(
    conn: &TargetConn<'_>,
    reader: &str,
) -> Result<(), StoreError> {
    let reader = quote_ident(reader)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "10s", "15s").await?;
    tx.batch_execute("SET LOCAL statement_timeout = '5s'")
        .await
        .map_err(query_error)?;
    require_target(&tx).await?;
    if tx
        .query_one(
            "SELECT to_regprocedure('oo_publish_target.visible_vector()') IS NULL",
            &[],
        )
        .await
        .map_err(query_error)?
        .get::<_, bool>(0)
    {
        tx.batch_execute(include_str!("../migrations/oo_publish_visibility_v1.sql"))
            .await
            .map_err(query_error)?;
    }
    tx.batch_execute(include_str!("../migrations/oo_publish_visibility_v2.sql")).await.map_err(query_error)?;
    tx.batch_execute(&format!("GRANT USAGE ON SCHEMA oo_publish_target TO {reader}; GRANT EXECUTE ON FUNCTION oo_publish_target.visible_vector() TO {reader}")).await.map_err(query_error)?;
    tx.commit().await.map_err(query_error)
}

pub struct VisibilityRef<'a> {
    pub bindings: &'a [crate::apply::VisibilityBinding],
    pub version: &'a str,
    pub pack: &'a str,
    pub candidate: &'a str,
    pub mapping: Option<&'a Value>,
}

pub async fn primary_visibility_barrier(
    conn: &TargetConn<'_>,
    reader_user: &str,
    reader_password: &str,
    expected: VisibilityRef<'_>,
) -> Result<Value, StoreError> {
    visibility_barrier(conn,reader_user,reader_password,expected,false).await
}

pub async fn replica_visibility_barrier(conn:&TargetConn<'_>,reader_user:&str,reader_password:&str,expected:VisibilityRef<'_>)->Result<Value,StoreError> {
    visibility_barrier(conn,reader_user,reader_password,expected,true).await
}

async fn visibility_barrier(conn:&TargetConn<'_>,reader_user:&str,reader_password:&str,expected:VisibilityRef<'_>,replica:bool)->Result<Value,StoreError> {
    let VisibilityRef {
        bindings,
        version,
        pack,
        candidate,
        mapping,
    } = expected;
    let mut session = open(conn.endpoint, reader_user, reader_password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    session
        .client
        .batch_execute("SET default_transaction_read_only = on; SET statement_timeout = '5s'")
        .await
        .map_err(query_error)?;
    let login = session.client.query_one("SELECT rolcanlogin AND NOT (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls), NOT pg_is_in_recovery() FROM pg_catalog.pg_roles WHERE rolname=session_user", &[]).await.map_err(query_error)?;
    if !login.get::<_, bool>(0) || login.get::<_, bool>(1) == replica {
        return Err(StoreError::LoginRole);
    }
    let tx = session
        .client
        .build_transaction()
        .read_only(true)
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await
        .map_err(query_error)?;
    let mut vector = Vec::new();
    for (schema, relation, role, columns) in bindings {
        let row = tx.query_one("SELECT opened_columns, opened_pack_digest, opened_version, candidate_id FROM oo_publish_target.visible_vector() WHERE role_name=$1 AND schema_name=$2 AND relation_name=$3", p!(role, schema, relation)).await.map_err(query_error)?;
        if row.get::<_, Vec<String>>(0) != *columns
            || row.get::<_, String>(1) != pack
            || row.get::<_, String>(2) != version
            || row.get::<_, String>(3) != candidate
        {
            return Err(StoreError::Input);
        }
        let policy = policy_expr(&tx, &FenceRef {
            schema_name: schema, relation_name: relation, role_name: role,
        }).await?;
        if !matches!(policy.as_deref(), Some("true" | "(true)")) {
            return Err(StoreError::Input);
        }
        vector.push(json!({"schema":schema,"relation":relation,"role":role,"columns":columns,"version":version,"pack_digest":pack}));
    }
    if let Some(mapping) = mapping {
        let contexts: Vec<Value> = if let Some(entries) = mapping["datasets"].as_array() {
            entries.iter().map(|entry| json!({"source":entry["source"],"config_digest":entry["config_digest"],"generation":entry["generation"]})).collect()
        } else {
            vec![mapping.clone()]
        };
        let admin = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
        require_tls(&admin.client).await?;
        for context in contexts {
            let (source, digest, generation) = validate_native_mapping(&context)?;
            let sql = "SELECT COALESCE(jsonb_agg(to_jsonb(m) ORDER BY level,logical_namespace,logical_table,logical_column),'[]'::jsonb)::text FROM _oo.physical_mappings m WHERE source=$1 AND config_digest=$2 AND run_generation=$3";
            let expected: String = admin
                .client
                .query_one(sql, p!(source, digest, generation))
                .await
                .map_err(query_error)?
                .get(0);
            let observed: String = tx
                .query_one(sql, p!(source, digest, generation))
                .await
                .map_err(query_error)?
                .get(0);
            if expected == "[]" || observed != expected {
                return Err(StoreError::Input);
            }
            let mappings: Value = serde_json::from_str(&observed).map_err(|_| StoreError::Input)?;
            for entry in mappings.as_array().ok_or(StoreError::Input)? {
                let word = match entry["level"].as_str() {
                    Some("n") => "namespace",
                    Some("t") => "table",
                    Some("c") => "column",
                    _ => return Err(StoreError::Input),
                };
                let namespace = entry["logical_namespace"]
                    .as_str()
                    .ok_or(StoreError::Input)?;
                let table = entry["logical_table"].as_str().ok_or(StoreError::Input)?;
                let column = entry["logical_column"].as_str().ok_or(StoreError::Input)?;
                let looked: String = tx
                    .query_one(
                        "SELECT _oo.lookup_physical_mapping($1,$2,$3,$4,$5,$6,$7)",
                        p!(source, digest, generation, word, namespace, table, column),
                    )
                    .await
                    .map_err(query_error)?
                    .get(0);
                if entry["physical_name"] != looked {
                    return Err(StoreError::Input);
                }
            }
            vector.push(json!({"source":source,"config_digest":digest,"generation":generation,"mappings":mappings}));
        }
        if let Some(publication) = crate::native_publication::from_context(mapping)? {
            for table in crate::native_library::tables(&publication.receipt)?
            {
                let physical = table["physical_name"].as_str().ok_or(StoreError::Input)?;
                let row = tx.query_one("SELECT r.source_generation,r.target_generation,r.state FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id) WHERE t.physical_name=$1 AND r.run_id=$2::text::uuid", p!(physical, table["run_id"].as_str().unwrap_or(&publication.run_id))).await.map_err(query_error)?;
                let source_generation = table["source_generation"]
                    .as_i64()
                    .or(publication.receipt["source_generation"].as_i64())
                    .ok_or(StoreError::Input)?;
                let target_generation = table["target_generation"]
                    .as_i64()
                    .or(publication.receipt["target_generation"].as_i64())
                    .ok_or(StoreError::Input)?;
                if row.get::<_, i64>(0) != source_generation
                    || row.get::<_, i64>(1) != target_generation
                    || row.get::<_, String>(2) != "published"
                {
                    return Err(StoreError::Input);
                }
                vector.push(json!({"physical_name":physical,"source_generation":source_generation,"target_generation":target_generation}));
            }
        }
    }
    for (schema, relation, role, columns) in bindings {
        tx.batch_execute(&format!("SET LOCAL ROLE {}", quote_ident(role)?))
            .await
            .map_err(query_error)?;
        let selected = columns
            .iter()
            .map(|column| quote_ident(column))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        tx.query(
            &format!(
                "SELECT {selected} FROM {}.{} LIMIT 1",
                quote_ident(schema)?,
                quote_ident(relation)?
            ),
            &[],
        )
        .await
        .map_err(query_error)?;
        tx.batch_execute("RESET ROLE").await.map_err(query_error)?;
    }
    tx.commit().await.map_err(query_error)?;
    Ok(
        json!({"barrier":"visible","deployment_mode":if replica {"primary_replica"} else {"all_in_one"},"readonly_replica":replica,"reader":reader_user,"version":version,"pack_digest":pack,"vector":vector}),
    )
}

pub async fn prepare_library_target(conn: &TargetConn<'_>, fence: &FenceRef<'_>) -> Result<Receipt,StoreError> {
    checked(fence)?;
    let mut session=open(conn.endpoint,conn.user,conn.password,conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx=begin_for(&mut session,"60s","60s").await?;
    require_target(&tx).await?;
    let unsafe_role=tx.query_opt("SELECT rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolinherit OR rolbypassrls OR rolreplication FROM pg_catalog.pg_roles WHERE rolname=$1",p!(fence.role_name)).await.map_err(query_error)?.map(|row|row.get::<_,bool>(0));
    if let Some(receipt)=library_role_guard(unsafe_role)? {return commit(tx,receipt).await;}
    if tx.query_opt(TARGET_MANIFEST_TABLE,&[]).await.map_err(query_error)?.is_none() {
        let receipt=install_body_for(&tx,fence,true).await?;
        ensure_library_schema_usage(&tx, fence).await?;
        return commit(tx,receipt).await;
    }
    let previous=tx.query("SELECT role_name FROM oo_publish_target.fence WHERE schema_name=$1 AND relation_name=$2 AND role_name<>$3 ORDER BY role_name FOR UPDATE",p!(fence.schema_name,fence.relation_name,fence.role_name)).await.map_err(query_error)?;
    for row in previous {
        let role:String=row.get(0);
        close_body(&tx,&FenceRef{schema_name:fence.schema_name,relation_name:fence.relation_name,role_name:&role}).await?;
    }
    let receipt=install_body_for(&tx,fence,true).await?;
    ensure_library_schema_usage(&tx, fence).await?;
    commit(tx,receipt).await
}

fn library_role_guard(unsafe_role:Option<bool>)->Result<Option<Receipt>,StoreError> {
    match unsafe_role {
        None=>Ok(Some(refused("role",json!({"database_used":true,"fence_touched":false})))),
        Some(true)=>Err(StoreError::Input),
        Some(false)=>Ok(None),
    }
}

async fn ensure_library_schema_usage(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<(), StoreError> {
    const HAS_USAGE: &str = "SELECT pg_catalog.has_schema_privilege($1::text,$2::text,'USAGE')";
    let allowed: bool = tx.query_one(HAS_USAGE, p!(fence.role_name, fence.schema_name))
        .await.map_err(query_error)?.get(0);
    if allowed { return Ok(()); }
    // Different table workers can share one namespace. Serialize only its
    // initial ACL update; repeating GRANT concurrently causes PostgreSQL XX000.
    tx.query_one("SELECT sql_sha256 FROM oo_publish_target.install_manifest WHERE singleton FOR UPDATE", &[])
        .await.map_err(query_error)?;
    let allowed: bool = tx.query_one(HAS_USAGE, p!(fence.role_name, fence.schema_name))
        .await.map_err(query_error)?.get(0);
    if !allowed {
        tx.batch_execute(&format!("GRANT USAGE ON SCHEMA {} TO {}",
            quote_ident(fence.schema_name)?, quote_ident(fence.role_name)?))
            .await.map_err(query_error)?;
    }
    Ok(())
}

pub async fn library_state(conn:&TargetConn<'_>,publication:&crate::native_publication::Publication,ready:bool)->Result<(),StoreError> {
    if !crate::native_library::is_library(&publication.receipt) {return Ok(());}
    let session=open(conn.endpoint,conn.user,conn.password,conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let library=publication.receipt["library"]["library_id"].as_str().ok_or(StoreError::Input)?;
    let sql=if ready {"SELECT _oo.native_library_mark_ready($1,$2)"} else {"SELECT _oo.native_library_fail_closed($1,$2)"};
    session.client.execute(sql,p!(library,&publication.receipt_sha256)).await.map_err(query_error)?;
    Ok(())
}

pub async fn verify_library_visibility(
    primary: &TargetConn<'_>,
    reader_target: &TargetConn<'_>,
    reader_user: &str,
    reader_password: &str,
    expected: VisibilityRef<'_>,
    publication: &crate::native_publication::Publication,
    replica: bool,
) -> Result<Value, StoreError> {
    if !crate::native_library::is_library(&publication.receipt) { return Err(StoreError::Input); }
    let result=async {
        let mut observed=visibility_barrier(reader_target,reader_user,reader_password,expected,replica).await?;
        library_state(primary,publication,true).await?;
        if replica { observed["ready_replay"]=replica_barrier(primary,reader_target).await?; }
        observed["library_id"]=publication.receipt["library"]["library_id"].clone();
        observed["library_receipt_sha256"]=json!(publication.receipt_sha256);
        Ok(observed)
    }.await;
    if result.is_err() { library_state(primary,publication,false).await?; }
    result
}

async fn assert_source_not_adopted(tx:&Transaction<'_>,publication:&crate::native_publication::Publication)->Result<(),StoreError> {
    if crate::native_library::is_library(&publication.receipt) {return Ok(());}
    if tx.query_one("SELECT to_regclass('_oo.native_library_member') IS NOT NULL",&[]).await.map_err(query_error)?.get::<_,bool>(0) {
        let source=publication.receipt["source_id"].as_str().ok_or(StoreError::Input)?;
        if tx.query_opt("SELECT 1 FROM _oo.native_library_member WHERE source_id=$1",p!(source)).await.map_err(query_error)?.is_some() {return Err(StoreError::Input);}
    }
    Ok(())
}

pub(crate) async fn library_semantic_observations(
    conn:&TargetConn<'_>,effective:&Value,logical:&[(String,String,String,Vec<String>)],physical:&[(String,String,String,Vec<String>)],
    max_relationships: usize,
)->Result<(crate::apply::Measured,crate::apply::Measured),StoreError> {
    if effective["relationships"].as_array().is_some_and(Vec::is_empty) && effective["enums"].as_array().is_some_and(Vec::is_empty) {
        return Ok((crate::apply::measure_g3(effective),crate::apply::measure_g4(effective)));
    }
    if logical.len()!=physical.len() {return Err(StoreError::Input);}
    let mut names=std::collections::BTreeMap::new();
    for (l,p) in logical.iter().zip(physical) {
        if l.3.len()!=p.3.len() {return Err(StoreError::Input);}
        for (lc,pc) in l.3.iter().zip(&p.3) {
            sql_ident(&p.0)?;sql_ident(&p.1)?;sql_ident(pc)?;
            if names.insert(format!("{}.{}.{}",l.0,l.1,lc),(p.0.clone(),p.1.clone(),pc.clone())).is_some() {
                return Err(StoreError::Input);
            }
        }
    }
    let mut session=open(conn.endpoint,conn.user,conn.password,conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx=begin_for(&mut session,"60s","60s").await?;
    tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY").await.map_err(query_error)?;
    let settings=tx.query_one("SELECT current_user::text,current_setting('transaction_read_only'),current_database()::text,pg_current_snapshot()::text,transaction_timestamp()::text",&[]).await.map_err(query_error)?;
    if settings.get::<_,String>(0)!=conn.user || settings.get::<_,String>(1)!="on" {return Err(StoreError::Input);}
    let observer=json!({"user":conn.user,"host":conn.endpoint.host,"port":conn.endpoint.port,
        "database":settings.get::<_,String>(2),"snapshot":settings.get::<_,String>(3),
        "observed_at":settings.get::<_,String>(4),"isolation":"repeatable_read","read_only":true});
    let relations_to_check: std::collections::BTreeSet<_> =
        names.values().map(|(schema, relation, _)| (schema, relation)).collect();
    for (schema,relation) in relations_to_check {
        let active:bool=tx.query_one("SELECT pg_catalog.row_security_active(c.oid) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2",p!(schema,relation)).await.map_err(query_error)?.get(0);
        if active {tx.rollback().await.map_err(query_error)?;return Ok(crate::apply::unavailable_library_observer());}
    }
    let mut checked=0usize;let mut empty=0usize;let mut failed=0usize;
    let mut parent_uniqueness=std::collections::BTreeMap::new();
    let relations=effective["relationships"].as_array().ok_or(StoreError::Input)?;
    for relation in relations {
        if relation["review"]["state"]!="approved" && relation["provenance"]!="declared" {continue;}
        let from=relation["from"]["columns"].as_array().ok_or(StoreError::Input)?;
        let to=relation["to"]["columns"].as_array().ok_or(StoreError::Input)?;
        if from.len()!=1 || to.len()!=1 {return Err(StoreError::Input);}
        let left=format!("{}.{}",relation["from"]["dataset"].as_str().ok_or(StoreError::Input)?,from[0].as_str().ok_or(StoreError::Input)?);
        let right=format!("{}.{}",relation["to"]["dataset"].as_str().ok_or(StoreError::Input)?,to[0].as_str().ok_or(StoreError::Input)?);
        let (Some(l),Some(r))=(names.get(&left),names.get(&right)) else {continue;};
        library_relationship_budget(checked, max_relationships)?;
        let parent=format!("SELECT count(*)=count(DISTINCT \"{}\") AND count(*)=count(\"{}\") FROM \"{}\".\"{}\"",r.2,r.2,r.0,r.1);
        let unique=if let Some(value)=parent_uniqueness.get(r) {*value} else {
            let value:bool=tx.query_one(&parent,&[]).await.map_err(query_error)?.get(0);
            parent_uniqueness.insert(r.clone(),value);
            value
        };
        let query=library_relationship_query(l,r);
        let row=tx.query_one(&query,&[]).await.map_err(query_error)?;
        let measured=crate::apply::library_relationship_observation(row.get(0),row.get(1),unique);
        checked+=1;if measured.status=="warn" {empty+=1;} else if measured.status!="pass" {failed+=1;}
    }
    let mut enum_observations=Vec::new();
    let mut enum_names=std::collections::BTreeSet::new();
    let enumerations=effective["enums"].as_array().ok_or(StoreError::Input)?;
    for enumeration in enumerations {
        let identity=enumeration["enum"].as_str().ok_or(StoreError::Input)?;
        if !enum_names.insert(identity) {return Err(StoreError::Input);}
        let mut observation=json!({
            "enum":identity,"definition_sha256":crate::canon::canonical_sha(enumeration),
            "status":"not_run","reason":"target_mapping_missing"
        });
        if let Some(p)=names.get(identity) {
            observation["target"]=json!({"schema":p.0,"relation":p.1,"column":p.2});
            match library_enum_codes(enumeration) {
                Ok((codes,approved)) => {
                    let counts = if approved.is_empty() {
                        library_unreviewed_enum_observation()
                    } else {
                        let row=tx.query_one(&library_enum_query(p),p!(&codes,&approved)).await.map_err(query_error)?;
                        library_enum_counts(row.get(0),row.get(1),row.get(2),approved.len())?
                    };
                    observation.as_object_mut().ok_or(StoreError::Input)?
                        .extend(counts.as_object().ok_or(StoreError::Input)?.clone());
                    observation["review_structure"]=json!("valid_declared_review_only");
                }
                Err((status,reason)) => {
                    observation["status"]=json!(status);
                    observation["reason"]=json!(reason);
                }
            }
        } else if enumeration["values"].as_array().is_some_and(|values|
            values.iter().any(|value|value["status"]=="approved")) {
            observation["status"]=json!("fail");
            observation["reason"]=json!("approved_enum_target_mapping_missing");
        }
        enum_observations.push(observation);
    }
    tx.rollback().await.map_err(query_error)?;
    let g3=crate::apply::Measured {gate_id:"G3",status:if failed>0 {"fail"} else {"warn"},blocking:failed>0,detail:format!("independent_target_containment;checked={checked};empty_unmeasured={empty};failed={failed};minimum_row_containment=0.98;fixture_aggregate_validation_only;production_certification=not_issued")};
    let g4=if enumerations.is_empty() {crate::apply::measure_g4(effective)} else {
        library_enum_gate(effective,enum_observations,&observer)
    };
    Ok((g3,g4))
}

fn library_enum_codes(enumeration:&Value)->Result<(Vec<String>,Vec<String>),(&'static str,&'static str)> {
    use oo_core::oos::{EnumFile,EnumValueStatus,ReviewState,validate_enum};
    let definition:EnumFile=serde_json::from_value(enumeration.clone())
        .map_err(|_|("fail","invalid_enum_definition"))?;
    if !validate_enum(&definition).is_empty() {return Err(("fail","invalid_enum_definition"));}
    // This observer has no row-time binding: never union historical and current meanings.
    if definition.values.iter().any(|value|value.valid_from.is_some() || value.valid_to.is_some()) {
        return Err(("not_run","temporal_scope_unbound"));
    }
    let mut codes=std::collections::BTreeSet::new();
    let mut approved=Vec::new();
    for value in &definition.values {
        if !codes.insert(value.code.as_str().to_owned()) {
            return Err(("fail","duplicate_enum_code"));
        }
        if value.status!=EnumValueStatus::Approved {continue;}
        if value.class.as_ref().is_none_or(|class|matches!(class.as_str(),"undetermined"|"unexplained")) {
            return Err(("fail","invalid_approved_class"));
        }
        if definition.owner.as_ref().is_none_or(|owner|owner.as_str().trim().is_empty())
            || value.review.as_ref().is_none_or(|review|review.state!=ReviewState::Approved
                || review.by.as_ref().is_none_or(|by|by.as_str().trim().is_empty())) {
            return Err(("not_run","approved_review_owner_evidence_missing"));
        }
        if value.label.as_ref().is_none_or(|label|label.as_str().trim().is_empty()) {
            return Err(("not_run","approved_label_missing"));
        }
        approved.push(value.code.as_str().to_owned());
    }
    Ok((codes.into_iter().collect(),approved))
}

fn library_unreviewed_enum_observation() -> Value {
    // No approved meaning exists to test. Unknown statistics are not zero rows.
    json!({"status":"warn","reason":"no_approved_semantics_statistics_not_measured",
        "approved_codes":0,"non_null_rows":null,"unlisted_rows":null,"approved_rows":null,
        "listed_unapproved_rows":null,
        "coverage":{"numerator":null,"denominator":null,"defined":false,"threshold_met":null},
        "review_needed":true})
}

fn library_enum_query(target:&(String,String,String))->String {
    format!(
        "SELECT count(\"{column}\")::bigint,\
         count(*) FILTER(WHERE \"{column}\" IS NOT NULL AND NOT (\"{column}\"::text COLLATE \"C\"=ANY($1::text[])))::bigint,\
         count(*) FILTER(WHERE \"{column}\" IS NOT NULL AND \"{column}\"::text COLLATE \"C\"=ANY($2::text[]))::bigint \
         FROM \"{schema}\".\"{relation}\"",
        schema=target.0,relation=target.1,column=target.2,
    )
}

fn library_enum_counts(rows:i64,unlisted:i64,approved:i64,approved_codes:usize)->Result<Value,StoreError> {
    if rows<0 || unlisted<0 || approved<0 || i128::from(unlisted)+i128::from(approved)>i128::from(rows)
        || (approved_codes==0 && approved>0) {
        return Err(StoreError::Input);
    }
    // Integer comparison preserves the exact 95/100 boundary, even for bigint counts.
    let meets=rows>0 && i128::from(approved)*100>=i128::from(rows)*95;
    Ok(json!({
        "status":if rows==0 && approved_codes>0 {"not_run"} else {"warn"},
        "reason":if rows==0 && approved_codes==0 {"no_approved_semantics_empty_field"}
            else if rows==0 {"coverage_undefined_no_non_null_rows"}
            else if meets {"coverage_threshold_met"} else {"coverage_below_threshold"},
        "non_null_rows":rows,"unlisted_rows":unlisted,"approved_rows":approved,"approved_codes":approved_codes,
        "listed_unapproved_rows":rows-unlisted-approved,
        "coverage":{"numerator":approved,"denominator":rows,"defined":rows>0,
            "threshold_met":if rows>0 {Some(meets)} else {None}},
        "review_needed":!meets || approved<rows
    }))
}

fn library_enum_gate(effective:&Value,observations:Vec<Value>,observer:&Value)->crate::apply::Measured {
    let failed=observations.iter().any(|item|item["status"]=="fail");
    let unmeasured=observations.is_empty()
        || observations.iter().any(|item|item["status"]!="warn" && item["status"]!="fail");
    let detail=json!({
        "contract":"oo.release-coordinator/enum-observations/v1",
        "effective_sha256":crate::canon::canonical_sha(effective),
        "observer":observer,
        "scope":"local_target_snapshot_code_coverage_and_review_structure",
        "minimum_approved_row_coverage":{"numerator":95,"denominator":100},
        "coverage_threshold_met":if failed || unmeasured
            || observations.iter().any(|item|item["coverage"]["defined"]!=true) {None} else {
            Some(observations.iter().all(|item|item["coverage"]["threshold_met"]==true))
        },
        "business_label_accuracy":"not_measured","business_owner_signed":false,
        "owner_authenticated":false,"production_certification":"not_issued",
        "enums":observations
    });
    crate::apply::Measured {
        gate_id:"G4",status:if failed {"fail"} else if unmeasured {"not_run"} else {"warn"},
        blocking:failed || unmeasured,
        detail:format!("independent_target_enum_observations;{}",crate::canon::canonical(&detail)),
    }
}

fn library_relationship_budget(checked: usize, maximum: usize) -> Result<(), StoreError> {
    if checked >= maximum { return Err(StoreError::Input); }
    Ok(())
}

fn library_relationship_query(left: &(String,String,String), right: &(String,String,String)) -> String {
    // Preserve row-weighted EXISTS semantics without a correlated scan per row.
    format!(
        "SELECT COALESCE(sum(c.n),0)::bigint,COALESCE(sum(c.n) FILTER(WHERE p.key IS NOT NULL),0)::bigint \
         FROM (SELECT \"{lc}\"::text AS key,count(*)::bigint AS n FROM \"{ls}\".\"{lt}\" \
         WHERE \"{lc}\" IS NOT NULL GROUP BY \"{lc}\"::text) c \
         LEFT JOIN (SELECT DISTINCT \"{rc}\"::text AS key FROM \"{rs}\".\"{rt}\" \
         WHERE \"{rc}\" IS NOT NULL) p ON p.key=c.key",
        ls=left.0,lt=left.1,lc=left.2,rs=right.0,rt=right.1,rc=right.2,
    )
}

#[cfg(test)]
mod library_observation_tests {
    use super::*;

    #[test]
    fn missing_library_role_is_refused_without_weakening_existing_role_checks() {
        let receipt=library_role_guard(None).unwrap().unwrap();
        assert_eq!(receipt.code,2);
        assert_eq!(receipt.body["outcome"],"refused");
        assert_eq!(receipt.body["reason"],"role");
        assert_eq!(receipt.body["fence_touched"],false);
        assert!(matches!(library_role_guard(Some(true)),Err(StoreError::Input)));
        assert!(library_role_guard(Some(false)).unwrap().is_none());
    }

    #[test]
    fn observation_budget_uses_the_installed_pack_ceiling() {
        assert!(library_relationship_budget(1443,32768).is_ok());
        assert!(library_relationship_budget(32768,32768).is_err());
        assert!(library_relationship_budget(0,0).is_err());
    }

    #[test]
    fn grouped_observation_preserves_row_weights_and_deduplicates_parents() {
        let sql=library_relationship_query(
            &("app".into(),"child".into(),"parent_id".into()),
            &("app".into(),"parent".into(),"id".into()));
        assert!(sql.contains("GROUP BY \"parent_id\"::text"));
        assert!(sql.contains("SELECT DISTINCT \"id\"::text AS key"));
        assert!(sql.contains("sum(c.n) FILTER(WHERE p.key IS NOT NULL)"));
        assert!(!sql.contains("EXISTS"));
    }
}

#[cfg(test)]
#[path = "../tests/observations/enum_observations.rs"]
mod enum_observation_tests;

#[cfg(test)]
#[path = "../tests/observations/native_context.rs"]
mod native_context_tests;

#[cfg(test)]
#[path = "../tests/observations/c5_overlay_visibility.rs"]
mod c5_overlay_visibility_tests;
