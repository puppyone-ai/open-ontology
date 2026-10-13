//! Separate Rust process for the W41 MongoDB driver preflight.
//! It is not linked into Gateway or Loader. The server fixture is plaintext.
//! rustls-tls is enabled because the crate refuses to compile without a TLS backend.

mod customer;
mod encode;

#[allow(dead_code)]
fn bson_lock_pin() -> bson::Bson {
    bson::Bson::Null
}

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use encode::encode_batch;
use encode::field_census;
use encode::ipc_bytes;
use encode::parse_shipment;
use encode::reject_decimal;
use encode::reject_marks;
use mongodb::Client;
use mongodb::ClientSession;
use mongodb::IndexModel;
use mongodb::SessionCursor;
use mongodb::bson::Bson;
use mongodb::bson::Document;
use mongodb::bson::doc;
use mongodb::error::Error;
use mongodb::error::ErrorKind;
use mongodb::event::EventHandler;
use mongodb::event::cmap::CmapEvent;
use mongodb::event::command::CommandEvent;
use mongodb::event::command::CommandFailedEvent;
use mongodb::event::command::CommandStartedEvent;
use mongodb::event::command::CommandSucceededEvent;
use mongodb::event::sdam::SdamEvent;
use mongodb::event::sdam::ServerHeartbeatFailedEvent;
use mongodb::event::sdam::ServerHeartbeatStartedEvent;
use mongodb::event::sdam::ServerHeartbeatSucceededEvent;
use mongodb::options::ClientOptions;
use mongodb::options::Credential;
use mongodb::options::ServerMonitoringMode;
use mongodb::results::CollectionSpecification;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;

const MAX_ROWS: usize = 16;
const MAX_ARROW_BYTES: usize = 262_144;
const TINY_BYTES: usize = 32;

struct Sinks {
    commands: Mutex<Vec<Value>>,
    connections: Mutex<Vec<Value>>,
    sdam: Mutex<Vec<Value>>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("oo-mongodb-driver-probe: {err}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<(), String> {
    let dir = out_dir();
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let sinks = Arc::new(Sinks {
        commands: Mutex::new(Vec::new()),
        connections: Mutex::new(Vec::new()),
        sdam: Mutex::new(Vec::new()),
    });
    let outcome = execute(&dir, &sinks).await;
    if let Err(err) = &outcome {
        let gates = json!({
            "promoted": false,
            "ready": false,
            "probe_error": err,
        });
        let _ = write_json(&dir.join("gates.json"), &gates);
    }
    outcome
}

async fn execute(dir: &Path, sinks: &Arc<Sinks>) -> Result<(), String> {
    let policy = load_customer()?;
    let denial = customer::pre_read_denial(&policy);
    write_json(&dir.join("pre-read-deny.json"), &denial)?;
    if denial.get("refused").and_then(Value::as_bool) != Some(true)
        || denial.get("find_sent").and_then(Value::as_bool) != Some(false)
    {
        return Err("unapproved object was not refused before the read".to_string());
    }
    let secret_path = resolve_customer_secret(&policy)?;
    let password = read_password_file(&secret_path)?;
    let (client, public_options) = connect(&password, sinks).await?;
    drop(password);
    write_json(&dir.join("client-options.json"), &public_options)?;
    let mut session = match client.start_session().await {
        Ok(session) => session,
        Err(err) => {
            client.shutdown().await;
            write_capture(dir, sinks)?;
            return Err(show_err(err));
        }
    };
    if std::env::var("OO_C5_CAPTURE_ONLY").as_deref() == Ok("1") {
        let outcome = capture_c5_documents(&client, &mut session, dir, &policy).await;
        drop(session);
        client.shutdown().await;
        write_capture(dir, sinks)?;
        return outcome;
    }
    let work = work(&client, &mut session, dir, &policy, sinks).await;
    drop(session);
    client.shutdown().await;
    write_capture(dir, sinks)?;
    let meta = work?;
    let events = read_json(&dir.join("commands.json"))?;
    let connections = read_json(&dir.join("connections.json"))?;
    let gates = build_gates(&meta, &events, &connections);
    write_json(&dir.join("gates.json"), &gates)?;
    if gates.get("hello_seen").and_then(Value::as_bool) != Some(true) {
        return Err("handshake command was not in the wire audit".to_string());
    }
    if gates.get("sasl_start_seen").and_then(Value::as_bool) != Some(true)
        || gates.get("auth_observation").and_then(Value::as_str) == Some("absent")
    {
        return Err("auth handshake was not in the wire audit".to_string());
    }
    if gates.get("secret_body_recorded").and_then(Value::as_bool) != Some(false) {
        return Err("statement audit recorded a secret body".to_string());
    }
    if gates.get("timeout_label").and_then(Value::as_str) != Some("timeout")
        || gates.get("timeout_code").and_then(Value::as_i64) != Some(50)
    {
        return Err("source deadline completed instead of MaxTimeMSExpired".to_string());
    }
    Ok(())
}

/// Native raw BSON capture after a values-free field census. The source-side
/// predicate checks the entire field set atomically before returning raw data.
/// Unknown-field rows are quarantined; no projection can silently strip them.
async fn capture_c5_documents(
    client: &Client,
    session: &mut ClientSession,
    dir: &Path,
    policy: &customer::Policy,
) -> Result<(), String> {
    let (_, collection_name) = customer::object_parts(&policy.batch_object).map_err(str::to_string)?;
    customer::authorize(policy, &policy.batch_object, &policy.approved_fields).map_err(str::to_string)?;
    let collection = client.database(&policy.namespace).collection::<Document>(collection_name);
    let source_count = collection.count_documents(doc! {})
        .comment(comment("oo-dl01w1-41:native-c5:reader:budget-count"))
        .session(&mut *session).await.map_err(show_err)?;
    if source_count > policy.budget_limit || source_count > u64::try_from(MAX_ROWS).map_err(|err| err.to_string())? {
        return Err("budget.rows before full BSON read".to_string());
    }
    let pairs = doc! { "$map": {"input": {"$objectToArray": "$$ROOT"}, "as": "p", "in": {"name": "$$p.k", "type": {"$type": "$$p.v"}}}};
    let mut census_cursor = collection.aggregate(vec![doc! {"$project": {"pairs": pairs}}])
        .comment(comment("oo-dl01w1-41:native-c5:reader:census"))
        .session(&mut *session).await.map_err(show_err)?;
    let census = collect_cursor(&mut census_cursor, session, MAX_ROWS).await?;
    drop(census_cursor);
    let allowed = policy.approved_fields.clone();
    let filter = doc! {"$expr": {"$eq": [{"$size": {"$setDifference": [{"$map": {"input": {"$objectToArray": "$$ROOT"}, "as": "p", "in": "$$p.k"}}, allowed]}}, 0]}};
    let mut cursor = collection.find(filter).sort(doc! {"_id": 1})
        .limit(i64::try_from(MAX_ROWS + 1).map_err(|err| err.to_string())?)
        .comment(comment("oo-dl01w1-41:native-c5:reader:full-approved"))
        .session(&mut *session).await.map_err(show_err)?;
    let docs = collect_cursor(&mut cursor, session, MAX_ROWS).await?;
    drop(cursor);
    write_document_bytes(dir, &docs, policy)?;
    let typed_rows = docs.iter().map(parse_shipment).collect::<Result<Vec<_>, _>>()?;
    let batch = encode_batch(&typed_rows)?;
    let bytes = ipc_bytes(&batch)?;
    if bytes.len() > MAX_ARROW_BYTES { return Err("budget.bytes".to_string()); }
    fs::write(dir.join("typed-19.arrow"), bytes).map_err(|err| err.to_string())?;
    write_json(&dir.join("c5-census.json"), &json!({
        "capture": "C5", "consistency": "B", "delete_detection": "reconcile",
        "snapshot_token": null, "quality": ["DELETES_LAGGED"],
        "source_rows": source_count, "metadata_rows": census.len(), "approved_rows": docs.len(),
        "metadata": census.iter().map(|row| Bson::Document(row.clone()).into_relaxed_extjson()).collect::<Vec<_>>(),
        "unknown_values_exported": false, "source_acceptance": false,
    }))
}

fn load_customer() -> Result<customer::Policy, String> {
    let path = std::env::var("OO_W141_CUSTOMER").map_err(|_| "OO_W141_CUSTOMER is unset".to_string())?;
    let text = fs::read_to_string(&path).map_err(|err| format!("customer file: {err}"))?;
    customer::load_policy(&text)
}

fn resolve_customer_secret(policy: &customer::Policy) -> Result<String, String> {
    let path = std::env::var("OO_W141_SECRET_REGISTRY").map_err(|_| "OO_W141_SECRET_REGISTRY is unset".to_string())?;
    let text = fs::read_to_string(&path).map_err(|err| format!("secret registry: {err}"))?;
    customer::resolve_secret(policy, &text)
}

struct Meta {
    rows: usize,
    ipc_bytes: usize,
    partial_written: bool,
    timeout_label: String,
    timeout_code: Option<i64>,
    change_stream_code: Option<i64>,
    change_stream_name: Option<String>,
}

async fn connect(password: &str, sinks: &Arc<Sinks>) -> Result<(Client, Value), String> {
    let mut options = ClientOptions::parse(
        "mongodb://127.0.0.1:27017/?directConnection=true&retryWrites=false&retryReads=false",
    )
    .await
    .map_err(show_err)?;
    options.credential = Some(
        Credential::builder()
            .username(Some("oo_dl01w1_41_reader".to_string()))
            .password(Some(password.to_string()))
            .source(Some("admin".to_string()))
            .build(),
    );
    options.retry_writes = Some(false);
    options.retry_reads = Some(false);
    options.direct_connection = Some(true);
    options.max_pool_size = Some(1);
    options.min_pool_size = Some(0);
    options.max_connecting = Some(1);
    options.app_name = Some("oo-dl01w1-41-reader".to_string());
    options.server_selection_timeout = Some(Duration::from_secs(8));
    options.server_monitoring_mode = Some(ServerMonitoringMode::Poll);
    let commands = Arc::clone(sinks);
    options.command_event_handler = Some(EventHandler::callback(move |event: CommandEvent| {
        push_command(&commands.commands, event);
    }));
    let connections = Arc::clone(sinks);
    options.cmap_event_handler = Some(EventHandler::callback(move |event: CmapEvent| {
        push_cmap(&connections.connections, event);
    }));
    let sdam = Arc::clone(sinks);
    options.sdam_event_handler = Some(EventHandler::callback(move |event: SdamEvent| {
        push_sdam(&sdam.sdam, event);
    }));
    let public_options = options_public(&options);
    let client = Client::with_options(options).map_err(show_err)?;
    Ok((client, public_options))
}

async fn work(
    client: &Client,
    session: &mut ClientSession,
    dir: &Path,
    policy: &customer::Policy,
    sinks: &Sinks,
) -> Result<Meta, String> {
    let (_, batch_collection) = customer::object_parts(&policy.batch_object)?;
    let batch_fields = policy.approved_fields.clone();
    customer::authorize(policy, &policy.batch_object, &batch_fields)?;
    let db = client.database(&policy.namespace);
    let shipments = db.collection::<Document>(batch_collection);
    let catalog = read_catalog(client, session, policy).await?;
    write_json(&dir.join("catalog.json"), &catalog)?;
    let rejections = read_rejections(&db, session, policy).await?;
    write_json(&dir.join("rejections.json"), &rejections)?;
    let (docs, ipc, rows) = read_batch(&shipments, session, policy).await?;
    let census = field_census(&docs);
    write_json(&dir.join("field-census.json"), &census)?;
    let raw_docs = encode::canonical_raw_documents(&docs)?;
    write_json(&dir.join("raw-docs.json"), &raw_docs)?;
    write_document_bytes(dir, &docs, policy)?;
    let ipc_len = ipc.len();
    let partial_written = if rows == MAX_ROWS && ipc_len <= MAX_ARROW_BYTES {
        fs::write(dir.join("batch.arrow.partial"), &ipc).map_err(|err| err.to_string())?;
        true
    } else {
        false
    };
    let tiny_len = TINY_BYTES.min(ipc_len);
    fs::write(dir.join("truncated.arrow.partial"), &ipc[..tiny_len]).map_err(|err| err.to_string())?;
    write_json(
        &dir.join("truncation.json"),
        &json!({
            "promoted": false,
            "reason": "BYTE_CAP",
            "max_bytes": TINY_BYTES,
            "actual_bytes": ipc_len,
            "written_bytes": tiny_len,
            "ready": false,
        }),
    )?;
    let cancel = cancel_batch(&shipments, session, sinks).await?;
    write_json(&dir.join("cancel.json"), &cancel)?;
    let timeout = timeout_batch(&shipments, session).await?;
    write_json(&dir.join("timeout.json"), &timeout)?;
    let change = change_stream(&shipments, session).await?;
    write_json(&dir.join("change-stream.json"), &change)?;
    Ok(Meta {
        rows,
        ipc_bytes: ipc_len,
        partial_written,
        timeout_label: timeout["label"].as_str().unwrap_or("unset").to_string(),
        timeout_code: timeout.get("code").and_then(Value::as_i64),
        change_stream_code: change.get("code").and_then(Value::as_i64),
        change_stream_name: change.get("code_name").and_then(|value| value.as_str()).map(str::to_string),
    })
}

async fn read_catalog(
    client: &Client,
    session: &mut ClientSession,
    policy: &customer::Policy,
) -> Result<Value, String> {
    let db = client.database(&policy.namespace);
    let (_, batch_collection) = customer::object_parts(&policy.batch_object)?;
    let mut collections = db
        .list_collections()
        .comment(comment("oo-dl01w1-41:reader:listCollections"))
        .session(&mut *session)
        .await
        .map_err(show_err)?;
    let specs = collect_cursor(&mut collections, session, 32).await?;
    drop(collections);
    let shipments = db.collection::<Document>(batch_collection);
    let mut indexes = shipments
        .list_indexes()
        .comment(comment("oo-dl01w1-41:reader:listIndexes"))
        .session(&mut *session)
        .await
        .map_err(show_err)?;
    let index_models = collect_cursor(&mut indexes, session, 16).await?;
    drop(indexes);
    let coll_stats = run_documented(
        &db,
        doc! { "collStats": batch_collection, "comment": "oo-dl01w1-41:reader:collStats" },
        session,
    )
    .await;
    let who = run_documented(
        &db,
        doc! { "connectionStatus": 1, "comment": "oo-dl01w1-41:reader:whoami" },
        session,
    )
    .await;
    let build = run_documented(
        &client.database("admin"),
        doc! { "buildInfo": 1, "comment": "oo-dl01w1-41:reader:buildInfo" },
        session,
    )
    .await;
    Ok(json!({
        "database": policy.namespace,
        "collections": specs.iter().map(collection_json).collect::<Vec<_>>(),
        "indexes": index_models.iter().map(index_json).collect::<Vec<_>>(),
        "coll_stats": coll_stats,
        "connection_status": who,
        "build_info": build,
        "sample_not_enforced_schema": true,
        "capture_limitations": [
            "field census is a sample of returned documents, not a server validator",
            "profiler level 2 is a command sample, not an enforced schema",
            "standalone topology has no replica-set change stream",
            "profiles read by the audit identity cover admin and oo_w141 only"
        ],
    }))
}

async fn read_rejections(
    db: &mongodb::Database,
    session: &mut ClientSession,
    policy: &customer::Policy,
) -> Result<Value, String> {
    let fields = vec!["_id".to_string(), "code".to_string(), "price".to_string(), "marks".to_string()];
    let mut allowed = Vec::new();
    for object in &policy.type_boundary_objects {
        if customer::authorize(policy, object, &fields).is_ok() {
            allowed.push(object.clone());
        }
    }
    if allowed.is_empty() {
        return Ok(json!({
            "decimal": [],
            "heterogeneous_array": [],
            "skipped_unapproved": true,
            "find_sent": false,
        }));
    }
    let price_odd = boundary_collection(&allowed, "price_odd")?;
    let decimals = db.collection::<Document>(&price_odd);
    let mut decimal_cursor = decimals
        .find(doc! {})
        .projection(doc! { "_id": 1, "code": 1, "price": 1 })
        .sort(doc! { "_id": 1 })
        .limit(8)
        .comment(comment("oo-dl01w1-41:reader:reject-decimal"))
        .session(&mut *session)
        .await
        .map_err(show_err)?;
    let decimal_docs = collect_cursor(&mut decimal_cursor, session, 8).await?;
    drop(decimal_cursor);
    let mut decimal_rows = Vec::new();
    for doc in &decimal_docs {
        decimal_rows.push(reject_decimal(doc)?);
    }
    let mixed_marks = boundary_collection(&allowed, "mixed_marks")?;
    let mixed = db.collection::<Document>(&mixed_marks);
    let mut mixed_cursor = mixed
        .find(doc! {})
        .projection(doc! { "_id": 1, "code": 1, "marks": 1 })
        .sort(doc! { "_id": 1 })
        .limit(8)
        .comment(comment("oo-dl01w1-41:reader:reject-array"))
        .session(&mut *session)
        .await
        .map_err(show_err)?;
    let mixed_docs = collect_cursor(&mut mixed_cursor, session, 8).await?;
    drop(mixed_cursor);
    let mut mixed_rows = Vec::new();
    for doc in &mixed_docs {
        mixed_rows.push(reject_marks(doc)?);
    }
    Ok(json!({
        "decimal": decimal_rows,
        "heterogeneous_array": mixed_rows,
    }))
}

fn write_document_bytes(dir: &Path, docs: &[Document], policy: &customer::Policy) -> Result<(), String> {
    const STORED: [&str; 15] = [
        "_id", "code", "flag", "units", "price", "created", "uid", "blob", "weight", "warehouse", "whCode", "lane", "memo", "marks", "pack",
    ];
    let mut full_lines = String::new();
    let mut seal_lines = String::new();
    let mut rows = Vec::with_capacity(docs.len());
    for (index, doc) in docs.iter().enumerate() {
        for key in doc.keys() {
            if !policy.approved_fields.iter().any(|field| field == key) {
                return Err(format!("captured key outside approval {key}"));
            }
            if !STORED.contains(&key.as_str()) {
                return Err(format!("captured key is not a stored shipment field {key}"));
            }
        }
        let weight_type = match doc.get("weight") {
            Some(Bson::Double(_)) => "double",
            Some(other) => return Err(format!("weight {}", type_name(Some(other)))),
            None => "absent",
        };
        let full = bson::to_vec(doc).map_err(|err| err.to_string())?;
        let mut seal_doc = doc.clone();
        seal_doc.remove("weight");
        let sealed = bson::to_vec(&seal_doc).map_err(|err| err.to_string())?;
        full_lines.push_str(&hex_encode(&full));
        full_lines.push('\n');
        seal_lines.push_str(&hex_encode(&sealed));
        seal_lines.push('\n');
        rows.push(json!({
            "index": index,
            "keys": doc.keys().cloned().collect::<Vec<_>>(),
            "weight_bson_type": weight_type,
            "full_bson_bytes": full.len(),
            "seal_bson_bytes": sealed.len(),
            "weight_in_seal_bson": false,
            "seal_omission": "weight",
            "seal_omission_reason": "fixed BSON decoder rejects type 0x01 double",
        }));
    }
    fs::write(dir.join("full-approved.bson.hex"), full_lines).map_err(|err| err.to_string())?;
    fs::write(dir.join("seal-scope.bson.hex"), seal_lines).map_err(|err| err.to_string())?;
    write_json(
        &dir.join("fulldoc-capture.json"),
        &json!({
            "protocol": "oo.mongodb-w141.fulldoc-capture/v1",
            "role": policy.role,
            "namespace": policy.namespace,
            "object": policy.batch_object,
            "projection": "inclusion",
            "projected_fields": STORED,
            "captured_rows": docs.len(),
            "write_attempts": 0,
            "hidden_key_in_capture": false,
            "weight_in_full_bson": true,
            "weight_in_seal_bson": false,
            "rows": rows,
        }),
    )
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn type_name(value: Option<&Bson>) -> &'static str {
    match value {
        Some(Bson::Double(_)) => "double",
        Some(Bson::String(_)) => "string",
        Some(Bson::Document(_)) => "document",
        Some(Bson::Array(_)) => "array",
        Some(Bson::Binary(_)) => "binary",
        Some(Bson::ObjectId(_)) => "objectId",
        Some(Bson::Boolean(_)) => "bool",
        Some(Bson::DateTime(_)) => "date",
        Some(Bson::Null) => "null",
        Some(Bson::Int32(_)) => "int32",
        Some(Bson::Int64(_)) => "int64",
        Some(Bson::Decimal128(_)) => "decimal128",
        Some(_) => "other",
        None => "absent",
    }
}

fn approved_projection(policy: &customer::Policy) -> Result<Document, String> {
    if policy.role != "read" {
        return Err("role".to_string());
    }
    if policy.budget_limit < u64::try_from(MAX_ROWS).map_err(|err| err.to_string())? {
        return Err("budget".to_string());
    }
    for name in encode::column_names() {
        if !policy.approved_fields.iter().any(|field| field == name) {
            return Err(format!("unapproved_field {name}"));
        }
    }
    Ok(projection())
}

async fn read_batch(
    coll: &mongodb::Collection<Document>,
    session: &mut ClientSession,
    policy: &customer::Policy,
) -> Result<(Vec<Document>, Vec<u8>, usize), String> {
    let projection = approved_projection(policy)?;
    let limit = i64::try_from(policy.budget_limit.min(u64::try_from(MAX_ROWS).map_err(|err| err.to_string())?))
        .map_err(|err| err.to_string())?;
    let mut cursor = coll
        .find(doc! {})
        .projection(projection)
        .sort(doc! { "_id": 1 })
        .batch_size(4_u32)
        .limit(limit)
        .max_time(Duration::from_secs(5))
        .comment(comment("oo-dl01w1-41:reader:batch"))
        .session(&mut *session)
        .await
        .map_err(show_err)?;
    let docs = collect_cursor(&mut cursor, session, MAX_ROWS + 1).await?;
    drop(cursor);
    if docs.len() != MAX_ROWS {
        return Err(format!("row count {}", docs.len()));
    }
    let mut rows = Vec::with_capacity(docs.len());
    for doc in &docs {
        rows.push(parse_shipment(doc).map_err(|err| format!("shipment: {err}"))?);
    }
    let batch = encode_batch(&rows)?;
    let bytes = ipc_bytes(&batch)?;
    Ok((docs, bytes, rows.len()))
}

async fn cancel_batch(
    coll: &mongodb::Collection<Document>,
    session: &mut ClientSession,
    sinks: &Sinks,
) -> Result<Value, String> {
    let mut cursor = coll
        .find(doc! {})
        .projection(projection())
        .sort(doc! { "_id": 1 })
        .batch_size(1_u32)
        .limit(8)
        .max_time(Duration::from_secs(5))
        .comment(comment("oo-dl01w1-41:reader:cancel"))
        .session(&mut *session)
        .await
        .map_err(show_err)?;
    let first = match cursor.next(&mut *session).await {
        Some(Ok(_)) => true,
        Some(Err(err)) => return Err(show_err(err)),
        None => false,
    };
    // Drop sends killCursors for this cursor only. The read role cannot killOp.
    drop(cursor);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let events = sinks.commands.lock().map_err(|_| "command audit lock".to_string())?;
    owned_cancel_record(&events, first)
}

fn owned_cancel_record(events: &[Value], read_one: bool) -> Result<Value, String> {
    let cursor_id = cursor_for_comment(events, "oo-dl01w1-41:reader:cancel")
        .filter(|id| id != "0")
        .ok_or_else(|| "cancel cursor was absent or exhausted before killCursors".to_string())?;
    let started = events.iter().find(|event| kill_names_cursor(event, &cursor_id));
    let Some(started) = started else {
        return Err("read-role killCursors did not name the owned cursor".to_string());
    };
    let request_id = started.get("request_id").and_then(Value::as_i64);
    let ended = events.iter().any(|event| {
        event.get("phase").and_then(Value::as_str) == Some("succeeded")
            && event.get("command").and_then(Value::as_str) == Some("killCursors")
            && event.get("request_id").and_then(Value::as_i64) == request_id
    });
    if !ended {
        return Err("read-role killCursors did not end the owned cursor".to_string());
    }
    Ok(json!({
        "comment": "oo-dl01w1-41:reader:cancel",
        "read_one": read_one,
        "promoted": false,
        "method": "read_role_killCursors",
        "dba_kill": false,
        "foreign_operations": false,
        "cursor_id": cursor_id,
        "server_ended": true,
        "client_cancel": true,
        "privilege": "read",
    }))
}

fn kill_names_cursor(event: &Value, cursor_id: &str) -> bool {
    event.get("phase").and_then(Value::as_str) == Some("started")
        && event.get("command").and_then(Value::as_str) == Some("killCursors")
        && event
            .get("kill_cursor_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(cursor_id)))
}

fn deadline_filter(sleep_ms: i64) -> Document {
    // maxTimeMS is checked at a server interrupt. A tight Date.now loop in
    // $function never yields, so the command returns ok after the budget.
    // $where sleep() yields and the server ends the find with code 50.
    let body = format!("sleep({sleep_ms}) || true");
    doc! { "$where": body }
}

async fn timeout_batch(coll: &mongodb::Collection<Document>, session: &mut ClientSession) -> Result<Value, String> {
    let attempts = [(80_u64, 200_i64), (50_u64, 200_i64)];
    let mut recorded = Vec::new();
    for (budget_ms, sleep_ms) in attempts {
        let result = coll
            .find(deadline_filter(sleep_ms))
            .batch_size(16_u32)
            .max_time(Duration::from_millis(budget_ms))
            .comment(comment("oo-dl01w1-41:reader:maxtime"))
            .session(&mut *session)
            .await;
        match result {
            Ok(mut cursor) => {
                let mut pulled = 0_u64;
                let mut expired = None;
                while pulled < 2 {
                    match cursor.next(&mut *session).await {
                        None => break,
                        Some(Ok(_)) => pulled += 1,
                        Some(Err(err)) => {
                            expired = Some(err);
                            break;
                        }
                    }
                }
                drop(cursor);
                if let Some(err) = expired {
                    recorded.push(timeout_row(sleep_ms, budget_ms, &command_failure(&err), pulled));
                } else {
                    recorded.push(json!({
                        "sleep_ms": sleep_ms,
                        "wait_ms": sleep_ms,
                        "budget_ms": budget_ms,
                        "mechanism": "find_where_sleep",
                        "label": "completed",
                        "code": null,
                        "pulled": pulled,
                        "server_ended": false,
                        "client_cancel": false,
                    }));
                }
            }
            Err(err) => {
                recorded.push(timeout_row(sleep_ms, budget_ms, &command_failure(&err), 0));
            }
        }
    }
    let chosen = recorded.iter().find(|row| row["label"] == "timeout").cloned();
    let chosen = chosen.unwrap_or_else(|| recorded.last().cloned().unwrap_or_else(|| json!({})));
    Ok(json!({
        "attempts": recorded,
        "label": chosen.get("label").cloned().unwrap_or(Value::Null),
        "code": chosen.get("code").cloned().unwrap_or(Value::Null),
        "code_name": chosen.get("code_name").cloned().unwrap_or(Value::Null),
        "server_ended": chosen.get("server_ended").cloned().unwrap_or(Value::Bool(false)),
        "client_cancel": false,
    }))
}

fn timeout_row(sleep_ms: i64, budget_ms: u64, failure: &Value, pulled: u64) -> Value {
    let code = failure.get("code").and_then(Value::as_i64);
    let label = if code == Some(50) { "timeout" } else { "not_timeout" };
    json!({
        "sleep_ms": sleep_ms,
        "wait_ms": sleep_ms,
        "budget_ms": budget_ms,
        "mechanism": "find_where_sleep",
        "label": label,
        "code": failure.get("code").cloned().unwrap_or(Value::Null),
        "code_name": failure.get("code_name").cloned().unwrap_or(Value::Null),
        "message": failure.get("message").cloned().unwrap_or(Value::Null),
        "pulled": pulled,
        "server_ended": code == Some(50),
        "client_cancel": false,
    })
}

fn boundary_collection(allowed: &[String], name: &str) -> Result<String, String> {
    allowed
        .iter()
        .find_map(|object| {
            let (_, collection) = customer::object_parts(object).ok()?;
            (collection == name).then(|| collection.to_string())
        })
        .ok_or_else(|| format!("type boundary {name} is not approved"))
}

async fn change_stream(coll: &mongodb::Collection<Document>, session: &mut ClientSession) -> Result<Value, String> {
    let result = coll
        .aggregate(vec![doc! { "$changeStream": {} }])
        .comment(comment("oo-dl01w1-41:reader:changestream"))
        .session(&mut *session)
        .await;
    match result {
        Ok(cursor) => {
            drop(cursor);
            Ok(json!({
                "opened": true,
                "code": null,
                "code_name": null,
                "label": "opened",
            }))
        }
        Err(err) => {
            let failure = command_failure(&err);
            Ok(json!({
                "opened": false,
                "code": failure.get("code").cloned().unwrap_or(Value::Null),
                "code_name": failure.get("code_name").cloned().unwrap_or(Value::Null),
                "message": failure.get("message").cloned().unwrap_or(Value::Null),
                "label": "server_error",
            }))
        }
    }
}

async fn collect_cursor<T>(
    cursor: &mut SessionCursor<T>,
    session: &mut ClientSession,
    limit: usize,
) -> Result<Vec<T>, String>
where
    T: DeserializeOwned + Send + Sync,
{
    let mut rows = Vec::new();
    while rows.len() < limit {
        match cursor.next(&mut *session).await {
            None => break,
            Some(Ok(row)) => rows.push(row),
            Some(Err(err)) => return Err(show_err(err)),
        }
    }
    Ok(rows)
}

async fn run_documented(db: &mongodb::Database, command: Document, session: &mut ClientSession) -> Value {
    match db.run_command(command).session(&mut *session).await {
        Ok(document) => command_reply_summary(&document),
        Err(err) => command_failure(&err),
    }
}

fn build_gates(meta: &Meta, events: &Value, connections: &Value) -> Value {
    let rows = events.get("events").and_then(Value::as_array);
    let cancel_id = rows.and_then(|events| cursor_for_comment(events, "oo-dl01w1-41:reader:cancel"));
    let batch_id = rows.and_then(|events| cursor_for_comment(events, "oo-dl01w1-41:reader:batch"));
    let names = events.get("names").and_then(Value::as_array);
    let write_names = names
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| is_write_command(name))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let created = connections
        .get("events")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|row| row.get("kind").and_then(Value::as_str) == Some("ConnectionCreated"))
                .count()
        })
        .unwrap_or(0);
    json!({
        "promoted": false,
        "ready": false,
        "rows": meta.rows,
        "ipc_bytes": meta.ipc_bytes,
        "batch_partial_written": meta.partial_written,
        "max_rows": MAX_ROWS,
        "max_arrow_bytes": MAX_ARROW_BYTES,
        "tiny_cap_bytes": TINY_BYTES,
        "cancel_cursor_id": cancel_id,
        "batch_cursor_id": batch_id,
        "cancel_kill_seen": rows.map(|events| kill_contains(events, cancel_id.as_deref())).unwrap_or(false),
        "batch_get_more_seen": rows.map(|events| get_more_contains(events, batch_id.as_deref())).unwrap_or(false),
        "hello_seen": wire_has(events, "hello") || wire_has(events, "ismaster") || wire_has(events, "isMaster"),
        "sasl_start_seen": wire_has(events, "saslStart"),
        "sasl_continue_seen": wire_has(events, "saslContinue"),
        "handshake_source": events.get("handshake_source").cloned().unwrap_or(Value::Null),
        "handshake_command": events.get("handshake_command").cloned().unwrap_or(Value::Null),
        "auth_observation": events.get("auth_observation").cloned().unwrap_or(Value::Null),
        "secret_body_recorded": events.get("secret_body_recorded").cloned().unwrap_or(Value::Bool(true)),
        "end_sessions_seen": names.map(|items| has_name(items, "endSessions")).unwrap_or(false),
        "reader_write_commands": write_names,
        "timeout_label": meta.timeout_label,
        "timeout_code": meta.timeout_code,
        "change_stream_code": meta.change_stream_code,
        "change_stream_code_name": meta.change_stream_name,
        "connection_created": created,
        "retry_writes": false,
    })
}

fn cursor_for_comment(events: &[Value], comment_text: &str) -> Option<String> {
    let request_id = events.iter().find_map(|event| {
        let matches = event.get("phase").and_then(Value::as_str) == Some("started")
            && event.get("command").and_then(Value::as_str) == Some("find")
            && event.get("comment").and_then(Value::as_str) == Some(comment_text);
        if matches { event.get("request_id").and_then(Value::as_i64) } else { None }
    })?;
    events.iter().find_map(|event| {
        let matches = event.get("phase").and_then(Value::as_str) == Some("succeeded")
            && event.get("request_id").and_then(Value::as_i64) == Some(request_id);
        if matches { event.get("cursor_id").and_then(Value::as_str).map(str::to_string) } else { None }
    })
}

fn kill_contains(events: &[Value], cursor_id: Option<&str>) -> bool {
    let Some(cursor_id) = cursor_id else {
        return false;
    };
    if cursor_id == "0" {
        return false;
    }
    events.iter().any(|event| {
        event.get("command").and_then(Value::as_str) == Some("killCursors")
            && event
                .get("kill_cursor_ids")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(cursor_id)))
    })
}

fn get_more_contains(events: &[Value], cursor_id: Option<&str>) -> bool {
    let Some(cursor_id) = cursor_id else {
        return false;
    };
    events.iter().any(|event| {
        event.get("command").and_then(Value::as_str) == Some("getMore")
            && event.get("cursor_id").and_then(Value::as_str) == Some(cursor_id)
    })
}

fn has_name(items: &[Value], name: &str) -> bool {
    items.iter().any(|item| item.as_str() == Some(name))
}

fn is_write_command(name: &str) -> bool {
    matches!(
        name,
        "insert"
            | "update"
            | "delete"
            | "findAndModify"
            | "drop"
            | "create"
            | "createIndexes"
            | "profile"
            | "compact"
            | "rename"
            | "dropDatabase"
            | "collMod"
    )
}

fn write_capture(dir: &Path, sinks: &Sinks) -> Result<(), String> {
    let commands = sinks.commands.lock().map_err(|err| err.to_string())?.clone();
    let monitor_names = commands
        .iter()
        .filter_map(|event| event.get("command").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    let wire = load_wire(dir)?;
    let mut names = monitor_names.clone();
    for row in &wire {
        if let Some(name) = row.get("command").and_then(Value::as_str) {
            if !names.iter().any(|item| item == name) {
                names.push(name.to_string());
            }
        }
    }
    let handshake_command = wire.first().and_then(|row| row.get("command")).cloned().unwrap_or(Value::Null);
    let auth_observation = auth_observation(&wire);
    write_json(
        &dir.join("commands.json"),
        &json!({
            "names": names,
            "monitor_names": monitor_names,
            "events": commands,
            "wire": wire,
            "handshake_source": "opmsg-wire",
            "handshake_command": handshake_command,
            "auth_observation": auth_observation,
            "secret_body_recorded": false,
        }),
    )?;
    let connections = sinks.connections.lock().map_err(|err| err.to_string())?.clone();
    let created = connections
        .iter()
        .filter(|event| event.get("kind").and_then(Value::as_str) == Some("ConnectionCreated"))
        .count();
    let sdam = sinks.sdam.lock().map_err(|err| err.to_string())?.clone();
    write_json(
        &dir.join("connections.json"),
        &json!({
            "connection_created": created,
            "events": connections,
            "sdam": sdam,
            "pool_bound": { "max_pool_size": 1, "min_pool_size": 0, "max_connecting": 1 },
            "server_monitoring_mode": "Poll",
        }),
    )
}

fn push_sdam(sink: &Mutex<Vec<Value>>, event: SdamEvent) {
    let value = match event {
        SdamEvent::ServerHeartbeatStarted(event) => heartbeat_started(&event),
        SdamEvent::ServerHeartbeatSucceeded(event) => heartbeat_succeeded(&event),
        SdamEvent::ServerHeartbeatFailed(event) => heartbeat_failed(&event),
        _ => json!({ "kind": "other" }),
    };
    if let Ok(mut guard) = sink.lock() {
        guard.push(value);
    }
}

fn heartbeat_started(event: &ServerHeartbeatStartedEvent) -> Value {
    json!({
        "kind": "ServerHeartbeatStarted",
        "address": event.server_address.to_string(),
        "awaited": event.awaited,
        "driver_connection_id": event.driver_connection_id,
        "server_connection_id": event.server_connection_id,
    })
}

fn heartbeat_succeeded(event: &ServerHeartbeatSucceededEvent) -> Value {
    json!({
        "kind": "ServerHeartbeatSucceeded",
        "address": event.server_address.to_string(),
        "awaited": event.awaited,
        "driver_connection_id": event.driver_connection_id,
        "server_connection_id": event.server_connection_id,
        "reply": {
            "ok": bson_ok(event.reply.get("ok")),
            "maxWireVersion": bson_i64(event.reply.get("maxWireVersion")),
            "minWireVersion": bson_i64(event.reply.get("minWireVersion")),
            "hello_ok": event.reply.get("helloOk").and_then(Bson::as_bool),
            "set_name_present": event.reply.contains_key("setName"),
            "speculative_authenticate_present": event.reply.contains_key("speculativeAuthenticate"),
        },
    })
}

fn heartbeat_failed(event: &ServerHeartbeatFailedEvent) -> Value {
    json!({
        "kind": "ServerHeartbeatFailed",
        "address": event.server_address.to_string(),
        "awaited": event.awaited,
        "driver_connection_id": event.driver_connection_id,
        "server_connection_id": event.server_connection_id,
        "message": truncate(&event.failure.to_string(), 180),
    })
}

fn push_command(sink: &Mutex<Vec<Value>>, event: CommandEvent) {
    let value = match event {
        CommandEvent::Started(event) => summarize_started(&event),
        CommandEvent::Succeeded(event) => summarize_succeeded(&event),
        CommandEvent::Failed(event) => summarize_failed(&event),
        _ => json!({ "phase": "other" }),
    };
    if let Ok(mut guard) = sink.lock() {
        guard.push(value);
    }
}

fn push_cmap(sink: &Mutex<Vec<Value>>, event: CmapEvent) {
    let value = match &event {
        CmapEvent::ConnectionCreated(event) => cmap_row("ConnectionCreated", Some(event.connection_id), Some(event.address.to_string())),
        CmapEvent::ConnectionReady(event) => cmap_row("ConnectionReady", Some(event.connection_id), Some(event.address.to_string())),
        CmapEvent::ConnectionClosed(event) => cmap_row("ConnectionClosed", Some(event.connection_id), Some(event.address.to_string())),
        CmapEvent::ConnectionCheckedOut(event) => {
            cmap_row("ConnectionCheckedOut", Some(event.connection_id), Some(event.address.to_string()))
        }
        CmapEvent::ConnectionCheckedIn(event) => cmap_row("ConnectionCheckedIn", Some(event.connection_id), Some(event.address.to_string())),
        CmapEvent::PoolCreated(_) => cmap_row("PoolCreated", None, None),
        CmapEvent::PoolReady(_) => cmap_row("PoolReady", None, None),
        CmapEvent::PoolCleared(_) => cmap_row("PoolCleared", None, None),
        CmapEvent::PoolClosed(_) => cmap_row("PoolClosed", None, None),
        CmapEvent::ConnectionCheckoutStarted(event) => cmap_row("ConnectionCheckoutStarted", None, Some(event.address.to_string())),
        CmapEvent::ConnectionCheckoutFailed(event) => cmap_row("ConnectionCheckoutFailed", None, Some(event.address.to_string())),
        _ => cmap_row("other", None, None),
    };
    if let Ok(mut guard) = sink.lock() {
        guard.push(value);
    }
}

fn cmap_row(kind: &str, connection_id: Option<u32>, address: Option<String>) -> Value {
    json!({ "kind": kind, "connection_id": connection_id, "address": address })
}

fn summarize_started(event: &CommandStartedEvent) -> Value {
    if secret_command(&event.command_name) {
        return json!({
            "phase": "started",
            "command": event.command_name,
            "db": event.db,
            "redacted": true,
            "request_id": event.request_id,
            "connection_id": event.connection.id,
        });
    }
    json!({
        "phase": "started",
        "command": event.command_name,
        "db": event.db,
        "request_id": event.request_id,
        "connection_id": event.connection.id,
        "comment": event.command.get("comment").and_then(Bson::as_str),
        "batch_size": bson_i64(event.command.get("batchSize")),
        "limit": bson_i64(event.command.get("limit")),
        "max_time_ms": bson_i64(event.command.get("maxTimeMS")),
        "collection": command_collection(&event.command),
        "cursor_id": cursor_id_text(&event.command),
        "kill_cursor_ids": kill_ids(&event.command),
        "speculative_authenticate_present": event.command.contains_key("speculativeAuthenticate"),
        "client_app": client_app(&event.command),
    })
}

fn summarize_succeeded(event: &CommandSucceededEvent) -> Value {
    if secret_command(&event.command_name) {
        return json!({
            "phase": "succeeded",
            "command": event.command_name,
            "redacted": true,
            "request_id": event.request_id,
            "connection_id": event.connection.id,
            "duration_ms": event.duration.as_millis() as u64,
        });
    }
    json!({
        "phase": "succeeded",
        "command": event.command_name,
        "request_id": event.request_id,
        "connection_id": event.connection.id,
        "duration_ms": event.duration.as_millis() as u64,
        "ok": bson_ok(event.reply.get("ok")),
        "cursor_id": reply_cursor_id(&event.reply),
        "batch_len": reply_batch_len(&event.reply),
        "ns": reply_ns(&event.reply),
        "hello": hello_fields(&event.command_name, &event.reply),
    })
}

fn summarize_failed(event: &CommandFailedEvent) -> Value {
    let failure = command_failure(&event.failure);
    json!({
        "phase": "failed",
        "command": event.command_name,
        "request_id": event.request_id,
        "connection_id": event.connection.id,
        "duration_ms": event.duration.as_millis() as u64,
        "code": failure.get("code").cloned().unwrap_or(Value::Null),
        "code_name": failure.get("code_name").cloned().unwrap_or(Value::Null),
        "message": failure.get("message").cloned().unwrap_or(Value::Null),
    })
}

fn hello_fields(name: &str, reply: &Document) -> Value {
    if name != "hello" && name != "ismaster" && name != "isMaster" {
        return Value::Null;
    }
    json!({
        "maxWireVersion": bson_i64(reply.get("maxWireVersion")),
        "minWireVersion": bson_i64(reply.get("minWireVersion")),
        "setName_present": reply.contains_key("setName"),
        "msg": reply.get("msg").and_then(Bson::as_str),
        "topology": if reply.contains_key("setName") { "set_name_present" } else { "standalone" },
    })
}

fn command_collection(command: &Document) -> Option<&str> {
    for key in ["find", "aggregate", "count", "collStats", "listIndexes", "killCursors"] {
        if let Some(value) = command.get(key).and_then(Bson::as_str) {
            return Some(value);
        }
    }
    command.get("collection").and_then(Bson::as_str)
}

fn cursor_id_text(command: &Document) -> Option<String> {
    if let Some(value) = bson_i64(command.get("getMore")) {
        return Some(value.to_string());
    }
    None
}

fn kill_ids(command: &Document) -> Vec<String> {
    command
        .get("cursors")
        .and_then(Bson::as_array)
        .map(|items| items.iter().filter_map(|item| bson_i64(Some(item)).map(|value| value.to_string())).collect())
        .unwrap_or_default()
}

fn reply_cursor_id(reply: &Document) -> Option<String> {
    reply
        .get("cursor")
        .and_then(Bson::as_document)
        .and_then(|cursor| bson_i64(cursor.get("id")))
        .map(|value| value.to_string())
}

fn reply_batch_len(reply: &Document) -> Option<usize> {
    let cursor = reply.get("cursor").and_then(Bson::as_document)?;
    let batch = cursor.get("firstBatch").or_else(|| cursor.get("nextBatch"))?;
    batch.as_array().map(Vec::len)
}

fn reply_ns(reply: &Document) -> Option<&str> {
    reply.get("cursor").and_then(Bson::as_document).and_then(|cursor| cursor.get("ns")).and_then(Bson::as_str)
}

fn client_app(command: &Document) -> Option<&str> {
    command
        .get("client")
        .and_then(Bson::as_document)
        .and_then(|client| client.get("application"))
        .and_then(Bson::as_document)
        .and_then(|application| application.get("name"))
        .and_then(Bson::as_str)
}

fn command_reply_summary(document: &Document) -> Value {
    let mut users = Vec::new();
    if let Some(info) = document.get("authInfo").and_then(Bson::as_document) {
        if let Some(rows) = info.get("authenticatedUsers").and_then(Bson::as_array) {
            for row in rows {
                if let Some(user) = row.as_document() {
                    users.push(json!({
                        "user": user.get("user").and_then(Bson::as_str),
                        "db": user.get("db").and_then(Bson::as_str),
                    }));
                }
            }
        }
    }
    let mut roles = Vec::new();
    if let Some(info) = document.get("authInfo").and_then(Bson::as_document) {
        if let Some(rows) = info.get("authenticatedUserRoles").and_then(Bson::as_array) {
            for row in rows {
                if let Some(role) = row.as_document() {
                    roles.push(json!({
                        "role": role.get("role").and_then(Bson::as_str),
                        "db": role.get("db").and_then(Bson::as_str),
                    }));
                }
            }
        }
    }
    json!({
        "ok": bson_ok(document.get("ok")),
        "ns": document.get("ns").and_then(Bson::as_str),
        "count": bson_i64(document.get("count")),
        "version": document.get("version").and_then(Bson::as_str),
        "gitVersion": document.get("gitVersion").and_then(Bson::as_str),
        "modules": document.get("modules").and_then(Bson::as_array).map(|items| {
            items.iter().filter_map(Bson::as_str).collect::<Vec<_>>()
        }),
        "authenticated_users": users,
        "authenticated_roles": roles,
    })
}

fn collection_json(spec: &CollectionSpecification) -> Value {
    json!({
        "name": spec.name,
        "type": format!("{:?}", spec.collection_type),
        "validator_present": spec.options.validator.is_some(),
        "validation_level_present": spec.options.validation_level.is_some(),
        "id_index_present": spec.id_index.is_some(),
    })
}

fn index_json(model: &IndexModel) -> Value {
    let options = model.options.as_ref();
    let collation = options.and_then(|options| options.collation.as_ref());
    json!({
        "name": options.and_then(|options| options.name.clone()),
        "keys": model.keys.iter().map(|(key, value)| json!([key, bson_i64(Some(value))])).collect::<Vec<_>>(),
        "unique": options.and_then(|options| options.unique),
        "collation": collation.map(|collation| json!({
            "locale": collation.locale,
            "strength": collation.strength.map(u32::from),
        })),
    })
}

fn command_failure(err: &Error) -> Value {
    match err.kind.as_ref() {
        ErrorKind::Command(cmd) => json!({
            "code": cmd.code,
            "code_name": cmd.code_name,
            "message": truncate(&cmd.message, 240),
        }),
        other => json!({
            "code": null,
            "code_name": null,
            "message": truncate(&other.to_string(), 240),
        }),
    }
}

fn options_public(options: &ClientOptions) -> Value {
    let credential = options.credential.as_ref();
    json!({
        "retry_writes": options.retry_writes,
        "retry_reads": options.retry_reads,
        "direct_connection": options.direct_connection,
        "max_pool_size": options.max_pool_size,
        "min_pool_size": options.min_pool_size,
        "max_connecting": options.max_connecting,
        "app_name": options.app_name,
        "server_selection_timeout_ms": options.server_selection_timeout.map(|value| value.as_millis() as u64),
        "server_monitoring_mode": mode_name(options.server_monitoring_mode.as_ref()),
        "tls": false,
        "openssl_tls": false,
        "in_use_encryption": false,
        "credential_username": credential.and_then(|value| value.username.clone()),
        "credential_source": credential.and_then(|value| value.source.clone()),
        "credential_password_present": credential.and_then(|value| value.password.as_ref()).is_some(),
    })
}

fn mode_name(mode: Option<&ServerMonitoringMode>) -> &'static str {
    match mode {
        Some(ServerMonitoringMode::Poll) => "Poll",
        Some(ServerMonitoringMode::Stream) => "Stream",
        Some(ServerMonitoringMode::Auto) => "Auto",
        _ => "unset",
    }
}

fn secret_command(name: &str) -> bool {
    matches!(name, "saslStart" | "saslContinue")
}

fn comment(text: &str) -> Bson {
    Bson::String(text.to_string())
}

fn projection() -> Document {
    doc! {
        "_id": 1,
        "code": 1,
        "flag": 1,
        "units": 1,
        "price": 1,
        "created": 1,
        "uid": 1,
        "blob": 1,
        "weight": 1,
        "warehouse": 1,
        "whCode": 1,
        "lane": 1,
        "memo": 1,
        "marks": 1,
        "pack": 1,
    }
}

fn bson_i64(value: Option<&Bson>) -> Option<i64> {
    match value? {
        Bson::Int32(number) => Some(i64::from(*number)),
        Bson::Int64(number) => Some(*number),
        _ => None,
    }
}

fn bson_ok(value: Option<&Bson>) -> Option<i64> {
    match value? {
        Bson::Double(number) if *number == 0.0 || *number == 1.0 => Some(*number as i64),
        other => bson_i64(Some(other)),
    }
}

fn show_err(err: Error) -> String {
    match err.kind.as_ref() {
        ErrorKind::Command(cmd) => format!("command {} {} {}", cmd.code, cmd.code_name, truncate(&cmd.message, 180)),
        other => truncate(&other.to_string(), 300),
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        format!("{}…", &text[..max])
    }
}

fn load_wire(dir: &Path) -> Result<Vec<Value>, String> {
    let path = std::env::var("OO_W141_OPMSG_LOG").unwrap_or_else(|_| dir.join("opmsg-names.jsonl").display().to_string());
    let text = fs::read_to_string(&path).map_err(|err| format!("wire audit: {err}"))?;
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).map_err(|_| "wire audit line".to_string())?;
        let extra = value.as_object().map(|object| {
            object.keys().any(|key| key != "command" && key != "speculative_authenticate")
        });
        if extra != Some(false) {
            return Err("wire audit recorded a field other than the command name".to_string());
        }
        let name = value.get("command").and_then(Value::as_str).unwrap_or("");
        if name.contains("payload") || name.contains("PASSWORD") {
            return Err("wire audit recorded a secret name".to_string());
        }
        rows.push(value);
    }
    if rows.is_empty() {
        return Err("wire audit is empty".to_string());
    }
    Ok(rows)
}

fn auth_observation(wire: &[Value]) -> &'static str {
    let names: Vec<&str> = wire.iter().filter_map(|row| row.get("command").and_then(Value::as_str)).collect();
    if names.contains(&"saslStart") {
        return "saslStart";
    }
    let speculative = wire.iter().any(|row| {
        let command = row.get("command").and_then(Value::as_str);
        let flag = row.get("speculative_authenticate").and_then(Value::as_bool) == Some(true);
        flag && matches!(command, Some("hello" | "ismaster" | "isMaster"))
    });
    if speculative && names.contains(&"saslContinue") {
        return "speculativeAuthenticate+saslContinue";
    }
    "absent"
}

fn wire_has(events: &Value, name: &str) -> bool {
    events
        .get("wire")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|row| row.get("command").and_then(Value::as_str) == Some(name)))
}

fn read_password_file(path: &str) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("password file: {err}"))?;
    let password = text.trim().to_string();
    if password.len() < 12 {
        return Err("password file is too short".to_string());
    }
    Ok(password)
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OO_OUT").unwrap_or_else(|_| "/out".to_string()))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, text + "\n").map_err(|err| err.to_string())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&text).map_err(|err| err.to_string())
}

#[cfg(test)]
mod cancel_record_tests {
    use super::owned_cancel_record;
    use serde_json::json;

    fn sample(kill_id: &str) -> Vec<serde_json::Value> {
        vec![
            json!({"phase":"started","command":"find","comment":"oo-dl01w1-41:reader:cancel","request_id":14}),
            json!({"phase":"succeeded","command":"find","request_id":14,"cursor_id":"3371997369355016991"}),
            json!({"phase":"started","command":"killCursors","request_id":15,"kill_cursor_ids":[kill_id]}),
            json!({"phase":"succeeded","command":"killCursors","request_id":15}),
        ]
    }

    #[test]
    fn owned_cancel_records_the_same_cursor_kill() {
        let row = owned_cancel_record(&sample("3371997369355016991"), true).unwrap();
        assert_eq!(row["method"], "read_role_killCursors");
        assert_eq!(row["cursor_id"], "3371997369355016991");
        assert_eq!(row["server_ended"], true);
        assert_eq!(row["client_cancel"], true);
        assert_eq!(row["dba_kill"], false);
        assert_eq!(row["privilege"], "read");
        assert_eq!(row["promoted"], false);
    }

    #[test]
    fn owned_cancel_rejects_a_different_cursor() {
        assert!(owned_cancel_record(&sample("222"), true).is_err());
    }
}
