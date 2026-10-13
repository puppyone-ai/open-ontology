use clickhouse_preview::decision_for;

const DIGEST: &str = "8b88485c0940e298a8edad500731f0a7688c61a135a943b217f9597e191c7f93";

#[test]
fn observed_grants_and_profile_are_admitted() {
    let decision = decision_for(DIGEST);
    assert!(decision.admitted);
    assert!(decision.blocks.is_empty());
    assert!(!decision.matrix_mutated);
    assert!(!decision.zero_write.proven);
    assert_eq!(decision.zero_write.basis, "not_audited");
}

#[test]
fn cli_writes_the_admission_and_exits_0() {
    let out = std::env::temp_dir().join(format!("oo-dl01w1-40-admit-{}.json", std::process::id()));
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_clickhouse-preview"))
        .args(["--driver-digest", DIGEST, "--out", out.to_str().expect("utf8")])
        .status()
        .expect("cli");
    assert_eq!(status.code(), Some(0));
    let text = std::fs::read_to_string(&out).expect("decision");
    let _ = std::fs::remove_file(&out);
    assert!(text.contains("\"admitted\": true"));
    assert!(!text.contains("PERMISSION_UNKNOWN"));
    assert!(!text.contains("COMPAT_MODE_UNKNOWN"));
    assert!(text.contains("secret://oo-dl01w1-40/reader"));
    assert!(!text.contains("X-ClickHouse-Key"));
}
