use std::process::Command;

#[test]
fn ab_help_and_invalid_arms() {
    let help = Command::new(env!("CARGO_BIN_EXE_oo-eval"))
        .args(["ab", "--help"])
        .output()
        .expect("run CLI");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--dataset"));
    let invalid = Command::new(env!("CARGO_BIN_EXE_oo-eval"))
        .args(["ab", "--arms", "c0,c4", "--dataset", "demo"])
        .output()
        .expect("run CLI");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    let missing = Command::new(env!("CARGO_BIN_EXE_oo-eval"))
        .args([
            "ab",
            "--arms",
            "c0,c3",
            "--dataset",
            "bird-mini-dev",
            "--output",
            "evidence/must-not-be-created",
        ])
        .output()
        .expect("run CLI");
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .contains("bird_assets_not_supplied_use_local_manifest")
    );
}

#[test]
fn scorer_health_and_p2_t_replay() {
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../../benchmarks/ab");
    let status = Command::new("python3")
        .args([
            "-m",
            "unittest",
            "-v",
            "test_ab",
            "test_live",
            "test_logistics",
        ])
        .current_dir(directory)
        .status()
        .expect("run evaluation tests");
    assert!(status.success());
}
