#![forbid(unsafe_code)]

use std::process::Command;

#[test]
fn usage_is_silent() {
    let output = Command::new(env!("CARGO_BIN_EXE_control-identity")).output().expect("run");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}
