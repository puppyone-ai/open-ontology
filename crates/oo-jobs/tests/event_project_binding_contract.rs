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
fn event_project_files_match_the_published_contract() {
    let crate_root = root();
    let repo = crate_root
        .parent()
        .and_then(Path::parent)
        .expect("repo root");
    let v1_path = repo.join("contracts/jobs/control-schema/jobs-control-v1.json");
    let contract_path = repo.join("contracts/jobs/event-project-binding/jobs-event-project-v1.json");
    let bootstrap = crate_root.join("migrations/0000_bootstrap_roles.sql");
    let expand = crate_root.join("migrations/0001_jobs_control_expand.sql");
    let binding = crate_root.join("migrations/0002_event_project_binding_expand.sql");
    let v1 = read(&v1_path);
    let contract = read(&contract_path);
    let binding_sql = read(&binding);
    let binding_lower = binding_sql.to_ascii_lowercase();

    let bootstrap_sha = sha256(&bootstrap);
    let expand_sha = sha256(&expand);
    let binding_sha = sha256(&binding);
    let v1_sha = sha256(&v1_path);

    assert_eq!(
        bootstrap_sha,
        "445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52"
    );
    assert_eq!(
        expand_sha,
        "b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75"
    );
    assert_eq!(
        v1_sha,
        "d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca"
    );
    assert_eq!(
        binding_sha,
        "4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee"
    );
    assert!(v1.contains(&bootstrap_sha));
    assert!(v1.contains(&expand_sha));
    assert!(!v1.contains("0002_event_project_binding_expand.sql"));
    assert!(contract.contains(&bootstrap_sha));
    assert!(contract.contains(&expand_sha));
    assert!(contract.contains(&binding_sha));
    assert!(contract.contains(&v1_sha));
    assert!(contract.contains("\"schema_version\": \"jobs-event-project/1\""));
    assert!(contract.contains("\"migration_lock_key\": 6401202601"));
    assert!(contract.contains("\"preexisting_mismatch_refused\": \"OO004\""));
    assert!(contract.contains("\"cross_project_event\": \"23503\""));

    let unique_at = binding_sql
        .find("ADD CONSTRAINT tasks_id_project_unique")
        .expect("unique constraint");
    let foreign_at = binding_sql
        .find("ADD CONSTRAINT task_events_task_project_fkey")
        .expect("foreign key");
    let refusal_at = binding_sql
        .find("refusing event project binding:")
        .expect("refusal");
    assert!(refusal_at < unique_at);
    assert!(unique_at < foreign_at);
    assert!(binding_sql.contains("UNIQUE (id, project_id)"));
    assert!(binding_sql.contains("FOREIGN KEY (task_id, project_id)"));
    assert!(binding_sql.contains("REFERENCES jobs.tasks (id, project_id)"));
    assert!(binding_sql.contains("ON DELETE RESTRICT"));
    assert!(binding_sql.contains("ON UPDATE RESTRICT"));
    assert!(binding_sql.contains("NOT DEFERRABLE"));
    assert!(binding_sql.contains("jobs-event-project/1"));
    assert!(binding_sql.contains("ERRCODE = 'OO004'"));
    assert!(binding_sql.contains(
        "GRANT SELECT ON jobs.event_project_readiness TO oo_jobs_runtime"
    ));
    assert!(!binding_lower.contains("security definer"));
    assert!(!binding_lower.contains("create extension"));
    assert!(!binding_lower.contains("delete from"));
    assert!(!binding_lower.contains("update jobs."));
    assert!(!binding_lower.contains("insert into jobs.task_events"));
    assert!(!binding_lower.contains("insert into jobs.tasks"));
    assert!(!binding_lower.contains("insert into jobs.schema_migrations"));
    assert!(!binding_lower.contains("drop trigger"));
    assert!(!binding_lower.contains("drop function"));
    assert!(!binding_lower.contains("grant insert"));
    assert!(!binding_lower.contains("grant update"));
    assert!(!binding_lower.contains("grant delete"));
    assert!(!binding_lower.contains("grant oo_jobs_owner"));
}
