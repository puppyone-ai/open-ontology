#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{SystemTime, UNIX_EPOCH};

use minimal_loop_acceptance::{
    config_is_secret_free, fresh_copy_performed, judge, parse, run_card, sha256_hex, CardOpts, Cell, FaultObs,
    MeasuredFile, Observation, QuestionObs, ToolObs,
};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn unmeasured_observation_cannot_pass() {
    let verdict = judge(&Observation::unmeasured());
    assert!(!verdict.passed);
    assert_eq!(verdict.exit_code, 4);
    assert!(!verdict.independent_approval);
    assert!(!verdict.parent_acceptance);
    assert!(!verdict.goal_acceptance);
    assert!(!verdict.runsc_certified);
    assert!(!verdict.arbitrary_database);
    assert!(!verdict.full_plan);
    let json = verdict.json();
    assert!(json.contains("\"parent_acceptance\":false"));
    assert!(json.contains("\"goal_acceptance\":false"));
    assert!(json.contains("\"runsc_certified\":false"));
    assert!(json.contains("\"full_plan\":false"));
}

#[test]
fn measured_complete_card_passes_without_signing_the_plan() {
    let verdict = judge(&accepted());
    let shown: Vec<String> = verdict
        .checks
        .iter()
        .map(|item| {
            let id = &item.id;
            let status = &item.status;
            format!("{id}:{status}")
        })
        .collect();
    let reasons: Vec<String> = verdict
        .checks
        .iter()
        .map(|item| {
            let id = &item.id;
            let reason = &item.reason;
            format!("{id} {reason}")
        })
        .collect();
    assert_eq!(
        shown,
        vec![
            "archive:pass".to_owned(),
            "MP-01a:pass".to_owned(),
            "MP-01b:pass".to_owned(),
            "MP-01c:pass".to_owned(),
            "MP-01d:pass".to_owned(),
            "MP-01:pass".to_owned(),
            "MP-02:pass".to_owned(),
            "MP-03:pass".to_owned(),
            "MP-04:pass".to_owned(),
            "MP-05:pass".to_owned(),
            "MP-06:pass".to_owned(),
        ],
        "{reasons:?}"
    );
    assert!(verdict.passed);
    assert_eq!(verdict.exit_code, 0);
    assert!(verdict.independent_approval);
    assert!(!verdict.parent_acceptance);
    assert!(!verdict.goal_acceptance);
    assert!(!verdict.runsc_certified);
}

#[test]
fn unrelated_worktree_drift_does_not_reject_the_fixed_archive() {
    let mut obs = accepted();
    obs.producer_dirty = true;
    obs.producer_untracked = true;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "archive"), "pass");
    assert!(verdict.passed);
    assert!(verdict.independent_approval);
    assert!(!verdict.parent_acceptance);
}

#[test]
fn moving_source_is_not_attested() {
    let mut obs = accepted();
    obs.moving_source_attested = true;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "archive"), "fail");
    assert!(verdict.checks.iter().any(|item| item.id == "archive" && item.reason.contains("moving source was attested")));
    assert!(!verdict.independent_approval);
    assert_eq!(verdict.exit_code, 1);
}

#[test]
fn missing_agent_entry_does_not_fail_the_archive() {
    let obs = accepted();
    assert!(!obs.a397_committed);
    assert!(!obs.agent_entry_runtime);
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "archive"), "pass");
    assert!(verdict.passed);
    assert!(!verdict.checks.iter().any(|item| item.reason.contains("tools/agent-entry")));
    assert!(verdict.json().contains("\"http_release_acceptance\":false"));
}

#[test]
fn agent_entry_runtime_and_http_release_fail_closed() {
    let mut obs = accepted();
    obs.agent_entry_runtime = true;
    assert_eq!(status(&judge(&obs), "archive"), "fail");
    obs.agent_entry_runtime = false;
    obs.http_release_claimed = true;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "archive"), "fail");
    assert!(!verdict.passed);
    assert!(verdict.json().contains("\"http_release_acceptance\":false"));
}

#[test]
fn capability_review_is_required_and_the_preserved_import_is_not() {
    let mut obs = accepted();
    assert_ne!(obs.import_hash, "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73");
    assert_eq!(status(&judge(&obs), "MP-01d"), "pass");
    obs.copy_capability_reviewed = false;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "MP-01d"), "fail");
    assert!(verdict.checks.iter().any(|item| item.reason.contains("independent copy capability review")));
    assert_ne!(status(&verdict, "MP-01"), "pass");
    assert!(!verdict.passed);
}

#[test]
fn old_row_counts_do_not_pass_the_revised_mp01() {
    let mut obs = accepted();
    obs.copy_from_connection_config = false;
    obs.support_plan_recorded = false;
    obs.config_redacted = false;
    obs.copy_performed = false;
    obs.typed_reconciliation = false;
    obs.typed_null = false;
    obs.typed_int8 = false;
    obs.typed_decimal = false;
    obs.typed_time = None;
    obs.typed_binary = None;
    obs.typed_key_coverage = false;
    obs.canonical_hashes_match = false;
    obs.snapshot_mode.clear();
    obs.source_writes = None;
    obs.fault_cancel_clean = false;
    obs.fault_timeout_clean = false;
    obs.fault_drift_clean = false;
    obs.fault_replay_clean = false;
    obs.no_partial_visibility = false;
    obs.no_source_rewrite = false;
    obs.no_snapshot_splice = false;
    obs.no_duplicate_side_effect = false;
    obs.backend_stopped = false;
    obs.source_unreachable_after_seal = false;
    obs.source_secret_withheld = false;
    obs.local_chain_on_copy = false;
    obs.audit_generation_bound = false;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "MP-01"), "fail");
    assert_ne!(status(&verdict, "MP-01a"), "pass");
    assert_ne!(status(&verdict, "MP-01b"), "pass");
    assert!(!verdict.passed);
}

#[test]
fn blocked_dependencies_stay_pending() {
    let mut obs = Observation::unmeasured();
    obs.measured = true;
    obs.dependency_probe = true;
    obs.head = COMMIT.to_owned();
    obs.pinned_commit = COMMIT.to_owned();
    obs.rows_party = Some(2000);
    obs.rows_account = Some(8000);
    obs.rows_entry = Some(90000);
    obs.server_version = "17.10".to_owned();
    obs.source_identity = "source-a".to_owned();
    obs.target_identity = "target-b".to_owned();
    obs.fixture_source_rows_ok = true;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "MP-01"), "pending");
    assert_eq!(status(&verdict, "MP-01a"), "pending");
    assert_eq!(status(&verdict, "MP-01b"), "pending");
    assert_eq!(status(&verdict, "MP-01c"), "pending");
    assert_eq!(status(&verdict, "MP-01d"), "pending");
    assert_eq!(status(&verdict, "MP-02"), "pending");
    assert_eq!(status(&verdict, "MP-03"), "pending");
    assert_eq!(status(&verdict, "archive"), "fail");
    assert_ne!(verdict.exit_code, 0);
    assert!(!verdict.passed);
    assert!(!verdict.independent_approval);
}

#[test]
fn missing_producer_binary_is_not_a_mock_pass() {
    let mut obs = accepted();
    obs.binary_present = false;
    obs.binary_from_archive = false;
    obs.binary_sha_matches = false;
    obs.doctor_ran = false;
    obs.run_ran = false;
    obs.ask_ran = false;
    obs.close_ran = false;
    obs.questions.clear();
    let verdict = judge(&obs);
    assert!(!verdict.passed);
    assert_ne!(verdict.exit_code, 0);
    assert!(!verdict.independent_approval);
}

#[test]
fn prompt_sql_or_gold_number_fails() {
    let mut obs = accepted();
    obs.questions[0].prompt = format!("{} SELECT 1", obs.questions[0].prompt);
    assert_eq!(status(&judge(&obs), "MP-03"), "fail");
    let mut obs = accepted();
    obs.questions[0].prompt = format!("{} 90000", obs.questions[0].prompt);
    assert_eq!(status(&judge(&obs), "MP-03"), "fail");
}

#[test]
fn operator_approval_is_not_a_tool() {
    let mut obs = accepted();
    obs.operator_approval_as_tool = true;
    assert_eq!(status(&judge(&obs), "MP-02"), "fail");
    let mut obs = accepted();
    obs.questions[0].tools[1].name = "approval".to_owned();
    assert_eq!(status(&judge(&obs), "MP-03"), "fail");
}

#[test]
fn wrong_cells_query_citation_or_proxy_fail() {
    let mut obs = accepted();
    obs.questions[0].cells[0][0] = Cell::Text("1".to_owned());
    assert_eq!(status(&judge(&obs), "MP-04"), "fail");
    let mut obs = accepted();
    obs.questions[1].audit_query_id = "q_other".to_owned();
    assert_eq!(status(&judge(&obs), "MP-03"), "fail");
    let mut obs = accepted();
    obs.questions[2].answer = "no citation".to_owned();
    assert_eq!(status(&judge(&obs), "MP-04"), "fail");
    let mut obs = accepted();
    obs.questions[3].proxy_presented_session = true;
    assert_eq!(status(&judge(&obs), "MP-03"), "fail");
    let mut obs = accepted();
    obs.questions[4].proxy_marker_mtls = false;
    assert_eq!(status(&judge(&obs), "MP-03"), "fail");
}

#[test]
fn pack_hash_rows_and_source_binding_fail_closed() {
    let mut obs = accepted();
    obs.files[0].sha256 = "b".repeat(64);
    assert_eq!(status(&judge(&obs), "MP-02"), "fail");
    let mut obs = accepted();
    obs.readme = "published".to_owned();
    assert_eq!(status(&judge(&obs), "MP-02"), "fail");
    let mut obs = accepted();
    obs.rows_entry = Some(1);
    assert_eq!(status(&judge(&obs), "MP-01"), "fail");
    let mut obs = accepted();
    obs.target_identity = obs.source_identity.clone();
    assert_eq!(status(&judge(&obs), "MP-01"), "fail");
    let mut obs = accepted();
    obs.source_access_after_seal = Some(1);
    assert_eq!(status(&judge(&obs), "MP-01"), "fail");
    let mut obs = accepted();
    obs.peak_connections = Some(2);
    assert_eq!(status(&judge(&obs), "MP-01"), "fail");
    let mut obs = accepted();
    obs.pack_hash_second = "different".to_owned();
    assert_eq!(status(&judge(&obs), "MP-02"), "fail");
}

#[test]
fn write_bad_model_and_timeout_cannot_look_successful() {
    let mut obs = accepted();
    obs.write_reject.exit = Some(0);
    obs.write_reject.status = "success".to_owned();
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    let mut obs = accepted();
    obs.bad_model.status = "validated".to_owned();
    obs.bad_model.exit = Some(0);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    let mut obs = accepted();
    obs.timeout_cancel.status = "validated".to_owned();
    obs.timeout_cancel.exit = Some(0);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    let mut obs = accepted();
    obs.timeout_cancel.incomplete = true;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "MP-05"), "incomplete");
    assert_eq!(verdict.exit_code, 4);
    assert!(!verdict.passed);
}

#[test]
fn close_must_revoke_without_dropping_the_database() {
    let mut obs = accepted();
    obs.database_dropped = Some(true);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    let mut obs = accepted();
    obs.mapping_revoked = Some(false);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    let mut obs = accepted();
    obs.residue = Some(true);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    let mut obs = accepted();
    obs.close_pid_absent = Some(false);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    let mut obs = accepted();
    obs.source_dropped = Some(true);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
    assert_eq!(status(&judge(&obs), "MP-01d"), "fail");
    let mut obs = accepted();
    obs.copy_dropped = Some(true);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
}

#[test]
fn broken_stream_stays_incomplete() {
    let mut obs = accepted();
    obs.stream_incomplete = true;
    let verdict = judge(&obs);
    assert_eq!(verdict.exit_code, 4);
    assert!(!verdict.passed);
    assert_eq!(verdict.checks[0].status, "incomplete");
}

#[test]
fn bash_before_read_fails() {
    let mut obs = accepted();
    obs.questions[0].tools.swap(0, 1);
    assert_eq!(status(&judge(&obs), "MP-03"), "fail");
}

#[test]
fn config_rejects_inline_secrets_and_allows_file_refs() {
    assert!(!config_is_secret_free(r#"{"password":"x"}"#));
    assert!(!config_is_secret_free(r#"{"database":{"token":"abc"}}"#));
    assert!(config_is_secret_free(r#"{"database":{"password_file":"/tmp/p","secret_ref":"secret://pg/a"}}"#));
}

#[test]
fn card_on_a_dirty_archive_does_not_approve_or_invent_a_run() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a407-card-{stamp}"));
    let archive = root.join("archive");
    let repo = root.join("repo");
    let out = root.join("out");
    write(&archive.join("tools/minimal-product-loop/src/lib.rs"), "committed\n");
    write(&archive.join("deploy/minimal-product-loop/README.md"), "committed\n");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    write(&repo.join("tools/minimal-product-loop/src/lib.rs"), "dirty\n");
    write(&repo.join("tools/minimal-product-loop/src/proxy.rs"), "untracked\n");
    write(&repo.join("deploy/minimal-product-loop/README.md"), "committed\n");
    write(&repo.join("tools/agent-entry/src/lib.rs"), "untracked\n");
    let verdict = run_card(&CardOpts {
        repo,
        archive,
        out: out.clone(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: None,
        execute: true,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: None,
        kit_root: None,
        sealed_manifest: None,
        copy_review: None,
        product_note: None,
        fixture_probe: None,
        import_manifest: None,
        binding_probe: None,
    })
    .expect("card");
    assert!(!verdict.passed);
    assert!(!verdict.independent_approval);
    assert_ne!(verdict.exit_code, 0);
    assert_eq!(status(&verdict, "archive"), "fail");
    let archive = verdict.checks.iter().find(|item| item.id == "archive").expect("archive");
    assert!(!archive.reason.contains("tools/agent-entry"));
    assert!(archive.reason.contains("binary"));
    assert!(!archive.reason.contains("worktree"));
    assert_eq!(status(&verdict, "MP-03"), "not_run");
    let stored = fs::read_to_string(out.join("verdict.json")).expect("verdict");
    assert!(stored.contains("\"parent_acceptance\":false"));
    assert!(stored.contains("\"goal_acceptance\":false"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn pi_pin_binds_only_the_published_commit_and_file_hashes() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a407-pin-{stamp}"));
    let archive = root.join("archive");
    let repo = root.join("repo");
    let executable = root.join("pi");
    let runtime = root.join("node");
    write(&executable, "pi-bytes");
    write(&runtime, "node-bytes");
    write(&archive.join("tools/minimal-product-loop/src/lib.rs"), "same\n");
    write(&archive.join("deploy/minimal-product-loop/README.md"), "same\n");
    write(&repo.join("tools/minimal-product-loop/src/lib.rs"), "same\n");
    write(&repo.join("deploy/minimal-product-loop/README.md"), "same\n");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    let executable_sha = sha256_hex(b"pi-bytes");
    let runtime_sha = sha256_hex(b"node-bytes");
    let config = root.join("config.json");
    write(&config, &format!(
        "{{\"pi\":{{\"executable_sha256\":\"{executable_sha}\",\"runtime_sha256\":\"{runtime_sha}\"}}}}"
    ));
    let out = root.join("out");
    let verdict = run_card(&pin_opts(&repo, &archive, &out, &config, &executable, &runtime, "7c10bd4337495ee613f2224843ecdf349b80d1df")).expect("card");
    let measurement = fs::read_to_string(out.join("measurement.txt")).expect("measurement");
    assert!(measurement.contains("pi_pins_match=true"));
    assert!(!verdict.passed);
    assert!(!verdict.independent_approval);
    let out_bad = root.join("out-bad");
    let _ = run_card(&pin_opts(&repo, &archive, &out_bad, &config, &executable, &runtime, "0000000000000000000000000000000000000000")).expect("card");
    let bad = fs::read_to_string(out_bad.join("measurement.txt")).expect("measurement");
    assert!(bad.contains("pi_pins_match=false"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn inventory_compares_the_archive_and_a_fixture_probe_is_not_a_copy() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a407-inventory-{stamp}"));
    let archive = root.join("archive");
    let repo = root.join("repo");
    write(&archive.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    write(&archive.join("deploy/minimal-product-loop/README.md"), "fixed\n");
    write(&archive.join("tools/agent-entry/src/lib.rs"), "fixed\n");
    write(&repo.join("tools/minimal-product-loop/src/lib.rs"), "moved\n");
    write(&repo.join("deploy/minimal-product-loop/README.md"), "fixed\n");
    write(&repo.join("tools/agent-entry/src/lib.rs"), "moved\n");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    let lib_sha = sha256_hex(b"fixed\n");
    let inventory = root.join("inventory.json");
    write(
        &inventory,
        &format!(
            "{{\"commit\":\"{COMMIT}\",\"files\":{{\"tools/minimal-product-loop/src/lib.rs\":\"{lib_sha}\",\"deploy/minimal-product-loop/README.md\":\"{lib_sha}\",\"tools/agent-entry/src/lib.rs\":\"{lib_sha}\"}}}}"
        ),
    );
    let probe = root.join("probe.txt");
    write(
        &probe,
        "source_system_identifier=111\ntarget_system_identifier=222\nsource_version=17.10\ntarget_version=17.10\nsource_party=2000\nsource_account=8000\nsource_entry=90000\ntarget_shop=0\ntarget_entry_regclass=absent\nsource_null_display=118\nsource_amount_3=1234567890.1234\nsource_qty_0=-9223372036854775808\nsource_qty_1=9223372036854775807\nsource_qty_2=0\nsource_party_0_display=\\N\nsource_party_0_active=t\nsource_account_0_party=p00000\nloader_connections=1\n",
    );
    let missing = root.join("missing-sealed.json");
    let out = root.join("out");
    let verdict = run_card(&CardOpts {
        repo,
        archive,
        out: out.clone(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: None,
        execute: false,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: Some(inventory),
        kit_root: None,
        sealed_manifest: Some(missing),
        import_manifest: None,
        copy_review: None,
        product_note: None,
        fixture_probe: Some(probe),
        binding_probe: None,
    })
    .expect("card");
    let measurement = fs::read_to_string(out.join("measurement.txt")).expect("measurement");
    assert!(measurement.contains("inventory_matches=true"), "{measurement}");
    assert!(measurement.contains("producer_dirty=true"), "{measurement}");
    assert!(measurement.contains("moving_source_attested=false"), "{measurement}");
    assert!(measurement.contains("copy_performed=false"), "{measurement}");
    assert!(measurement.contains("fixture_source_rows_ok=true"), "{measurement}");
    assert!(measurement.contains("fixture_target_empty=true"), "{measurement}");
    assert!(measurement.contains("fixture_typed_samples_ok=true"), "{measurement}");
    assert!(measurement.contains("fixture_identities_distinct=true"), "{measurement}");
    assert_eq!(status(&verdict, "MP-01"), "pending");
    assert_ne!(verdict.exit_code, 0);
    assert!(!verdict.passed);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn author_seal_is_not_an_independent_signature() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a407-seal-{stamp}"));
    let archive = root.join("archive");
    let repo = root.join("repo");
    write(&archive.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    write(&repo.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    let target_body = r#"{"ca_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","database":"mirrordb","host":"db","port":5432,"protocol":"oo.pipeline.local-pg-mirror-target/v1","server_name":"db","tls":"verify-full","user":"reader"}"#;
    let target = root.join("target.json");
    write(&target, target_body);
    let target_sha = sha256_hex(target_body.as_bytes());
    let table = |name: &str, rows: i64, columns: &str| {
        format!(r#"{{"schema":"public","name":"{name}","physical_name":"pt_{name}","rows":{rows},"columns":[{columns}]}}"#)
    };
    let manifest_body = format!(
        r#"{{"protocol":"oo.pipeline.local-pg-mirror-manifest/v1","mirror_state":"complete","customer_complete":false,"consistency":"single_snapshot","source_socket_peak":1,"source_socket_fact":"repeatable_read_read_only","source_write_tuples":0,"import_generation":1,"import_hash":"{SHA}","snapshot_id":"00000004-00000013-1","target_config_sha256":"{target_sha}","tables_completed":[{},{},{}]}}"#,
        table("batch_a", 2000, r#"{"name":"id","oid":20,"typmod":-1},{"name":"amount","oid":1700,"typmod":2490376}"#),
        table("batch_b", 8000, r#"{"name":"at_time","oid":1083,"typmod":-1},{"name":"blob","oid":17,"typmod":-1}"#),
        table("batch_c", 90000, r#"{"name":"qty","oid":20,"typmod":-1},{"name":"amount","oid":1700,"typmod":2490376}"#),
    );
    let manifest = root.join("manifest.json");
    write(&manifest, &manifest_body);
    let review = root.join("review.json");
    write(&review, &format!(r#"{{"protocol":"oo.acceptance.local-copy-verdict/v1","mp01_signed":true,"provenance":"author","import_hash":"{SHA}"}}"#));
    let note = root.join("kit.txt");
    write(&note, "kit_consumed=true\ncommit=0123456789abcdef0123456789abcdef01234567\nmp01_signed=false\n");
    let probe = root.join("probe.txt");
    write(
        &probe,
        &format!(
            "import_hash={SHA}\ngeneration=1\nsnapshot_id=00000004-00000013-1\nrunner_source_connects=0\npublic_batch_a=absent\npublic_batch_b=absent\npublic_batch_c=absent\nbatch_a_rows=2000\nbatch_b_rows=8000\nbatch_c_rows=90000\nbatch_a_id_oid=20\nbatch_a_id_typmod=-1\nbatch_a_amount_oid=1700\nbatch_a_amount_typmod=2490376\nbatch_b_time_oid=1083\nbatch_b_time_typmod=-1\nbatch_b_blob_oid=17\nbatch_b_blob_typmod=-1\nbatch_c_qty_oid=20\nbatch_c_qty_typmod=-1\nbatch_c_amount_oid=1700\nbatch_c_amount_typmod=2490376\nbatch_a_null=t\nbatch_a_amount_2=1234567890123456789012345678901234.1234\nbatch_b_time_1=13:14:15.123456\nbatch_b_blob_1=0001ff\nbatch_c_qty_1=9223372036854775807\nbatch_c_qty_2=-9223372036854775808\ncancel_select=42501\npartial_select=42501\n"
        ),
    );
    let out = root.join("out");
    let verdict = run_card(&seal_opts(&root, &out, &manifest)).expect("card");
    let measurement = fs::read_to_string(out.join("measurement.txt")).expect("measurement");
    assert!(measurement.contains("sealed_manifest_present=true"), "{measurement}");
    assert!(measurement.contains("copy_review_signed=false"), "{measurement}");
    assert!(measurement.contains("copy_capability_reviewed=false"), "{measurement}");
    assert!(measurement.contains("product_consumes_kit=false"), "{measurement}");
    assert!(measurement.contains("copy_performed=false"), "{measurement}");
    assert!(measurement.contains("typed_reconciliation=false"), "{measurement}");
    assert!(measurement.contains("binding_rows_ok=true"), "{measurement}");
    assert!(measurement.contains("binding_types_ok=true"), "{measurement}");
    assert!(measurement.contains("binding_samples_ok=true"), "{measurement}");
    assert!(measurement.contains("binding_partial_denied=true"), "{measurement}");
    assert!(measurement.contains("author_customer_complete=Some(false)"), "{measurement}");
    assert!(measurement.contains(&format!("import_hash={SHA}")), "{measurement}");
    assert_eq!(status(&verdict, "MP-01"), "pending");
    assert_eq!(status(&verdict, "MP-01b"), "pending");
    assert!(!verdict.passed);
    assert!(!verdict.independent_approval);
    let claimed = root.join("claimed.json");
    write(&claimed, &manifest_body.replace("\"customer_complete\":false", "\"customer_complete\":true"));
    let out_claimed = root.join("out-claimed");
    let claimed_verdict = run_card(&seal_opts(&root, &out_claimed, &claimed)).expect("card");
    let claimed_measurement = fs::read_to_string(out_claimed.join("measurement.txt")).expect("measurement");
    assert!(claimed_measurement.contains("sealed_manifest_present=false"), "{claimed_measurement}");
    assert!(claimed_measurement.contains("author_customer_complete=Some(true)"), "{claimed_measurement}");
    assert!(!claimed_verdict.passed);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn whole_signature_of_the_same_import_is_not_the_capability_review() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a415-whole-{stamp}"));
    let archive = root.join("archive");
    let repo = root.join("repo");
    write(&archive.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    write(&repo.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    let review = root.join("review.json");
    write(
        &review,
        &format!(
            r#"{{"protocol":"oo.acceptance.local-copy-verdict/v1","mp01_signed":true,"provenance":"live","import_hash":"{SHA}","customer_complete":false}}"#
        ),
    );
    let out = root.join("out");
    let verdict = run_card(&CardOpts {
        repo,
        archive,
        out: out.clone(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: None,
        execute: false,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: None,
        kit_root: None,
        sealed_manifest: None,
        import_manifest: None,
        copy_review: Some(review),
        product_note: None,
        fixture_probe: None,
        binding_probe: None,
    })
    .expect("card");
    let measurement = fs::read_to_string(out.join("measurement.txt")).expect("measurement");
    assert!(measurement.contains("copy_capability_reviewed=false"), "{measurement}");
    assert!(measurement.contains("copy_review_signed=false"), "{measurement}");
    assert_eq!(status(&verdict, "MP-01d"), "pending");
    assert!(!verdict.passed);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn independent_copy_capability_does_not_need_the_campaign_import() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a415-abc-{stamp}"));
    let archive = root.join("archive");
    let repo = root.join("repo");
    write(&archive.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    write(&archive.join("tools/pi-harness/src/lib.rs"), "pi\n");
    write(&repo.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    write(&repo.join("tools/pi-harness/src/lib.rs"), "pi\n");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    let lib_sha = sha256_hex(b"fixed\n");
    let pi_sha = sha256_hex(b"pi\n");
    let inventory = root.join("inventory.json");
    write(
        &inventory,
        &format!(
            "{{\"commit\":\"{COMMIT}\",\"files\":{{\"tools/minimal-product-loop/src/lib.rs\":\"{lib_sha}\",\"tools/pi-harness/src/lib.rs\":\"{pi_sha}\"}}}}"
        ),
    );
    write(&root.join("review.json"), &capability_verdict());
    write(&root.join("case.json"), &capability_case());
    let out = root.join("out");
    let verdict = run_card(&CardOpts {
        repo,
        archive,
        out: out.clone(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: None,
        execute: false,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: Some(inventory),
        kit_root: None,
        sealed_manifest: None,
        import_manifest: None,
        copy_review: Some(root.join("review.json")),
        product_note: None,
        fixture_probe: None,
        binding_probe: None,
    })
    .expect("card");
    let measurement = fs::read_to_string(out.join("measurement.txt")).expect("measurement");
    assert!(measurement.contains("inventory_matches=true"), "{measurement}");
    assert!(measurement.contains("copy_capability_reviewed=true"), "{measurement}");
    assert!(measurement.contains("copy_review_signed=false"), "{measurement}");
    assert!(measurement.contains("copy_performed=false"), "{measurement}");
    assert!(measurement.contains("a397_committed=false"), "{measurement}");
    assert!(measurement.contains("agent_entry_runtime=false"), "{measurement}");
    assert_eq!(status(&verdict, "MP-01"), "pending");
    assert!(!verdict.passed);
    let authored = capability_verdict().replacen("\"mp01_signed\":false", "\"mp01_signed\":false,\"provenance\":\"author\"", 1);
    write(&root.join("review.json"), &authored);
    let out_author = root.join("out-author");
    let author = run_card(&CardOpts {
        repo: root.join("repo"),
        archive: root.join("archive"),
        out: out_author.clone(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: None,
        execute: false,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: None,
        kit_root: None,
        sealed_manifest: None,
        import_manifest: None,
        copy_review: Some(root.join("review.json")),
        product_note: None,
        fixture_probe: None,
        binding_probe: None,
    })
    .expect("card");
    let author_measurement = fs::read_to_string(out_author.join("measurement.txt")).expect("measurement");
    assert!(author_measurement.contains("copy_capability_reviewed=false"), "{author_measurement}");
    assert!(!author.passed);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn replay_text_does_not_perform_a_copy_and_fresh_phases_do() {
    let stale = SHA;
    let fresh = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let document = |body: &str| parse(body).expect("json");
    assert!(!fresh_copy_performed(&document(&format!(r#"{{"line":"complete 100000 {stale} replay_complete false"}}"#)), ""));
    assert!(!fresh_copy_performed(
        &document(&format!(
            r#"{{"phases":[{{"name":"copy","event":"complete","failure":"replay_complete","mirror_state":"complete","customer_complete":false,"import_hash":"{fresh}"}}]}}"#
        )),
        "",
    ));
    assert!(!fresh_copy_performed(
        &document(&format!(
            r#"{{"phases":[{{"name":"discover","event":"begin","from":"source_connection"}},{{"name":"copy","event":"begin","from":"source_connection"}},{{"name":"copy","event":"complete","failure":"none","mirror_state":"complete","customer_complete":false,"import_hash":"{stale}","snapshot_id":"0000000C-00000009-1"}}]}}"#
        )),
        stale,
    ));
    assert!(!fresh_copy_performed(
        &document(&format!(
            r#"{{"phases":[{{"name":"discover","event":"begin","from":"source_connection"}},{{"name":"copy","event":"begin","from":"source_connection"}},{{"name":"copy","event":"complete","failure":"none","mirror_state":"complete","customer_complete":false,"import_hash":"{fresh}"}}]}}"#
        )),
        stale,
    ));
    assert!(fresh_copy_performed(
        &document(&format!(
            r#"{{"phases":[{{"name":"discover","event":"begin","from":"source_connection"}},{{"name":"copy","event":"begin","from":"source_connection"}},{{"name":"copy","event":"complete","failure":"none","mirror_state":"complete","customer_complete":false,"import_hash":"{fresh}","snapshot_id":"0000000C-00000009-1"}}]}}"#
        )),
        stale,
    ));
    assert!(fresh_copy_performed(
        &document(&format!(
            r#"{{"phases":[{{"name":"copy","event":"complete","failure":"replay_complete","mirror_state":"complete","customer_complete":false,"import_hash":"{stale}","snapshot_id":"0000000B-00000008-1"}},{{"name":"discover","event":"begin","from":"source_connection"}},{{"name":"copy","event":"begin","from":"source_connection"}},{{"name":"copy","event":"complete","failure":"none","mirror_state":"complete","customer_complete":false,"import_hash":"{fresh}","snapshot_id":"0000000C-00000009-1"}}]}}"#
        )),
        "",
    ));
}

#[test]
fn missing_typed_binding_fails_mp01d() {
    let mut obs = accepted();
    obs.binding_rows_ok = false;
    obs.binding_types_ok = false;
    obs.binding_samples_ok = false;
    obs.binding_partial_denied = false;
    obs.binding_source_connects = None;
    let verdict = judge(&obs);
    assert_eq!(status(&verdict, "MP-01d"), "fail");
    assert!(verdict.checks.iter().any(|item| item.id == "MP-01d" && item.reason.contains("typed binding")));
    assert_ne!(status(&verdict, "MP-01"), "pass");
    assert!(!verdict.passed);
}

#[test]
fn pi_harness_omitted_from_inventory_does_not_match_and_agent_entry_may_pack() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a415-inventory-{stamp}"));
    let archive = root.join("archive");
    let repo = root.join("repo");
    write(&archive.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    write(&archive.join("deploy/minimal-product-loop/README.md"), "fixed\n");
    write(&archive.join("tools/pi-harness/src/lib.rs"), "pi\n");
    write(&repo.join("tools/minimal-product-loop/src/lib.rs"), "fixed\n");
    write(&repo.join("deploy/minimal-product-loop/README.md"), "fixed\n");
    write(&repo.join("tools/pi-harness/src/lib.rs"), "pi\n");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    let lib_sha = sha256_hex(b"fixed\n");
    let pi_sha = sha256_hex(b"pi\n");
    let omitted = root.join("omitted.json");
    write(
        &omitted,
        &format!(
            "{{\"commit\":\"{COMMIT}\",\"files\":{{\"tools/minimal-product-loop/src/lib.rs\":\"{lib_sha}\",\"deploy/minimal-product-loop/README.md\":\"{lib_sha}\"}}}}"
        ),
    );
    let out = root.join("out-omitted");
    let verdict = run_card(&inventory_opts(&root, &out, &omitted)).expect("card");
    let measurement = fs::read_to_string(out.join("measurement.txt")).expect("measurement");
    assert!(measurement.contains("inventory_matches=false"), "{measurement}");
    assert_eq!(status(&verdict, "archive"), "fail");
    let packed = root.join("packed.json");
    write(&archive.join("tools/agent-entry/src/lib.rs"), "pack\n");
    let agent_sha = sha256_hex(b"pack\n");
    write(
        &packed,
        &format!(
            "{{\"commit\":\"{COMMIT}\",\"files\":{{\"tools/minimal-product-loop/src/lib.rs\":\"{lib_sha}\",\"deploy/minimal-product-loop/README.md\":\"{lib_sha}\",\"tools/pi-harness/src/lib.rs\":\"{pi_sha}\",\"tools/agent-entry/src/lib.rs\":\"{agent_sha}\"}}}}"
        ),
    );
    let out_packed = root.join("out-packed");
    let _ = run_card(&inventory_opts(&root, &out_packed, &packed)).expect("card");
    let packed_measurement = fs::read_to_string(out_packed.join("measurement.txt")).expect("measurement");
    assert!(packed_measurement.contains("inventory_matches=true"), "{packed_measurement}");
    assert!(packed_measurement.contains("a397_committed=true"), "{packed_measurement}");
    assert!(packed_measurement.contains("agent_entry_runtime=false"), "{packed_measurement}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn user_cli_phases_perform_a_copy_and_the_same_precomplete_hash_does_not() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("oo-a415-phases-{stamp}"));
    let fresh = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let script = root.join("loop");
    executable(&script, &phase_script(fresh));
    let binary_sha = sha256_hex(&fs::read(&script).expect("script"));
    let built = root.join("built.json");
    write(
        &built,
        &format!("{{\"commit\":\"{COMMIT}\",\"built_from\":\"git-archive\",\"binary_sha256\":\"{binary_sha}\"}}"),
    );
    let archive = root.join("archive");
    fs::create_dir_all(&archive).expect("archive");
    fs::write(archive.join("COMMIT"), format!("{COMMIT}\n")).expect("commit");
    fs::create_dir_all(root.join("repo")).expect("repo");
    let target_body = r#"{"ca_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","database":"oo_a407_dst","host":"oo-a407-dst-pg","port":5432,"protocol":"oo.pipeline.local-pg-mirror-target/v1","server_name":"oo-a407-dst-pg","tls":"verify-full","user":"oo_a407_reader"}"#;
    let target = root.join("target.json");
    write(&target, target_body);
    let target_sha = sha256_hex(target_body.as_bytes());
    let same = root.join("same.json");
    write(&same, &bulk_manifest(fresh, &target_sha));
    let other = root.join("other.json");
    write(&other, &bulk_manifest(SHA, &target_sha));
    let mismatch = root.join("mismatch.json");
    write(&mismatch, &bulk_manifest(SHA, &target_sha));
    let config_same = root.join("config-same.json");
    let config_new = root.join("config-new.json");
    let config_mismatch = root.join("config-mismatch.json");
    phase_config(&config_same, &same, "keep");
    phase_config(&config_new, &other, "match");
    phase_config(&config_mismatch, &mismatch, "mismatch");
    let out_same = root.join("out-same");
    let same_verdict = run_card(&phase_opts(&root, &out_same, &script, &built, &config_same, &same, &target)).expect("card");
    let same_measurement = fs::read_to_string(out_same.join("measurement.txt")).expect("measurement");
    assert_eq!(fs::read_to_string(out_same.join("phases/doctor.exit")).expect("doctor").trim(), "0");
    assert_eq!(fs::read_to_string(out_same.join("phases/doctor-mysql.exit")).expect("mysql").trim(), "7");
    assert_eq!(fs::read_to_string(out_same.join("phases/run.exit")).expect("run").trim(), "0");
    assert_eq!(fs::read_to_string(out_same.join("phases/close.exit")).expect("close").trim(), "0");
    assert!(same_measurement.contains("copy_performed=false"), "{same_measurement}");
    assert!(same_measurement.contains("precomplete_manifest=true"), "{same_measurement}");
    assert!(same_measurement.contains(&format!("precomplete_import_hash={fresh}")), "{same_measurement}");
    assert!(same_measurement.contains("target_was_preloaded=true"), "{same_measurement}");
    assert!(!same_verdict.passed);
    let out_mismatch = root.join("out-mismatch");
    let mismatch_verdict = run_card(&phase_opts(&root, &out_mismatch, &script, &built, &config_mismatch, &mismatch, &target)).expect("card");
    let mismatch_measurement = fs::read_to_string(out_mismatch.join("measurement.txt")).expect("measurement");
    assert!(mismatch_measurement.contains("copy_performed=false"), "{mismatch_measurement}");
    assert!(mismatch_measurement.contains(&format!("precomplete_import_hash={SHA}")), "{mismatch_measurement}");
    assert!(mismatch_measurement.contains(&format!("copy_phase_import={fresh}")), "{mismatch_measurement}");
    assert!(mismatch_measurement.contains("copy_phase_snapshot=0000000C-00000009-1"), "{mismatch_measurement}");
    assert!(mismatch_measurement.contains("manifest_snapshot=0000000D-00000001-1"), "{mismatch_measurement}");
    assert!(mismatch_measurement.contains("target_was_preloaded=true"), "{mismatch_measurement}");
    assert!(!mismatch_verdict.passed);
    let out_new = root.join("out-new");
    let new_verdict = run_card(&phase_opts(&root, &out_new, &script, &built, &config_new, &other, &target)).expect("card");
    let new_measurement = fs::read_to_string(out_new.join("measurement.txt")).expect("measurement");
    assert!(new_measurement.contains("copy_performed=true"), "{new_measurement}");
    assert!(new_measurement.contains(&format!("precomplete_import_hash={SHA}")), "{new_measurement}");
    assert!(new_measurement.contains(&format!("import_hash={fresh}")), "{new_measurement}");
    assert!(new_measurement.contains(&format!("copy_phase_import={fresh}")), "{new_measurement}");
    assert!(new_measurement.contains("copy_phase_snapshot=0000000C-00000009-1"), "{new_measurement}");
    assert!(new_measurement.contains("manifest_snapshot=0000000C-00000009-1"), "{new_measurement}");
    assert!(new_measurement.contains("target_was_preloaded=false"), "{new_measurement}");
    assert!(!new_verdict.passed);
    assert_ne!(new_verdict.exit_code, 0);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn fresh_campaign_config_is_secret_free_and_not_the_preserved_import() {
    let text = include_str!("../linux/a415/config/fresh-campaign.json");
    assert!(config_is_secret_free(text));
    assert!(!text.contains("49dbba"));
    assert!(!text.contains("90000"));
    assert!(!text.contains("1234567890"));
    assert!(text.contains("oo-a407-dst-pg"));
    assert!(text.contains("\"table\": \"batch_a\""));
    assert!(!text.contains("oo-a410-src"));
}

#[test]
fn campaign_evidence_status_matched_is_not_an_answer() {
    let mut obs = accepted();
    obs.questions[0].answer.clear();
    obs.questions[0].status = "matched".to_owned();
    obs.questions[0].matched = true;
    assert_eq!(status(&judge(&obs), "MP-04"), "fail");
    let mut obs = accepted();
    obs.questions[0].status = "released".to_owned();
    obs.questions[0].matched = false;
    assert_eq!(status(&judge(&obs), "MP-04"), "pass");
    let mut obs = accepted();
    obs.questions[1].audit_http = Some(500);
    assert_eq!(status(&judge(&obs), "MP-04"), "fail");
}

#[test]
fn campaign_evidence_id_overlap_needs_the_cited_result() {
    let mut obs = accepted();
    obs.questions[1].cells = vec![vec![Cell::Text("a".to_owned()), Cell::Text("b".to_owned())]];
    obs.questions[1].tool_sql = "select a.id, c.id from public.batch_a a join public.batch_c c on a.id = c.id".to_owned();
    assert_eq!(status(&judge(&obs), "MP-04"), "fail");
    let mut obs = accepted();
    obs.questions[1].cells = vec![vec![Cell::Text("2000".to_owned()), Cell::Text("ids".to_owned())]];
    obs.questions[1].tool_sql = "select count(*) as n, 'ids' from public.batch_a a join public.batch_c c on a.id = c.id".to_owned();
    assert_eq!(status(&judge(&obs), "MP-04"), "pass");
    let mut obs = accepted();
    obs.questions[1].cells = (0..2000).map(|index| vec![Cell::Text(format!("id{index}"))]).collect();
    obs.questions[1].tool_sql = "select a.id from public.batch_a a join public.batch_c c on a.id = c.id".to_owned();
    assert_eq!(status(&judge(&obs), "MP-04"), "pass");
    let mut obs = accepted();
    obs.questions[1].cells = vec![vec![Cell::Text("2000".to_owned()), Cell::Text("ids".to_owned())]];
    obs.questions[1].tool_sql = "select 2000".to_owned();
    assert_eq!(status(&judge(&obs), "MP-04"), "fail");
    let mut obs = accepted();
    obs.questions[1].query_id.clear();
    obs.questions[1].answer = "2000".to_owned();
    assert_eq!(status(&judge(&obs), "MP-04"), "fail");
}

#[test]
fn campaign_evidence_draft_ready_counts_and_a_fault_still_fails() {
    let mut obs = accepted();
    obs.generation_outcome.clear();
    obs.run_draft_ready = true;
    assert_eq!(status(&judge(&obs), "MP-02"), "pass");
    let mut obs = accepted();
    obs.generation_outcome.clear();
    obs.run_draft_ready = false;
    assert_eq!(status(&judge(&obs), "MP-02"), "fail");
    let mut obs = accepted();
    obs.write_reject.status = "draft_ready".to_owned();
    obs.write_reject.exit = Some(0);
    assert_eq!(status(&judge(&obs), "MP-05"), "fail");
}

fn capability_verdict() -> String {
    r#"{"protocol":"oo.acceptance.local-copy-verdict/v1","mp01_signed":false,"mp01abc_signed":true,"customer_complete":false,"consistency_claim":"single_snapshot","business_peak":1,"producer_source_socket_fact":"repeatable_read_read_only","runner_source_sockets":0,"cards":{"MP-01a":{"checks_pass":true},"MP-01b":{"checks_pass":true},"MP-01c":{"checks_pass":true},"MP-01d":{"checks_pass":false}},"typed":[{"name":"batch_a","source_count":2000,"target_count":2000,"hash_equal":true,"key_coverage":true},{"name":"batch_b","source_count":8000,"target_count":8000,"hash_equal":true,"key_coverage":true},{"name":"batch_c","source_count":90000,"target_count":90000,"hash_equal":true,"key_coverage":true}]}"#.to_owned()
}

fn capability_case() -> String {
    r#"{"protocol":"oo.acceptance.local-copy/v1","provenance":"live","customer_complete":false,"producer":{"binary_sha256":"fcec36dbf83c97fcecf2805cce1860746ad9b47b954f03b1e5d0bccf9cd24e76","binary_present":true,"invoked":true,"exit_code":0,"source_socket_peak":1,"source_socket_fact":"repeatable_read_read_only","fixed_single_connection":true}}"#.to_owned()
}

fn inventory_opts(root: &std::path::Path, out: &std::path::Path, inventory: &std::path::Path) -> CardOpts {
    CardOpts {
        repo: root.join("repo"),
        archive: root.join("archive"),
        out: out.to_path_buf(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: None,
        execute: false,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: Some(inventory.to_path_buf()),
        kit_root: None,
        sealed_manifest: None,
        import_manifest: None,
        copy_review: None,
        product_note: None,
        fixture_probe: None,
        binding_probe: None,
    }
}

fn phase_opts(
    root: &std::path::Path,
    out: &std::path::Path,
    binary: &std::path::Path,
    manifest: &std::path::Path,
    config: &std::path::Path,
    import_manifest: &std::path::Path,
    sealed: &std::path::Path,
) -> CardOpts {
    CardOpts {
        repo: root.join("repo"),
        archive: root.join("archive"),
        out: out.to_path_buf(),
        pinned_commit: COMMIT.to_owned(),
        binary: Some(binary.to_path_buf()),
        manifest: Some(manifest.to_path_buf()),
        config: Some(config.to_path_buf()),
        execute: true,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: None,
        kit_root: None,
        sealed_manifest: Some(sealed.to_path_buf()),
        import_manifest: Some(import_manifest.to_path_buf()),
        copy_review: None,
        product_note: None,
        fixture_probe: None,
        binding_probe: None,
    }
}

fn phase_config(path: &std::path::Path, manifest: &std::path::Path, rewrite: &str) {
    write(
        path,
        &format!(
            r#"{{"database":{{"type":"postgresql","host":"oo-a407-dst-pg","name":"oo_a407_dst","password_file":"configured-by-pipeline/target-reader.password"}},"import_manifest":"{}","rewrite":"{rewrite}"}}"#,
            manifest.display()
        ),
    );
}

fn phase_script(import_hash: &str) -> String {
    format!(
        r#"#!/bin/sh
set -u
cmd="${{1:-}}"
shift || true
cfg=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --config) cfg="$2"; shift 2 ;;
    *) shift ;;
  esac
done
case "$cmd" in
  doctor)
    case "$(cat "$cfg")" in
      *'"type":"mysql"'*) exit 7 ;;
    esac
    exit 0
    ;;
  run)
    mode=$(sed -n 's/.*"rewrite":"\([^"]*\)".*/\1/p' "$cfg")
    manifest=$(sed -n 's/.*"import_manifest":"\([^"]*\)".*/\1/p' "$cfg")
    snap=0000000C-00000009-1
    if [ -n "$manifest" ]; then
      case "$mode" in
        match)
          sed -e 's/"import_hash":"[0-9a-fA-F][0-9a-fA-F]*"/"import_hash":"{import_hash}"/' -e "s/\"snapshot_id\":\"[^\"]*\"/\"snapshot_id\":\"$snap\"/" "$manifest" > "$manifest.tmp"
          mv "$manifest.tmp" "$manifest"
          ;;
        mismatch)
          sed -e 's/"import_hash":"[0-9a-fA-F][0-9a-fA-F]*"/"import_hash":"{import_hash}"/' -e 's/"snapshot_id":"[^"]*"/"snapshot_id":"0000000D-00000001-1"/' "$manifest" > "$manifest.tmp"
          mv "$manifest.tmp" "$manifest"
          ;;
      esac
    fi
    printf '%s\n' '{{"phases":[{{"name":"discover","event":"begin","from":"source_connection"}},{{"name":"copy","event":"begin","from":"source_connection"}},{{"name":"copy","event":"complete","failure":"none","mirror_state":"complete","customer_complete":false,"import_hash":"{import_hash}","snapshot_id":"0000000C-00000009-1"}}]}}'
    exit 0
    ;;
  close)
    printf '%s\n' '{{"pid_absent":true,"database_dropped":false}}'
    exit 0
    ;;
esac
exit 0
"#
    )
}

fn bulk_manifest(import_hash: &str, target_sha: &str) -> String {
    let table = |name: &str, rows: i64, columns: &str| {
        format!(r#"{{"schema":"public","name":"{name}","physical_name":"pt_{name}","rows":{rows},"columns":[{columns}]}}"#)
    };
    format!(
        r#"{{"protocol":"oo.pipeline.local-pg-mirror-manifest/v1","mirror_state":"complete","customer_complete":false,"consistency":"single_snapshot","source_socket_peak":1,"source_socket_fact":"repeatable_read_read_only","source_write_tuples":0,"import_generation":1,"import_hash":"{import_hash}","snapshot_id":"0000000B-00000008-1","target_config_sha256":"{target_sha}","tables_completed":[{},{},{}]}}"#,
        table("batch_a", 2000, r#"{"name":"id","oid":20,"typmod":-1},{"name":"amount","oid":1700,"typmod":2490376}"#),
        table("batch_b", 8000, r#"{"name":"at_time","oid":1083,"typmod":-1},{"name":"blob","oid":17,"typmod":-1}"#),
        table("batch_c", 90000, r#"{"name":"qty","oid":20,"typmod":-1},{"name":"amount","oid":1700,"typmod":2490376}"#),
    )
}

fn executable(path: &std::path::Path, body: &str) {
    write(path, body);
    let mut permissions = fs::metadata(path).expect("meta").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("mode");
}

fn seal_opts(root: &std::path::Path, out: &std::path::Path, manifest: &std::path::Path) -> CardOpts {
    CardOpts {
        repo: root.join("repo"),
        archive: root.join("archive"),
        out: out.to_path_buf(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: None,
        execute: false,
        pi_commit: None,
        pi_executable: None,
        pi_runtime: None,
        inventory: None,
        kit_root: None,
        sealed_manifest: Some(root.join("target.json")),
        import_manifest: Some(manifest.to_path_buf()),
        copy_review: Some(root.join("review.json")),
        product_note: Some(root.join("kit.txt")),
        fixture_probe: None,
        binding_probe: Some(root.join("probe.txt")),
    }
}

fn pin_opts(
    repo: &std::path::Path,
    archive: &std::path::Path,
    out: &std::path::Path,
    config: &std::path::Path,
    executable: &std::path::Path,
    runtime: &std::path::Path,
    commit: &str,
) -> CardOpts {
    CardOpts {
        repo: repo.to_path_buf(),
        archive: archive.to_path_buf(),
        out: out.to_path_buf(),
        pinned_commit: COMMIT.to_owned(),
        binary: None,
        manifest: None,
        config: Some(config.to_path_buf()),
        execute: false,
        pi_commit: Some(commit.to_owned()),
        pi_executable: Some(executable.to_path_buf()),
        pi_runtime: Some(runtime.to_path_buf()),
        inventory: None,
        kit_root: None,
        sealed_manifest: None,
        copy_review: None,
        product_note: None,
        fixture_probe: None,
        import_manifest: None,
        binding_probe: None,
    }
}

fn status(verdict: &minimal_loop_acceptance::Verdict, id: &str) -> String {
    verdict.checks.iter().find(|item| item.id == id).expect(id).status.clone()
}

fn accepted() -> Observation {
    let mut obs = Observation::unmeasured();
    obs.measured = true;
    obs.head = COMMIT.to_owned();
    obs.pinned_commit = COMMIT.to_owned();
    obs.a397_committed = false;
    obs.agent_entry_runtime = false;
    obs.http_release_claimed = false;
    obs.copy_capability_reviewed = true;
    obs.sealed_manifest_present = true;
    obs.campaign_provenance_ok = true;
    obs.binding_rows_ok = true;
    obs.binding_types_ok = true;
    obs.binding_samples_ok = true;
    obs.binding_partial_denied = true;
    obs.binding_source_connects = Some(0);
    obs.import_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    obs.precomplete_manifest = false;
    obs.inventory_matches = true;
    obs.moving_source_attested = false;
    obs.binary_present = true;
    obs.binary_from_archive = true;
    obs.binary_sha_matches = true;
    obs.pi_pins_match = true;
    obs.development_ack = "local-process-not-a-sandbox-certificate".to_owned();
    obs.publication = "not_published".to_owned();
    obs.llm_review = "unreviewed".to_owned();
    obs.rows_party = Some(2000);
    obs.rows_account = Some(8000);
    obs.rows_entry = Some(90000);
    obs.server_version = "17.10".to_owned();
    obs.source_identity = "source-a".to_owned();
    obs.target_identity = "target-b".to_owned();
    obs.source_sha256 = SHA.to_owned();
    obs.pack_source_sha256 = SHA.to_owned();
    obs.pack_hash = "pack-hash".to_owned();
    obs.pack_hash_second = "pack-hash".to_owned();
    obs.profile_bound = true;
    obs.peak_connections = Some(1);
    obs.source_access_after_seal = Some(0);
    obs.typed_reconciliation = true;
    obs.copy_performed = true;
    obs.manifest_present = true;
    obs.support_plan_recorded = true;
    obs.config_redacted = true;
    obs.copy_from_connection_config = true;
    obs.target_was_preloaded = false;
    obs.source_writes = Some(0);
    obs.typed_null = true;
    obs.typed_int8 = true;
    obs.typed_decimal = true;
    obs.typed_time = Some(true);
    obs.typed_binary = Some(true);
    obs.typed_key_coverage = true;
    obs.canonical_hashes_match = true;
    obs.snapshot_mode = "repeatable_read_read_only".to_owned();
    obs.fault_cancel_clean = true;
    obs.fault_timeout_clean = true;
    obs.fault_drift_clean = true;
    obs.fault_replay_clean = true;
    obs.no_partial_visibility = true;
    obs.no_source_rewrite = true;
    obs.no_snapshot_splice = true;
    obs.no_duplicate_side_effect = true;
    obs.backend_stopped = true;
    obs.source_unreachable_after_seal = true;
    obs.source_secret_withheld = true;
    obs.local_chain_on_copy = true;
    obs.audit_generation_bound = true;
    obs.source_dropped = Some(false);
    obs.copy_dropped = Some(false);
    obs.mysql_rejected = Some(true);
    obs.model_id = "grok-4.7".to_owned();
    obs.cache_first = "miss".to_owned();
    obs.cache_second = "hit".to_owned();
    obs.tokens_consistent = true;
    obs.cost_status = "unknown".to_owned();
    obs.generation_outcome = "validated".to_owned();
    obs.fresh_request = true;
    obs.readme = readme();
    obs.files = pack_files();
    obs.questions = questions();
    obs.write_reject = fault("rejected");
    obs.bad_model = fault("failed");
    obs.timeout_cancel = fault("timeout");
    obs.close_pid_absent = Some(true);
    obs.database_dropped = Some(false);
    obs.mapping_revoked = Some(true);
    obs.residue = Some(false);
    obs.config_secret_free = Some(true);
    obs.doctor_ran = true;
    obs.run_ran = true;
    obs.ask_ran = true;
    obs.close_ran = true;
    obs.raw_exits_present = true;
    obs.repro_command = "minimal-loop-acceptance card --pinned-commit 0123456789abcdef0123456789abcdef01234567".to_owned();
    obs
}

fn fault(status: &str) -> FaultObs {
    FaultObs { ran: true, incomplete: false, exit: Some(1), http_status: Some(400), status: status.to_owned() }
}

fn readme() -> String {
    format!(
        "# DRAFT — unreviewed — not published\n\nsource_sha256: {SHA}\nshop.party rows: 2000\nshop.account rows: 8000\nshop.entry rows: 90000\nreview_state: unreviewed\npublication: not_published\n"
    )
}

fn pack_files() -> Vec<MeasuredFile> {
    ["README.md", "ontology.yaml", "party.yaml", "account.yaml", "entry.yaml"]
        .into_iter()
        .map(|name| {
            let body = if name == "README.md" {
                readme()
            } else {
                format!("dataset: {name}\nsource_sha256: {SHA}\nreview_state: unreviewed\npublication: not_published\n")
            };
            MeasuredFile { sha256: sha256_hex(body.as_bytes()), path: name.to_owned(), body }
        })
        .collect()
}

fn questions() -> Vec<QuestionObs> {
    let specs = [
        ("count-batch-c", "90000", vec![vec![Cell::Text("90000".to_owned())]]),
        ("id-overlap", "2000", vec![vec![Cell::Text("2000".to_owned())]]),
        ("amount-when-qty-above", "134997.0000", vec![vec![Cell::Text("134997.0000".to_owned())]]),
        ("null-amount", "1", vec![vec![Cell::Text("1".to_owned())]]),
        (
            "batch-a-amount",
            "1234567890123456789012345678901234.1234",
            vec![vec![Cell::Text("1234567890123456789012345678901234.1234".to_owned())]],
        ),
        (
            "batch-c-qty-bounds",
            "-9223372036854775808",
            vec![
                vec![Cell::Text("1".to_owned()), Cell::Text("9223372036854775807".to_owned())],
                vec![Cell::Text("2".to_owned()), Cell::Text("-9223372036854775808".to_owned())],
            ],
        ),
    ];
    specs
        .into_iter()
        .map(|(id, token, cells)| QuestionObs {
            id: id.to_owned(),
            prompt: minimal_loop_acceptance_prompt(id),
            status: "matched".to_owned(),
            matched: true,
            query_id: format!("q_{id}"),
            answer: format!("result {token} [q_{id}]"),
            cells,
            tools: vec![
                ToolObs { name: "read".to_owned(), args: "/knowledge/README.md".to_owned() },
                ToolObs { name: "bash".to_owned(), args: "oo query --json -f /tmp/q.sql".to_owned() },
            ],
            proxy_marker_mtls: true,
            proxy_presented_session: false,
            audit_query_id: format!("q_{id}"),
            audit_http: Some(200),
            audit_database: "target-b".to_owned(),
            tool_sql: format!("SELECT placeholder_{id}"),
        })
        .collect()
}

fn minimal_loop_acceptance_prompt(id: &str) -> String {
    match id {
        "count-batch-c" => "How many rows are in public.batch_c? Write the SQL yourself and run it with oo query.",
        "id-overlap" => "How many public.batch_a ids also appear in public.batch_c? There is no foreign key between these tables. Write the SQL yourself and run it with oo query.",
        "amount-when-qty-above" => "What is the exact sum of public.batch_c.amount for rows whose qty is greater than 2? Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
        "null-amount" => "How many public.batch_a rows have a null amount? Write the SQL yourself and run it with oo query.",
        "batch-a-amount" => "Return the amount of the public.batch_a row whose id is 2. Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
        "batch-c-qty-bounds" => "Return id and qty for the public.batch_c rows whose id is 1 or 2, ordered by id. Write the SQL yourself and run it with oo query.",
        _ => "",
    }
    .to_owned()
}

fn write(path: &std::path::Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("dir");
    }
    fs::write(path, body).expect("write");
}
