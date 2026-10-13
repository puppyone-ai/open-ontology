//! Least-privilege PostgreSQL sessions.
//!
//! The platform reader issues SELECT, SHOW-equivalent current_setting, BEGIN,
//! SET, and ROLLBACK only. COPY and DDL belong to the separate DBA login.
//! Passwords are read from files and are not formatted into errors.

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use std::pin::pin;

use bytes::Bytes;
use futures_util::SinkExt;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;
use tokio_postgres_rustls::MakeRustlsConnect;
use zeroize::Zeroizing;

use crate::admit::SessionFacts;
use crate::arrow_out::{parse_utc_micros, parse_uuid, write_logistics_batch, ArrowRow};
use crate::audit_local::platform_sql_allowed;
use crate::config::ObjectName;
use crate::error::Fail;
use crate::logistics::{
    copy_event, copy_hub, copy_shipment, schema_sql, type_allowed, EVENT_ROWS, HUB_ROWS, SHIPMENT_ROWS,
};
use crate::oracle::{judge, oracle_statements};

#[derive(Clone)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    password: Zeroizing<String>,
    ca_pem: Vec<u8>,
}

pub struct Session {
    pub(crate) client: Client,
    ca_pem: Vec<u8>,
}

pub struct Measure {
    pub rows: u64,
    pub bytes: u64,
}

pub struct OracleFact {
    pub name: String,
    pub got: String,
}

pub struct TypeGate {
    pub rejected: Vec<String>,
}

pub struct GateStop {
    pub sqlstate: String,
}

pub fn read_password(path: &str) -> Result<Zeroizing<String>, Fail> {
    let bytes = fs::read(path).map_err(|_| Fail::new("secret.read", 4))?;
    if bytes.is_empty() || bytes.len() > 1024 {
        return Err(Fail::new("secret.read", 4));
    }
    let mut text = String::from_utf8(bytes).map_err(|_| Fail::new("secret.read", 4))?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() || text.contains('\0') || text.len() > 1024 {
        return Err(Fail::new("secret.read", 4));
    }
    Ok(Zeroizing::new(text))
}

pub fn endpoint_from_files(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password_file: &str,
    ca_file: &str,
) -> Result<Endpoint, Fail> {
    if host.parse::<std::net::IpAddr>().is_ok() || host.is_empty() || port == 0 {
        return Err(Fail::new("tls.host", 4));
    }
    let password = read_password(password_file)?;
    let ca_pem = fs::read(ca_file).map_err(|_| Fail::new("tls.ca", 4))?;
    Ok(Endpoint {
        host: host.to_owned(),
        port,
        database: database.to_owned(),
        username: username.to_owned(),
        password,
        ca_pem,
    })
}

pub async fn connect(endpoint: &Endpoint) -> Result<Session, Fail> {
    let tls = pg_tls(&endpoint.ca_pem)?;
    let mut config = tokio_postgres::Config::new();
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.user(&endpoint.username);
    config.dbname(&endpoint.database);
    config.password(endpoint.password.as_str());
    config.application_name("oo-dl01w1-46");
    config.ssl_mode(tokio_postgres::config::SslMode::Require);
    config.connect_timeout(Duration::from_secs(10));
    let pending = config.connect(tls);
    let (client, connection) = tokio::time::timeout(Duration::from_secs(10), pending)
        .await
        .map_err(|_| Fail::new("connect", 4))?
        .map_err(connect_fail)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(Session { client, ca_pem: endpoint.ca_pem.clone() })
}

pub async fn connect_read_only(endpoint: &Endpoint) -> Result<Session, Fail> {
    let session = connect(endpoint).await?;
    session
        .client
        .batch_execute("SET default_transaction_read_only = on")
        .await
        .map_err(|_| Fail::new("session.observe", 4))?;
    Ok(session)
}

pub fn observe_sql(objects: &[ObjectName]) -> Result<String, Fail> {
    if objects.is_empty() {
        return Err(Fail::new("config.budget", 1));
    }
    let mut select_terms = Vec::new();
    let mut write_terms = Vec::new();
    for object in objects {
        let relation = relation(object)?;
        select_terms.push(format!("has_table_privilege(current_user, '{relation}', 'SELECT')"));
        for privilege in ["INSERT", "UPDATE", "DELETE", "TRUNCATE", "TRIGGER"] {
            write_terms.push(format!("has_table_privilege(current_user, '{relation}', '{privilege}')"));
        }
    }
    write_terms.push("pg_has_role(current_user, 'pg_write_all_data', 'member')".to_owned());
    write_terms.push("pg_has_role(current_user, 'pg_database_owner', 'member')".to_owned());
    let sql = format!(
        "SELECT current_setting('server_version'), current_setting('server_version_num'), current_user::text, session_user::text, current_database(), r.rolsuper, r.rolcreaterole, r.rolcreatedb, r.rolreplication, r.rolbypassrls, r.rolcanlogin, {selects}, {writes}, current_setting('statement_timeout'), current_setting('lock_timeout'), current_setting('default_transaction_read_only'), COALESCE((SELECT s.version FROM pg_stat_ssl s WHERE s.pid = pg_backend_pid()), '') FROM pg_roles r WHERE r.rolname = current_user",
        selects = select_terms.join(" AND "),
        writes = write_terms.join(" OR "),
    );
    if !platform_sql_allowed(&sql) {
        return Err(Fail::new("audit.write", 7));
    }
    Ok(sql)
}

pub async fn observe_session(session: &Session, objects: &[ObjectName]) -> Result<SessionFacts, Fail> {
    let sql = observe_sql(objects)?;
    let row = session.client.query_one(&sql, &[]).await.map_err(|_| Fail::new("session.observe", 4))?;
    let version_num: String = row.get(1);
    Ok(SessionFacts {
        current_user: row.get(2),
        session_user: row.get(3),
        database: row.get(4),
        server_version: row.get(0),
        server_version_num: version_num.parse().map_err(|_| Fail::new("session.observe", 4))?,
        superuser: row.get(5),
        create_role: row.get(6),
        create_db: row.get(7),
        replication: row.get(8),
        bypass_rls: row.get(9),
        can_login: row.get(10),
        select_ok: row.get(11),
        write_privilege: row.get(12),
        statement_timeout: row.get(13),
        lock_timeout: row.get(14),
        default_transaction_read_only: row.get(15),
        negotiated_tls: row.get(16),
    })
}

pub async fn load_logistics(endpoint: &Endpoint, reader: &str) -> Result<(), Fail> {
    if endpoint.username == reader || !safe_ident(reader) {
        return Err(Fail::new("identity.separate", 6));
    }
    if !platform_sql_allowed("SELECT 1") {
        return Err(Fail::new("audit.write", 7));
    }
    if platform_sql_allowed(schema_sql()) {
        return Err(Fail::new("audit.write", 7));
    }
    let session = connect(endpoint).await?;
    session.client.batch_execute(schema_sql()).await.map_err(|_| Fail::new("dba.schema", 1))?;
    copy_table(
        &session,
        "COPY public.hubs (hub_id, hub_token, hub_code, hub_name, region_code, latitude, longitude, capacity_grams, opened_at, badge, note) FROM STDIN",
        HUB_ROWS,
        copy_hub,
    )
    .await?;
    copy_table(
        &session,
        "COPY public.shipments (shipment_id, shipment_token, origin_hub_id, dest_hub_id, status, transport_mode, weight_grams, cost_amount, created_at, promised_at, seal, comment) FROM STDIN",
        SHIPMENT_ROWS,
        copy_shipment,
    )
    .await?;
    copy_table(
        &session,
        "COPY public.tracking_events (event_id, event_token, shipment_id, hub_id, status, event_at, recorded_weight_grams, payload, comment) FROM STDIN",
        EVENT_ROWS,
        copy_event,
    )
    .await?;
    session
        .client
        .batch_execute("INSERT INTO public.type_probe (probe_id, payload) VALUES (1, '{\"kind\":\"jsonb\"}'::jsonb)")
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?;
    let grant = format!(
        "GRANT USAGE ON SCHEMA public TO {reader}; GRANT SELECT ON TABLE public.hubs, public.shipments, public.tracking_events, public.type_probe TO {reader}"
    );
    session.client.batch_execute(&grant).await.map_err(|_| Fail::new("dba.schema", 1))?;
    Ok(())
}

pub struct CustomerLoad {
    pub rows: u64,
    pub distinct_note_ids: u64,
    pub hubs: i64,
    pub shipments: i64,
    pub tracking_events: i64,
    pub payroll_select: bool,
    pub note_insert: bool,
}

/// DBA window for one caller-named schema. The product reader is not this session.
/// The preserved logistics counts are checked before any customer DDL.
pub async fn load_customer_east(endpoint: &Endpoint, reader: &str) -> Result<CustomerLoad, Fail> {
    if endpoint.username == reader || !safe_ident(reader) {
        return Err(Fail::new("identity.separate", 6));
    }
    let ddl = "CREATE SCHEMA IF NOT EXISTS customer_east; DROP TABLE IF EXISTS customer_east.delivery_notes; DROP TABLE IF EXISTS customer_east.payroll; CREATE TABLE customer_east.delivery_notes (note_id bigint PRIMARY KEY, amount numeric(38,4), receipt bytea, note_token uuid NOT NULL, signed_at timestamp with time zone, memo text); CREATE TABLE customer_east.payroll (payroll_id bigint PRIMARY KEY, internal_note text)";
    if platform_sql_allowed(ddl) {
        return Err(Fail::new("audit.write", 7));
    }
    let session = connect(endpoint).await?;
    let preserved = session
        .client
        .query_one(
            "SELECT (SELECT count(*) FROM public.hubs), (SELECT count(*) FROM public.shipments), (SELECT count(*) FROM public.tracking_events)",
            &[],
        )
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?;
    let hubs: i64 = preserved.get(0);
    let shipments: i64 = preserved.get(1);
    let tracking_events: i64 = preserved.get(2);
    if hubs != 2000 || shipments != 8000 || tracking_events != 90000 {
        return Err(Fail::new("dba.schema", 1));
    }
    session.client.batch_execute(ddl).await.map_err(|_| Fail::new("dba.schema", 1))?;
    let rows: i64 = 50;
    copy_table(
        &session,
        "COPY customer_east.delivery_notes (note_id, amount, receipt, note_token, signed_at, memo) FROM STDIN",
        rows,
        customer_note_line,
    )
    .await?;
    session
        .client
        .batch_execute("INSERT INTO customer_east.payroll (payroll_id, internal_note) VALUES (1, 'withheld')")
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?;
    let grant = format!("GRANT USAGE ON SCHEMA customer_east TO {reader}; GRANT SELECT ON TABLE customer_east.delivery_notes TO {reader}");
    session.client.batch_execute(&grant).await.map_err(|_| Fail::new("dba.schema", 1))?;
    let payroll_select: bool = session
        .client
        .query_one("SELECT has_table_privilege($1, 'customer_east.payroll', 'SELECT')", &[&reader])
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?
        .get(0);
    let note_insert: bool = session
        .client
        .query_one("SELECT has_table_privilege($1, 'customer_east.delivery_notes', 'INSERT')", &[&reader])
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?
        .get(0);
    let counted: i64 = session
        .client
        .query_one("SELECT count(*) FROM customer_east.delivery_notes", &[])
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?
        .get(0);
    let distinct: i64 = session
        .client
        .query_one("SELECT count(DISTINCT note_id) FROM customer_east.delivery_notes", &[])
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?
        .get(0);
    if payroll_select || note_insert || counted != rows || distinct != rows {
        return Err(Fail::new("dba.schema", 1));
    }
    let again = session
        .client
        .query_one(
            "SELECT (SELECT count(*) FROM public.hubs), (SELECT count(*) FROM public.shipments), (SELECT count(*) FROM public.tracking_events)",
            &[],
        )
        .await
        .map_err(|_| Fail::new("dba.schema", 1))?;
    if again.get::<_, i64>(0) != hubs || again.get::<_, i64>(1) != shipments || again.get::<_, i64>(2) != tracking_events {
        return Err(Fail::new("dba.schema", 1));
    }
    Ok(CustomerLoad {
        rows: u64::try_from(counted).map_err(|_| Fail::new("dba.schema", 1))?,
        distinct_note_ids: u64::try_from(distinct).map_err(|_| Fail::new("dba.schema", 1))?,
        hubs,
        shipments,
        tracking_events,
        payroll_select,
        note_insert,
    })
}

fn customer_note_line(id: i64) -> String {
    let amount = if id == 1 { "\\N".to_owned() } else if id == 2 { "-0.0001".to_owned() } else { format!("{id}.2500") };
    let receipt = if id == 1 { "\\N".to_owned() } else { "\\\\x6162".to_owned() };
    let token = format!("00000000-0000-4000-8000-{id:012x}");
    let signed = if id == 3 { "\\N".to_owned() } else { "2026-10-11 00:00:00+00".to_owned() };
    let memo = if id % 5 == 0 { "\\N".to_owned() } else { format!("note-{id}") };
    format!("{id}\t{amount}\t{receipt}\t{token}\t{signed}\t{memo}")
}

pub async fn measure(session: &Session) -> Result<Measure, Fail> {
    let sql = "SELECT ((SELECT count(*) FROM public.hubs) + (SELECT count(*) FROM public.shipments) + (SELECT count(*) FROM public.tracking_events))::text, (pg_total_relation_size('public.hubs') + pg_total_relation_size('public.shipments') + pg_total_relation_size('public.tracking_events'))::text";
    require_allowed(sql)?;
    let row = session.client.query_one(sql, &[]).await.map_err(measure_fail)?;
    let rows: String = row.get(0);
    let bytes: String = row.get(1);
    Ok(Measure {
        rows: rows.parse().map_err(|_| Fail::new("measure", 8))?,
        bytes: bytes.parse().map_err(|_| Fail::new("measure", 8))?,
    })
}

/// Measure exactly the caller-approved objects. Identifiers are checked before any SQL.
/// The relation is an ordinary literal. A `$1::regclass` parameter is not encodable by the reader client.
pub async fn measure_objects(session: &Session, objects: &[ObjectName]) -> Result<Measure, Fail> {
    if objects.is_empty() {
        return Err(Fail::new("config.objects", 2));
    }
    let mut total = Measure { rows: 0, bytes: 0 };
    for object in objects {
        let sql = measure_object_sql(object)?;
        require_allowed(&sql)?;
        let row = session.client.query_one(&sql, &[]).await.map_err(measure_fail)?;
        let rows: String = row.get(0); let bytes: String = row.get(1);
        total.rows = total.rows.checked_add(rows.parse().map_err(|_| Fail::new("measure", 8))?).ok_or(Fail::new("budget_cap", 10))?;
        total.bytes = total.bytes.checked_add(bytes.parse().map_err(|_| Fail::new("measure", 8))?).ok_or(Fail::new("budget_cap", 10))?;
    }
    Ok(total)
}

fn measure_object_sql(object: &ObjectName) -> Result<String, Fail> {
    if !safe_ident(&object.schema) || !safe_ident(&object.table) {
        return Err(Fail::new("config.objects", 2));
    }
    let relation = format!("{}.{}", object.schema, object.table);
    Ok(format!("SELECT count(*)::text, pg_total_relation_size('{relation}')::text FROM {relation}"))
}

pub async fn run_oracle(session: &Session) -> Result<Vec<OracleFact>, Fail> {
    let mut facts = Vec::new();
    for check in oracle_statements() {
        require_allowed(check.sql)?;
        let row = session.client.query_one(check.sql, &[]).await.map_err(|_| Fail::new("oracle.mismatch", 8))?;
        let got: String = row.get(0);
        facts.push(OracleFact { name: check.name.to_owned(), got });
    }
    Ok(facts)
}

pub fn judge_oracle(facts: &[OracleFact]) -> Result<(), Fail> {
    for fact in facts {
        judge(&fact.name, &fact.got)?;
    }
    Ok(())
}

pub async fn type_gate(session: &Session, objects: &[ObjectName]) -> Result<TypeGate, Fail> {
    let sql = "SELECT t.typname::text FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_type t ON t.oid = a.atttypid WHERE a.attnum > 0 AND NOT a.attisdropped AND n.nspname = $1 AND c.relname = $2";
    require_allowed(sql)?;
    let mut rejected = Vec::new();
    for object in objects {
        if !safe_ident(&object.schema) || !safe_ident(&object.table) {
            rejected.push(format!("ident:{}.{}", object.schema, object.table));
            continue;
        }
        let rows = session
            .client
            .query(sql, &[&object.schema.as_str(), &object.table.as_str()])
            .await
            .map_err(|_| Fail::new("type_gate", 5))?;
        if rows.is_empty() {
            rejected.push(format!("missing:{}.{}", object.schema, object.table));
            continue;
        }
        for row in rows {
            let typname: String = row.get(0);
            if !type_allowed(&typname) {
                rejected.push(format!("{typname}:{}.{}", object.schema, object.table));
            }
        }
    }
    Ok(TypeGate { rejected })
}

pub async fn deadline_sleep(session: &Session) -> Result<GateStop, Fail> {
    require_allowed("BEGIN")?;
    require_allowed("SET LOCAL statement_timeout = '400ms'")?;
    require_allowed("SELECT pg_sleep(2)")?;
    require_allowed("ROLLBACK")?;
    session.client.batch_execute("BEGIN").await.map_err(|_| Fail::new("deadline", 8))?;
    session
        .client
        .batch_execute("SET LOCAL statement_timeout = '400ms'")
        .await
        .map_err(|_| Fail::new("deadline", 8))?;
    let result = session.client.query_opt("SELECT pg_sleep(2)", &[]).await;
    let _ = session.client.batch_execute("ROLLBACK").await;
    match result {
        Err(err) if sqlstate(&err) == Some("57014") => Ok(GateStop { sqlstate: "57014".to_owned() }),
        Ok(_) => Err(Fail::new("halfready", 8)),
        Err(_) => Err(Fail::new("deadline", 8)),
    }
}

pub async fn cancel_sleep(session: &Session, endpoint: &Endpoint) -> Result<GateStop, Fail> {
    require_allowed("SELECT pg_backend_pid()")?;
    require_allowed("SELECT pg_sleep(30)")?;
    let activity = "SELECT count(*)::text FROM pg_stat_activity WHERE pid = $1 AND state = 'active' AND query LIKE 'SELECT pg_sleep%'";
    require_allowed(activity)?;
    let pid: i32 = session.client.query_one("SELECT pg_backend_pid()", &[]).await.map_err(|_| Fail::new("cancel", 8))?.get(0);
    let token = session.client.cancel_token();
    let tls = pg_tls(&session.ca_pem)?;
    let canceler = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        token.cancel_query(tls).await
    });
    let result = tokio::time::timeout(Duration::from_secs(5), session.client.query_opt("SELECT pg_sleep(30)", &[])).await;
    let _ = canceler.await;
    match result {
        Ok(Err(err)) if sqlstate(&err) == Some("57014") => {}
        Ok(Ok(_)) => return Err(Fail::new("halfready", 8)),
        _ => return Err(Fail::new("cancel", 8)),
    }
    let checker = connect_read_only(endpoint).await?;
    let params: [&(dyn ToSql + Sync); 1] = [&pid];
    let count: String = checker
        .client
        .query_one(activity, &params)
        .await
        .map_err(|_| Fail::new("cancel", 8))?
        .get(0);
    if count != "0" {
        return Err(Fail::new("halfready", 8));
    }
    Ok(GateStop { sqlstate: "57014".to_owned() })
}

pub async fn arrow_sample(session: &Session) -> Result<Vec<u8>, Fail> {
    let sql = "SELECT shipment_id, shipment_token::text, cost_amount::text, weight_grams, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), seal, comment FROM public.shipments WHERE shipment_id IN (1, 2, 17) ORDER BY shipment_id";
    require_allowed(sql)?;
    let rows = session.client.query(sql, &[]).await.map_err(|_| Fail::new("arrow.ipc", 1))?;
    if rows.len() != 3 {
        return Err(Fail::new("arrow.empty", 8));
    }
    let mut encoded = Vec::with_capacity(3);
    for row in &rows {
        let token: String = row.get(1);
        let cost: Option<String> = row.get(2);
        let created: String = row.get(4);
        let seal: Option<Vec<u8>> = row.get(5);
        let comment: Option<String> = row.get(6);
        let weight: Option<i64> = row.get(3);
        encoded.push(ArrowRow {
            shipment_id: row.get(0),
            shipment_token: parse_uuid(&token)?,
            cost_amount: match cost.as_deref() {
                None => None,
                Some(text) => Some(crate::logistics::parse_scale4(text).ok_or(Fail::new("arrow.decimal", 5))?),
            },
            weight_grams: weight,
            created_at_us: Some(parse_utc_micros(&created)?),
            seal,
            comment,
        });
    }
    if encoded[0].shipment_id != 1
        || crate::logistics::format_scale4(encoded[0].cost_amount.ok_or(Fail::new("arrow.decimal", 5))?)
            != crate::logistics::EXACT_COST
    {
        return Err(Fail::new("arrow.decimal", 5));
    }
    if encoded[1].shipment_id != 2
        || crate::logistics::format_scale4(encoded[1].cost_amount.ok_or(Fail::new("arrow.decimal", 5))?)
            != crate::logistics::NEGATIVE_MILL
    {
        return Err(Fail::new("arrow.decimal", 5));
    }
    if encoded[2].shipment_id != 17 || encoded[2].cost_amount.is_some() {
        return Err(Fail::new("arrow.decimal", 5));
    }
    let bytes = write_logistics_batch(&encoded, false)?;
    let (admitted, read) = crate::arrow_out::read_logistics_batch(&bytes)?;
    if admitted || read != encoded {
        return Err(Fail::new("arrow.ipc", 1));
    }
    Ok(bytes)
}

pub struct SealedRun {
    pub config_digest: String,
    pub type_digest: String,
    pub input_digest: String,
    pub schema_digest: String,
    pub run_id: String,
    pub source_generation: i64,
    pub target_generation: i64,
}

pub async fn grant_runtime(endpoint: &Endpoint, member: &str) -> Result<(), Fail> {
    if !safe_ident(member) {
        return Err(Fail::new("target.schema", 1));
    }
    let session = connect(endpoint).await?;
    let sql = format!("GRANT oo_pipeline_runtime TO {member} WITH INHERIT TRUE, SET FALSE");
    session.client.batch_execute(&sql).await.map_err(|_| Fail::new("target.schema", 1))
}

/// Import needs this login to inherit `oo_pipeline_runtime`. That inheritance
/// holds schema USAGE plus EXECUTE on both mapping functions, and not SELECT
/// on `_oo.physical_mappings`. `provision-mapping-reader` refuses that partial
/// set and does not finish it. After import, keep the membership and turn
/// inherit off so the official command sees an absent set and grants the three
/// mapping privileges itself. This does not grant those privileges.
pub async fn suspend_runtime_inherit(endpoint: &Endpoint, member: &str) -> Result<&'static str, Fail> {
    if !safe_ident(member) {
        return Err(Fail::new("consumer.request", 3));
    }
    let session = connect(endpoint).await?;
    let row = session
        .client
        .query_opt(
            "SELECT a.inherit_option \
             FROM pg_catalog.pg_auth_members AS a \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = a.member \
             JOIN pg_catalog.pg_roles AS granted_role ON granted_role.oid = a.roleid \
             WHERE member_role.rolname = $1 AND granted_role.rolname = 'oo_pipeline_runtime'",
            &[&member],
        )
        .await
        .map_err(|_| Fail::new("consumer.pending", 8))?;
    let Some(row) = row else {
        return Ok("absent");
    };
    let inherit: bool = row.get(0);
    if !inherit {
        return Ok("already_false");
    }
    let sql = format!("GRANT oo_pipeline_runtime TO {member} WITH INHERIT FALSE, SET FALSE");
    session
        .client
        .batch_execute(&sql)
        .await
        .map_err(|_| Fail::new("consumer.pending", 8))?;
    Ok("suspended")
}

/// The column installer replays a role only when its grants already equal the
/// full policy list. It does not add a table to a shorter list. This drops
/// that one `oo_r_` role so the requested list can be installed once. It does
/// not touch the login, any other role, or the source database.
pub async fn drop_partial_column_reader(endpoint: &Endpoint, login: &str, role: &str) -> Result<&'static str, Fail> {
    if !safe_ident(login) || !safe_ident(role) || !role.starts_with("oo_r_") {
        return Err(Fail::new("consumer.request", 3));
    }
    let session = connect(endpoint).await?;
    let exists: bool = session
        .client
        .query_one("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)", &[&role])
        .await
        .map_err(|_| Fail::new("consumer.acl", 7))?
        .get(0);
    if !exists {
        return Ok("absent");
    }
    let member: bool = session
        .client
        .query_one(
            "SELECT EXISTS ( \
                SELECT 1 FROM pg_catalog.pg_auth_members AS membership \
                JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
                JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
                WHERE granted.rolname = $1 AND member_role.rolname = $2)",
            &[&role, &login],
        )
        .await
        .map_err(|_| Fail::new("consumer.acl", 7))?
        .get(0);
    if member {
        session
            .client
            .batch_execute(&format!("REVOKE {role} FROM {login}"))
            .await
            .map_err(|_| Fail::new("consumer.acl", 7))?;
    }
    session
        .client
        .batch_execute(&format!("DROP OWNED BY {role}"))
        .await
        .map_err(|_| Fail::new("consumer.acl", 7))?;
    session
        .client
        .batch_execute(&format!("DROP ROLE {role}"))
        .await
        .map_err(|_| Fail::new("consumer.acl", 7))?;
    Ok("dropped")
}

pub async fn object_columns(session: &Session, object: &ObjectName) -> Result<Vec<String>, Fail> {
    if !safe_ident(&object.schema) || !safe_ident(&object.table) {
        return Err(Fail::new("config.objects", 2));
    }
    let sql = "SELECT a.attname::text FROM pg_catalog.pg_attribute AS a JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace WHERE a.attnum > 0 AND NOT a.attisdropped AND n.nspname = $1 AND c.relname = $2 ORDER BY a.attnum";
    require_allowed(sql)?;
    let rows = session
        .client
        .query(sql, &[&object.schema.as_str(), &object.table.as_str()])
        .await
        .map_err(|_| Fail::new("type_gate", 5))?;
    if rows.is_empty() {
        return Err(Fail::new("unapproved", 6));
    }
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

pub struct RoleFacts {
    pub exists: bool,
    pub grants: i64,
}

pub fn role_facts(endpoint: Endpoint, role: String) -> Result<RoleFacts, Fail> {
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_| Fail::new("connect", 4))?;
        runtime.block_on(async move {
            if !safe_ident(&role) {
                return Err(Fail::new("consumer.request", 3));
            }
            let session = connect(&endpoint).await?;
            let exists: bool = session
                .client
                .query_one("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)", &[&role])
                .await
                .map_err(|_| Fail::new("consumer.acl", 7))?
                .get(0);
            let grants: i64 = session
                .client
                .query_one("SELECT count(*) FROM information_schema.column_privileges WHERE grantee = $1", &[&role])
                .await
                .map_err(|_| Fail::new("consumer.acl", 7))?
                .get(0);
            Ok(RoleFacts { exists, grants })
        })
    })
    .join()
    .map_err(|_| Fail::new("connect", 4))?
}

pub struct CurrentTable {
    pub physical: String,
    pub max_generation: i64,
    pub retained: Vec<String>,
}

/// Bind the logical table through `_oo.lookup_physical_mapping` at the authority
/// generation. That generation must be `max(run_generation)` for the source and
/// config digest, which is the same replaced-generation rule the typed consumer
/// uses. Older physical rows stay. The role may hold column grants only on the
/// current physical table.
pub fn bind_current_generation(
    endpoint: Endpoint,
    source: String,
    config_digest: String,
    generation: i64,
    schema: String,
    table: String,
    role: String,
) -> Result<CurrentTable, Fail> {
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_| Fail::new("connect", 4))?;
        runtime.block_on(async move {
            if generation < 1 || !safe_ident(&source) || !hex_digest(&config_digest) || !safe_ident(&schema) || !safe_ident(&table) || !safe_ident(&role) {
                return Err(Fail::new("consumer.request", 3));
            }
            let session = connect(&endpoint).await?;
            let max: Option<i64> = session
                .client
                .query_one(
                    "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2",
                    &[&source, &config_digest],
                )
                .await
                .map_err(|_| Fail::new("consumer.request", 3))?
                .get(0);
            if max != Some(generation) {
                return Err(Fail::new("consumer.request", 3));
            }
            let physical: String = session
                .client
                .query_one(
                    "SELECT _oo.lookup_physical_mapping($1, $2, $3, 'table', $4, $5, '')",
                    &[&source, &config_digest, &generation, &schema, &table],
                )
                .await
                .map_err(|_| Fail::new("consumer.request", 3))?
                .get(0);
            if !mapped_table_name(&physical) {
                return Err(Fail::new("consumer.request", 3));
            }
            let retained_rows = session
                .client
                .query(
                    "SELECT physical_name FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2 AND level = 't' AND logical_namespace = $3 AND logical_table = $4 AND run_generation <> $5 ORDER BY run_generation",
                    &[&source, &config_digest, &schema, &table, &generation],
                )
                .await
                .map_err(|_| Fail::new("consumer.request", 3))?;
            let retained: Vec<String> = retained_rows.iter().map(|row| row.get(0)).collect();
            let grant_rows = session
                .client
                .query(
                    "SELECT DISTINCT table_name FROM information_schema.column_privileges WHERE grantee = $1 AND table_schema = '_oo'",
                    &[&role],
                )
                .await
                .map_err(|_| Fail::new("consumer.acl", 7))?;
            let granted: Vec<String> = grant_rows.iter().map(|row| row.get(0)).collect();
            let holds_current = granted.contains(&physical);
            let holds_retained = retained.iter().any(|name| granted.contains(name));
            if !holds_current || holds_retained {
                return Err(Fail::new("consumer.acl", 7));
            }
            Ok(CurrentTable { physical, max_generation: generation, retained })
        })
    })
    .join()
    .map_err(|_| Fail::new("connect", 4))?
}

pub async fn apply_sql(endpoint: &Endpoint, sql: &str) -> Result<(), String> {
    let session = connect(endpoint).await.map_err(|_| "connect".to_owned())?;
    session.client.batch_execute(sql).await.map_err(|err| {
        err.code().map(|code| code.code().to_owned()).unwrap_or_else(|| "none".to_owned())
    })
}

pub async fn read_sealed_run(endpoint: &Endpoint, source_id: &str, run_id: &str) -> Result<SealedRun, Fail> {
    if run_id.len() != 36 || !run_id.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-') {
        return Err(Fail::new("import.identity", 1));
    }
    let session = connect(endpoint).await?;
    let sql = "SELECT config_digest, type_digest, input_digest, schema_digest, run_id::text, source_generation, target_generation FROM _oo.full_import_run WHERE source_id = $1 AND run_id::text = $2 AND state = 'awaiting_publish'";
    let rows = session.client.query(sql, &[&source_id, &run_id]).await.map_err(|_| Fail::new("import.identity", 1))?;
    if rows.len() != 1 {
        return Err(Fail::new("import.identity", 1));
    }
    let row = &rows[0];
    Ok(SealedRun {
        config_digest: row.get(0),
        type_digest: row.get(1),
        input_digest: row.get(2),
        schema_digest: row.get(3),
        run_id: row.get(4),
        source_generation: row.get(5),
        target_generation: row.get(6),
    })
}

pub struct HeldLock {
    release: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        self.release.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Superuser lock on one mapped `_oo` table. This is not a source-reader statement.
/// The typed consumer reads the physical table, so a lock on `public.hubs` does not hold it.
pub fn hold_mapped_table(endpoint: Endpoint, physical: String) -> Result<HeldLock, Fail> {
    if !mapped_table_name(&physical) {
        return Err(Fail::new("consumer.pending", 8));
    }
    let release = Arc::new(AtomicBool::new(false));
    let release_thread = Arc::clone(&release);
    let ready = Arc::new(AtomicBool::new(false));
    let ready_thread = Arc::clone(&ready);
    let worker = thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() else {
            return;
        };
        runtime.block_on(async move {
            let Ok(session) = connect(&endpoint).await else {
                return;
            };
            let present = session
                .client
                .query_one(
                    "SELECT count(*)::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = $1 AND c.relkind = 'r'",
                    &[&physical],
                )
                .await
                .ok()
                .and_then(|row| row.try_get::<_, String>(0).ok());
            if present.as_deref() != Some("1") {
                return;
            }
            let statement = format!(
                "BEGIN; SET LOCAL lock_timeout = '20s'; LOCK TABLE \"_oo\".\"{physical}\" IN ACCESS EXCLUSIVE MODE"
            );
            if session.client.batch_execute(&statement).await.is_err() {
                return;
            }
            ready_thread.store(true, Ordering::SeqCst);
            while !release_thread.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let _ = session.client.batch_execute("ROLLBACK").await;
        });
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.load(Ordering::SeqCst) {
        if Instant::now() >= deadline {
            release.store(true, Ordering::SeqCst);
            let _ = worker.join();
            return Err(Fail::new("consumer.pending", 8));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(HeldLock { release, thread: Some(worker) })
}

pub(crate) fn mapped_table_name(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("pt_") else {
        return false;
    };
    value.len() == 63
        && rest.len() == 60
        && rest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn backend_active(endpoint: Endpoint, pid: i32) -> Result<bool, Fail> {
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| Fail::new("connect", 4))?;
        runtime.block_on(async move {
            let session = connect(&endpoint).await?;
            let row = session
                .client
                .query_opt("SELECT state FROM pg_stat_activity WHERE pid = $1", &[&pid])
                .await
                .map_err(|_| Fail::new("measure", 8))?;
            let state = row.and_then(|row| row.try_get::<_, Option<String>>(0).ok().flatten());
            Ok(state.as_deref() == Some("active"))
        })
    })
    .join()
    .map_err(|_| Fail::new("connect", 4))?
}

pub async fn target_partial(session: &Session) -> Result<(), Fail> {
    session
        .client
        .batch_execute(
            "DROP TABLE IF EXISTS public.tracking_events, public.shipments, public.hubs, public.type_probe; CREATE TABLE public.hubs (hub_id bigint PRIMARY KEY); INSERT INTO public.hubs (hub_id) VALUES (1)",
        )
        .await
        .map_err(|_| Fail::new("partial_not_cleaned", 9))?;
    Ok(())
}

pub async fn prepare_target_schema(session: &Session) -> Result<(), Fail> {
    session
        .client
        .batch_execute(schema_sql())
        .await
        .map_err(|_| Fail::new("target.schema", 1))
}

pub async fn target_cleanup(session: &Session) -> Result<(), Fail> {
    session
        .client
        .batch_execute("DROP TABLE IF EXISTS public.tracking_events, public.shipments, public.hubs, public.type_probe")
        .await
        .map_err(|_| Fail::new("partial_not_cleaned", 9))?;
    let left = target_relations(session).await?;
    if left != 0 {
        return Err(Fail::new("partial_not_cleaned", 9));
    }
    Ok(())
}

pub async fn target_relations(session: &Session) -> Result<u64, Fail> {
    let sql = "SELECT count(*)::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public' AND c.relkind = 'r' AND c.relname IN ('hubs', 'shipments', 'tracking_events', 'type_probe')";
    require_allowed(sql)?;
    let text: String = session.client.query_one(sql, &[]).await.map_err(|_| Fail::new("measure", 8))?.get(0);
    text.parse().map_err(|_| Fail::new("measure", 8))
}

fn require_allowed(sql: &str) -> Result<(), Fail> {
    if platform_sql_allowed(sql) { Ok(()) } else { Err(Fail::new("audit.write", 7)) }
}

async fn copy_table(
    session: &Session,
    sql: &str,
    rows: i64,
    line: fn(i64) -> String,
) -> Result<(), Fail> {
    let sink = session.client.copy_in(sql).await.map_err(|_| Fail::new("dba.copy", 1))?;
    let mut sink = pin!(sink);
    let mut buffer = Vec::with_capacity(256 * 1024);
    for id in 1..=rows {
        let text = line(id);
        if buffer.len() + text.len() + 1 > 250_000 {
            sink.as_mut()
                .send(Bytes::copy_from_slice(&buffer))
                .await
                .map_err(|_| Fail::new("dba.copy", 1))?;
            buffer.clear();
        }
        buffer.extend_from_slice(text.as_bytes());
        buffer.push(b'\n');
    }
    if !buffer.is_empty() {
        sink.as_mut()
            .send(Bytes::copy_from_slice(&buffer))
            .await
            .map_err(|_| Fail::new("dba.copy", 1))?;
    }
    let copied = sink.as_mut().finish().await.map_err(|_| Fail::new("dba.copy", 1))?;
    if i64::try_from(copied).ok() != Some(rows) {
        return Err(Fail::new("dba.copy", 1));
    }
    Ok(())
}

fn relation(object: &ObjectName) -> Result<String, Fail> {
    if safe_ident(&object.schema) && safe_ident(&object.table) {
        Ok(format!("{}.{}", object.schema, object.table))
    } else {
        Err(Fail::new("type_gate", 5))
    }
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first == '_' => {
            chars.all(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || char == '_')
        }
        _ => false,
    }
}

fn pg_tls(ca_pem: &[u8]) -> Result<MakeRustlsConnect, Fail> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| Fail::new("tls.ca", 4))?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|_| Fail::new("tls.ca", 4))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| Fail::new("tls", 4))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

fn connect_fail(err: tokio_postgres::Error) -> Fail {
    let text = err.to_string().to_ascii_lowercase();
    if text.contains("certificate") || text.contains("tls") || text.contains("ssl") {
        Fail::new("tls", 4)
    } else {
        Fail::new("connect", 4)
    }
}

fn measure_fail(err: tokio_postgres::Error) -> Fail {
    if sqlstate(&err) == Some("42P01") { Fail::new("no_data", 8) } else { Fail::new("measure", 8) }
}

fn sqlstate(err: &tokio_postgres::Error) -> Option<&str> {
    err.as_db_error().map(|db| db.code().code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_sql_is_allowed_and_dba_sql_is_not() {
        let objects = vec![
            ObjectName { schema: "public".to_owned(), table: "hubs".to_owned() },
            ObjectName { schema: "public".to_owned(), table: "shipments".to_owned() },
            ObjectName { schema: "public".to_owned(), table: "tracking_events".to_owned() },
        ];
        let sql = observe_sql(&objects).unwrap();
        assert!(platform_sql_allowed(&sql));
        assert!(platform_sql_allowed("SELECT pg_sleep(2)"));
        assert!(!platform_sql_allowed(schema_sql()));
        assert!(!platform_sql_allowed("COPY public.hubs FROM STDIN"));
        assert!(!platform_sql_allowed("GRANT oo_pipeline_runtime TO oo_dl01w1_46_target WITH INHERIT TRUE, SET FALSE"));
        assert!(!platform_sql_allowed("GRANT oo_pipeline_runtime TO oo_dl01w1_46_target WITH INHERIT FALSE, SET FALSE"));
        assert!(!platform_sql_allowed("SAVEPOINT reader_probe"));
        assert!(!safe_ident("Reader"));
        assert!(safe_ident("oo_dl01w1_46_reader"));
        let notes = ObjectName { schema: "customer_east".to_owned(), table: "delivery_notes".to_owned() };
        let sql = measure_object_sql(&notes).unwrap();
        assert!(platform_sql_allowed(&sql));
        assert!(sql.contains("pg_total_relation_size('customer_east.delivery_notes')"));
        assert!(!sql.contains("$1"));
        assert!(measure_object_sql(&ObjectName { schema: "Customer".to_owned(), table: "notes".to_owned() }).is_err());
    }
}
