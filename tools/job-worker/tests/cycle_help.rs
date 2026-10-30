use std::process::Command;

#[test]
fn cycle_policy_and_safe_cancellation_help_need_no_configuration_or_credentials() {
    for (command, expected) in [
        ("native-cycle-policy","FUTURE managed MySQL"),
        ("native-cancel","zero private CDC receipts"),
    ] {
        let output=Command::new(env!("CARGO_BIN_EXE_job-worker"))
            .args([command,"--help"]).env_clear().output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains(expected));
        assert!(output.stderr.is_empty());
    }
}
