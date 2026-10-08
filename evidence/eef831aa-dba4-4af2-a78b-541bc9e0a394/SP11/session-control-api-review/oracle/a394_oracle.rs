#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use session_control_api::{
    fixture_stage, load_config, CancelTrigger, Config, OWNER_UUID,
};
use session_store::{
    get, transition, Disposition, Endpoint, Event, IdentityTuple, Outcome, Reason, TransitionRequest,
};
use session_store::rustls_connector;
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;

const MARKER: &str = "a394-transcript-marker";

struct Facts {
    path: PathBuf,
    body: serde_json::Map<String, Value>,
}

impl Facts {
    fn set(&mut self, key: &str, value: Value) {
        self.body.insert(key.to_owned(), value);
        let _ = fs::write(&self.path, serde_json::to_vec_pretty(&self.body).unwrap_or_default());
    }
}

struct ExitChild(Option<Child>);

impl Drop for ExitChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Clone)]
struct Sample {
    revocation_rows: i64,
    mapping_state: String,
    permit_state: String,
    backend_count: i64,
    harness_alive: bool,
    harness_class: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_rows_revoke_and_restart_is_observed() {
    let fixture = PathBuf::from(std::env::var("OO_A394_FIXTURE").expect("OO_A394_FIXTURE"));
    let report_path = PathBuf::from(std::env::var("OO_A394_REPORT").expect("OO_A394_REPORT"));
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut facts = Facts { path: report_path, body: serde_json::Map::new() };
    assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    facts.set("review", json!("a394"));
    facts.set("sha256_abc", json!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    facts.set("product_owner_uuid_constant", json!(OWNER_UUID));
    facts.set("api_listen", json!("http/1.1 127.0.0.1 ephemeral"));
    facts.set("pg_sslmode", json!("require"));

    let root = PathBuf::from("/tmp/oo-a394-review");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("grants")).unwrap();
    fs::create_dir_all(root.join("run")).unwrap();
    write_bindings(&root.join("bindings.json"));
    let config_path = root.join("config.json");
    write_config(&config_path, &fixture, &root, "localprocess", None);
    let bin = env!("CARGO_BIN_EXE_session-control-api");

    let (code, out, _) = run_to_exit(bin, "install", &config_path, 60);
    assert_eq!(code, Some(0), "install");
    assert!(out.contains("\"production_signed\":false"));
    facts.set("install_exit", json!(0));
    facts.set("install_production_signed", json!(false));

    let mut grants = Vec::new();
    for (principal, project) in [("alice", "proj-a"), ("bob", "proj-a"), ("dana", "proj-b")] {
        write_config(
            &config_path,
            &fixture,
            &root,
            "localprocess",
            Some((principal, project, root.join("grants").join(format!("{principal}.grant")))),
        );
        let (code, _, _) = run_to_exit(bin, "issue-grant", &config_path, 30);
        assert_eq!(code, Some(0), "issue {principal}");
        grants.push(fs::read_to_string(root.join("grants").join(format!("{principal}.grant"))).unwrap());
    }

    write_config(&config_path, &fixture, &root, "production", None);
    let (code, _, err) = run_to_exit(bin, "serve", &config_path, 20);
    assert_eq!(code, Some(2), "production serve");
    let production_reason = if err.contains("runtime_unavailable") {
        "runtime_unavailable"
    } else if err.contains("production_driver_absent") {
        "production_driver_absent"
    } else {
        "other"
    };
    assert_ne!(production_reason, "other");
    facts.set("production_serve_exit", json!(2));
    facts.set("production_reason", json!(production_reason));
    facts.set("production_port_written", json!(port_file_ready(&root.join("run/port.txt"))));
    assert!(!port_file_ready(&root.join("run/port.txt")), "production wrote a listener");

    write_config(&config_path, &fixture, &root, "localprocess", None);
    let cfg = load_config(&fs::read(&config_path).unwrap()).unwrap();
    let endpoint = Endpoint {
        host: cfg.pg.host.clone(),
        port: cfg.pg.port,
        database: cfg.pg.database.clone(),
    };
    let obs = Obs::connect(&cfg).await;
    let ssl = own_ssl(&obs.api).await;
    facts.set("pg_stat_ssl_own_backend", json!(ssl));
    assert_eq!(ssl, "true", "api runtime connection was not reported ssl");

    let mut server = spawn_server(bin, &config_path);
    let port = wait_port(&root.join("run/port.txt"), &mut server);
    let mut guard = ExitChild(Some(server));

    let alice = consume(port, &grants[0]);
    let bob = consume(port, &grants[1]);
    let dana = consume(port, &grants[2]);
    let again = http(port, "POST", "/control/v1/entry/session/consume", "", "", &json!({"grant": grants[0].trim()}).to_string());
    assert_eq!(again.0, 409, "second consume");
    let current = http(port, "GET", "/control/v1/entry/session/current", &alice.cookie, "", "");
    assert_eq!(current.0, 200, "current");
    let current_body: Value = serde_json::from_str(&current.2).unwrap();
    assert_eq!(current_body["principal_id"], "alice");
    assert_eq!(current_body["project_id"], "proj-a");
    facts.set("current_principal", json!("alice"));
    facts.set("current_project", json!("proj-a"));
    let forged = http(port, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, &json!({"principal_id":"bob"}).to_string());
    assert_eq!(forged.0, 422, "self asserted body");
    facts.set("self_asserted_status", json!(forged.0));

    let created = http(port, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, "{}");
    assert_eq!(created.0, 201, "create");
    let created_body: Value = serde_json::from_str(&created.2).unwrap();
    let conversation = created_body["conversation_id"].as_str().unwrap().to_owned();
    let session = created_body["session_id"].as_str().unwrap().to_owned();
    assert_eq!(created_body["production_signed"], false);
    let dana_created = http(port, "POST", "/control/v1/conversations", &dana.cookie, &dana.csrf, "{}");
    assert_eq!(dana_created.0, 201, "dana create");
    let dana_body: Value = serde_json::from_str(&dana_created.2).unwrap();
    let dana_conversation = dana_body["conversation_id"].as_str().unwrap().to_owned();

    let bob_get = http(port, "GET", &format!("/control/v1/conversations/{conversation}"), &bob.cookie, "", "");
    let dana_get = http(port, "GET", &format!("/control/v1/conversations/{conversation}"), &dana.cookie, "", "");
    let alice_on_dana = http(port, "GET", &format!("/control/v1/conversations/{dana_conversation}"), &alice.cookie, "", "");
    let bob_list = http(port, "GET", "/control/v1/conversations", &bob.cookie, "", "");
    let dana_list = http(port, "GET", "/control/v1/conversations", &dana.cookie, "", "");
    assert_eq!(bob_get.0, 403, "cross person");
    assert_eq!(dana_get.0, 403, "cross project");
    assert_eq!(alice_on_dana.0, 403, "alice on dana project");
    assert!(!bob_list.2.contains(&conversation), "bob list");
    assert!(!dana_list.2.contains(&conversation), "dana list");
    assert!(!bob_get.2.contains(MARKER) && !dana_get.2.contains(MARKER));
    facts.set("cross_person_status", json!(bob_get.0));
    facts.set("cross_project_status", json!(dana_get.0));
    facts.set("cross_project_reverse_status", json!(alice_on_dana.0));

    let before = obs.snapshot("proj-a", &session, &root).await;
    facts.set("before_turn", before.summary());
    assert_eq!(before.mapping_state, "present");
    assert_eq!(before.permit_state, "held");
    assert_eq!(before.token_rows, 1);
    assert_eq!(before.revocation_rows, 0);
    assert!(before.backend_pid.is_none(), "backend existed before turn");
    assert!(before.harness_pid.is_none(), "harness existed before turn");
    assert!(!before.checkpoint_exists, "checkpoint existed before cancel");
    assert_eq!(before.map_ip, "10.88.0.8");

    let early = http(port, "POST", &format!("/control/v1/conversations/{conversation}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(early.0, 409, "early cancel");
    let early_body: Value = serde_json::from_str(&early.2).unwrap();
    assert_eq!(early_body["reason"], "backend_absent");
    assert_eq!(early_body["retryable"], true);
    assert_ne!(early_body["phase"], "closed");
    let still = obs.snapshot("proj-a", &session, &root).await;
    assert_eq!(still.permit_state, "held");
    assert_eq!(still.mapping_state, "present");
    assert_eq!(still.revocation_rows, 0);
    facts.set("early_cancel_status", json!(409));
    facts.set("early_cancel_left_credentials", json!(true));

    let turned = http(
        port,
        "POST",
        &format!("/control/v1/conversations/{conversation}/turns"),
        &alice.cookie,
        &alice.csrf,
        &json!({"turn_id":"turn-1","text": MARKER}).to_string(),
    );
    assert_eq!(turned.0, 200, "turn");
    let running = obs.snapshot("proj-a", &session, &root).await;
    facts.set("after_turn", running.summary());
    assert!(running.backend_pid.is_some(), "backend pid missing");
    assert!(running.backend_count >= 1, "pg_sleep not visible");
    assert!(running.harness_pid.is_none(), "a harness pid was stored before cancel");
    assert!(!running.checkpoint_exists, "checkpoint before cancel");
    assert!(running.workspace_exists, "workspace missing after turn");
    assert!(running.transcript_exists, "transcript missing");
    assert_eq!(running.revocation_rows, 0);
    assert_eq!(running.mapping_state, "present");
    assert_eq!(running.permit_state, "held");
    assert_eq!(running.map_ip, "10.88.0.8");
    assert!(running.turn_event, "turn event missing");
    let too_many = http(port, "GET", &format!("/control/v1/conversations/{conversation}/events?limit=51"), &alice.cookie, "", "");
    assert_eq!(too_many.0, 422, "limit");

    let head = get(
        &endpoint,
        "oo_session_runtime",
        &cfg.pg.session_runtime_password,
        cfg.pg.ca_pem.as_bytes(),
        "proj-a",
        &session,
    )
    .await
    .expect("head");
    assert!(head.revision >= 2, "revision {}", head.revision);
    assert!(head.owner_generation >= 1, "generation {}", head.owner_generation);
    assert!(!head.physical_release_certified);
    let identity = head.state.bound.clone().expect("bound identity");
    let revision_before = head.revision;
    let generation_before = head.owner_generation;
    let phase_before = serde_json::to_value(head.state.phase).unwrap();
    refuse_stale(&cfg, &endpoint, &identity, &session, &head.state, revision_before.saturating_sub(1), generation_before, "a394-stale-revision").await;
    refuse_stale(&cfg, &endpoint, &identity, &session, &head.state, revision_before, generation_before - 1, "a394-old-generation").await;
    let after_neg = get(
        &endpoint,
        "oo_session_runtime",
        &cfg.pg.session_runtime_password,
        cfg.pg.ca_pem.as_bytes(),
        "proj-a",
        &session,
    )
    .await
    .expect("head after negative");
    assert_eq!(after_neg.revision, revision_before);
    assert_eq!(after_neg.owner_generation, generation_before);
    assert_eq!(serde_json::to_value(after_neg.state.phase).unwrap(), phase_before);
    assert!(!after_neg.physical_release_certified);
    facts.set("stale_revision_refused", json!(true));
    facts.set("old_generation_refused", json!(true));
    facts.set("negative_left_revision", json!(revision_before));
    facts.set("negative_left_generation", json!(generation_before));
    facts.set("store_get_certified_flag", json!(false));

    let slot = running.pool_slot.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let samples = Arc::new(Mutex::new(Vec::<Sample>::new()));
    let stop_poll = Arc::clone(&stop);
    let samples_poll = Arc::clone(&samples);
    let cfg_poll = cfg.clone();
    let session_poll = session.clone();
    let root_poll = root.clone();
    let poller = tokio::spawn(async move {
        let watch = Obs::connect(&cfg_poll).await;
        let started = Instant::now();
        while !stop_poll.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(40) {
            let snap = watch.snapshot("proj-a", &session_poll, &root_poll).await;
            samples_poll.lock().expect("samples").push(Sample {
                revocation_rows: snap.revocation_rows,
                mapping_state: snap.mapping_state.clone(),
                permit_state: snap.permit_state.clone(),
                backend_count: snap.backend_count,
                harness_alive: snap.harness_alive,
                harness_class: snap.harness_class.clone(),
            });
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    });
    thread::sleep(Duration::from_millis(80));
    let cancel_path = format!("/control/v1/conversations/{conversation}/cancel");
    let cookie = alice.cookie.clone();
    let csrf = alice.csrf.clone();
    let closed = tokio::task::spawn_blocking(move || http(port, "POST", &cancel_path, &cookie, &csrf, "{}"))
    .await
    .expect("join");
    stop.store(true, Ordering::Relaxed);
    poller.await.expect("poller");
    assert_eq!(closed.0, 200, "cancel status");
    assert_closed(&closed.2, "closed");
    let taken = samples.lock().expect("samples").clone();
    let live_while_revoked = taken.iter().any(|sample| {
        sample.revocation_rows >= 1
            && sample.mapping_state == "deleted"
            && sample.permit_state == "released"
            && (sample.backend_count > 0 || sample.harness_alive)
    });
    let fixture_child_seen = taken.iter().any(|sample| sample.harness_alive && sample.harness_class == "release_fixture_bash");
    facts.set("poll_samples", json!(taken.len()));
    facts.set("revoked_while_backend_or_fixture_child_alive", json!(live_while_revoked));
    facts.set("release_spawned_fixture_child_seen", json!(fixture_child_seen));
    facts.set("preowned_harness_before_cancel", json!(false));
    assert!(live_while_revoked, "did not observe revocation while backend or fixture child was still alive, samples={}", taken.len());

    let done = obs.snapshot("proj-a", &session, &root).await;
    facts.set("after_close", done.summary());
    assert_eq!(done.revocation_rows, 1, "token revocation row");
    assert_eq!(done.token_rows, 1, "token record removed instead of revoked");
    assert_eq!(done.mapping_state, "deleted");
    assert_eq!(done.permit_state, "released");
    assert_eq!(done.present_mappings, 0);
    assert_eq!(done.held_permits, 0);
    assert_eq!(done.backend_count, 0);
    assert!(!done.workspace_exists, "workspace still present");
    assert!(!done.snapshot_exists, "snapshot present on the default path");
    assert!(!done.token_file_exists, "token file remains");
    assert_eq!(done.pool_state, "destroyed");
    assert!(done.replacement_empty >= 1, "no replacement slot");
    assert!(!done.harness_alive, "harness pid still alive");
    let stored = get(
        &endpoint,
        "oo_session_runtime",
        &cfg.pg.session_runtime_password,
        cfg.pg.ca_pem.as_bytes(),
        "proj-a",
        &session,
    )
    .await
    .expect("closed head");
    let release = serde_json::to_value(&stored.state.release).unwrap();
    assert_eq!(release["s8"]["status"], "confirmed");
    assert_eq!(release["s8"]["actual_absence"], true);
    assert_eq!(release["s5"]["status"], "skipped");
    assert_eq!(release["s5"]["actual_absence"], false);
    assert_eq!(serde_json::to_value(stored.state.phase).unwrap(), "closed");
    assert!(!stored.physical_release_certified);
    let outputs = obs.output_hash("proj-a", &session, "outputs").await;
    let transcript = obs.output_hash("proj-a", &session, "transcript").await;
    assert_eq!(release["s3"]["evidence_hash"], collect_hash("outputs", &outputs.0, outputs.1));
    assert_eq!(release["s4"]["evidence_hash"], collect_hash("transcript", &transcript.0, transcript.1));
    assert_eq!(release["s6"]["evidence_hash"], label_hash("gateway_token_store", "revoked"));
    assert_eq!(release["s7"]["evidence_hash"], label_hash("proxy_mapping", "present_count_0"));
    assert_eq!(release["s10"]["evidence_hash"], label_hash("permit", "held_0"));
    facts.set("s6_s7_s10_evidence_is_constant_label", json!(true));
    facts.set("s3_s4_match_output_bytes", json!(true));
    facts.set("reducer_phase", json!("closed"));
    facts.set("reducer_physical_release_certified_field_on_state", json!("absent"));
    let still_out = http(port, "GET", &format!("/control/v1/conversations/{conversation}/outputs/transcript"), &alice.cookie, "", "");
    assert_eq!(still_out.0, 200, "output after close");
    assert!(still_out.2.contains(MARKER));
    let replay = http(port, "POST", &format!("/control/v1/conversations/{conversation}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(replay.0, 200, "replay");
    assert_closed(&replay.2, "replay");
    let replay_rows = obs.snapshot("proj-a", &session, &root).await;
    assert_eq!(replay_rows.revocation_rows, 1);
    assert_eq!(replay_rows.mapping_state, "deleted");
    assert_eq!(replay_rows.permit_state, "released");
    facts.set("cancel_retry_disposition", json!("replay"));

    let second = http(port, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, "{}");
    assert_eq!(second.0, 201, "second create");
    let second_body: Value = serde_json::from_str(&second.2).unwrap();
    let conversation2 = second_body["conversation_id"].as_str().unwrap().to_owned();
    let session2 = second_body["session_id"].as_str().unwrap().to_owned();
    let turn2 = http(
        port,
        "POST",
        &format!("/control/v1/conversations/{conversation2}/turns"),
        &alice.cookie,
        &alice.csrf,
        &json!({"turn_id":"turn-2","text":"restart-marker"}).to_string(),
    );
    assert_eq!(turn2.0, 200, "turn 2");
    let map_again = obs.snapshot("proj-a", &session2, &root).await;
    assert_eq!(map_again.map_ip, "10.88.0.8");
    facts.set("second_session_map_ip", json!(map_again.map_ip));
    let first_pid = guard.0.as_ref().unwrap().id();
    let staged = fixture_stage(&cfg, "proj-a", &session2, CancelTrigger::User).await.expect("fixture stage");
    assert_eq!(staged.disposition, "fixture_persisted");
    assert_eq!(staged.phase, "releasing");
    assert!(staged.retryable);
    assert!(!staged.acceptable());
    assert!(!staged.reducer_physical_release_certified);
    facts.set("fixture_stage_counted_as_close", json!(false));
    facts.set("fixture_stage_disposition", json!(staged.disposition));
    let mid = obs.snapshot("proj-a", &session2, &root).await;
    assert_eq!(mid.stage, "fixture_done");
    assert_eq!(mid.revocation_rows, 1);
    assert!(!mid.workspace_exists);
    if let Some(child) = guard.0.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    guard.0 = None;
    let _ = fs::remove_file(root.join("run/port.txt"));
    let mut restarted = spawn_server(bin, &config_path);
    let restarted_pid = restarted.id();
    assert_ne!(first_pid, restarted_pid, "server process was not replaced");
    let port2 = wait_port(&root.join("run/port.txt"), &mut restarted);
    guard.0 = Some(restarted);
    facts.set("server_pid_replaced", json!(true));
    let resumed = http(port2, "POST", &format!("/control/v1/conversations/{conversation2}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(resumed.0, 200, "restart finish");
    assert_closed(&resumed.2, "closed");
    let resumed_rows = obs.snapshot("proj-a", &session2, &root).await;
    assert_eq!(resumed_rows.backend_count, 0);
    assert!(!resumed_rows.workspace_exists);
    assert!(!resumed_rows.snapshot_exists);
    assert!(!resumed_rows.harness_alive);
    assert_eq!(resumed_rows.mapping_state, "deleted");
    assert_eq!(resumed_rows.permit_state, "released");
    let resumed_head = get(
        &endpoint,
        "oo_session_runtime",
        &cfg.pg.session_runtime_password,
        cfg.pg.ca_pem.as_bytes(),
        "proj-a",
        &session2,
    )
    .await
    .expect("resumed head");
    assert_eq!(serde_json::to_value(resumed_head.state.phase).unwrap(), "closed");
    assert!(!resumed_head.physical_release_certified);
    facts.set("restart_process_closed", json!(true));

    let third = http(port2, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, "{}");
    assert_eq!(third.0, 201, "third");
    let third_body: Value = serde_json::from_str(&third.2).unwrap();
    let conversation3 = third_body["conversation_id"].as_str().unwrap().to_owned();
    let session3 = third_body["session_id"].as_str().unwrap().to_owned();
    let turn3 = http(
        port2,
        "POST",
        &format!("/control/v1/conversations/{conversation3}/turns"),
        &alice.cookie,
        &alice.csrf,
        &json!({"turn_id":"turn-3","text":"snapshot-marker"}).to_string(),
    );
    assert_eq!(turn3.0, 200, "turn 3");
    let staged3 = fixture_stage(&cfg, "proj-a", &session3, CancelTrigger::User).await.expect("stage 3");
    assert!(!staged3.acceptable());
    let snap = root.join("run").join(OWNER_UUID).join("snapshots").join(&session3);
    fs::create_dir_all(&snap).unwrap();
    fs::write(snap.join("snapshot.bin"), b"keep").unwrap();
    let blocked = http(port2, "POST", &format!("/control/v1/conversations/{conversation3}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(blocked.0, 409, "snapshot block");
    let blocked_body: Value = serde_json::from_str(&blocked.2).unwrap();
    assert_eq!(blocked_body["reason"], "snapshot_present_uncollected");
    assert_ne!(blocked_body["phase"], "closed");
    let blocked_head = get(
        &endpoint,
        "oo_session_runtime",
        &cfg.pg.session_runtime_password,
        cfg.pg.ca_pem.as_bytes(),
        "proj-a",
        &session3,
    )
    .await
    .expect("blocked head");
    assert_ne!(serde_json::to_value(blocked_head.state.phase).unwrap(), "closed");
    fs::remove_file(snap.join("snapshot.bin")).unwrap();
    let finished = http(port2, "POST", &format!("/control/v1/conversations/{conversation3}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(finished.0, 200, "snapshot removed");
    assert_closed(&finished.2, "closed");
    facts.set("snapshot_present_blocks_close", json!(true));
    facts.set("snapshot_absent_then_closes", json!(true));
    facts.set("slot_destroyed", json!(slot));
    facts.set("observation", json!("localprocess_resources_observed"));
    drop(guard);
}

async fn refuse_stale(
    cfg: &Config,
    endpoint: &Endpoint,
    identity: &IdentityTuple,
    session: &str,
    state: &session_store::SessionState,
    revision: u64,
    generation: u64,
    event_id: &str,
) {
    let event = Event::Heartbeat {
        owner_id: "entry".to_owned(),
        lease_expires_at_ms: state.lease_expires_at_ms.saturating_add(1_000),
    };
    let receipt = transition(&TransitionRequest {
        endpoint,
        user: "oo_session_runtime",
        password: &cfg.pg.session_runtime_password,
        ca_pem: cfg.pg.ca_pem.as_bytes(),
        project_id: "proj-a",
        session_id: session,
        identity,
        expected_revision: revision,
        expected_owner_generation: generation,
        event_id,
        now_ms: state.last_now_ms,
        event: &event,
    })
    .await
    .expect("stale transition");
    assert_eq!(receipt.disposition, Disposition::Acknowledged, "{event_id}");
    assert_eq!(receipt.outcome, Some(Outcome::Refused), "{event_id}");
    assert_eq!(receipt.reason, Some(Reason::Stale), "{event_id}");
    assert!(!receipt.physical_release_certified, "{event_id}");
}

fn assert_closed(body: &str, disposition: &str) {
    let value: Value = serde_json::from_str(body).expect("close json");
    assert_eq!(value["disposition"], disposition);
    assert_eq!(value["phase"], "closed");
    assert_eq!(value["production_signed"], false);
    assert_eq!(value["deployment"], "localprocess");
    assert_eq!(value["retryable"], false);
    assert_eq!(value["reducer_physical_release_certified"], false);
    let steps = value["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 11);
    let step = |id: &str| steps.iter().find(|item| item["id"] == id).unwrap_or_else(|| panic!("missing {id}"));
    assert_eq!(step("s5")["status"], "skipped");
    assert_eq!(step("s5")["actual_absence"], false);
    assert_eq!(step("s8")["status"], "confirmed");
    assert_eq!(step("s8")["actual_absence"], true);
    for id in ["s1", "s2", "s3", "s4", "s6", "s7", "s9", "s10", "s11"] {
        assert_eq!(step(id)["status"], "confirmed", "{id}");
        assert_eq!(step(id)["actual_absence"], false, "{id}");
    }
}

#[allow(clippy::struct_excessive_bools)]
struct Shot {
    mapping_state: String,
    map_ip: String,
    permit_state: String,
    present_mappings: i64,
    held_permits: i64,
    token_rows: i64,
    revocation_rows: i64,
    backend_count: i64,
    backend_pid: Option<i32>,
    harness_pid: Option<i64>,
    checkpoint_exists: bool,
    harness_alive: bool,
    harness_class: String,
    stage: String,
    workspace_exists: bool,
    transcript_exists: bool,
    snapshot_exists: bool,
    token_file_exists: bool,
    pool_slot: String,
    pool_state: String,
    replacement_empty: i64,
    turn_event: bool,
}

impl Shot {
    fn summary(&self) -> Value {
        json!({
            "mapping_state": self.mapping_state,
            "map_ip": self.map_ip,
            "permit_state": self.permit_state,
            "present_mappings": self.present_mappings,
            "held_permits": self.held_permits,
            "token_rows": self.token_rows,
            "revocation_rows": self.revocation_rows,
            "backend_count": self.backend_count,
            "backend_pid_present": self.backend_pid.is_some(),
            "harness_pid_present": self.harness_pid.is_some(),
            "checkpoint_exists": self.checkpoint_exists,
            "harness_alive": self.harness_alive,
            "harness_class": self.harness_class,
            "stage": self.stage,
            "workspace_exists": self.workspace_exists,
            "transcript_exists": self.transcript_exists,
            "snapshot_exists": self.snapshot_exists,
            "token_file_exists": self.token_file_exists,
            "pool_state": self.pool_state,
            "replacement_empty": self.replacement_empty,
            "turn_event": self.turn_event,
        })
    }
}

struct Obs {
    api: Client,
    token: Client,
    query: Client,
}

impl Obs {
    async fn connect(cfg: &Config) -> Self {
        Self {
            api: connect_cfg(cfg, "oo_session_api_runtime", &cfg.pg.api_runtime_password).await,
            token: connect_cfg(cfg, "oo_token_runtime", &cfg.pg.token_runtime_password).await,
            query: connect_cfg(cfg, "oo_fixture_query", &cfg.pg.query_password).await,
        }
    }

    async fn snapshot(&self, project: &str, session: &str, root: &Path) -> Shot {
        let row = self
            .api
            .query_one(
                "SELECT stage, backend_pid, harness_pid, pool_slot FROM oo_session_api.resource WHERE project_id = $1 AND session_id = $2",
                &[&project, &session],
            )
            .await
            .expect("resource");
        let stage: String = row.get(0);
        let backend_pid: Option<i32> = row.get(1);
        let harness_pid: Option<i64> = row.get(2);
        let pool_slot: String = row.get(3);
        let mapping = self
            .api
            .query_opt(
                "SELECT state, map_ip FROM oo_session_api.proxy_mapping WHERE project_id = $1 AND session_id = $2",
                &[&project, &session],
            )
            .await
            .expect("mapping");
        let (mapping_state, map_ip) = match mapping {
            Some(row) => (row.get(0), row.get(1)),
            None => ("absent".to_owned(), String::new()),
        };
        let permit = self
            .api
            .query_opt("SELECT state FROM oo_session_api.permit WHERE session_id = $1", &[&session])
            .await
            .expect("permit");
        let permit_state = permit.map(|row| row.get(0)).unwrap_or_else(|| "absent".to_owned());
        let present_mappings: i64 = self
            .api
            .query_one(
                "SELECT count(*) FROM oo_session_api.proxy_mapping WHERE project_id = $1 AND session_id = $2 AND state = 'present'",
                &[&project, &session],
            )
            .await
            .expect("present")
            .get(0);
        let held_permits: i64 = self
            .api
            .query_one(
                "SELECT count(*) FROM oo_session_api.permit WHERE session_id = $1 AND state = 'held'",
                &[&session],
            )
            .await
            .expect("held")
            .get(0);
        let token_rows: i64 = self
            .token
            .query_one("SELECT count(*) FROM oo_token_control.token_record WHERE session_id = $1", &[&session])
            .await
            .expect("token rows")
            .get(0);
        let revocation_rows: i64 = self
            .token
            .query_one(
                "SELECT count(*) FROM oo_token_control.revocation WHERE scope = 'session' AND project_id = $1 AND session_id = $2",
                &[&project, &session],
            )
            .await
            .expect("revocation")
            .get(0);
        let application = format!("q{session}");
        let backend_count: i64 = self
            .query
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name = $1 AND state = 'active'",
                &[&application],
            )
            .await
            .expect("activity")
            .get(0);
        let pool_state: String = self
            .api
            .query_one("SELECT state FROM oo_session_api.pool_slot WHERE slot_id = $1", &[&pool_slot])
            .await
            .expect("slot")
            .get(0);
        let replacement_empty: i64 = self
            .api
            .query_one(
                "SELECT count(*) FROM oo_session_api.pool_slot WHERE replacement_for = $1 AND state = 'empty'",
                &[&pool_slot],
            )
            .await
            .expect("replacement")
            .get(0);
        let turn_event: bool = self
            .api
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM oo_session_api.api_event e JOIN oo_session_api.conversation c ON c.project_id = e.project_id AND c.conversation_id = e.conversation_id WHERE c.project_id = $1 AND c.session_id = $2 AND e.kind = 'turn' AND e.body = $3)",
                &[&project, &session, &MARKER],
            )
            .await
            .expect("turn event")
            .get(0);
        let base = root.join("run").join(OWNER_UUID);
        let workspace = base.join("pool").join(&pool_slot);
        let checkpoint = base.join("checkpoints").join(session).join("checkpoint.json");
        let snapshot = base.join("snapshots").join(session).join("snapshot.bin");
        let token_file = base.join("secrets").join(format!("{session}.token"));
        let transcript = workspace.join("transcript.txt");
        let (checkpoint_pid, harness_class_file) = read_checkpoint(&checkpoint);
        let pid = harness_pid.or(checkpoint_pid);
        let (harness_alive, harness_class) = match pid {
            Some(pid) if pid > 0 => {
                let alive = fs::symlink_metadata(format!("/proc/{pid}")).is_ok();
                let class = if alive {
                    u32::try_from(pid).map(classify_pid).unwrap_or_else(|_| "other".to_owned())
                } else {
                    harness_class_file
                };
                (alive, class)
            }
            _ => (false, "absent".to_owned()),
        };
        Shot {
            mapping_state,
            map_ip,
            permit_state,
            present_mappings,
            held_permits,
            token_rows,
            revocation_rows,
            backend_count,
            backend_pid,
            harness_pid,
            checkpoint_exists: checkpoint.is_file(),
            harness_alive,
            harness_class,
            stage,
            workspace_exists: workspace.is_dir(),
            transcript_exists: transcript.is_file(),
            snapshot_exists: snapshot.is_file(),
            token_file_exists: token_file.is_file(),
            pool_slot,
            pool_state,
            replacement_empty,
            turn_event,
        }
    }

    async fn output_hash(&self, project: &str, session: &str, name: &str) -> (String, u64) {
        let row = self
            .api
            .query_one(
                "SELECT sha256, octet_length(bytes)::bigint FROM oo_session_api.output_object WHERE project_id = $1 AND session_id = $2 AND name = $3",
                &[&project, &session, &name],
            )
            .await
            .expect("output");
        let sha: String = row.get(0);
        let len: i64 = row.get(1);
        (sha, u64::try_from(len).unwrap_or(0))
    }
}

async fn own_ssl(client: &Client) -> String {
    match client.query_one("SELECT ssl::text FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()", &[]).await {
        Ok(row) => row.get(0),
        Err(_) => "unavailable".to_owned(),
    }
}

async fn connect_cfg(cfg: &Config, user: &str, password: &str) -> Client {
    let tls = rustls_connector(cfg.pg.ca_pem.as_bytes()).expect("ca");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&cfg.pg.host);
    config.port(cfg.pg.port);
    config.dbname(&cfg.pg.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-a394-observe");
    let (client, connection) = config.connect(tls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn read_checkpoint(path: &Path) -> (Option<i64>, String) {
    let Ok(bytes) = fs::read(path) else {
        return (None, "absent".to_owned());
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return (None, "unreadable".to_owned());
    };
    let pid = value["harness"]["pid"].as_u64().and_then(|pid| i64::try_from(pid).ok());
    (pid, "checkpoint".to_owned())
}

fn classify_pid(pid: u32) -> String {
    let Ok(bytes) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return "unreadable".to_owned();
    };
    if bytes.windows(9).any(|window| window == b"/bin/bash") && bytes.windows(7).any(|window| window == b"sleep 6") {
        "release_fixture_bash".to_owned()
    } else {
        "other".to_owned()
    }
}

fn collect_hash(kind: &str, sha: &str, len: u64) -> String {
    #[derive(Serialize)]
    struct Collect<'a> {
        adapter: &'static str,
        kind: &'a str,
        content_sha256: &'a str,
        len: u64,
    }
    hash_json(&Collect { adapter: "fixture_bounded_collect", kind, content_sha256: sha, len })
}

fn label_hash(adapter: &str, observation: &str) -> String {
    #[derive(Serialize)]
    struct Label<'a> {
        adapter: &'a str,
        observation: &'a str,
    }
    hash_json(&Label { adapter, observation })
}

fn hash_json<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("hash json");
    hex(&sha256(&bytes))
}

#[allow(clippy::many_single_char_names, clippy::needless_range_loop)]
fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
        0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
        0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
        0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7, 0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
        0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85,
        0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
        0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
    ];
    let bit_len = (input.len() as u64).saturating_mul(8);
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    let mut state: [u32; 8] = [0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19];
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for index in 0..16 {
            let start = index * 4;
            word[index] = u32::from_be_bytes([chunk[start], chunk[start + 1], chunk[start + 2], chunk[start + 3]]);
        }
        for index in 16..64 {
            let s0 = word[index - 15].rotate_right(7) ^ word[index - 15].rotate_right(18) ^ (word[index - 15] >> 3);
            let s1 = word[index - 2].rotate_right(17) ^ word[index - 2].rotate_right(19) ^ (word[index - 2] >> 10);
            word[index] = word[index - 16].wrapping_add(s0).wrapping_add(word[index - 7]).wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[index]).wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

struct Session {
    cookie: String,
    csrf: String,
}

fn consume(port: u16, grant: &str) -> Session {
    let response = http(port, "POST", "/control/v1/entry/session/consume", "", "", &json!({"grant": grant.trim()}).to_string());
    assert_eq!(response.0, 200, "consume");
    let body: Value = serde_json::from_str(&response.2).unwrap();
    let cookie = response
        .1
        .lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("set-cookie:").map(|value| value.trim().split(';').next().unwrap_or("").to_owned()))
        .expect("cookie");
    assert!(cookie.starts_with("oo-entry="));
    Session { cookie, csrf: body["csrf"].as_str().unwrap().to_owned() }
}

fn http(port: u16, method: &str, path: &str, cookie: &str, csrf: &str, body: &str) -> (u16, String, String) {
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(90))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    if !cookie.is_empty() {
        head.push_str(&format!("cookie: {cookie}\r\n"));
    }
    if !csrf.is_empty() {
        head.push_str(&format!("x-oo-csrf: {csrf}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let status = head.lines().next().unwrap_or("").split(' ').nth(1).unwrap_or("0").parse().unwrap_or(0);
    (status, head.to_owned(), body.to_owned())
}

fn spawn_server(bin: &str, config: &Path) -> Child {
    Command::new(bin)
        .arg("serve")
        .stdin(Stdio::from(File::open(config).unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn wait_port(path: &Path, child: &mut Child) -> u16 {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("server exited {} before listen", status.code().unwrap_or(99));
        }
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(port) = text.trim().parse::<u16>()
            && port != 0
        {
            return port;
        }
        if start.elapsed() > Duration::from_secs(20) {
            panic!("server did not write a port");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn port_file_ready(path: &Path) -> bool {
    fs::read_to_string(path).ok().and_then(|text| text.trim().parse::<u16>().ok()).is_some_and(|port| port != 0)
}

fn run_to_exit(bin: &str, cmd: &str, config: &Path, seconds: u64) -> (Option<i32>, String, String) {
    let mut child = Command::new(bin)
        .arg(cmd)
        .stdin(Stdio::from(File::open(config).unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status.code().unwrap_or(99));
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut out = String::new();
    let mut err = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }
    (code, out, err)
}

fn write_bindings(path: &Path) {
    let body = json!({
        "bindings": [
            binding("alice", "proj-a", "analyst", &["session.use"]),
            binding("bob", "proj-a", "analyst", &["session.use"]),
            binding("dana", "proj-b", "analyst", &["session.use"])
        ]
    });
    fs::write(path, serde_json::to_vec_pretty(&body).unwrap()).unwrap();
}

fn binding(principal: &str, project: &str, role: &str, caps: &[&str]) -> Value {
    json!({
        "principal_id": principal,
        "project_id": project,
        "role": role,
        "claims": [["dept", "sales"]],
        "policy_version": "pol-1",
        "knowledge_version": "know-1",
        "config_version": "cfg-1",
        "capabilities": caps,
    })
}

fn write_config(path: &Path, fixture: &Path, root: &Path, profile: &str, issue: Option<(&str, &str, PathBuf)>) {
    let endpoint = fs::read_to_string(fixture.join("endpoint.txt")).unwrap();
    let mut lines = endpoint.lines();
    let host = lines.next().unwrap();
    let port: u16 = lines.next().unwrap().parse().unwrap();
    let database = lines.next().unwrap();
    let mut value = json!({
        "contract": "session-control-api-v1",
        "profile": profile,
        "bind": "127.0.0.1:0",
        "owner_uuid": OWNER_UUID,
        "state_dir": root.join("run"),
        "bindings_path": root.join("bindings.json"),
        "audience_url": "https://gateway.example/v1",
        "pg": {
            "host": host,
            "port": port,
            "database": database,
            "ca_pem": fs::read_to_string(fixture.join("ca.crt")).unwrap(),
            "api_admin_password": read_secret(&fixture.join("api-admin.pass")),
            "api_runtime_password": read_secret(&fixture.join("api-runtime.pass")),
            "session_admin_password": read_secret(&fixture.join("admin.pass")),
            "session_runtime_password": read_secret(&fixture.join("runtime.pass")),
            "cancel_password": read_secret(&fixture.join("cancel.pass")),
            "query_password": read_secret(&fixture.join("query.pass")),
            "token_admin_password": read_secret(&fixture.join("token-admin.pass")),
            "token_runtime_password": read_secret(&fixture.join("token-runtime.pass")),
        }
    });
    if let Some((principal, project, grant_out)) = issue {
        value["issue"] = json!({
            "principal_id": principal,
            "project_id": project,
            "grant_out": grant_out,
        });
    }
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(path).unwrap();
    file.write_all(&serde_json::to_vec(&value).unwrap()).unwrap();
    file.sync_all().unwrap();
}

fn read_secret(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_owned()
}
