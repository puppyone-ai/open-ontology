#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use job_worker::api::{ADAPT_SHA, BOOT_SHA, CONTROL_SHA, EVENT_SHA, LEASE_SHA};
use job_worker::sha256::sha256_hex;

#[test]
fn adapt_and_migrations_match_the_frozen_hashes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let adapt = fs::read(root.join("adapt.sql")).expect("adapt");
    assert_eq!(sha256_hex(&adapt), ADAPT_SHA);
    let migrations = root.join("../../crates/oo-jobs/migrations");
    let files = [
        ("0000_bootstrap_roles.sql", BOOT_SHA),
        ("0001_jobs_control_expand.sql", CONTROL_SHA),
        ("0002_event_project_binding_expand.sql", EVENT_SHA),
        ("0003_job_lease_operations_expand.sql", LEASE_SHA),
    ];
    for (name, expected) in files {
        let bytes = fs::read(migrations.join(name)).expect(name);
        assert_eq!(sha256_hex(&bytes), expected, "{name}");
    }
    let contract = fs::read_to_string(root.join("CONTRACT.md")).expect("contract");
    for sha in [BOOT_SHA, CONTROL_SHA, EVENT_SHA, LEASE_SHA] {
        assert!(contract.contains(sha), "{sha}");
    }
}

#[test]
fn driver_calls_the_published_functions_and_does_not_rewrite_task_rows() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut combined = String::new();
    for entry in fs::read_dir(&root).expect("src") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            combined.push_str(&fs::read_to_string(&path).expect("read"));
            combined.push('\n');
        }
    }
    for required in [
        "jobs.enqueue",
        "jobs.claim",
        "jobs.heartbeat",
        "jobs.complete_success",
        "jobs.complete_failure",
        "jobs.retry_failure",
        "jobs.cancel",
        "pg_notify",
        "UPDATE jobs.schedules",
    ] {
        assert!(combined.contains(required), "missing {required}");
    }
    for forbidden in [
        "UPDATE jobs.tasks",
        "INSERT INTO jobs.tasks",
        "DELETE FROM jobs.tasks",
        "UPDATE jobs.task_events",
        "INSERT INTO jobs.task_events",
        "DELETE FROM jobs.task_events",
        "interval '30",
    ] {
        assert!(!combined.contains(forbidden), "found {forbidden}");
    }
}
