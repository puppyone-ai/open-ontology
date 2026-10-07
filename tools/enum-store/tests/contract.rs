#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use enum_store::{migration_sha256, CONTRACT};

#[test]
fn contract_name_and_migration_hash_are_fixed() {
    assert_eq!(CONTRACT, "enum-store-v1");
    let hash = migration_sha256().expect("migration hash");
    assert_eq!(hash, "b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd");
}

#[test]
fn sources_do_not_read_a_clock_or_repair_a_schema() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rust = ["src/lib.rs", "src/adopt.rs", "src/error.rs", "src/publish.rs", "src/role.rs", "src/store.rs", "src/tls.rs"]
        .iter()
        .map(|path| fs::read_to_string(root.join(path)).expect(path))
        .collect::<String>();
    for token in [
        "SystemTime",
        "Instant::now",
        "Utc::now",
        "Local::now",
        "chrono::Utc",
        "chrono::Local",
        "current_date",
        "current_timestamp",
        "clock_timestamp",
        "date_trunc",
        "current_setting",
    ] {
        assert!(!rust.contains(token), "{token}");
    }
    let migration = fs::read_to_string(root.join("migrations/oo_enum_control_v1.sql")).expect("migration");
    let lower = migration.to_ascii_lowercase();
    for token in [
        "now(",
        "current_date",
        "current_timestamp",
        "clock_timestamp",
        "date_trunc",
        "current_setting",
        "statement_timestamp",
        "transaction_timestamp",
        "timeofday",
        "if not exists",
        "create or replace",
        "drop ",
    ] {
        assert!(!lower.contains(token), "{token}");
    }
    assert!(!migration.contains("oo_r_"));
    assert!(lower.contains("security_invoker = false"));
    assert!(lower.contains("security_barrier = true"));
    assert!(lower.contains("oo_dict.enums"));
}
