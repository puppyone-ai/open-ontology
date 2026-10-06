#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use calendar_store::{migration_sha256, CONTRACT};

#[test]
fn contract_name_and_migration_hash_are_fixed() {
    assert_eq!(CONTRACT, "calendar-store-v1");
    let hash = migration_sha256().expect("migration hash");
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
}

#[test]
fn sources_do_not_read_a_clock_or_publish_private_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rust = [
        "src/lib.rs",
        "src/error.rs",
        "src/publish.rs",
        "src/role.rs",
        "src/store.rs",
        "src/tls.rs",
    ]
    .iter()
    .map(|path| fs::read_to_string(root.join(path)).expect(path))
    .collect::<String>();
    for token in ["SystemTime", "Instant::now", "Utc::now", "Local::now", "chrono::Utc", "chrono::Local"] {
        assert!(!rust.contains(token), "{token}");
    }
    let migration = fs::read_to_string(root.join("migrations/oo_calendar_control_v1.sql")).expect("migration");
    let lower = migration.to_ascii_lowercase();
    for token in [
        "now(",
        "current_date",
        "current_timestamp",
        "clock_timestamp",
        "date_trunc",
        "current_setting",
        "if not exists",
        "create or replace",
    ] {
        assert!(!lower.contains(token), "{token}");
    }
    assert!(!migration.contains("oo_r_"));
    assert!(lower.contains("security_invoker = false"));
    assert!(lower.contains("security_barrier = true"));
}
