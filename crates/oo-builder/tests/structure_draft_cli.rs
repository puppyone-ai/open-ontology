//! The structure-draft binary reads one envelope and writes canonical bytes.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oo-builder"))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/ontology-draft")
}

#[test]
fn skeleton_and_unknown_commands_exit_2() {
    let bare = bin().output().unwrap();
    assert_eq!(bare.status.code(), Some(2));
    let stderr = String::from_utf8(bare.stderr).unwrap();
    assert!(stderr.contains("process entry is not implemented (SP00-W01 skeleton)"));

    let unknown = bin().arg("publish").output().unwrap();
    assert_eq!(unknown.status.code(), Some(2));

    let extra = bin()
        .args(["structure-draft", "--publish"])
        .output()
        .unwrap();
    assert_eq!(extra.status.code(), Some(2));
}

#[test]
fn structure_draft_writes_the_customer_golden_and_rejects_a_duplicate_key() {
    let input = fs::File::open(root().join("goldens/customer.input.json")).unwrap();
    let expected = fs::read(root().join("goldens/customer.draft.json")).unwrap();
    let compiled = bin()
        .arg("structure-draft")
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(
        compiled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert_eq!(compiled.stdout, expected);
    assert!(!compiled.stdout.ends_with(b"\n"));

    let mut rejected = bin()
        .arg("structure-draft")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    rejected
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"protocol":"oo.builder.catalog-evidence/v1","protocol":"again"}"#)
        .unwrap();
    let rejected = rejected.wait_with_output().unwrap();
    assert_eq!(rejected.status.code(), Some(1));
    let stderr = String::from_utf8(rejected.stderr).unwrap();
    assert!(stderr.contains("/json.duplicate_key"));
    assert!(rejected.stdout.is_empty());
}

fn contract_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/builder/structure-draft/fixtures")
        .join(name)
}

#[test]
fn repeated_key_member_exits_nonzero_without_echoing_the_name() {
    let input = fs::File::open(contract_fixture("duplicate-pk-member.json")).unwrap();
    let compiled = bin()
        .arg("structure-draft")
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(
        compiled.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert!(compiled.stdout.is_empty());
    let stderr = String::from_utf8(compiled.stderr).unwrap();
    assert!(stderr.contains("/duplicate.key_member"));
    assert!(stderr.contains("repeated key member"));
    assert!(!stderr.contains("leak_pk_9f3a"));
    assert!(!stderr.contains("alias_pk_9f3a"));
    assert!(!stderr.contains("DO_NOT_SURFACE_INSTRUCTION_9f3a"));
}

#[test]
fn rejected_fk_endpoint_closes_the_relationship_on_the_cli() {
    let input = fs::File::open(contract_fixture("fk-from-endpoint-rejected.json")).unwrap();
    let compiled = bin()
        .arg("structure-draft")
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(
        compiled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert!(!compiled.stdout.ends_with(b"\n"));
    assert!(compiled.stderr.is_empty());
    let stdout = String::from_utf8(compiled.stdout).unwrap();
    assert!(stdout.contains("relationship_endpoint_not_effective"));
    assert!(stdout.contains("from_column_not_effective"));
    assert!(stdout.contains("ship_to_fkey"));
    assert!(!stdout.contains("leak_pk_9f3a"));
    assert!(!stdout.contains("USING (tenant"));
}
