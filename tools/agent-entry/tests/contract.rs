#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use agent_entry::{
    account_party, active_parties, amount_text, citations, complete_decision, entry_rows, fetch_active_pack, gold_for, memo,
    pack_digest, parse_active_pack, parse_query_pg_stdout, party, prompt_for, qty, query_id_from_hash, receipt_closed, self_asserted,
    serve_private_release, sql_allowed,
    startup_gate, CompleteInput, GOLD_ENTRY, QUESTION_CLARIFY, QUESTION_COUNT, QUESTION_PHONE,
};
use serde_json::json;

#[test]
fn gold_matches_the_published_shop_formula() {
    assert_eq!(entry_rows(), 90_000);
    assert_eq!(active_parties(), 1_000);
    let first = party(0);
    assert_eq!(first.party_key, "p00000");
    assert_eq!(first.display_name, None);
    assert!(first.active);
    assert_eq!(first.phone, "tel-00000");
    assert_eq!(party(1).display_name.as_deref(), Some("n00001"));
    assert!(!party(1).active);
    assert_eq!(qty(0), i64::MIN);
    assert_eq!(qty(1), i64::MAX);
    assert_eq!(qty(2), 0);
    assert_eq!(qty(7), 7 - 45_000);
    assert_eq!(amount_text(3).as_deref(), Some("1234567890.1234"));
    assert_eq!(amount_text(4).as_deref(), Some("-1234567890.1234"));
    assert_eq!(amount_text(5).as_deref(), Some("0.0001"));
    assert_eq!(amount_text(6).as_deref(), Some("-0.0001"));
    assert_eq!(amount_text(0), None);
    assert_eq!(memo(0), None);
    assert_eq!(memo(1).as_deref(), Some("m1"));
    assert_eq!(account_party(2_000), "p00000");
    assert_eq!(gold_for(QUESTION_COUNT), Some(GOLD_ENTRY));
    assert_eq!(gold_for(QUESTION_CLARIFY), None);
    assert_eq!(gold_for(QUESTION_PHONE), None);
}

#[test]
fn count_prompt_has_no_answer_sql() {
    let prompt = prompt_for("/usr/local/bin/oo", "/tmp/gateway.token", "http://127.0.0.1:9", QUESTION_COUNT);
    assert!(!prompt.contains("90000"));
    assert!(!prompt.contains("shop.entry"));
    assert!(!prompt.contains("count(*)"));
    assert!(prompt.contains("/usr/local/bin/oo"));
    assert!(prompt.contains(QUESTION_COUNT));
    assert!(prompt.contains("http://127.0.0.1:9"));
    assert!(!prompt.contains("/v1/query"));
}

#[test]
fn sql_gate_allows_one_read_and_refuses_the_rest() {
    assert!(sql_allowed("SELECT count(*) FROM shop.entry").is_ok());
    assert!(sql_allowed("WITH rows AS (SELECT entry_id FROM shop.entry) SELECT entry_id FROM rows").is_ok());
    assert!(sql_allowed("SELECT phone FROM shop.party WHERE party_key = 'p00001'").is_ok());
    assert!(sql_allowed("").is_err());
    assert!(sql_allowed("SELECT 1; SELECT 2").is_err());
    assert!(sql_allowed("SELECT 1 -- comment").is_err());
    assert!(sql_allowed("DELETE FROM shop.entry").is_err());
    assert!(sql_allowed("SELECT 1 FROM shop.entry WHERE note = 'set'").is_err());
}

#[test]
fn http_200_does_not_complete_the_turn() {
    let cited = vec!["q_abcdef".to_owned()];
    let missing = CompleteInput {
        tool_ran: true,
        audit_query_id: Some("q_abcdef"),
        cited: &cited,
        result_text: Some("90000"),
        gold: Some("90000"),
        stream_broken: false,
        cancelled: false,
    };
    assert!(complete_decision(&missing));
    let mut broken = missing.clone();
    broken.stream_broken = true;
    assert!(!complete_decision(&broken));
    let mut cancelled = missing.clone();
    cancelled.cancelled = true;
    cancelled.stream_broken = false;
    assert!(!complete_decision(&cancelled));
    let mut no_tool = missing.clone();
    no_tool.cancelled = false;
    no_tool.tool_ran = false;
    assert!(!complete_decision(&no_tool));
    let mut wrong = missing.clone();
    wrong.tool_ran = true;
    wrong.result_text = Some("1");
    assert!(!complete_decision(&wrong));
    let mut uncited = missing.clone();
    uncited.result_text = Some("90000");
    uncited.cited = &[];
    assert!(!complete_decision(&uncited));
}

#[test]
fn citations_require_a_bracketed_query_id() {
    assert_eq!(citations("rows [q_abcdef] done"), vec!["q_abcdef".to_owned()]);
    assert!(citations("q_abcdef").is_empty());
    assert!(citations("[q_ab]").is_empty());
    assert_eq!(citations("see [q_AbC123]"), vec!["q_AbC123".to_owned()]);
}

#[test]
fn production_profile_does_not_listen() {
    assert_eq!(startup_gate("localprocess"), Ok(()));
    let runsc = Path::new("/usr/local/bin/runsc").is_file() || Path::new("/usr/bin/runsc").is_file();
    let err = startup_gate("production").expect_err("production");
    if runsc {
        assert_eq!(err, "production_driver_absent");
    } else {
        assert_eq!(err, "runsc_absent");
    }
    assert!(startup_gate("other").is_err());
}

#[test]
fn self_asserted_identity_is_rejected() {
    assert!(self_asserted(&json!({"role": "admin"})));
    assert!(self_asserted(&json!({"principal_id": "wideowner"})));
    assert!(self_asserted(&json!({"pack_sha256": "ab"})));
    assert!(self_asserted(&json!({"source_git_commit": "ab"})));
    assert!(self_asserted(&json!({"content_hash": "ab"})));
    assert!(self_asserted(&json!({"operation_id": "rel"})));
    assert!(!self_asserted(&json!({"text": "hello"})));
    assert!(!self_asserted(&json!({})));
}

#[test]
fn active_pack_parse_requires_every_get_active_role_pack_field() {
    let digest = "ab".repeat(32);
    let commit = "cd".repeat(20);
    let missing = format!(
        r#"{{"operation":"getActiveRolePack","operation_id":"rel-shop-wide","project_id":"shop","role":"oo_r_wide","pack_sha256":"{digest}","source_git_commit":"{commit}","policy":"pol-1","knowledge":"know-1","published":true}}"#
    );
    assert_eq!(parse_active_pack(missing.as_bytes(), "shop", "oo_r_wide").unwrap_err().reason, "pack_unpublished");
    let present = format!(
        r#"{{"operation":"getActiveRolePack","operation_id":"rel-shop-wide","project_id":"shop","role":"oo_r_wide","pack_sha256":"{digest}","source_git_commit":"{commit}","policy":"pol-1","knowledge":"know-1","content_hash":"{digest}","published":true}}"#
    );
    let pack = parse_active_pack(present.as_bytes(), "shop", "oo_r_wide").unwrap();
    assert_eq!(pack.pack_sha256, digest);
    assert_eq!(pack.source_git_commit, commit);
    assert_eq!(pack.content_hash, digest);
    assert!(parse_active_pack(br#"{"operation":"getActiveRolePack","published":false}"#, "shop", "oo_r_wide").is_err());
}

#[tokio::test]
async fn private_get_active_role_pack_refuses_until_published() {
    let root = std::env::temp_dir().join(format!("oo-a397-rel-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let state = root.join("state.json");
    let digest = "ab".repeat(32);
    let commit = "cd".repeat(20);
    fs::write(&state, r#"{"packs":[]}"#).unwrap();
    let port = serve_private_release(state.clone()).await.unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    assert_eq!(fetch_active_pack(&origin, "shop", "oo_r_wide").await.unwrap_err().reason, "pack_unpublished");
    let body = format!(
        r#"{{"packs":[{{"project_id":"shop","role":"oo_r_wide","published":true,"operation_id":"rel-shop-wide","pack_sha256":"{digest}","source_git_commit":"{commit}","policy":"pol-1","knowledge":"know-1","content_hash":"{digest}"}}]}}"#
    );
    fs::write(&state, body).unwrap();
    let pack = fetch_active_pack(&origin, "shop", "oo_r_wide").await.unwrap();
    assert_eq!(pack.operation_id, "rel-shop-wide");
    assert_eq!(pack.pack_sha256, digest);
    assert_eq!(pack.policy, "pol-1");
    assert_eq!(pack.knowledge, "know-1");
    assert!(pack.published);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn pack_digest_is_sorted_path_lines() {
    let root = std::env::temp_dir().join(format!("oo-a397-pack-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("b.txt"), b"yo").unwrap();
    fs::write(root.join("nested/a.txt"), b"hi\n").unwrap();
    let digest = pack_digest(&root).unwrap();
    let b = agent_entry::hex_encode(&agent_entry::sha256(b"yo"));
    let a = agent_entry::hex_encode(&agent_entry::sha256(b"hi\n"));
    let body = format!("b.txt {b}\nnested/a.txt {a}\n");
    assert_eq!(digest, agent_entry::hex_encode(&agent_entry::sha256(body.as_bytes())));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn close_receipt_needs_eleven_steps_and_s8_absence() {
    let mut steps = Vec::new();
    for id in ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11"] {
        steps.push(json!({"id": id, "status": "confirmed", "actual_absence": id == "s8"}));
    }
    let receipt = json!({
        "phase": "closed",
        "production_signed": false,
        "deployment": "localprocess",
        "retryable": false,
        "steps": steps,
    });
    assert!(receipt_closed(200, &receipt));
    assert!(!receipt_closed(409, &receipt));
    let mut open = receipt.clone();
    open["phase"] = json!("ready");
    assert!(!receipt_closed(200, &open));
}

#[test]
fn query_id_comes_from_the_query_pg_hash() {
    let hash = "ab".repeat(32);
    let stdout = format!(
        "{{\"record\":\"row\",\"turn\":0,\"cells\":[{{\"kind\":\"int\",\"value\":\"90000\"}}]}}\n{{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"truncated\":false,\"returned_rows\":1,\"original_sha256\":\"{hash}\",\"executed_sha256\":\"{hash}\"}}\n"
    );
    let view = parse_query_pg_stdout(&stdout).expect("stdout");
    assert_eq!(view.query_id, query_id_from_hash(&hash).expect("id"));
    assert_eq!(view.result_text, "90000");
    assert_eq!(view.query_id.len(), 34);
    assert!(parse_query_pg_stdout("{\"record\":\"summary\",\"turn\":0,\"status\":\"failed\",\"original_sha256\":\"ab\"}\n").is_err());
}
