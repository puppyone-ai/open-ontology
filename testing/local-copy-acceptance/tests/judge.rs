use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use local_copy_acceptance::{
    canonical_preimage, judge, parse_case, reconcile_table, run_producer, sha256_hex, verdict_json,
    Case, Cell, Class, Conn, Fault, FixtureClass, Provenance, Row, Table,
};

fn archive_text() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/archive-unfixed.json");
    fs::read_to_string(path).unwrap()
}

fn card_reasons(case: &Case, id: &str) -> Vec<&'static str> {
    judge(case)
        .cards
        .into_iter()
        .find(|card| card.id == id)
        .unwrap()
        .reasons
}

fn rich(i: i64) -> Row {
    let mut cells = BTreeMap::new();
    cells.insert("id".to_string(), Cell::Int8(i));
    let amount = if i == 0 { "10.50" } else { "0.00" };
    cells.insert(
        "amount".to_string(),
        Cell::Decimal {
            scale: 2,
            text: amount.to_string(),
        },
    );
    cells.insert("note".to_string(), Cell::Null);
    cells.insert(
        "at".to_string(),
        Cell::Time("2024-03-04 05:06:07".to_string()),
    );
    let blob = if i == 0 { "0001ff" } else { "00" };
    cells.insert("blob".to_string(), Cell::Binary(blob.to_string()));
    Row { cells }
}

fn conn(class: Class, pid: i64, datname: &str, isolation: &str, read_only: bool) -> Conn {
    Conn {
        class,
        usename: "oo_src".to_string(),
        datname: datname.to_string(),
        application_name: if class == Class::Monitor {
            "copy-acceptance-monitor".to_string()
        } else {
            "oo-pg-source".to_string()
        },
        backend_type: "client backend".to_string(),
        xact_isolation: isolation.to_string(),
        read_only,
        state: "active".to_string(),
        pid,
    }
}

fn fault(kind: &str) -> Fault {
    Fault {
        kind: kind.to_string(),
        mirror_state: "incomplete".to_string(),
        consistency: "not_a_snapshot".to_string(),
        public: false,
        source_backends_after: 0,
        target_rows_rewritten: false,
        mixed_snapshot: false,
    }
}

fn skeleton() -> Case {
    let row = rich(1);
    let table = Table {
        schema: "public".to_string(),
        name: "amounts".to_string(),
        key: vec!["id".to_string()],
        count: 1,
        source_rows: vec![row.clone()],
        target_rows: vec![row],
    };
    let text = archive_text();
    let mut case = parse_case(&text).unwrap();
    case.provenance = Provenance::Live;
    case.fixture_class = FixtureClass::Public;
    case.producer.binary_sha256 = "ab".repeat(32);
    case.producer.binary_present = true;
    case.producer.invoked = true;
    case.producer.fixed_single_connection = true;
    case.producer.source_socket_fact = local_copy_acceptance::FACT_SINGLE.to_string();
    case.producer.source_socket_peak = Some(1);
    case.producer.exit_code = Some(0);
    case.source.system_identifier = "src-sys-1".to_string();
    case.source.postmaster_start = "2026-10-08 00:00:00".to_string();
    case.source.database = "srcdb".to_string();
    case.target.system_identifier = "agent-sys-2".to_string();
    case.target.postmaster_start = "2026-10-08 00:00:01".to_string();
    case.target.database = "agentdb".to_string();
    case.snapshot.isolation = "repeatable read".to_string();
    case.snapshot.read_only = true;
    case.snapshot.catalog_snapshot_id = "snap-1".to_string();
    case.snapshot.data_snapshot_id = "snap-1".to_string();
    case.snapshot.proof = "sealed_spool".to_string();
    case.snapshot.later_count = false;
    case.snapshot.opened_second_source_connection = false;
    case.samples = vec![vec![
        conn(Class::Business, 7, "srcdb", "repeatable read", true),
        conn(Class::Monitor, 8, "postgres", "read committed", false),
    ]];
    case.tables = vec![table];
    case.faults = vec![
        fault("partial"),
        fault("cancel"),
        fault("snapshot_disconnect"),
        fault("restart"),
        fault("idempotency"),
        fault("mixed_version"),
    ];
    case.after_seal.public_fixture_source_blocked = true;
    case.after_seal.network_to_source = false;
    case.after_seal.enterprise_stop_requested = false;
    case.after_seal.source_secret_in_subsequent_process = false;
    case.after_seal.generation = "g1".to_string();
    case.after_seal.audits = vec![
        local_copy_acceptance::Audit {
            component: "405".to_string(),
            generation: "g1".to_string(),
            target_instance: "agent-sys-2".to_string(),
            source_access_count: 0,
            query_id: "q405".to_string(),
            database: "agentdb".to_string(),
        },
        local_copy_acceptance::Audit {
            component: "397".to_string(),
            generation: "g1".to_string(),
            target_instance: "agent-sys-2".to_string(),
            source_access_count: 0,
            query_id: "q397".to_string(),
            database: "agentdb".to_string(),
        },
    ];
    case
}

#[test]
fn canonical_vector_locks_the_preimage() {
    let mut cells = BTreeMap::new();
    cells.insert("id".to_string(), Cell::Int8(1));
    cells.insert("note".to_string(), Cell::Null);
    let text =
        canonical_preimage("public", "amounts", &["id".to_string()], &[Row { cells }]).unwrap();
    assert_eq!(
        text,
        "local-copy-acceptance/v1 table\nschema=public\nname=amounts\nkeys=id\ncount=1\n---\nid=int8:1\tnote=null:\n"
    );
    assert_eq!(
        sha256_hex(text.as_bytes()),
        "667f23b6789a9affd9ea6d54b404e2ee3fecbd6b61ac6b12cee8e0e84d8877fd"
    );
}

#[test]
fn archive_fixture_stays_unsigned() {
    let case = parse_case(&archive_text()).unwrap();
    let verdict = judge(&case);
    assert!(!verdict.mp01_signed);
    assert!(!verdict.mp01abc_signed);
    assert!(!verdict.customer_complete);
    assert!(!verdict.issued_stop);
    assert_eq!(verdict.runner_source_sockets, 0);
    assert_eq!(verdict.consistency_claim, "not_a_snapshot");
    assert_eq!(verdict.enterprise, "not_configured");
    assert_eq!(verdict.business_peak, 2);
    assert_eq!(verdict.business_peak_source, "producer_field");
    assert!(verdict.guard_recorded);
    assert!(!verdict.guard_hidden);
    assert_eq!(verdict.producer_source_socket_fact, "verified_reader_guard");
    let blockers = verdict.sign_blockers;
    for code in [
        "provenance_not_live",
        "producer_absent",
        "producer_not_invoked",
        "producer_unfixed",
        "guard_not_a_single_connection",
        "business_peak",
        "fingerprint_incomplete",
        "not_100k",
        "fault_missing",
        "audit_missing",
        "fixture_not_blocked",
        "source_network_open",
    ] {
        assert!(blockers.contains(&code), "{code} missing from {blockers:?}");
    }
    let text = verdict_json(&judge(&parse_case(&archive_text()).unwrap()));
    assert!(text.contains("\"customer_complete\": false"));
    assert!(text.contains("\"issued_stop\": false"));
    assert!(text.contains("\"runner_source_sockets\": 0"));
    assert!(text.contains("\"mp01_signed\": false"));
}

#[test]
fn toy_live_bundle_does_not_sign_mp01b() {
    let case = skeleton();
    let verdict = judge(&case);
    assert!(!verdict.mp01_signed);
    assert!(
        verdict
            .cards
            .iter()
            .find(|card| card.id == "MP-01a")
            .unwrap()
            .checks_pass
    );
    assert_eq!(card_reasons(&case, "MP-01b"), vec!["not_100k"]);
    assert!(
        verdict
            .cards
            .iter()
            .find(|card| card.id == "MP-01c")
            .unwrap()
            .checks_pass
    );
    assert!(
        verdict
            .cards
            .iter()
            .find(|card| card.id == "MP-01d")
            .unwrap()
            .checks_pass
    );
    assert_eq!(verdict.business_peak, 1);
    assert_eq!(verdict.monitor_peak, 1);
    assert_eq!(verdict.connections.len(), 2);
    assert!(verdict
        .connections
        .iter()
        .any(|conn| conn.class == "monitor"));
    assert!(!verdict.guard_recorded);
    let mut fixture = case.clone();
    fixture.provenance = Provenance::Fixture;
    assert!(judge(&fixture)
        .sign_blockers
        .contains(&"provenance_not_live"));
}

#[test]
fn monitor_is_not_a_business_socket_and_a_guard_is() {
    let mut hidden = skeleton();
    hidden.producer.fixed_single_connection = true;
    hidden.producer.source_socket_fact = "verified_reader_guard".to_string();
    hidden.producer.source_socket_peak = Some(2);
    let verdict = judge(&hidden);
    assert!(verdict.guard_hidden);
    assert!(card_reasons(&hidden, "MP-01b").contains(&"guard_hidden"));
    assert!(card_reasons(&hidden, "MP-01a").contains(&"producer_unfixed"));
    assert!(!verdict.mp01_signed);

    let mut shown = skeleton();
    shown.producer.source_socket_fact = "verified_reader_guard".to_string();
    shown.producer.source_socket_peak = Some(2);
    shown.samples[0].push(conn(Class::Guard, 9, "srcdb", "read committed", true));
    let verdict = judge(&shown);
    assert!(!verdict.guard_hidden);
    assert!(verdict.guard_recorded);
    assert_eq!(verdict.business_peak, 2);
    assert_eq!(verdict.monitor_peak, 1);
    let reasons = card_reasons(&shown, "MP-01b");
    assert!(reasons.contains(&"business_peak"));
    assert!(reasons.contains(&"guard_not_a_single_connection"));
    assert!(!reasons.contains(&"guard_hidden"));
}

#[test]
fn same_instance_second_source_later_count_and_budget_fail() {
    let mut case = skeleton();
    case.target.system_identifier = case.source.system_identifier.clone();
    assert!(card_reasons(&case, "MP-01a").contains(&"fingerprint_same_instance"));

    let mut case = skeleton();
    case.snapshot.opened_second_source_connection = true;
    assert!(card_reasons(&case, "MP-01b").contains(&"second_source_connection"));

    let mut case = skeleton();
    case.snapshot.later_count = true;
    assert!(card_reasons(&case, "MP-01b").contains(&"later_count"));

    let mut case = skeleton();
    case.budgets.batch_rows = 1001;
    case.budgets.statement_timeout_ms = 30001;
    case.budgets.lock_timeout_ms = 1001;
    case.budgets.snapshot_deadline_ms = 300001;
    let reasons = card_reasons(&case, "MP-01b");
    assert!(reasons.contains(&"budget_batch"));
    assert!(reasons.contains(&"budget_statement"));
    assert!(reasons.contains(&"budget_lock"));
    assert!(reasons.contains(&"budget_snapshot"));
}

#[test]
fn typed_mismatch_is_not_excused_by_equal_counts() {
    let mut case = skeleton();
    case.tables[0].target_rows[0].cells.insert(
        "amount".to_string(),
        Cell::Decimal {
            scale: 2,
            text: "10.51".to_string(),
        },
    );
    let report = reconcile_table(&case.tables[0]);
    assert_eq!(report.source_count, report.target_count);
    assert!(!report.hash_equal);
    assert!(report.reasons.contains(&"typed_mismatch"));

    case.tables[0].target_rows[0]
        .cells
        .insert("note".to_string(), Cell::Text(String::new()));
    let report = reconcile_table(&case.tables[0]);
    assert!(report.reasons.contains(&"typed_mismatch"));
}

#[test]
fn faults_and_enterprise_stop_stay_non_public() {
    let mut case = skeleton();
    case.faults[0].public = true;
    case.faults[1].mirror_state = "complete".to_string();
    case.faults[4].target_rows_rewritten = true;
    case.faults[5].mixed_snapshot = true;
    case.faults[2].source_backends_after = 1;
    let reasons = card_reasons(&case, "MP-01c");
    assert!(reasons.contains(&"fault_public"));
    assert!(reasons.contains(&"fault_complete"));
    assert!(reasons.contains(&"fault_rewritten"));
    assert!(reasons.contains(&"fault_snapshot_mixed"));
    assert!(reasons.contains(&"fault_backend"));

    let mut case = skeleton();
    case.fixture_class = FixtureClass::Enterprise;
    case.after_seal.enterprise_stop_requested = true;
    case.after_seal.public_fixture_source_blocked = false;
    let verdict = judge(&case);
    assert!(!verdict.issued_stop);
    let reasons = card_reasons(&case, "MP-01d");
    assert!(reasons.contains(&"enterprise_stop_refused"));
    assert!(reasons.contains(&"enterprise_not_a_public_fixture"));
    assert!(!verdict.mp01_signed);
}

#[test]
fn secret_duplicate_customer_and_bad_int8_are_malformed() {
    let mut text = archive_text();
    text = text.replacen("\"protocol\"", "\"password\": \"x\", \"protocol\"", 1);
    assert_eq!(parse_case(&text).unwrap_err().code, "secret_key");

    let text = archive_text().replacen(
        "\"protocol\"",
        "\"protocol\": \"oo.acceptance.local-copy/v1\", \"protocol\"",
        1,
    );
    assert_eq!(parse_case(&text).unwrap_err().code, "duplicate_key");

    let text = archive_text().replace(
        "\"customer_complete\": false",
        "\"customer_complete\": true",
    );
    assert_eq!(parse_case(&text).unwrap_err().code, "config");

    let text = archive_text().replace(
        "\"tables\": []",
        "\"tables\": [{\"schema\":\"public\",\"name\":\"amounts\",\"key\":[\"id\"],\"count\":1,\"source_rows\":[{\"id\":{\"t\":\"int8\",\"v\":\"9223372036854775808\"}}],\"target_rows\":[{\"id\":{\"t\":\"int8\",\"v\":\"1\"}}],\"source_spool\":\"\",\"target_catalog\":\"\"}]",
    );
    assert_eq!(parse_case(&text).unwrap_err().code, "typed_cell");

    let text = archive_text().replace(
        "\"tables\": []",
        "\"tables\": [{\"schema\":\"public\",\"name\":\"amounts\",\"key\":[\"id\"],\"count\":1,\"source_rows\":[{\"id\":{\"t\":\"int8\",\"v\":\"9223372036854775807\"},\"blob\":{\"t\":\"binary\",\"v\":\"0001FF\"}}],\"target_rows\":[{\"id\":{\"t\":\"int8\",\"v\":\"9223372036854775807\"},\"blob\":{\"t\":\"binary\",\"v\":\"0001ff\"}}],\"source_spool\":\"\",\"target_catalog\":\"\"}]",
    );
    assert_eq!(parse_case(&text).unwrap_err().code, "typed_cell");
}

#[test]
fn spool_files_are_the_source_proof_and_a_symlink_is_refused() {
    let dir = std::env::temp_dir().join(format!("oo-a409-spool-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.jsonl");
    let target = dir.join("target.jsonl");
    let line = "{\"id\":{\"t\":\"int8\",\"v\":\"7\"},\"note\":{\"t\":\"null\"}}\n";
    fs::write(&source, line).unwrap();
    fs::write(&target, line).unwrap();
    let text = archive_text().replace(
        "\"tables\": []",
        &format!(
            "\"tables\": [{{\"schema\":\"public\",\"name\":\"amounts\",\"key\":[\"id\"],\"count\":1,\"source_rows\":[],\"target_rows\":[],\"source_spool\":\"{}\",\"target_catalog\":\"{}\"}}]",
            source.display(),
            target.display()
        ),
    );
    let case = parse_case(&text).unwrap();
    let report = reconcile_table(&case.tables[0]);
    assert!(report.hash_equal);
    assert!(report.key_coverage);
    let link = dir.join("link.jsonl");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    let text = archive_text().replace(
        "\"tables\": []",
        &format!(
            "\"tables\": [{{\"schema\":\"public\",\"name\":\"amounts\",\"key\":[\"id\"],\"count\":1,\"source_rows\":[],\"target_rows\":[],\"source_spool\":\"{}\",\"target_catalog\":\"{}\"}}]",
            link.display(),
            target.display()
        ),
    );
    assert_eq!(parse_case(&text).unwrap_err().code, "config");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unfixed_and_unpinned_producer_is_not_executed() {
    let dir = std::env::temp_dir().join(format!("oo-a409-run-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("marker");
    let script = dir.join("producer.sh");
    fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let archive = parse_case(&archive_text()).unwrap();
    let report = run_producer(&script, &archive).unwrap();
    assert!(!report.executed);
    assert_eq!(report.reason, "producer_unfixed");
    assert!(!marker.exists());

    let case = skeleton();
    let report = run_producer(&script, &case).unwrap();
    assert!(!report.executed);
    assert_eq!(report.reason, "producer_pin_mismatch");
    assert!(!marker.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn decision_fixture_can_sign_only_when_every_measured_input_holds() {
    let mut case = skeleton();
    case.tables = vec![bulk("small", 2000), bulk("mid", 8000), bulk("big", 90000)];
    let verdict = judge(&case);
    assert!(
        verdict.sign_blockers.is_empty(),
        "{:?}",
        verdict.sign_blockers
    );
    assert!(verdict.mp01_signed);
    assert!(verdict.mp01abc_signed);
    assert!(!verdict.customer_complete);
    assert_eq!(verdict.consistency_claim, "single_snapshot");
    assert_eq!(verdict.runner_source_sockets, 0);
    assert!(!verdict.issued_stop);
    assert_eq!(verdict.business_peak, 1);
    assert_eq!(verdict.monitor_peak, 1);
    assert!(verdict
        .typed
        .iter()
        .all(|report| report.hash_equal && report.key_coverage));
    assert_eq!(
        verdict
            .typed
            .iter()
            .map(|report| report.source_count)
            .sum::<u64>(),
        100_000
    );
}

fn bulk(name: &str, count: u64) -> Table {
    let rows: Vec<Row> = (0..count).map(|index| rich(index as i64)).collect();
    Table {
        schema: "public".to_string(),
        name: name.to_string(),
        key: vec!["id".to_string()],
        count,
        target_rows: rows.clone(),
        source_rows: rows,
    }
}

#[test]
fn cli_judge_writes_an_unsigned_archive_verdict() {
    let dir = std::env::temp_dir().join(format!("oo-a409-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let verdict = dir.join("verdict.json");
    let bin = env!("CARGO_BIN_EXE_local-copy-acceptance");
    let case = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/archive-unfixed.json");
    let output = Command::new(bin)
        .args(["judge", case.to_str().unwrap(), verdict.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("withheld "));
    let body = fs::read_to_string(&verdict).unwrap();
    assert!(body.contains("\"mp01_signed\": false"));
    let run_out = dir.join("run.json");
    let output = Command::new(bin)
        .args([
            "run",
            "/no/such/local-pg-mirror",
            case.to_str().unwrap(),
            run_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let body = fs::read_to_string(&run_out).unwrap();
    assert!(body.contains("\"executed\": false"));
    assert!(body.contains("\"reason\": \"producer_unfixed\""));
    assert!(body.contains("\"mp01_signed\": false"));
    let _ = fs::remove_dir_all(&dir);
}
