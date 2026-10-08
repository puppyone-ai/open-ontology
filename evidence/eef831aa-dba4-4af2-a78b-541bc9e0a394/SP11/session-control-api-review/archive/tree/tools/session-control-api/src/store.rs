//! Durable rows in schema oo_session_api. This is not the session-head reducer.

use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Transaction};

use session_store::Endpoint;

use crate::error::Fault;
use crate::sha256::{hex, sha256};
use crate::sql::{self, API_ADMIN};

#[derive(Clone, Debug)]
pub struct HttpRow {
    pub principal_id: String,
    pub project_id: String,
    pub role_name: String,
    pub claims_hash: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub config_version: String,
    pub csrf_sha256: String,
}

#[derive(Clone, Debug)]
pub struct ConversationRow {
    pub project_id: String,
    pub conversation_id: String,
    pub principal_id: String,
    pub session_id: String,
}

#[derive(Clone, Debug)]
pub struct ResourceRow {
    pub project_id: String,
    pub session_id: String,
    pub conversation_id: String,
    pub principal_id: String,
    pub role_name: String,
    pub claims_hash: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub config_version: String,
    pub workspace_id: String,
    pub event_prefix: String,
    pub pool_slot: String,
    pub permit_id: String,
    pub mapping_id: String,
    pub token_sha256: String,
    pub application_name: String,
    pub backend_pid: Option<i32>,
    pub clock_ms: i64,
    pub stage: String,
    pub harness_pid: Option<i64>,
    pub claims_json: String,
}

#[derive(Clone, Debug)]
pub struct EventRow {
    pub seq: i32,
    pub event_id: String,
    pub kind: String,
    pub body: String,
}

pub struct Db<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
}

pub async fn connect(db: &Db<'_>) -> Result<Client, Fault> {
    let tls = session_store::rustls_connector(db.ca_pem).map_err(|_| Fault::Unavailable("tls"))?;
    let mut config = tokio_postgres::Config::new();
    config.user(db.user);
    config.password(db.password);
    config.host(&db.endpoint.host);
    config.port(db.endpoint.port);
    config.dbname(&db.endpoint.database);
    config.ssl_mode(SslMode::Require);
    let (client, connection) = config.connect(tls).await.map_err(|_| Fault::Unavailable("connect"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

async fn begin(client: &mut Client) -> Result<Transaction<'_>, Fault> {
    let tx = client.transaction().await.map_err(|_| Fault::Unavailable("begin"))?;
    tx.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::Unavailable("begin"))?;
    Ok(tx)
}

fn sqlstate(err: &tokio_postgres::Error) -> Option<String> {
    err.code().map(|code| code.code().to_owned())
}

pub async fn install(db: &Db<'_>) -> Result<&'static str, Fault> {
    if db.user != API_ADMIN {
        return Err(Fault::Forbidden("login_role"));
    }
    let hash = {
        let digest = sha256(sql::MIGRATION_SQL.as_bytes()).ok_or(Fault::Malformed("migration"))?;
        hex(&digest)
    };
    let mut client = connect(db).await?;
    let tx = begin(&mut client).await?;
    tx.execute("SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)", &[&sql::INSTALL_LOCK])
        .await
        .map_err(|_| Fault::Unavailable("lock"))?;
    let exists: bool = tx
        .query_one("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1)", &[&sql::SCHEMA])
        .await
        .map_err(|_| Fault::Unavailable("query"))?
        .get(0);
    if exists {
        let stored: Result<String, _> = tx
            .query_one("SELECT sql_sha256 FROM oo_session_api.install_manifest WHERE singleton = TRUE", &[])
            .await
            .map(|row| row.get(0));
        let stored = stored.map_err(|_| Fault::Conflict("install_drift"))?;
        if stored != hash {
            let _ = tx.rollback().await;
            return Err(Fault::Conflict("install_drift"));
        }
        tx.commit().await.map_err(|_| Fault::Unavailable("commit"))?;
        return Ok("replay");
    }
    tx.batch_execute(sql::MIGRATION_SQL).await.map_err(|_| Fault::Unavailable("migration"))?;
    tx.execute("INSERT INTO oo_session_api.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)", &[&hash])
        .await
        .map_err(|_| Fault::Unavailable("migration"))?;
    tx.commit().await.map_err(|_| Fault::Unavailable("commit"))?;
    Ok("installed")
}

pub async fn insert_grant(db: &Db<'_>, grant_sha256: &str, principal_id: &str, project_id: &str) -> Result<(), Fault> {
    let mut client = connect(db).await?;
    let tx = begin(&mut client).await?;
    let result = tx
        .execute(
            "INSERT INTO oo_session_api.entry_grant (grant_sha256, principal_id, project_id, consumed) VALUES ($1, $2, $3, FALSE)",
            &[&grant_sha256, &principal_id, &project_id],
        )
        .await;
    match result {
        Ok(_) => {
            tx.commit().await.map_err(|_| Fault::Unavailable("commit"))?;
            Ok(())
        }
        Err(err) if sqlstate(&err).as_deref() == Some("23505") => {
            let _ = tx.rollback().await;
            Err(Fault::Conflict("grant_exists"))
        }
        Err(_) => {
            let _ = tx.rollback().await;
            Err(Fault::Unavailable("query"))
        }
    }
}

pub async fn consume_grant(db: &Db<'_>, grant_sha256: &str, session: &HttpRow, session_sha256: &str) -> Result<(), Fault> {
    let mut client = connect(db).await?;
    let tx = begin(&mut client).await?;
    let row = tx
        .query_opt(
            "SELECT principal_id, project_id, consumed FROM oo_session_api.entry_grant WHERE grant_sha256 = $1 FOR UPDATE",
            &[&grant_sha256],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    let Some(row) = row else {
        let _ = tx.rollback().await;
        return Err(Fault::Unauthenticated);
    };
    let principal: String = row.get(0);
    let project: String = row.get(1);
    let consumed: bool = row.get(2);
    if consumed {
        let _ = tx.rollback().await;
        return Err(Fault::Conflict("grant_consumed"));
    }
    if principal != session.principal_id || project != session.project_id {
        let _ = tx.rollback().await;
        return Err(Fault::Forbidden("binding_mismatch"));
    }
    tx.execute("UPDATE oo_session_api.entry_grant SET consumed = TRUE WHERE grant_sha256 = $1", &[&grant_sha256])
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    tx.execute(
        "INSERT INTO oo_session_api.http_session (session_sha256, csrf_sha256, principal_id, project_id, role_name, claims_hash, policy_version, knowledge_version, config_version, revoked) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,FALSE)",
        &[
            &session_sha256,
            &session.csrf_sha256,
            &session.principal_id,
            &session.project_id,
            &session.role_name,
            &session.claims_hash,
            &session.policy_version,
            &session.knowledge_version,
            &session.config_version,
        ],
    )
    .await
    .map_err(|_| Fault::Unavailable("query"))?;
    tx.commit().await.map_err(|_| Fault::Unavailable("commit"))?;
    Ok(())
}

pub async fn load_grant(db: &Db<'_>, grant_sha256: &str) -> Result<(String, String), Fault> {
    let client = connect(db).await?;
    let row = client
        .query_opt(
            "SELECT principal_id, project_id, consumed FROM oo_session_api.entry_grant WHERE grant_sha256 = $1",
            &[&grant_sha256],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    let Some(row) = row else {
        return Err(Fault::Unauthenticated);
    };
    let consumed: bool = row.get(2);
    if consumed {
        return Err(Fault::Conflict("grant_consumed"));
    }
    Ok((row.get(0), row.get(1)))
}

pub async fn load_session(db: &Db<'_>, session_sha256: &str) -> Result<HttpRow, Fault> {
    let client = connect(db).await?;
    let row = client
        .query_opt(
            "SELECT principal_id, project_id, role_name, claims_hash, policy_version, knowledge_version, config_version, csrf_sha256, revoked FROM oo_session_api.http_session WHERE session_sha256 = $1",
            &[&session_sha256],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    let Some(row) = row else {
        return Err(Fault::Unauthenticated);
    };
    let revoked: bool = row.get(8);
    if revoked {
        return Err(Fault::Unauthenticated);
    }
    Ok(HttpRow {
        principal_id: row.get(0),
        project_id: row.get(1),
        role_name: row.get(2),
        claims_hash: row.get(3),
        policy_version: row.get(4),
        knowledge_version: row.get(5),
        config_version: row.get(6),
        csrf_sha256: row.get(7),
    })
}

pub async fn held_permits(db: &Db<'_>, project_id: &str, principal_id: &str) -> Result<i64, Fault> {
    let client = connect(db).await?;
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM oo_session_api.permit WHERE project_id = $1 AND principal_id = $2 AND state = 'held'",
            &[&project_id, &principal_id],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?
        .get(0);
    Ok(count)
}

pub async fn insert_created(
    db: &Db<'_>,
    conversation: &ConversationRow,
    resource: &ResourceRow,
    permit_id: &str,
    mapping_handle: &str,
    map_ip: &str,
) -> Result<(), Fault> {
    let mut client = connect(db).await?;
    let tx = begin(&mut client).await?;
    tx.execute(
        "INSERT INTO oo_session_api.conversation (project_id, conversation_id, principal_id, session_id) VALUES ($1,$2,$3,$4)",
        &[&conversation.project_id, &conversation.conversation_id, &conversation.principal_id, &conversation.session_id],
    )
    .await
    .map_err(|_| Fault::Unavailable("query"))?;
    tx.execute(
        "INSERT INTO oo_session_api.permit (permit_id, project_id, principal_id, session_id, state) VALUES ($1,$2,$3,$4,'held')",
        &[&permit_id, &resource.project_id, &resource.principal_id, &resource.session_id],
    )
    .await
    .map_err(|_| Fault::Unavailable("query"))?;
    tx.execute(
        "INSERT INTO oo_session_api.proxy_mapping (mapping_id, project_id, session_id, sandbox_id, credential_handle, map_ip, state) VALUES ($1,$2,$3,$4,$5,$6,'present')",
        &[&resource.mapping_id, &resource.project_id, &resource.session_id, &resource.workspace_id, &mapping_handle, &map_ip],
    )
    .await
    .map_err(|_| Fault::Unavailable("query"))?;
    tx.execute(
        "INSERT INTO oo_session_api.pool_slot (slot_id, state, session_id, replacement_for) VALUES ($1,'used',$2,NULL)",
        &[&resource.pool_slot, &resource.session_id],
    )
    .await
    .map_err(|_| Fault::Unavailable("query"))?;
    insert_resource(&tx, resource).await?;
    tx.commit().await.map_err(|_| Fault::Unavailable("commit"))?;
    Ok(())
}

async fn insert_resource(tx: &Transaction<'_>, resource: &ResourceRow) -> Result<(), Fault> {
    tx.execute(
        "INSERT INTO oo_session_api.resource (project_id, session_id, conversation_id, principal_id, role_name, claims_hash, policy_version, knowledge_version, config_version, workspace_id, event_prefix, pool_slot, permit_id, mapping_id, token_sha256, application_name, backend_pid, clock_ms, stage, harness_pid, claims_json) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)",
        &[
            &resource.project_id,
            &resource.session_id,
            &resource.conversation_id,
            &resource.principal_id,
            &resource.role_name,
            &resource.claims_hash,
            &resource.policy_version,
            &resource.knowledge_version,
            &resource.config_version,
            &resource.workspace_id,
            &resource.event_prefix,
            &resource.pool_slot,
            &resource.permit_id,
            &resource.mapping_id,
            &resource.token_sha256,
            &resource.application_name,
            &resource.backend_pid,
            &resource.clock_ms,
            &resource.stage,
            &resource.harness_pid,
            &resource.claims_json,
        ],
    )
    .await
    .map_err(|_| Fault::Unavailable("query"))?;
    Ok(())
}

pub async fn set_running(db: &Db<'_>, project_id: &str, session_id: &str, backend_pid: i32, clock_ms: i64) -> Result<(), Fault> {
    let client = connect(db).await?;
    let count = client
        .execute(
            "UPDATE oo_session_api.resource SET stage = 'running', backend_pid = $3, clock_ms = $4 WHERE project_id = $1 AND session_id = $2 AND stage = 'ready'",
            &[&project_id, &session_id, &backend_pid, &clock_ms],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    if count != 1 {
        return Err(Fault::Conflict("stage"));
    }
    Ok(())
}

pub async fn set_stage(db: &Db<'_>, project_id: &str, session_id: &str, stage: &str, harness_pid: Option<i64>) -> Result<(), Fault> {
    let client = connect(db).await?;
    let count = client
        .execute(
            "UPDATE oo_session_api.resource SET stage = $3, harness_pid = COALESCE($4, harness_pid) WHERE project_id = $1 AND session_id = $2",
            &[&project_id, &session_id, &stage, &harness_pid],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    if count != 1 {
        return Err(Fault::NotFound);
    }
    Ok(())
}

pub async fn load_conversation(db: &Db<'_>, conversation_id: &str) -> Result<Option<ConversationRow>, Fault> {
    let client = connect(db).await?;
    let row = client
        .query_opt(
            "SELECT project_id, conversation_id, principal_id, session_id FROM oo_session_api.conversation WHERE conversation_id = $1",
            &[&conversation_id],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(row.map(|row| ConversationRow {
        project_id: row.get(0),
        conversation_id: row.get(1),
        principal_id: row.get(2),
        session_id: row.get(3),
    }))
}

pub async fn list_conversations(db: &Db<'_>, project_id: &str, principal_id: &str) -> Result<Vec<ConversationRow>, Fault> {
    let client = connect(db).await?;
    let rows = client
        .query(
            "SELECT project_id, conversation_id, principal_id, session_id FROM oo_session_api.conversation WHERE project_id = $1 AND principal_id = $2 ORDER BY conversation_id",
            &[&project_id, &principal_id],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(rows
        .into_iter()
        .map(|row| ConversationRow {
            project_id: row.get(0),
            conversation_id: row.get(1),
            principal_id: row.get(2),
            session_id: row.get(3),
        })
        .collect())
}

pub async fn load_resource(db: &Db<'_>, project_id: &str, session_id: &str) -> Result<ResourceRow, Fault> {
    let client = connect(db).await?;
    let row = client
        .query_opt(
            "SELECT project_id, session_id, conversation_id, principal_id, role_name, claims_hash, policy_version, knowledge_version, config_version, workspace_id, event_prefix, pool_slot, permit_id, mapping_id, token_sha256, application_name, backend_pid, clock_ms, stage, harness_pid, claims_json FROM oo_session_api.resource WHERE project_id = $1 AND session_id = $2",
            &[&project_id, &session_id],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    let Some(row) = row else {
        return Err(Fault::NotFound);
    };
    Ok(ResourceRow {
        project_id: row.get(0),
        session_id: row.get(1),
        conversation_id: row.get(2),
        principal_id: row.get(3),
        role_name: row.get(4),
        claims_hash: row.get(5),
        policy_version: row.get(6),
        knowledge_version: row.get(7),
        config_version: row.get(8),
        workspace_id: row.get(9),
        event_prefix: row.get(10),
        pool_slot: row.get(11),
        permit_id: row.get(12),
        mapping_id: row.get(13),
        token_sha256: row.get(14),
        application_name: row.get(15),
        backend_pid: row.get(16),
        clock_ms: row.get(17),
        stage: row.get(18),
        harness_pid: row.get(19),
        claims_json: row.get(20),
    })
}

pub async fn next_seq(db: &Db<'_>, project_id: &str, conversation_id: &str) -> Result<i32, Fault> {
    let client = connect(db).await?;
    let current: i32 = client
        .query_one(
            "SELECT COALESCE(MAX(seq), 0)::integer FROM oo_session_api.api_event WHERE project_id = $1 AND conversation_id = $2",
            &[&project_id, &conversation_id],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?
        .get(0);
    if current >= 256 {
        return Err(Fault::Conflict("event_cap"));
    }
    Ok(current + 1)
}

pub async fn insert_event(db: &Db<'_>, project_id: &str, conversation_id: &str, seq: i32, event_id: &str, kind: &str, body: &str) -> Result<(), Fault> {
    if body.len() > 1024 {
        return Err(Fault::Malformed("event_body"));
    }
    let client = connect(db).await?;
    client
        .execute(
            "INSERT INTO oo_session_api.api_event (project_id, conversation_id, seq, event_id, kind, body) VALUES ($1,$2,$3,$4,$5,$6)",
            &[&project_id, &conversation_id, &seq, &event_id, &kind, &body],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(())
}

pub async fn list_events(db: &Db<'_>, project_id: &str, conversation_id: &str, after: i32, limit: i64) -> Result<Vec<EventRow>, Fault> {
    let client = connect(db).await?;
    let rows = client
        .query(
            "SELECT seq, event_id, kind, body FROM oo_session_api.api_event WHERE project_id = $1 AND conversation_id = $2 AND seq > $3 ORDER BY seq LIMIT $4",
            &[&project_id, &conversation_id, &after, &limit],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(rows
        .into_iter()
        .map(|row| EventRow { seq: row.get(0), event_id: row.get(1), kind: row.get(2), body: row.get(3) })
        .collect())
}

pub async fn release_credentials(db: &Db<'_>, resource: &ResourceRow) -> Result<(), Fault> {
    let mut client = connect(db).await?;
    let tx = begin(&mut client).await?;
    tx.execute(
        "UPDATE oo_session_api.proxy_mapping SET state = 'deleted' WHERE mapping_id = $1",
        &[&resource.mapping_id],
    )
    .await
    .map_err(|_| Fault::Unavailable("query"))?;
    tx.execute("UPDATE oo_session_api.permit SET state = 'released' WHERE permit_id = $1", &[&resource.permit_id])
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    tx.commit().await.map_err(|_| Fault::Unavailable("commit"))?;
    Ok(())
}

pub async fn mapping_present(db: &Db<'_>, project_id: &str, session_id: &str) -> Result<i64, Fault> {
    let client = connect(db).await?;
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM oo_session_api.proxy_mapping WHERE project_id = $1 AND session_id = $2 AND state = 'present'",
            &[&project_id, &session_id],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?
        .get(0);
    Ok(count)
}

pub async fn permits_held_by_session(db: &Db<'_>, session_id: &str) -> Result<i64, Fault> {
    let client = connect(db).await?;
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM oo_session_api.permit WHERE session_id = $1 AND state = 'held'",
            &[&session_id],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?
        .get(0);
    Ok(count)
}

pub async fn replacement_slot(db: &Db<'_>, replaced_slot: &str) -> Result<Option<String>, Fault> {
    let client = connect(db).await?;
    let row = client
        .query_opt(
            "SELECT slot_id FROM oo_session_api.pool_slot WHERE replacement_for = $1 AND state = 'empty' ORDER BY slot_id LIMIT 1",
            &[&replaced_slot],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn insert_pool_empty(db: &Db<'_>, slot_id: &str, replacement_for: &str) -> Result<(), Fault> {
    let client = connect(db).await?;
    client
        .execute(
            "INSERT INTO oo_session_api.pool_slot (slot_id, state, session_id, replacement_for) VALUES ($1,'empty',NULL,$2)",
            &[&slot_id, &replacement_for],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(())
}

pub async fn mark_slot_destroyed(db: &Db<'_>, slot_id: &str) -> Result<(), Fault> {
    let client = connect(db).await?;
    client
        .execute("UPDATE oo_session_api.pool_slot SET state = 'destroyed' WHERE slot_id = $1", &[&slot_id])
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(())
}

pub async fn insert_output(db: &Db<'_>, resource: &ResourceRow, name: &str, sha: &str, bytes: &[u8]) -> Result<(), Fault> {
    let client = connect(db).await?;
    client
        .execute(
            "INSERT INTO oo_session_api.output_object (project_id, session_id, name, sha256, bytes, bound_role, bound_claims_hash, bound_policy_version, bound_knowledge_version, bound_config_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
            &[
                &resource.project_id,
                &resource.session_id,
                &name,
                &sha,
                &bytes,
                &resource.role_name,
                &resource.claims_hash,
                &resource.policy_version,
                &resource.knowledge_version,
                &resource.config_version,
            ],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    Ok(())
}

pub async fn load_output(db: &Db<'_>, project_id: &str, session_id: &str, name: &str) -> Result<(String, Vec<u8>, ResourceRow), Fault> {
    let resource = load_resource(db, project_id, session_id).await?;
    let client = connect(db).await?;
    let row = client
        .query_opt(
            "SELECT sha256, bytes FROM oo_session_api.output_object WHERE project_id = $1 AND session_id = $2 AND name = $3",
            &[&project_id, &session_id, &name],
        )
        .await
        .map_err(|_| Fault::Unavailable("query"))?;
    let Some(row) = row else {
        return Err(Fault::NotFound);
    };
    Ok((row.get(0), row.get(1), resource))
}

pub async fn insert_audit(db: &Db<'_>, project_id: &str, session_id: &str, step_id: &str, evidence_sha256: &str) -> Result<(), Fault> {
    let client = connect(db).await?;
    let result = client
        .execute(
            "INSERT INTO oo_session_api.release_audit (project_id, session_id, step_id, evidence_sha256) VALUES ($1,$2,$3,$4)",
            &[&project_id, &session_id, &step_id, &evidence_sha256],
        )
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(err) if sqlstate(&err).as_deref() == Some("23505") => Ok(()),
        Err(_) => Err(Fault::Unavailable("query")),
    }
}
