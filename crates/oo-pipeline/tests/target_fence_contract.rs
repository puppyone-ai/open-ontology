use std::path::{Path, PathBuf};
use std::process::Command;

const BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const EXPAND: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const CONTRACT: &str = include_str!(
    "../../../contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json"
);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn sha_after_filename<'a>(contract: &'a str, filename: &str) -> &'a str {
    let marker = format!("\"filename\": \"{filename}\"");
    let slice = contract
        .get(contract.find(&marker).unwrap_or_else(|| panic!("missing {filename}"))..)
        .expect("slice");
    let key = "\"sha256\": \"";
    let start = slice.find(key).expect("sha256") + key.len();
    let rest = &slice[start..];
    &rest[..rest.find('"').expect("sha end")]
}

#[test]
fn target_fence_files_match_the_published_contract() {
    let crate_root = root();
    let bootstrap_path = crate_root.join("migrations/0000_bootstrap_roles.sql");
    let expand_path = crate_root.join("migrations/0001_target_fence_expand.sql");
    let bootstrap_disk = std::fs::read_to_string(&bootstrap_path).unwrap();
    let expand_disk = std::fs::read_to_string(&expand_path).unwrap();
    let library = std::fs::read_to_string(crate_root.join("src/lib.rs")).unwrap();
    assert_eq!(BOOTSTRAP, bootstrap_disk);
    assert_eq!(EXPAND, expand_disk);

    assert_eq!(
        json_string(CONTRACT, "contract"),
        "oo.pipeline.target-fence-receipts/v1"
    );
    assert_eq!(
        json_string(CONTRACT, "schema_version"),
        "target-fence-receipts/1"
    );
    assert_eq!(json_string(CONTRACT, "phase"), "expand");
    assert!(CONTRACT.contains("\"migration_lock_key\": 6404202605"));
    assert!(!CONTRACT.contains("6401202601"));
    assert_eq!(sha256(&bootstrap_path), sha_after_filename(CONTRACT, "0000_bootstrap_roles.sql"));
    assert_eq!(sha256(&expand_path), sha_after_filename(CONTRACT, "0001_target_fence_expand.sql"));

    for token in [
        "target_generations",
        "apply_receipts",
        "clock_timestamp()",
        "config/v1",
        "type-policy/v1",
        "batch/v1",
        "target-fence-receipts/1",
        "6404202605",
        "does not fence arbitrary SQL",
    ] {
        assert!(EXPAND.contains(token), "missing {token}");
    }
    assert!(!EXPAND.contains("reject_event"));
    assert!(!EXPAND.to_ascii_lowercase().contains("unlogged"));
    assert!(!EXPAND.contains("6401202601"));
    assert!(!EXPAND.to_ascii_lowercase().contains("create extension"));
    assert!(!BOOTSTRAP.to_ascii_lowercase().contains("security definer"));
    assert!(!BOOTSTRAP.contains("GRANT oo_target_owner TO oo_pipeline_runtime"));
    assert!(BOOTSTRAP.contains("NOLOGIN"));
    assert!(BOOTSTRAP.contains("NOSUPERUSER"));
    assert!(BOOTSTRAP.contains("NOBYPASSRLS"));
    assert!(BOOTSTRAP.contains("NOCREATEROLE"));
    assert!(BOOTSTRAP.contains("NOCREATEDB"));
    assert!(!library.contains("sqlx"));
    assert!(!library.contains("tokio_postgres"));

    let mut lines = EXPAND.lines();
    while let Some(line) = lines.next() {
        if line.contains("SECURITY DEFINER") {
            let next = lines.next().unwrap_or("");
            assert!(
                next.contains("SET search_path"),
                "security definer without search_path near {line}"
            );
        }
    }
}
