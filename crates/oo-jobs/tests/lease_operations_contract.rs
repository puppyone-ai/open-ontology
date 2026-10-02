use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn sha256(path: &Path) -> String {
    let output = if Path::new("/usr/bin/sha256sum").exists() {
        Command::new("/usr/bin/sha256sum").arg(path).output()
    } else {
        Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
    }
    .unwrap_or_else(|error| panic!("hash {}: {error}", path.display()));
    assert!(
        output.status.success(),
        "hash {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .expect("hash")
        .to_string()
}

#[test]
fn lease_operations_files_match_the_published_contract() {
    let crate_root = root();
    let repo = crate_root
        .parent()
        .and_then(Path::parent)
        .expect("repo root");
    let contract_path = repo.join("contracts/jobs/lease-operations/jobs-lease-operations-v1.json");
    let contract = read(&contract_path);
    let migration = crate_root.join("migrations/0003_job_lease_operations_expand.sql");
    let sql = read(&migration);
    let library = read(&crate_root.join("src/lib.rs"));
    let published = [
        (
            crate_root.join("migrations/0000_bootstrap_roles.sql"),
            "445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52",
        ),
        (
            crate_root.join("migrations/0001_jobs_control_expand.sql"),
            "b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75",
        ),
        (
            crate_root.join("migrations/0002_event_project_binding_expand.sql"),
            "4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee",
        ),
    ];

    assert!(contract.contains("\"schema_version\": \"jobs-lease-operations/1\""));
    assert!(contract.contains("\"lease_seconds\": 30"));
    assert!(contract.contains("\"heartbeat_poll_seconds\": 10"));
    assert!(contract.contains("\"claim_cap_budget\": 8"));
    assert!(contract.contains("\"migration_lock_key\": 6401202601"));
    assert_eq!(
        sha256(&migration),
        "5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034"
    );
    assert!(contract.contains(&sha256(&migration)));
    for (path, digest) in published {
        assert_eq!(sha256(&path), digest);
        assert!(contract.contains(digest));
    }
    assert!(contract.contains(
        "d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca"
    ));

    for name in [
        "jobs.enqueue",
        "jobs.claim",
        "jobs.heartbeat",
        "jobs.complete_success",
        "jobs.complete_failure",
        "jobs.retry_failure",
        "jobs.cancel",
    ] {
        assert!(sql.contains(&format!("CREATE FUNCTION {name}(")), "{name}");
    }
    assert_eq!(
        sql.matches("LANGUAGE plpgsql\nSECURITY DEFINER\nSET search_path = pg_catalog, jobs")
            .count(),
        7
    );
    assert!(sql.contains("SET search_path = pg_catalog, jobs"));
    assert!(sql.contains("FOR v_step IN 1..8 LOOP"));
    assert!(sql.contains("interval '30 seconds'"));
    assert!(sql.contains("FOR UPDATE SKIP LOCKED"));
    assert!(sql.contains("lease_expired_attempt_cap"));
    assert!(sql.contains("attempt_exhausted"));
    assert!(sql.contains("REVOKE ALL ON jobs.tasks FROM oo_jobs_runtime"));
    assert!(sql.contains("REVOKE ALL ON jobs.task_events FROM oo_jobs_runtime"));
    assert!(sql.contains("GRANT SELECT ON jobs.tasks TO oo_jobs_runtime"));
    assert!(sql.contains("target_fence_certified"));
    assert!(sql.contains("ERRCODE = 'OO010'"));
    assert!(sql.contains("ERRCODE = 'OO011'"));
    assert!(sql.contains("ERRCODE = 'OO012'"));
    assert!(sql.contains("ERRCODE = 'OO013'"));
    assert!(sql.contains("ERRCODE = 'OO014'"));
    assert!(sql.contains("ERRCODE = 'OO015'"));
    assert!(sql.contains("ERRCODE = 'OO016'"));
    assert!(sql.contains("ERRCODE = 'OO017'"));
    assert!(!sql.contains("'already_applied'"));
    assert!(!sql.contains("\nCOMMIT"));
    assert!(!sql.contains("NOTIFY "));
    assert!(!sql.contains("EXECUTE format"));
    assert!(!sql.contains("ADD COLUMN"));
    assert!(sql.contains("clock_timestamp()"));
    assert!(!sql.contains("SECURITY DEFINER\nSET search_path = public"));
    assert!(library.contains("is not implemented here"));
    assert!(!library.contains("sqlx"));
    assert!(!library.contains("tokio_postgres"));
}
