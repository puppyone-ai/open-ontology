//! Frozen builder contracts: cache, field staging, review, and pack.

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use pg_builder::{
    assemble_cards, cache_hit_same, canonical_sha256, catalog_request, column_batch_ranges, generate,
    load_config, pack_pair, post_loopback, read_bearer, review_only, seal_request, scaffold_curated,
    store_validated, GenerateIn, PackIn, ScriptedTransport,
};
use common::{catalog_json, classifications, comments, operator_review, profile, roles, settings, DATASET};

const DESCRIPTION_RESPONSE_PROTOCOL: &str = "oo.builder.description-response/v1";

static TEMP: AtomicU64 = AtomicU64::new(0);

fn read_http_request(stream: &mut std::net::TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 256];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() >= 8192 {
            return;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => buf.extend_from_slice(&chunk[..count]),
        }
    };
    let mut length = 0usize;
    for line in buf[..header_end].split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(split) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        if !line[..split].eq_ignore_ascii_case(b"content-length") {
            continue;
        }
        let text = std::str::from_utf8(line[split + 1..].trim_ascii()).unwrap_or("");
        length = text.parse().unwrap_or(0);
    }
    let mut have = buf.len() - (header_end + 4);
    while have < length && buf.len() < 16384 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                have += count;
                buf.extend_from_slice(&chunk[..count]);
            }
        }
    }
}

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "pg-builder-{}-{}-{}",
        std::process::id(),
        nanos,
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).expect("temp");
    path
}

fn write_config(dir: &Path, purpose: &str, port: u16, price: &str, bearer_name: &str) -> String {
    let bearer = dir.join(bearer_name);
    fs::write(&bearer, b"abc123\n").expect("bearer");
    format!(
        r#"{{
            "protocol": "oo.pg-builder.config/v1",
            "adapter": "loopback_http_local_dev",
            "namespace": "pg-builder",
            "purpose": "{purpose}",
            "sample_policy": "none",
            "temperature": 0,
            "endpoint": {{
                "origin": "http://127.0.0.1:{port}",
                "path": "/v1/chat/completions",
                "model_id": "grok-4.7"
            }},
            "secret": {{"bearer_file": "{}"}},
            "declared_price": {price},
            "budget": {{
                "deadline_ms": 30000,
                "max_request_bytes": 262144,
                "max_response_bytes": 262144,
                "max_prompt_tokens": 100000,
                "max_completion_tokens": 100000,
                "max_total_tokens": 200000,
                "max_tables": 16,
                "max_in_flight": 1
            }}
        }}"#,
        bearer.display()
    )
}

fn approval(cards: &Value) -> Value {
    let tables = cards["tables"].as_array().expect("tables");
    json!({
        "protocol": "oo.pg-builder.card-approval/v1",
        "approved": true,
        "authenticated": false,
        "parent_auth": "required",
        "reviewer_label": "local-operator",
        "reviewer_at": "2026-10-07",
        "evidence_hash": cards["evidence_hash"],
        "schema_hash": cards["schema_hash"],
        "cards_sha256": canonical_sha256(cards),
        "tables": tables.iter().map(|card| json!({
            "dataset": card["dataset"],
            "card_sha256": card["card_sha256"]
        })).collect::<Vec<_>>()
    })
}

struct World {
    command: String,
    request: Value,
    cards: Value,
    curated: Value,
}

fn world() -> World {
    let catalog_text = catalog_json();
    let seal = seal_request(&catalog_text).unwrap_or_else(|err| panic!("seal {} {}", err.code, err.message));
    assert_eq!(seal["prepare_status"], "request");
    let catalog: Value = serde_json::from_str(&catalog_text).expect("catalog");
    let cards = assemble_cards(
        &catalog,
        &profile(),
        &comments(),
        seal["schema_hash"].as_str().expect("schema"),
        seal["evidence_hash"].as_str().expect("evidence"),
        16,
    )
    .unwrap_or_else(|err| panic!("cards {} {}", err.code, err.message));
    World {
        command: seal["command"].as_str().expect("command").to_owned(),
        request: seal["prepared_request"].clone(),
        cards,
        curated: scaffold_curated(&catalog).expect("curated"),
    }
}

fn table_response(request: &Value) -> Value {
    let card = request["boundary"]["data"]["tables"][0]["card_sha256"]
        .as_str()
        .expect("compiler card");
    json!({
        "budget_sha256": request["budget_sha256"],
        "candidates": [{
            "card_sha256": card,
            "caveats": [],
            "confidence": "0",
            "dataset": DATASET,
            "description": "Party rows for the fixture.",
            "grain": "one party",
            "label": "Party register",
            "open_questions": []
        }],
        "complete": true,
        "evidence_hash": request["evidence_hash"],
        "model_id": request["model_id"],
        "prompt_sha256": request["prompt_sha256"],
        "prompt_version": request["prompt_version"],
        "protocol": DESCRIPTION_RESPONSE_PROTOCOL,
        "request_sha256": request["request_sha256"],
        "schema_hash": request["schema_hash"],
        "source_id": request["source_id"]
    })
}

fn field_rows(cards: &Value) -> Value {
    let card = cards["tables"][0]["card_sha256"].as_str().expect("evidence card");
    json!([
        {
            "attnum": 1,
            "card_sha256": card,
            "caveats": [],
            "column": "id",
            "confidence": "0.5000",
            "dataset": DATASET,
            "description": "FIELD_ONLY_a328 identifier column",
            "label": "Id",
            "open_questions": []
        },
        {
            "attnum": 2,
            "card_sha256": card,
            "caveats": ["untrusted comment is data"],
            "column": "note",
            "confidence": "1.0",
            "dataset": DATASET,
            "description": "FIELD_ONLY_a328 note column text",
            "label": "Note",
            "open_questions": ["what does the note mean"]
        }
    ])
}

fn s5(request: &Value, cards: &Value) -> String {
    serde_json::to_string(&json!({
        "protocol": "oo.pg-builder.s5-response/v1",
        "complete": true,
        "table_response": table_response(request),
        "field_candidates": field_rows(cards)
    }))
    .expect("s5")
}

fn run_generate(
    world: &World,
    dir: &Path,
    content: &str,
    finish: &str,
    refusal: Option<&'static str>,
) -> (Value, u32) {
    let text = write_config(dir, "generate", 8317, r#"{"status":"unknown"}"#, "bearer");
    let config = load_config(&text).expect("config");
    let token = read_bearer(&config.bearer_file).expect("token");
    assert_eq!(token, "abc123");
    let mut transport = ScriptedTransport {
        http_status: 200,
        content: content.to_owned(),
        finish_reason: finish.to_owned(),
        usage: Some((12, 8, 20)),
        calls: 0,
        bodies: Vec::new(),
        refusal,
    };
    let approved = approval(&world.cards);
    let doc = generate(GenerateIn {
        config: &config,
        config_bytes: text.as_bytes(),
        token: &token,
        command_text: &world.command,
        cards: &world.cards,
        approval: &approved,
        cache_dir: dir,
        transport: &mut transport,
    })
    .unwrap_or_else(|err| panic!("generate {} {}", err.code, err.message));
    if transport.calls > 0 {
        let body = String::from_utf8_lossy(&transport.bodies[0]);
        assert!(body.contains("oo.pg-builder.model-input/v2"), "{body}");
        assert!(!body.contains("compiler_request"), "{body}");
        assert!(!body.contains("oo.builder.description-request/v1"), "{body}");
        assert!(!body.contains("most_common_vals"), "{body}");
        assert!(!body.contains("histogram_bounds"), "{body}");
    }
    (doc, transport.calls)
}

#[test]
fn validated_draft_caches_and_pack_omits_field_text() {
    let world = world();
    let dir = temp_dir();
    let (doc, calls) = run_generate(&world, &dir, &s5(&world.request, &world.cards), "stop", None);
    assert_eq!(calls, 1);
    assert_eq!(doc["outcome"], "validated");
    assert_eq!(doc["field_outcome"], "validated");
    assert_eq!(doc["repair"], "not_performed");
    assert_eq!(doc["transport"]["socket"], "opened");
    assert_eq!(doc["transport"]["attempt"], 1);
    assert_eq!(doc["transport"]["status"], "complete");
    assert_eq!(doc["transport"]["usage"]["total_tokens"], 20);
    assert_eq!(doc["transport"]["declared_cost"]["status"], "unknown");
    assert_eq!(doc["transport"]["declared_cost"]["amount"], Value::Null);
    assert_eq!(doc["transport"]["declared_cost"]["invoice"], Value::Bool(false));
    assert_eq!(doc["transport"]["declared_cost"]["billing_certificate"], Value::Bool(false));
    assert_eq!(doc["statements"]["publication"], "unavailable");
    assert_eq!(doc["statements"]["pii_classification"], "not_performed");
    assert!(doc["candidates"]["field"].to_string().contains("FIELD_ONLY_a328"));
    assert!(dir.join("pg-builder").read_dir().expect("cache").next().is_some());

    let text = write_config(&dir, "generate", 8317, r#"{"status":"unknown"}"#, "bearer");
    let config = load_config(&text).expect("config");
    let token = read_bearer(&config.bearer_file).expect("token");
    let mut again = ScriptedTransport {
        http_status: 200,
        content: "should-not-be-read".to_owned(),
        finish_reason: "stop".to_owned(),
        usage: Some((1, 1, 2)),
        calls: 0,
        bodies: Vec::new(),
        refusal: None,
    };
    let approved = approval(&world.cards);
    let hit = generate(GenerateIn {
        config: &config,
        config_bytes: text.as_bytes(),
        token: &token,
        command_text: &world.command,
        cards: &world.cards,
        approval: &approved,
        cache_dir: &dir,
        transport: &mut again,
    })
    .expect("hit");
    assert_eq!(again.calls, 0);
    assert_eq!(hit["cache"], "hit");
    assert_eq!(hit["transport"]["socket"], "not_opened");
    assert_eq!(hit["transport"]["attempt"], 0);
    assert!(cache_hit_same(&doc, &hit));

    let stored_sha = doc["candidate_sha256"].clone();
    let stored_command = doc["complete_command"].as_str().expect("command").to_owned();
    let review = review_only(&doc, &world.curated, &operator_review())
        .unwrap_or_else(|err| panic!("review {} {}", err.code, err.message));
    assert_eq!(review["authenticated"], Value::Bool(false));
    assert_eq!(review["outcome"], "reviewed");
    assert_eq!(review["rebind"]["bound"], Value::Bool(true));
    assert_eq!(review["rebind"]["stored_generation"], "unchanged");
    assert_eq!(review["rebind"]["model_bytes"], "copied");
    assert_eq!(review["rebind"]["stored_candidate_sha256"], stored_sha);
    assert_eq!(doc["candidate_sha256"], stored_sha);
    assert!(stored_command.contains("oo.builder.description-response/v1"));
    assert!(!stored_command.contains("oo.builder.description-prompt/v2"));
    assert!(review["review_command"].as_str().expect("command").contains("FIELD_ONLY_a328"));
    assert!(!review["review"]["datasets"].to_string().contains("FIELD_ONLY_a328"));

    let packed = pack_pair(&PackIn {
        generation: &doc,
        curated: &world.curated,
        operator: &operator_review(),
        settings: &settings(),
        classifications: &classifications(),
        roles: &roles(&["id", "note"]),
        hidden_columns: &["national_id"],
    })
    .unwrap_or_else(|err| panic!("pack {} {}", err.code, err.message));
    assert_eq!(packed["field_text_absent"], Value::Bool(true));
    assert_eq!(packed["diff"]["changed"], Value::Bool(true));
    assert_eq!(packed["hidden_columns"][0]["absent"], Value::Bool(true));
    assert_eq!(packed["statements"]["model_publication"], Value::Bool(false));
    let after = packed["after"].to_string();
    let before = packed["before"].to_string();
    assert!(after.contains("Party register"));
    assert!(!before.contains("Party register"));
    assert!(!after.contains("FIELD_ONLY_a328"));
    assert!(!before.contains("FIELD_ONLY_a328"));
    assert_eq!(packed["statements"]["field_candidates_applied"], Value::Bool(false));
}

#[test]
fn approved_column_text_reaches_only_the_allowlisted_role() {
    let world = world();
    let dir = temp_dir();
    let (doc, _) = run_generate(&world, &dir, &s5(&world.request, &world.cards), "stop", None);
    let operator = column_operator();
    let review = review_only(&doc, &world.curated, &operator)
        .unwrap_or_else(|err| panic!("review {} {}", err.code, err.message));
    assert_eq!(review["outcome"], "reviewed");
    assert_eq!(review["authenticated"], Value::Bool(false));
    let records = review["review"]["column_decisions"].as_array().expect("columns");
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|row| row["confidence_effect"] == "recorded_only"));
    assert!(records.iter().all(|row| row["authenticated"] == Value::Bool(false)));
    assert!(records.iter().any(|row| row["candidate_confidence"] == "0.5000"));
    assert!(records.iter().any(|row| row["candidate_caveats"].to_string().contains("untrusted comment is data")));
    assert!(records.iter().any(|row| row["candidate_open_questions"].to_string().contains("what does the note mean")));
    let mut role_set = roles(&["id", "note"]);
    let mut restricted = role_set[0].clone();
    restricted["principal"] = json!("restricted_user");
    restricted["role"] = json!("oo_r_restricted");
    restricted["datasets"][0]["columns"] = json!(["id"]);
    role_set.as_array_mut().expect("roles").push(restricted);
    let mut shown = settings();
    shown["ontology"]["publish"]["include_unreviewed"] =
        json!(["dataset_descriptions", "column_descriptions"]);
    let packed = pack_pair(&PackIn {
        generation: &doc,
        curated: &world.curated,
        operator: &operator,
        settings: &shown,
        classifications: &classifications(),
        roles: &role_set,
        hidden_columns: &["national_id"],
    })
    .unwrap_or_else(|err| panic!("pack {} {}", err.code, err.message));
    assert_eq!(packed["statements"]["model_publication"], Value::Bool(false));
    assert_eq!(packed["statements"]["field_candidates_applied"], Value::Bool(true));
    assert_eq!(packed["rebind"]["bound"], Value::Bool(true));
    assert_eq!(packed["hidden_columns"][0]["absent"], Value::Bool(true));
    let before_narrow = role_file_text(&packed["before"], "oo_r_narrow");
    let before_restricted = role_file_text(&packed["before"], "oo_r_restricted");
    let after_narrow = role_file_text(&packed["after"], "oo_r_narrow");
    let after_restricted = role_file_text(&packed["after"], "oo_r_restricted");
    let id_text = "FIELD_ONLY_a328 identifier column";
    let note_text = "FIELD_ONLY_a328 note column text";
    assert!(records.iter().any(|row| row["candidate_description"] == id_text));
    assert!(records.iter().any(|row| row["candidate_description"] == note_text));
    assert_eq!(effective_description(&packed["after"], "id").as_deref(), Some(id_text));
    assert_eq!(effective_description(&packed["after"], "note").as_deref(), Some(note_text));
    assert_eq!(effective_description(&packed["before"], "id").as_deref(), None);
    assert_eq!(effective_description(&packed["before"], "note").as_deref(), None);
    assert!(after_narrow.contains("| `id` | Id |"));
    assert!(after_narrow.contains("| `note` | Note |"));
    assert!(after_restricted.contains("| `id` | Id |"));
    assert!(!after_restricted.contains("| `note` |"));
    assert!(!before_narrow.contains("| `id` | Id |"));
    assert!(!before_restricted.contains("| `note` |"));
    let id_visible = format!("UNREVIEWED: {id_text}");
    let note_visible = format!("UNREVIEWED: {note_text}");
    assert!(after_narrow.contains(&id_visible));
    assert!(after_narrow.contains(&note_visible));
    assert!(after_restricted.contains(&id_visible));
    assert!(!after_restricted.contains(&note_visible));
    assert!(!before_narrow.contains(&id_visible));
    assert!(!before_restricted.contains(&id_visible));
    assert!(!after_narrow.contains("national_id"));
    assert!(!after_restricted.contains("national_id"));
    let queue = review["review"]["queue"].to_string();
    assert!(queue.contains("caveats"));
    assert!(queue.contains("open_questions"));
}

fn column_operator() -> Value {
    json!({
        "protocol": "oo.pg-builder.operator-review/v1",
        "authenticated": false,
        "parent_auth": "required",
        "decisions": [{
            "dataset": DATASET,
            "decision": "approve",
            "reviewer_label": "local-operator",
            "reviewer_at": "2026-10-07",
            "fields": {
                "label": {"overwrite": true},
                "description": {"overwrite": false},
                "grain": {"overwrite": false}
            }
        }],
        "column_decisions": [
            {
                "dataset": DATASET,
                "column": "id",
                "decision": "approve",
                "reviewer_label": "local-operator",
                "reviewer_at": "2026-10-07",
                "fields": {
                    "label": {"overwrite": true},
                    "description": {"overwrite": true}
                }
            },
            {
                "dataset": DATASET,
                "column": "note",
                "decision": "approve",
                "reviewer_label": "local-operator",
                "reviewer_at": "2026-10-07",
                "fields": {
                    "label": {"overwrite": true},
                    "description": {"overwrite": true}
                }
            }
        ]
    })
}

fn effective_description(pack: &Value, column: &str) -> Option<String> {
    let datasets = pack.pointer("/effective/effective_oos/datasets")?.as_array()?;
    for dataset in datasets {
        let Some(columns) = dataset.get("columns").and_then(Value::as_array) else {
            continue;
        };
        for item in columns {
            if item.get("name").and_then(Value::as_str) == Some(column) {
                return item.get("description").and_then(Value::as_str).map(str::to_owned);
            }
        }
    }
    None
}

fn role_file_text(pack: &Value, role: &str) -> String {
    let packs = pack.get("packs").and_then(Value::as_array).expect("packs");
    let item = packs
        .iter()
        .find(|entry| entry.get("role").and_then(Value::as_str) == Some(role))
        .expect("role");
    let files = item.pointer("/artifact/files").and_then(Value::as_array).expect("files");
    let mut text = String::new();
    for file in files {
        if let Some(body) = file.get("text").and_then(Value::as_str) {
            text.push_str(body);
            text.push('\n');
        }
    }
    text
}

#[test]
fn failures_do_not_cache_or_review() {
    let world = world();
    let cases = [
        ("{", "stop", None),
        ("{\"publication\":true}", "stop", None),
        ("truncated", "length", None),
        ("", "stop", Some("timeout")),
        ("", "stop", Some("unavailable")),
    ];
    for (content, finish, refusal) in cases {
        let dir = temp_dir();
        let body = if finish == "length" { "partial" } else { content };
        let (doc, _) = run_generate(&world, &dir, body, finish, refusal);
        assert_eq!(doc["outcome"], "structural_draft", "{content}");
        assert_eq!(doc["cache_store"], "not_stored");
        assert!(review_only(&doc, &world.curated, &operator_review()).is_err());
        assert!(!dir.join("pg-builder").exists());
    }
}

#[test]
fn field_publication_key_is_not_validated() {
    let world = world();
    let dir = temp_dir();
    let mut rows = field_rows(&world.cards);
    rows[0]["pii"] = json!("personal");
    let content = serde_json::to_string(&json!({
        "protocol": "oo.pg-builder.s5-response/v1",
        "complete": true,
        "table_response": table_response(&world.request),
        "field_candidates": rows
    }))
    .expect("s5");
    let (doc, _) = run_generate(&world, &dir, &content, "stop", None);
    assert_eq!(doc["outcome"], "structural_draft");
    assert_eq!(doc["field_outcome"], "rejected");
    assert!(review_only(&doc, &world.curated, &operator_review()).is_err());
}

#[test]
fn stale_evidence_misses_and_secret_in_config_opens_no_socket() {
    let world = world();
    let dir = temp_dir();
    let _ = run_generate(&world, &dir, &s5(&world.request, &world.cards), "stop", None);
    let mut stale_cards = world.cards.clone();
    stale_cards["evidence_hash"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let text = write_config(&dir, "generate", 8317, r#"{"status":"unknown"}"#, "bearer");
    let config = load_config(&text).expect("config");
    let token = read_bearer(&config.bearer_file).expect("token");
    let mut transport = ScriptedTransport {
        http_status: 200,
        content: s5(&world.request, &stale_cards),
        finish_reason: "stop".to_owned(),
        usage: Some((1, 1, 2)),
        calls: 0,
        bodies: Vec::new(),
        refusal: None,
    };
    let approved = approval(&stale_cards);
    let doc = generate(GenerateIn {
        config: &config,
        config_bytes: text.as_bytes(),
        token: &token,
        command_text: &world.command,
        cards: &stale_cards,
        approval: &approved,
        cache_dir: &dir,
        transport: &mut transport,
    })
    .expect("stale");
    assert_eq!(transport.calls, 1);
    assert_eq!(doc["cache"], "miss");

    let secret_dir = temp_dir();
    let secret_text = write_config(&secret_dir, "generate", 8317, r#"{"status":"unknown"}"#, "abc123");
    let secret_config = load_config(&secret_text).expect("config");
    let secret_token = read_bearer(&secret_config.bearer_file).expect("token");
    let mut blocked = ScriptedTransport {
        http_status: 200,
        content: String::new(),
        finish_reason: "stop".to_owned(),
        usage: None,
        calls: 0,
        bodies: Vec::new(),
        refusal: None,
    };
    let err = generate(GenerateIn {
        config: &secret_config,
        config_bytes: secret_text.as_bytes(),
        token: &secret_token,
        command_text: &world.command,
        cards: &world.cards,
        approval: &approval(&world.cards),
        cache_dir: &secret_dir,
        transport: &mut blocked,
    })
    .expect_err("secret");
    assert!(err.code.contains("config.secret"));
    assert_eq!(blocked.calls, 0);
}

#[test]
fn config_keeps_loopback_and_unknown_price() {
    let dir = temp_dir();
    let good = write_config(&dir, "fault_injection", 9, r#"{"status":"unknown"}"#, "bearer");
    assert!(load_config(&good).is_ok());
    let generate_wrong = write_config(&dir, "generate", 9, r#"{"status":"unknown"}"#, "bearer");
    assert!(load_config(&generate_wrong).is_err());
    let https = good.replace("http://127.0.0.1:9", "https://127.0.0.1:9");
    assert!(load_config(&https).is_err());
    let fault_long = good.replace("\"deadline_ms\": 30000", "\"deadline_ms\": 180000");
    assert!(load_config(&fault_long).is_err());
    let local = write_config(&dir, "generate", 8317, r#"{"status":"unknown"}"#, "deadline-bearer");
    assert!(load_config(&local).is_ok());
    let local_long = local.replace("\"deadline_ms\": 30000", "\"deadline_ms\": 180000");
    assert!(load_config(&local_long).is_ok());
    let local_mid = local.replace("\"deadline_ms\": 30000", "\"deadline_ms\": 30001");
    assert!(load_config(&local_mid).is_ok());
    let local_over = local.replace("\"deadline_ms\": 30000", "\"deadline_ms\": 180001");
    assert!(load_config(&local_over).is_err());
    let priced = write_config(
        &dir,
        "generate",
        8317,
        r#"{"status":"declared_non_billing","currency":"USD","per_total_token":"0.00000001"}"#,
        "priced-bearer",
    );
    let priced_config = load_config(&priced).expect("price");
    let world = world();
    let token = read_bearer(&priced_config.bearer_file).expect("token");
    let mut transport = ScriptedTransport {
        http_status: 200,
        content: s5(&world.request, &world.cards),
        finish_reason: "stop".to_owned(),
        usage: Some((3, 5, 8)),
        calls: 0,
        bodies: Vec::new(),
        refusal: None,
    };
    let doc = generate(GenerateIn {
        config: &priced_config,
        config_bytes: priced.as_bytes(),
        token: &token,
        command_text: &world.command,
        cards: &world.cards,
        approval: &approval(&world.cards),
        cache_dir: &dir.join("priced"),
        transport: &mut transport,
    })
    .expect("priced generate");
    let _ = fs::create_dir_all(dir.join("priced"));
    assert_eq!(doc["transport"]["declared_cost"]["status"], "declared_non_billing");
    assert_eq!(doc["transport"]["declared_cost"]["invoice"], Value::Bool(false));
    assert_eq!(doc["transport"]["declared_cost"]["amount"], "0.00000008");
}

#[test]
fn unclassified_role_column_is_refused_before_pack() {
    let world = world();
    let dir = temp_dir();
    let (doc, _) = run_generate(&world, &dir, &s5(&world.request, &world.cards), "stop", None);
    let err = pack_pair(&PackIn {
        generation: &doc,
        curated: &world.curated,
        operator: &operator_review(),
        settings: &settings(),
        classifications: &classifications(),
        roles: &roles(&["id", "note", "national_id"]),
        hidden_columns: &[],
    })
    .expect_err("hidden");
    assert!(err.code.contains("classification.missing"));
}

#[test]
fn profile_values_and_authenticated_approval_are_refused() {
    let catalog: Value = serde_json::from_str(&catalog_json()).expect("catalog");
    let mut bad = profile();
    bad["tables"][0]["columns"][0]["most_common_vals"] = json!(["secret"]);
    let err = assemble_cards(&catalog, &bad, &comments(), "ab", "cd", 16).expect_err("values");
    assert!(err.code.contains("profile.values"));
    let mut claimed = approval(&json!({"tables": [], "evidence_hash": "e", "schema_hash": "s"}));
    claimed["authenticated"] = json!(true);
    let cards = json!({
        "protocol": "oo.pg-builder.cards/v1",
        "evidence_hash": "e",
        "schema_hash": "s",
        "tables": []
    });
    let err = pg_builder::approval_matches(&cards, &claimed).expect_err("auth");
    assert!(err.code.contains("approval.authentication"));
}

#[test]
fn catalog_request_has_no_password_bytes() {
    let dir = temp_dir();
    let text = write_config(&dir, "generate", 8317, r#"{"status":"unknown"}"#, "bearer");
    let mut config = load_config(&text).expect("config");
    config.postgres = Some(pg_builder::Postgres {
        host: "oo-a328-pg".to_owned(),
        port: 5432,
        database: "oo_a328_src".to_owned(),
        username: "oo_a328_reader".to_owned(),
        password_file: dir.join("password"),
        ca_file: dir.join("ca.pem"),
        catalog_bin: dir.join("oo-pipeline"),
        tables: vec![("shop".to_owned(), "party".to_owned())],
    });
    fs::write(dir.join("password"), b"super-secret-value\n").expect("password");
    let request = catalog_request(config.postgres.as_ref().expect("pg"), "pubfix", "secret://pg/a328/reader")
        .expect("request");
    let encoded = serde_json::to_string(&request).expect("json");
    assert!(!encoded.contains("super-secret-value"));
    assert!(encoded.contains("password_file"));
    assert!(encoded.contains("oo.pipeline.catalog-pg/v1"));
}

#[test]
fn column_batches_are_eight() {
    assert!(column_batch_ranges(0).is_empty());
    assert_eq!(column_batch_ranges(5), vec![(0, 5)]);
    assert_eq!(column_batch_ranges(8), vec![(0, 8)]);
    assert_eq!(column_batch_ranges(9), vec![(0, 8), (8, 9)]);
}

#[test]
fn cache_refuses_structural_drafts() {
    let dir = temp_dir();
    let err = store_validated(&dir, "abc", &json!({"outcome": "structural_draft"})).expect_err("store");
    assert!(err.code.contains("cache.refused"));
}

#[test]
fn loopback_timeout_redirect_and_chunked_body() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let timeout_addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        std::thread::sleep(Duration::from_millis(400));
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
    });
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let timed = runtime
        .block_on(post_loopback(timeout_addr, "/v1/chat/completions", "abc123", b"{}", Duration::from_millis(50), 1024))
        .expect("timeout call");
    assert_eq!(timed.refusal, Some("timeout"));
    handle.join().expect("server");

    let closed = TcpListener::bind("127.0.0.1:0").expect("bind");
    let closed_port = closed.local_addr().expect("addr").port();
    drop(closed);
    let unavailable = runtime
        .block_on(post_loopback(
            std::net::SocketAddr::from(([127, 0, 0, 1], closed_port)),
            "/v1/chat/completions",
            "abc123",
            b"{}",
            Duration::from_millis(500),
            1024,
        ))
        .expect("unavailable call");
    assert_eq!(unavailable.refusal, Some("unavailable"));

    let redirect_listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let redirect_port = redirect_listener.local_addr().expect("addr").port();
    let redirect_handle = std::thread::spawn(move || {
        let (mut stream, _) = redirect_listener.accept().expect("accept");
        read_http_request(&mut stream);
        let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://example.test/\r\nContent-Length: 0\r\n\r\n");
    });
    let redirected = runtime
        .block_on(post_loopback(
            std::net::SocketAddr::from(([127, 0, 0, 1], redirect_port)),
            "/v1/chat/completions",
            "abc123",
            b"{}",
            Duration::from_secs(2),
            1024,
        ))
        .expect("redirect call");
    assert_eq!(redirected.refusal, Some("redirect"));
    assert!(redirected.body.is_empty());
    redirect_handle.join().expect("redirect server");

    let chunk_listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let chunk_port = chunk_listener.local_addr().expect("addr").port();
    let chunk_handle = std::thread::spawn(move || {
        let (mut stream, _) = chunk_listener.accept().expect("accept");
        read_http_request(&mut stream);
        let body = b"5\r\nhello\r\n0\r\n\r\n";
        let header = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body);
    });
    let chunked = runtime
        .block_on(post_loopback(
            std::net::SocketAddr::from(([127, 0, 0, 1], chunk_port)),
            "/v1/chat/completions",
            "abc123",
            b"{}",
            Duration::from_secs(2),
            1024,
        ))
        .expect("chunk call");
    assert_eq!(chunked.refusal, None);
    assert_eq!(chunked.body, b"hello");
    chunk_handle.join().expect("chunk server");
}
