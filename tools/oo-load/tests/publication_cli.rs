use serde_json::Value;
use std::process::Command;

#[test]
fn publication_help_discloses_pending_admission() {
    let output = Command::new(env!("CARGO_BIN_EXE_oo-load"))
        .args(["publish", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("publish --generation ID --library ID"));
    assert!(help.contains("pending is NOT a Gateway-ready"));
}

#[test]
fn publication_flags_cannot_be_used_as_load_or_sync() {
    for (args, error) in [
        (
            vec!["--generation", "first"],
            "publication_subcommand_required",
        ),
        (
            vec!["--library", "owned"],
            "publication_subcommand_required",
        ),
        (
            vec!["--outcome-run", "current"],
            "publication_subcommand_required",
        ),
        (
            vec!["prune", "--outcome-run", "current"],
            "publication_arguments",
        ),
        (vec!["publish", "--c5", "unused"], "publication_arguments"),
        (vec!["publish", "--sync", "unused"], "publication_arguments"),
        (
            vec!["publish", "--run-id", "first"],
            "publication_arguments",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_oo-load"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let result: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(result["error"], error);
    }
}
