use oo_product_stack::{health, identity_commit, journey, preflight, sbom};

fn gate(profile: &str, runsc: &str, memory: i64, disk: i64) -> String {
    format!(
        r#"{{"profile":"{profile}","runsc":"{runsc}","runtimes":["runc"],"arch":"aarch64","disk_free_bytes":{disk},"memory_bytes":{memory},"ports_free":true,"secret_mode_ok":true,"docker_at_least_24":true,"compose_v2":true,"pinned_images_present":true,"observability_images_pinned":false}}"#
    )
}

#[test]
fn standard_without_runsc_fails_closed_and_does_not_substitute_runc() {
    let decision = preflight(&gate("standard", "absent", 32i64 << 30, 40i64 << 30)).expect("parsed");
    assert!(!decision.ready);
    assert!(decision.body.contains("\"runc_substituted\":false"));
    assert!(decision.body.contains("\"runtime_unavailable\""));
    assert!(decision.body.contains("\"production_signed\":false"));
    assert!(decision.body.contains("\"allow\":false"));
}

#[test]
fn dev_is_localprocess_and_not_production() {
    let decision = preflight(&gate("dev", "absent", 8i64 << 30, 10i64 << 30)).expect("parsed");
    assert!(decision.ready);
    assert!(decision.body.contains("\"deployment\":\"localprocess\""));
    assert!(decision.body.contains("\"production_signed\":false"));
    assert!(!decision.body.contains("\"runsc\":\"registered\""));
}

#[test]
fn minimal_does_not_claim_a_physical_replica() {
    let decision = preflight(&gate("minimal", "registered", 8i64 << 30, 20i64 << 30)).expect("parsed");
    assert!(decision.ready);
    assert!(decision.body.contains("\"physical_replica\":false"));
    assert!(decision.body.contains("\"replica\":\"not_claimed\""));
}

#[test]
fn observability_without_pinned_images_is_refused() {
    let text = gate("observability", "registered", 32i64 << 30, 40i64 << 30);
    let decision = preflight(&text).expect("parsed");
    assert!(!decision.ready);
    assert!(decision.body.contains("images_unpinned"));
}

#[test]
fn fabricated_production_signature_is_malformed() {
    let text = r#"{"profile":"dev","production_signed":true,"runsc":"absent","runtimes":[],"arch":"aarch64","disk_free_bytes":9000000000,"memory_bytes":9000000000,"ports_free":true,"secret_mode_ok":true,"docker_at_least_24":true,"compose_v2":true,"pinned_images_present":true,"observability_images_pinned":false}"#;
    assert!(preflight(text).is_err());
}

#[test]
fn tcp_probe_is_not_ready() {
    let text = r#"{"profile":"dev","deployment":"localprocess","runsc":"not_used","exchanges":[{"name":"oidc_current","http":200,"tcp_only":true,"body":"{}"}]}"#;
    let decision = health(text).expect("parsed");
    assert!(!decision.ready);
    assert!(decision.body.contains("tcp_only"));
    assert!(decision.body.contains("catalog"));
}

#[test]
fn listen_line_is_not_a_closed_session() {
    let hash = "ab".repeat(32);
    let body = serde_json_string(r#"{"contract":"session-control-api-v1","listening":true,"production_signed":false,"deployment":"localprocess"}"#);
    let text = format!(
        r#"{{"profile":"dev","deployment":"localprocess","runsc":"not_used","exchanges":[{{"name":"session_close","http":200,"tcp_only":false,"body":{body}}}],"before":{{"secret_sha256":"{hash}","rows":{{"n":1}}}},"after_repeat":{{"secret_sha256":"{hash}","rows":{{"n":1}}}},"after_restart":{{"secret_sha256":"{hash}","rows":{{"n":1}}}},"secret_files":[{{"path":"pg.pass","mode":"0600"}}],"resources_after_cleanup":[],"foreign_before":["oo-a373-pg"],"foreign_after":["oo-a373-pg"]}}"#
    );
    let decision = journey(&text).expect("parsed");
    assert!(!decision.ready);
    assert!(decision.body.contains("journey_incomplete"));
}

#[test]
fn complete_dev_journey_is_ready_and_does_not_accept_the_parent() {
    let decision = journey(&complete()).expect("parsed");
    assert!(decision.ready, "{}", decision.body);
    assert!(decision.body.contains("\"parent_accepted\":false"));
    assert!(decision.body.contains("\"goal_accepted\":false"));
    assert!(decision.body.contains("\"production_signed\":false"));
    assert!(decision.body.contains("\"deployment\":\"localprocess\""));
}

#[test]
fn repeat_drift_and_foreign_cleanup_are_refused() {
    let text = complete().replace(
        "\"after_repeat\":{\"secret_sha256\":\"abababababababababababababababababababababababababababababababab\",\"rows\":{\"n\":1}}",
        "\"after_repeat\":{\"secret_sha256\":\"abababababababababababababababababababababababababababababababab\",\"rows\":{\"n\":2}}",
    );
    let decision = journey(&text).expect("parsed");
    assert!(!decision.ready);
    assert!(decision.body.contains("repeat_drift"));
    let leaked = complete().replace("\"resources_after_cleanup\":[]", "\"resources_after_cleanup\":[\"oo-a395-pg\"]");
    let decision = journey(&leaked).expect("parsed");
    assert!(decision.body.contains("leaked_resources"));
    let foreign = complete().replace("\"foreign_after\":[\"oo-a373-pg\"]", "\"foreign_after\":[]");
    let decision = journey(&foreign).expect("parsed");
    assert!(decision.body.contains("foreign_touched"));
}

#[test]
fn secret_mode_must_be_owner_only() {
    let text = complete().replace("\"mode\":\"0600\"", "\"mode\":\"0644\"");
    let decision = journey(&text).expect("parsed");
    assert!(!decision.ready);
    assert!(decision.body.contains("secrets"));
}

#[test]
fn identity_commit_matches_the_canonical_preimage() {
    let text = r#"{"audience":"oo-a395","client_id":"oo-a395-client","issuer":"https://idp.oo-a395.invalid","revision":1,"redirect_uris":["https://identity.oo-a395.invalid/control/v1/auth/callback"],"bindings":[{"project_id":"alpha","role":"admin","subject":"fixture-admin"}],"bootstrap":{"project_id":"alpha","role":"admin","subject":"fixture-admin"}}"#;
    let decision = identity_commit(text).expect("parsed");
    assert!(decision.body.contains("9e1100b1ed6c6e132b9ba757ad43e3cb2428c1cf88e12bbf6c76a72f45b1f639"));
}

#[test]
fn sbom_keeps_recipe_only_units_unlocked() {
    let text = r#"{"units":[{"name":"source-control-api","commit":"uncommitted","status":"recipe_only","lock_text":""},{"name":"demo","commit":"abc","status":"fixed","lock_text":"[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\nchecksum = \"aa\"\n"}]}"#;
    let decision = sbom(text).expect("parsed");
    assert!(decision.body.contains("\"status\":\"recipe_only\""));
    assert!(decision.body.contains("\"locked\":false"));
    assert!(decision.body.contains("\"checksum\":\"aa\""));
    assert!(decision.body.contains("\"parent_accepted\":false"));
}

fn complete() -> String {
    let hash = "ab".repeat(32);
    let evidence = "cd".repeat(32);
    let mut steps = String::from("[");
    for (index, id) in ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11"].iter().enumerate() {
        if index > 0 {
            steps.push(',');
        }
        let (status, absence) = match *id {
            "s5" => ("skipped", "false"),
            "s8" => ("confirmed", "true"),
            _ => ("confirmed", "false"),
        };
        steps.push_str(&format!(
            r#"{{"id":"{id}","status":"{status}","actual_absence":{absence},"evidence_sha256":"{evidence}"}}"#
        ));
    }
    steps.push(']');
    let oidc = r#"{"contract":"oo.control-identity/v1","operation":"getIdentityCurrent","install_state":"initialized","authority":"management_session","customer_integration":"not_claimed","data_grants":"none"}"#;
    let catalog = r#"{"contract":"oo.source-control.exchange/v1","operation":"getSourceCatalog","catalog":{"rows":100000},"published":false,"build_review":"not_claimed","release":"not_claimed","customer_integration":"not_claimed"}"#;
    let pack = r#"{"contract":"oo.selfbuild-control-api/v1","operation":"compileBuildPack","published":false,"active":false,"compiled_pack":"not_published","data_grants":"none","management_identity":"verified","producer_authenticated":false,"producer":{"protocol":"oo.pg-builder.pack-combined/v1","authenticated":false,"statements":{"model_publication":false},"product":{"packs":[{},{}]}}}"#;
    let release = r#"{"contract":"oo.release-control-api/v1","production_ready":false,"full_sp08_accepted":false,"certificate_class":"test_scope_not_production","customer_gate_verified":false,"runsc_verified":false}"#;
    let session = format!(
        r#"{{"contract":"session-control-api-v1","disposition":"closed","phase":"closed","deployment":"localprocess","production_signed":false,"retryable":false,"reducer_physical_release_certified":false,"steps":{steps}}}"#
    );
    let audit = r#"{"protocol":"oo.audit.ingest/v1","spool_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","request_id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","receipt_id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","payload_hash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","event_seq":1}"#;
    format!(
        r#"{{"profile":"dev","deployment":"localprocess","runsc":"not_used","exchanges":[{oidc},{catalog},{pack},{release},{session},{audit}],"before":{{"secret_sha256":"{hash}","rows":{{"n":1}}}},"after_repeat":{{"secret_sha256":"{hash}","rows":{{"n":1}}}},"after_restart":{{"secret_sha256":"{hash}","rows":{{"n":1}}}},"secret_files":[{{"path":"pg.pass","mode":"0600"}}],"resources_after_cleanup":[],"foreign_before":["oo-a373-pg"],"foreign_after":["oo-a373-pg"]}}"#,
        oidc = exchange("oidc_current", oidc),
        catalog = exchange("catalog", catalog),
        pack = exchange("pack", pack),
        release = exchange("release", release),
        session = exchange("session_close", &session),
        audit = exchange("audit_ack", audit),
    )
}

fn exchange(name: &str, body: &str) -> String {
    format!(r#"{{"name":"{name}","http":200,"tcp_only":false,"body":{}}}"#, serde_json_string(body))
}

fn serde_json_string(body: &str) -> String {
    let mut out = String::from("\"");
    for byte in body.chars() {
        match byte {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
