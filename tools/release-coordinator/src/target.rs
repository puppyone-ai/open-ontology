//! Target-database fence. Privileges are separate transactions from the control CAS.

use serde_json::{Value, json};
use tokio_postgres::Transaction;

use crate::error::{StoreError, query_error};
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
 if bindings.is_empty() || bindings.len()>32 || !crate::seal::hex64(pack_digest)
  || !crate::seal::token_ok(version) || !crate::seal::token_ok(candidate_id) {return Err(StoreError::Input);}
 for (schema,table,role,columns) in bindings {
  checked(&FenceRef {schema_name:schema,relation_name:table,role_name:role})?;clean_columns(columns)?;
 }
 let mut session=open(conn.endpoint,conn.user,conn.password,conn.ca_pem).await?;require_tls(&session.client).await?;
 let tx=begin_for(&mut session,"60s","60s").await?;
 let text=serde_json::to_string(&publication.receipt).map_err(|_|StoreError::Input)?;
 let raw:String=tx.query_one("SELECT _oo.publish_native_c1_collection($1::text::uuid,$2,$3::text::jsonb)::text",p!(&publication.run_id,&publication.receipt_sha256,&text)).await.map_err(query_error)?.try_get(0).map_err(|_|StoreError::Input)?;
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
        let receipt = serde_json::to_string(&publication.receipt).map_err(|_| StoreError::Input)?;
        let row = tx
            .query_one(
                if publication.receipt.get("run_ids").is_some() {
                    "SELECT _oo.publish_native_c1_collection($1::text::uuid, $2, $3::text::jsonb)::text"
                } else { "SELECT _oo.publish_native_full_import($1::text::uuid, $2, $3::text::jsonb)::text" },
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
    sql_ident(schema_name)?;
    sql_ident(relation_name)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let rows = match tx.query(COLUMN_TYPES, p!(schema_name, relation_name)).await {
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
    require_target(tx).await?;
    let relation = tx
        .query_opt(RELATION_EXISTS, p!(fence.schema_name, fence.relation_name))
        .await
        .map_err(query_error)?;
    if relation.is_none() {
        return Ok(refused("relation", json!({"database_used": true})));
    }
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
    if tx
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
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    let revoked = revoked(tx, fence).await?;
    let published = columns_published(&row.state, &row.opened, &revoked, None);
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
    exec_ident(
        tx,
        &format!("LOCK TABLE {} IN SHARE ROW EXCLUSIVE MODE", table(fence)?),
        &[],
    )
    .await
}

async fn close_body(tx: &Transaction<'_>, fence: &FenceRef<'_>) -> Result<Receipt, StoreError> {
    require_target(tx).await?;
    lock_relation(tx, fence).await?;
    bind_row(tx, fence).await?;
    let row = lock_fence(tx, fence).await?;
    exec_ident(tx, &revoke_all(fence)?, &[]).await?;
    exec_ident(tx, &revoke_public(fence)?, &[]).await?;
    exec_ident(
        tx,
        &format!("ALTER TABLE {} ENABLE ROW LEVEL SECURITY", table(fence)?),
        &[],
    )
    .await?;
    exec_ident(
        tx,
        &format!("ALTER TABLE {} FORCE ROW LEVEL SECURITY", table(fence)?),
        &[],
    )
    .await?;
    exec_ident(
        tx,
        &format!("DROP POLICY IF EXISTS {POLICY} ON {}", table(fence)?),
        &[],
    )
    .await?;
    exec_ident(
        tx,
        &format!(
            "CREATE POLICY {POLICY} ON {} FOR SELECT TO {} USING (false)",
            table(fence)?,
            quote_ident(fence.role_name)?
        ),
        &[],
    )
    .await?;
    let streams = terminate_role(tx, fence.role_name).await?;
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
    exec_ident(tx, &revoke_all(fence)?, &[]).await?;
    exec_ident(
        tx,
        &format!("DROP POLICY IF EXISTS {POLICY} ON {}", table(fence)?),
        &[],
    )
    .await?;
    exec_ident(
        tx,
        &format!(
            "CREATE POLICY {POLICY} ON {} FOR SELECT TO {} USING (false)",
            table(fence)?,
            quote_ident(fence.role_name)?
        ),
        &[],
    )
    .await?;
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
        receipt.body["policy"] = json!(policy_expr(tx, fence).await?);
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
    exec_ident(tx, &revoke_all(fence)?, &[]).await?;
    exec_ident(tx, &revoke_public(fence)?, &[]).await?;
    exec_ident(
        tx,
        &format!("DROP POLICY IF EXISTS {POLICY} ON {}", table(fence)?),
        &[],
    )
    .await?;
    if grant_cols.is_empty() {
        exec_ident(
            tx,
            &format!(
                "CREATE POLICY {POLICY} ON {} FOR SELECT TO {} USING (false)",
                table(fence)?,
                quote_ident(fence.role_name)?
            ),
            &[],
        )
        .await?;
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
    exec_ident(
        tx,
        &format!(
            "CREATE POLICY {POLICY} ON {} FOR SELECT TO {} USING (true)",
            table(fence)?,
            quote_ident(fence.role_name)?
        ),
        &[],
    )
    .await?;
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
    let pack_matches =
        row.state == "open" && row.pack.as_deref().is_some() && row.pack.as_deref() == pointer_pack;
    let published = pack_matches && columns_published(&row.state, &row.opened, &revoked, visible);
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
    receipt.body["policy"] = json!(policy_expr(tx, fence).await?);
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
    let rows = tx
        .query(
            "SELECT pid FROM pg_catalog.pg_stat_activity WHERE usename = $1 AND datname = current_database() AND pid <> pg_catalog.pg_backend_pid()",
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
            .query_one("SELECT pg_catalog.pg_terminate_backend($1)", p!(pid))
            .await
            .map_err(query_error)?;
        let ok: bool = killed
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        if ok {
            stopped += 1;
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

const RELATION_EXISTS: &str = "\
SELECT 1 FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind = 'r'";

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
        if entries.is_empty() || entries.len() > 256 {
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
/// Resolve logical policy fields through the actual imported mapping, never a client-provided physical name.
pub async fn resolve_native_binding(
    conn: &TargetConn<'_>,
    mapping: &Value,
    b: &crate::apply::VisibilityBinding,
) -> Result<crate::apply::VisibilityBinding, StoreError> {
    validate_native_context(mapping)?;
    let context = if let Some(entries) = mapping.get("datasets").and_then(Value::as_array) {
        let dataset = format!("{}.{}", b.0, b.1);
        let entry = entries
            .iter()
            .find(|e| e["dataset"].as_str() == Some(dataset.as_str()))
            .ok_or(StoreError::Input)?;
        let mut object = entry.as_object().ok_or(StoreError::Input)?.clone();
        object.remove("dataset");
        Value::Object(object)
    } else {
        mapping.clone()
    };
    let (source, digest, generation) = validate_native_mapping(&context)?;
    let publication = crate::native_publication::from_context(mapping)?;
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin_for(&mut session, "60s", "60s").await?;
    let pending: Option<(String, i64)> = tx
        .query_opt(
            "SELECT run_id::text, target_generation FROM _oo.full_import_run \
         WHERE source_id = $1 AND source_generation = $2 AND state = 'awaiting_publish' \
         ORDER BY target_generation DESC LIMIT 1",
            p!(source, generation),
        )
        .await
        .map_err(query_error)?
        .map(|row| (row.get(0), row.get(1)));
    if let Some((run_id, target_generation)) = pending {
        if publication.as_ref().is_none_or(|receipt| {
            (if let Some(runs)=receipt.receipt["run_ids"].as_array() {
                !runs.iter().any(|id| id.as_str()==Some(run_id.as_str()))
            } else { receipt.run_id != run_id })
                || receipt.receipt["target_generation"].as_i64() != Some(target_generation)
        }) {
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
        .map_err(query_error)?
        .try_get::<_, String>(0)
        .map_err(|_| StoreError::Input)?;
    if !native_shadow(&namespace, "pn_") {
        return Err(StoreError::Input);
    }
    let table = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1,$2,$3,'table',$4,$5,$6)",
            p!(source, digest, generation, b.0, b.1, empty),
        )
        .await
        .map_err(query_error)?
        .try_get::<_, String>(0)
        .map_err(|_| StoreError::Input)?;
    if !native_shadow(&table, "pt_") {
        return Err(StoreError::Input);
    }
    let stored = tx.query(
        "SELECT n.nspname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE c.relname=$1 AND c.relkind='r' AND (n.nspname=$2 OR n.nspname='_oo')",
        p!(table, namespace),
    ).await.map_err(query_error)?;
    if stored.len() != 1 {
        return Err(StoreError::Input);
    }
    let namespace: String = stored[0].try_get(0).map_err(|_| StoreError::Input)?;
    let mut columns = Vec::new();
    for name in &b.3 {
        let column = tx
            .query_one(
                "SELECT _oo.lookup_physical_mapping($1,$2,$3,'column',$4,$5,$6)",
                p!(source, digest, generation, b.0, b.1, name),
            )
            .await
            .map_err(query_error)?
            .try_get::<_, String>(0)
            .map_err(|_| StoreError::Input)?;
        if !native_shadow(&column, "pc_") || columns.contains(&column) {
            return Err(StoreError::Input);
        }
        columns.push(column);
    }
    tx.commit().await.map_err(query_error)?;
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
        let endpoint = Endpoint { host:cfg["host"].as_str().unwrap().into(), port:5432,
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
