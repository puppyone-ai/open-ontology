use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex};

use crate::config::{state_path, Config, CONTRACT, DEPLOYMENT};
use crate::error::{conflict, malformed, not_found, unauthenticated, unavailable, unmapped, Fault};
use crate::forward::{self, Forward, PUBLIC_MARKER};
use crate::gateway::{self, Bearer};
use crate::httputil::{cookie_pair, read_request, write_json, write_response};
use crate::identity::{confirm_presented_csrf, fetch_current};
use crate::mapping::{find_operator, find_row, load_map, self_asserted, MapRow};
use crate::pack::pack_digest;
use crate::release::fetch_active_pack;
use crate::sessionapi::{self, receipt_closed, SessionDoor};
use crate::sha::{hex_encode, random_bytes, sha256};
use pi_harness::{ProductEvent, StreamItem, Termination};

use crate::turn::{
    self, accept_prompt, complete_decision, gold_for, launch_pi, next_item, prompt_for, redact_value, CompleteInput, SessionDirs,
};
use crate::PARENT_ACCEPTANCE;

struct Session {
    subject: String,
    management_role: String,
    data_role: String,
    principal: String,
    pack_sha: String,
    door: SessionDoor,
    token_file: PathBuf,
    token: String,
    dirs: SessionDirs,
    events: Vec<Value>,
    seq: u64,
    cancel: Option<watch::Sender<bool>>,
    active: bool,
    harness_pid: Option<u32>,
}

struct App {
    config: Config,
    rows: Vec<MapRow>,
    sessions: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
    bearers: Arc<Mutex<HashMap<String, Bearer>>>,
    gateway_origin: String,
    forward: Forward,
}

pub async fn serve(config: Config) -> Result<(), Fault> {
    fs::create_dir_all(&config.state_dir).map_err(|_| unavailable("state"))?;
    let rows = load_map(&config.mapping_path)?;
    let bearers = Arc::new(Mutex::new(HashMap::new()));
    let gateway_port = gateway::serve(&config, Arc::clone(&bearers)).await?;
    let forward = forward::serve(config.upstream_token_file.clone(), forward::note_path(&config.state_dir)).await?;
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| unavailable("bind"))?;
    let port = listener.local_addr().map_err(|_| unavailable("bind"))?.port();
    fs::write(state_path(&config, "port.txt"), format!("{port}\n")).map_err(|_| unavailable("port"))?;
    let line = json!({
        "contract": CONTRACT,
        "listening": true,
        "port": port,
        "gateway_port": gateway_port,
        "forward_port": forward.port,
        "production_signed": false,
        "deployment": DEPLOYMENT,
        "parent_acceptance": PARENT_ACCEPTANCE,
        "producer_query": "query-pg",
        "product_http_sql": "unsupported",
        "product_route": "mtls_health",
        "pack_mode": config.pack_mode.clone(),
    });
    println!("{line}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let app = Arc::new(App {
        config,
        rows,
        sessions: Mutex::new(HashMap::new()),
        bearers,
        gateway_origin: format!("http://127.0.0.1:{gateway_port}"),
        forward,
    });
    loop {
        let (sock, _) = listener.accept().await.map_err(|_| unavailable("accept"))?;
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            let _ = dispatch(sock, app).await;
        });
    }
}

async fn dispatch(mut sock: TcpStream, app: Arc<App>) -> Result<(), Fault> {
    let incoming = match read_request(&mut sock, 65_536).await {
        Ok(incoming) => incoming,
        Err(fault) => {
            write_error(&mut sock, &fault).await?;
            return Ok(());
        }
    };
    let result = route(&app, &incoming).await;
    match result {
        Ok((status, body, csrf)) => {
            let mut extra = Vec::new();
            if let Some(token) = csrf.as_deref() {
                extra.push(("x-oo-csrf", token));
            }
            write_response(&mut sock, status, "application/json", &extra, &body).await?;
        }
        Err(fault) => write_error(&mut sock, &fault).await?,
    }
    Ok(())
}

async fn write_error(sock: &mut TcpStream, fault: &Fault) -> Result<(), Fault> {
    let body = json!({"contract": CONTRACT, "error": fault.reason, "parent_acceptance": PARENT_ACCEPTANCE}).to_string();
    write_json(sock, fault.status, body.as_bytes()).await
}

type Reply = Result<(u16, Vec<u8>, Option<String>), Fault>;

async fn route(app: &App, incoming: &crate::httputil::Incoming) -> Reply {
    let parts: Vec<&str> = incoming.path.split('/').filter(|part| !part.is_empty()).collect();
    let value = if incoming.body.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(&incoming.body).map_err(|_| malformed("json"))?
    };
    if incoming.method == "POST" && self_asserted(&value) {
        return Err(malformed("self_asserted_rejected"));
    }
    match (incoming.method.as_str(), parts.as_slice()) {
        ("POST", ["entry", "v1", "sessions"]) => create(app, incoming, &value).await,
        ("POST", ["entry", "v1", "sessions", id, "turns"]) => begin_turn(app, incoming, id, &value).await,
        ("GET", ["entry", "v1", "sessions", id, "events"]) => events(app, id, &incoming.query).await,
        ("POST", ["entry", "v1", "sessions", id, "cancel"]) => cancel(app, incoming, id).await,
        ("POST", ["entry", "v1", "sessions", id, "close"]) => close(app, incoming, id).await,
        _ => Err(not_found()),
    }
}

fn presented_cookie(header: &str) -> Result<String, Fault> {
    cookie_pair(header, "__Host-oo-session").filter(|value| !value.is_empty()).ok_or_else(unauthenticated)
}

async fn establish(app: &App, incoming: &crate::httputil::Incoming) -> Result<crate::identity::Current, Fault> {
    let cookie = presented_cookie(&incoming.cookie)?;
    confirm_presented_csrf(&app.config, &cookie, &incoming.csrf).await?;
    fetch_current(&app.config, &cookie).await
}

struct Bound {
    subject: String,
    management_role: String,
    data_role: String,
    principal: String,
    project_id: String,
    csrf: Option<String>,
}

fn bound_from(row: &MapRow, csrf: Option<String>) -> Bound {
    Bound {
        subject: row.subject.clone(),
        management_role: row.management_role.clone(),
        data_role: row.data_role.clone(),
        principal: row.session_principal.clone(),
        project_id: row.project_id.clone(),
        csrf,
    }
}

async fn bind_actor(app: &App, incoming: &crate::httputil::Incoming) -> Result<Bound, Fault> {
    if app.config.pack_mode == "development_draft" {
        return Ok(bound_from(operator_row(app, incoming)?, None));
    }
    let current = establish(app, incoming).await?;
    if current.install_state != "initialized" || current.data_grants != "none" {
        return Err(malformed("identity"));
    }
    let Some(row) = find_row(&app.rows, &current.subject, &current.project_id, &current.role) else {
        return Err(unmapped());
    };
    Ok(bound_from(row, Some(current.csrf_token)))
}

async fn create(app: &App, incoming: &crate::httputil::Incoming, _value: &Value) -> Reply {
    let bound = bind_actor(app, incoming).await?;
    let pack_dir = Path::new(&app.config.pack_root).join(&bound.data_role);
    let digest = pack_digest(&pack_dir)?;
    if app.config.pack_mode == "active_release" {
        let active = fetch_active_pack(&app.config.release_origin, &bound.project_id, &bound.data_role).await?;
        if digest != active.pack_sha256 || active.role != bound.data_role || !active.published {
            return Err(conflict("pack_unpublished"));
        }
    }
    let door = sessionapi::open_conversation(&app.config, &bound.principal).await?;
    let id = format!("s{}", &hex_encode(&random_bytes(16).map_err(|_| unavailable("random"))?)[..32]);
    let root = Path::new(&app.config.state_dir).join("sessions").join(&id);
    let dirs = SessionDirs {
        pack: pack_dir,
        workspace: root.join("workspace"),
        home: root.join("home"),
        agent: root.join("agent"),
        pi_session: root.join("pi-session"),
    };
    for path in [&dirs.workspace, &dirs.home, &dirs.agent, &dirs.pi_session] {
        fs::create_dir_all(path).map_err(|_| unavailable("session_dir"))?;
    }
    let token_file = root.join("gateway.token");
    write_secret(&token_file, PUBLIC_MARKER)?;
    app.bearers.lock().await.insert(id.clone(), Bearer {
        data_role: bound.data_role.clone(),
        principal: bound.principal.clone(),
        session_id: id.clone(),
        project_id: bound.project_id.clone(),
        pack_sha256: digest.clone(),
        turn_active: false,
    });
    let session = Session {
        subject: bound.subject.clone(),
        management_role: bound.management_role.clone(),
        data_role: bound.data_role.clone(),
        principal: bound.principal.clone(),
        pack_sha: digest.clone(),
        door,
        token_file,
        token: PUBLIC_MARKER.to_owned(),
        dirs,
        events: Vec::new(),
        seq: 0,
        cancel: None,
        active: false,
        harness_pid: None,
    };
    let body = json!({
        "contract": CONTRACT,
        "operation": "createSession",
        "session_id": id,
        "project_id": bound.project_id,
        "management_role": session.management_role,
        "data_role": session.data_role,
        "pack_sha256": session.pack_sha,
        "deployment": DEPLOYMENT,
        "production_signed": false,
        "runsc": "absent_development_local_process",
        "conversation_id": session.door.conversation_id,
        "parent_acceptance": PARENT_ACCEPTANCE,
        "runtime": "development_local_process",
        "sandbox_certificate": false,
    });
    let csrf = bound.csrf.clone();
    app.sessions.lock().await.insert(id, Arc::new(Mutex::new(session)));
    Ok((201, serde_json::to_vec(&body).map_err(|_| unavailable("json"))?, csrf))
}

async fn begin_turn(app: &App, incoming: &crate::httputil::Incoming, id: &str, value: &Value) -> Reply {
    let (subject, next) = authorize(app, incoming).await?;
    let text = value.get("text").and_then(Value::as_str).ok_or_else(|| malformed("turn"))?;
    if value.as_object().is_none_or(|object| object.len() != 1) || text.is_empty() || text.len() > 2_000 || text.contains('\0') {
        return Err(malformed("turn"));
    }
    let text = text.to_owned();
    let session = lookup(app, id).await?;
    {
        let guard = session.lock().await;
        same_subject(&guard.subject, &subject)?;
        if guard.active {
            return Err(conflict("turn_active"));
        }
    }
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let turn_id = format!("t{}", &hex_encode(&random_bytes(8).map_err(|_| unavailable("random"))?)[..16]);
    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut guard = session.lock().await;
        guard.active = true;
        guard.cancel = Some(cancel_tx);
    }
    let session_task = Arc::clone(&session);
    let config = app.config.clone();
    let gateway_origin = app.gateway_origin.clone();
    let forward_port = app.forward.port;
    let bearers = Arc::clone(&app.bearers);
    let statuses = Arc::clone(&app.forward.statuses);
    let turn_for_task = turn_id.clone();
    let session_id = id.to_owned();
    tokio::spawn(async move {
        let _ = run_turn(session_task, cancel_rx, Some(tx), turn_for_task, session_id, text, config, gateway_origin, forward_port, bearers, statuses).await;
    });
    let pid = match rx.await {
        Ok(Ok(pid)) => pid,
        Ok(Err(fault)) => {
            let mut guard = session.lock().await;
            guard.active = false;
            return Err(fault);
        }
        Err(_) => {
            let mut guard = session.lock().await;
            guard.active = false;
            return Err(unavailable("turn"));
        }
    };
    let body = json!({
        "contract": CONTRACT,
        "operation": "beginTurn",
        "session_id": id,
        "turn_id": turn_id,
        "status": "accepted",
        "model_http_status": null,
        "complete": false,
        "harness_pid": pid,
        "parent_acceptance": PARENT_ACCEPTANCE,
    });
    Ok((202, serde_json::to_vec(&body).map_err(|_| unavailable("json"))?, next))
}

async fn run_turn(
    session: Arc<Mutex<Session>>,
    mut cancel_rx: watch::Receiver<bool>,
    mut ready: Option<tokio::sync::oneshot::Sender<Result<u32, Fault>>>,
    turn_id: String,
    session_id: String,
    question: String,
    config: Config,
    gateway_origin: String,
    forward_port: u16,
    bearers: Arc<Mutex<HashMap<String, Bearer>>>,
    statuses: Arc<Mutex<Vec<u16>>>,
) -> Result<(), Fault> {
    gateway::set_turn(&bearers, &session_id, true).await;
    let outcome = drive(&session, &mut cancel_rx, &mut ready, &turn_id, &question, &config, &gateway_origin, forward_port, &statuses).await;
    gateway::set_turn(&bearers, &session_id, false).await;
    if let Err(fault) = &outcome {
        if let Some(sender) = ready.take() {
            let _ = sender.send(Err(fault.clone()));
        }
        let mut guard = session.lock().await;
        guard.active = false;
    }
    outcome.map(|_| ())
}

async fn drive(
    session: &Mutex<Session>,
    cancel_rx: &mut watch::Receiver<bool>,
    ready: &mut Option<tokio::sync::oneshot::Sender<Result<u32, Fault>>>,
    turn_id: &str,
    question: &str,
    config: &Config,
    gateway_origin: &str,
    forward_port: u16,
    statuses: &Mutex<Vec<u16>>,
) -> Result<(), Fault> {
    let (dirs, token_file, principal, token) = {
        let guard = session.lock().await;
        (guard.dirs.clone(), guard.token_file.clone(), guard.principal.clone(), guard.token.clone())
    };
    link_knowledge(&dirs.pack)?;
    let receipt_path = state_path(config, "query-receipts.jsonl");
    let before = gateway::audit_rows(&receipt_path, &principal)?.len();
    let status_before = statuses.lock().await.len();
    let prompt = prompt_for(&config.oo_bin, &token_file.display().to_string(), gateway_origin, question);
    if prompt.contains("90000") || prompt.contains("shop.entry") || prompt.contains("count(*)") {
        return Err(malformed("prompt"));
    }
    if *cancel_rx.borrow() {
        push(session, turn_id, "incomplete", json!({"reason": "cancelled", "turn_id": turn_id})).await;
        let mut guard = session.lock().await;
        guard.active = false;
        return Err(unavailable("cancelled"));
    }
    let origin = format!("http://127.0.0.1:{forward_port}/v1");
    let mut launched = launch_pi(config, &dirs, &origin).await?;
    accept_prompt(&launched.harness, &prompt).await?;
    let pid = launched.harness.child_pid();
    {
        let mut guard = session.lock().await;
        guard.harness_pid = Some(pid);
    }
    if let Some(sender) = ready.take() {
        let _ = sender.send(Ok(pid));
    }
    push(session, turn_id, "turn_started", json!({"turn_id": turn_id})).await;
    let secrets = vec![token];
    let mut tool_ran = false;
    let mut answer = String::new();
    let mut cancelled = false;
    let mut stream_broken = false;
    let mut break_reason = String::new();
    loop {
        tokio::select! {
            biased;
            changed = cancel_rx.changed() => {
                if changed.is_err() || *cancel_rx.borrow() {
                    cancelled = true;
                    let _ = launched.harness.abort().await;
                    break;
                }
            }
            item = next_item(&mut launched.events) => {
                match item {
                    StreamItem::Event(sequenced) => {
                        match &sequenced.event {
                            ProductEvent::ToolCallStarted { .. } | ProductEvent::ToolCallFinished { .. } => tool_ran = true,
                            ProductEvent::TurnFinished { answer: Some(text), .. } => {
                                answer = text.clone();
                            }
                            _ => {}
                        }
                        let (kind, mut fields) = turn::map_product(&sequenced.event);
                        if fields.get("turn_id").is_none() {
                            fields["turn_id"] = json!(turn_id);
                        }
                        if kind == "model_stop" {
                            let guard = statuses.lock().await;
                            if let Some(status) = guard.get(status_before..).and_then(|slice| slice.last().copied()) {
                                fields["model_http_status"] = json!(status);
                            }
                        }
                        redact_value(&mut fields, &secrets);
                        push(session, turn_id, kind, fields).await;
                    }
                    StreamItem::Terminated(Termination::Finished) => break,
                    StreamItem::Terminated(Termination::Incomplete { reason }) => {
                        if *cancel_rx.borrow() || reason == "aborted" {
                            cancelled = true;
                        } else {
                            stream_broken = true;
                            break_reason = reason;
                        }
                        break;
                    }
                }
            }
        }
    }
    let model_status = {
        let guard = statuses.lock().await;
        guard.get(status_before..).and_then(|slice| slice.last().copied())
    };
    if cancelled || *cancel_rx.borrow() {
        push(session, turn_id, "incomplete", json!({"reason": "cancelled", "turn_id": turn_id})).await;
    } else if stream_broken {
        push(session, turn_id, "incomplete", json!({"reason": break_reason, "turn_id": turn_id})).await;
    } else {
        let rows = gateway::audit_rows(&receipt_path, &principal)?;
        let cited = turn::citations(&answer);
        let gold = gold_for(question);
        let mut finished = false;
        for (query_id, result_text) in rows.iter().skip(before) {
            let decision = complete_decision(&CompleteInput {
                tool_ran,
                audit_query_id: Some(query_id),
                cited: &cited,
                result_text: Some(result_text),
                gold,
                stream_broken: false,
                cancelled: false,
            });
            if decision {
                push(session, turn_id, "citation", json!({"turn_id": turn_id, "query_id": query_id})).await;
                let mut fields = json!({
                    "turn_id": turn_id,
                    "query_id": query_id,
                    "complete": true,
                    "text": result_text,
                });
                if let Some(status) = model_status {
                    fields["model_http_status"] = json!(status);
                }
                push(session, turn_id, "turn_finished", fields).await;
                finished = true;
                break;
            }
        }
        if !finished && !tool_ran && rows.len() == before {
            let shown = turn::redact(&answer, &secrets);
            push(session, turn_id, "clarification", json!({"turn_id": turn_id, "text": shown})).await;
        } else if !finished {
            push(session, turn_id, "incomplete", json!({"reason": "no-citation", "turn_id": turn_id})).await;
        }
    }
    let mut guard = session.lock().await;
    guard.active = false;
    guard.harness_pid = None;
    Ok(())
}

async fn events(app: &App, id: &str, query: &str) -> Reply {
    let after = event_after(query)?;
    let session = lookup(app, id).await?;
    let guard = session.lock().await;
    let last = guard.seq;
    if after > last {
        return Err(conflict("gap"));
    }
    let events: Vec<_> = guard.events.iter().filter(|event| event.get("sequence").and_then(Value::as_u64).unwrap_or(0) > after).cloned().collect();
    let body = json!({"contract": CONTRACT, "session_id": id, "events": events});
    Ok((200, serde_json::to_vec(&body).map_err(|_| unavailable("json"))?, None))
}

fn event_after(query: &str) -> Result<u64, Fault> {
    if query.is_empty() {
        return Ok(0);
    }
    let mut after = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(|| malformed("query"))?;
        if key == "after" {
            after = Some(value.parse::<u64>().map_err(|_| malformed("query"))?);
        } else {
            return Err(malformed("query"));
        }
    }
    after.ok_or_else(|| malformed("query"))
}

async fn cancel(app: &App, incoming: &crate::httputil::Incoming, id: &str) -> Reply {
    let (subject, next) = authorize(app, incoming).await?;
    let session = lookup(app, id).await?;
    {
        let guard = session.lock().await;
        same_subject(&guard.subject, &subject)?;
        if let Some(sender) = &guard.cancel {
            let _ = sender.send(true);
        }
    }
    let body = json!({
        "contract": CONTRACT,
        "operation": "cancelSession",
        "session_id": id,
        "status": "incomplete",
        "reason": "cancelled",
        "turn_finished": false,
        "parent_acceptance": PARENT_ACCEPTANCE,
    });
    Ok((200, serde_json::to_vec(&body).map_err(|_| unavailable("json"))?, next))
}

async fn close(app: &App, incoming: &crate::httputil::Incoming, id: &str) -> Reply {
    let (subject, next) = authorize(app, incoming).await?;
    let session = lookup(app, id).await?;
    let door = {
        let guard = session.lock().await;
        same_subject(&guard.subject, &subject)?;
        if guard.active {
            return Err(conflict("turn_active"));
        }
        guard.door.clone()
    };
    let (status, receipt) = sessionapi::release_close(&app.config, &door).await?;
    let closed = receipt_closed(status, &receipt);
    let body = json!({
        "contract": CONTRACT,
        "operation": "closeSession",
        "session_id": id,
        "parent_acceptance": PARENT_ACCEPTANCE,
        "production_signed": false,
        "receipt": receipt,
    });
    if closed {
        app.bearers.lock().await.remove(id);
    }
    let http = if closed { 200 } else { 409 };
    Ok((http, serde_json::to_vec(&body).map_err(|_| unavailable("json"))?, next))
}

fn link_knowledge(pack: &Path) -> Result<(), Fault> {
    let link = Path::new("/knowledge");
    match fs::symlink_metadata(link) {
        Ok(meta) if meta.file_type().is_symlink() => fs::remove_file(link).map_err(|_| unavailable("knowledge"))?,
        Ok(_) => return Err(unavailable("knowledge")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(unavailable("knowledge")),
    }
    std::os::unix::fs::symlink(pack, link).map_err(|_| unavailable("knowledge"))
}

async fn authorize(app: &App, incoming: &crate::httputil::Incoming) -> Result<(String, Option<String>), Fault> {
    if app.config.pack_mode == "development_draft" {
        let row = operator_row(app, incoming)?;
        return Ok((row.subject.clone(), None));
    }
    let current = establish(app, incoming).await?;
    if find_row(&app.rows, &current.subject, &current.project_id, &current.role).is_none() {
        return Err(unmapped());
    }
    Ok((current.subject, Some(current.csrf_token)))
}

fn operator_row<'a>(app: &'a App, incoming: &crate::httputil::Incoming) -> Result<&'a MapRow, Fault> {
    let token = bearer_token(&incoming.authorization)?;
    if token == PUBLIC_MARKER || !operator_token_ok(token) {
        return Err(unauthenticated());
    }
    let hash = hex_encode(&sha256(token.as_bytes()));
    find_operator(&app.rows, &hash).ok_or_else(unauthenticated)
}

fn bearer_token(header: &str) -> Result<&str, Fault> {
    let rest = header.strip_prefix("Bearer ").ok_or_else(unauthenticated)?;
    if rest.is_empty() || rest.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(unauthenticated());
    }
    Ok(rest)
}

fn operator_token_ok(token: &str) -> bool {
    (43..=256).contains(&token.len()) && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn same_subject(session_subject: &str, subject: &str) -> Result<(), Fault> {
    if session_subject == subject {
        Ok(())
    } else {
        Err(unauthenticated())
    }
}

async fn lookup(app: &App, id: &str) -> Result<Arc<Mutex<Session>>, Fault> {
    app.sessions.lock().await.get(id).cloned().ok_or_else(not_found)
}

fn write_secret(path: &Path, token: &str) -> Result<(), Fault> {
    if path.exists() {
        return Err(unavailable("token_file"));
    }
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(path).map_err(|_| unavailable("token_file"))?;
    use std::io::Write;
    file.write_all(token.as_bytes()).map_err(|_| unavailable("token_file"))?;
    file.sync_all().map_err(|_| unavailable("token_file"))?;
    Ok(())
}

async fn push(session: &Mutex<Session>, _id: &str, kind: &str, fields: Value) {
    let mut guard = session.lock().await;
    guard.seq += 1;
    let event = turn::event_value(guard.seq, kind, fields);
    let path = guard.dirs.workspace.join("events.jsonl");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(file, "{event}");
    }
    guard.events.push(event);
}


