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

fn json_string<'a>(text: &'a str, key: &str) -> &'a str {
    let needle = format!("\"{key}\": \"");
    let start = text
        .find(&needle)
        .unwrap_or_else(|| panic!("missing {key}"))
        + needle.len();
    let rest = &text[start..];
    let end = rest.find('"').expect("string end");
    &rest[..end]
}

#[test]
fn control_schema_files_match_the_published_contract() {
    let crate_root = root();
    let repo = crate_root
        .parent()
        .and_then(Path::parent)
        .expect("repo root");
    let contract = read(&repo.join("contracts/jobs/control-schema/jobs-control-v1.json"));
    let bootstrap = crate_root.join("migrations/0000_bootstrap_roles.sql");
    let expand = crate_root.join("migrations/0001_jobs_control_expand.sql");
    let bootstrap_sql = read(&bootstrap);
    let expand_sql = read(&expand);
    let library = read(&crate_root.join("src/lib.rs"));

    assert_eq!(json_string(&contract, "schema_version"), "jobs-control/1");
    assert_eq!(json_string(&contract, "phase"), "expand");
    assert!(contract.contains("\"migration_lock_key\": 6401202601"));
    assert_eq!(
        sha256(&bootstrap),
        "445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52"
    );
    assert_eq!(
        sha256(&expand),
        "b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75"
    );
    assert!(contract.contains(&sha256(&bootstrap)));
    assert!(contract.contains(&sha256(&expand)));

    for column in [
        "id",
        "project_id",
        "kind",
        "payload_version",
        "payload",
        "config_version",
        "artifact_digest",
        "dedupe_key",
        "state",
        "run_at",
        "attempt",
        "max_attempts",
        "lease_owner",
        "lease_until",
        "generation",
        "last_error",
        "created_at",
        "updated_at",
    ] {
        assert!(
            expand_sql.contains(&format!("{column} ")),
            "missing column {column}"
        );
    }

    let expand_lower = expand_sql.to_ascii_lowercase();
    let bootstrap_lower = bootstrap_sql.to_ascii_lowercase();
    assert!(!expand_lower.contains("security definer"));
    assert!(!bootstrap_lower.contains("security definer"));
    assert!(!expand_lower.contains("create extension"));
    assert!(!bootstrap_lower.contains("create extension"));
    assert!(expand_sql.contains("SECURITY INVOKER"));
    assert!(expand_sql.contains("target_fence_certified"));
    assert!(expand_sql.contains("jobs-control/1"));
    assert!(expand_sql.contains("security_invoker = true"));
    assert!(bootstrap_sql.contains("NOLOGIN"));
    assert!(bootstrap_sql.contains("NOSUPERUSER"));
    assert!(bootstrap_sql.contains("NOBYPASSRLS"));
    assert!(bootstrap_sql.contains("NOCREATEROLE"));
    assert!(bootstrap_sql.contains("NOCREATEDB"));
    assert!(!bootstrap_sql.contains("GRANT oo_jobs_owner TO oo_jobs_runtime"));
    assert!(!library.contains("sqlx"));
    assert!(!library.contains("tokio_postgres"));
}
