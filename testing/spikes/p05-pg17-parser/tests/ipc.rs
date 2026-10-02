use std::path::PathBuf;

use oo_p05_pg17_parser::{exercise_worker, KIND_CRASH, KIND_FAKE_ALLOW, KIND_GARBAGE, KIND_SPIN};

#[derive(serde::Deserialize)]
struct Corpus {
    ipc: Vec<IpcCase>,
}

#[derive(serde::Deserialize)]
struct IpcCase {
    id: String,
    code: String,
}

#[test]
fn ipc_failures_never_allow() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus: Corpus = serde_json::from_str(&std::fs::read_to_string(root.join("corpus/expected.json")).unwrap()).unwrap();
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-parser-worker"));
    let expect = |id: &str| -> String { corpus.ipc.iter().find(|item| item.id == id).unwrap().code.clone() };

    let spin = exercise_worker(&worker, KIND_SPIN, b"{}");
    assert_eq!(spin.code, expect("spin"));
    assert!(!spin.syntax_allow);
    assert!(spin.executed_sql.is_empty());
    assert!(spin.reaped, "spin not reaped");
    assert!(spin.workdir_removed, "spin workdir");

    let crash = exercise_worker(&worker, KIND_CRASH, b"{}");
    assert_eq!(crash.code, expect("crash"));
    assert!(!crash.syntax_allow);
    assert!(crash.reaped);
    assert!(crash.workdir_removed);

    let garbage = exercise_worker(&worker, KIND_GARBAGE, b"{}");
    assert_eq!(garbage.code, expect("garbage"));
    assert!(!garbage.syntax_allow);
    assert!(garbage.reaped);
    assert!(garbage.workdir_removed);

    let version = exercise_worker(&worker, KIND_FAKE_ALLOW, br#"{"mode":"version"}"#);
    assert_eq!(version.code, expect("fake-version"));
    assert!(!version.syntax_allow);
    assert!(version.executed_sql.is_empty());
    assert!(version.reaped);

    let oid = exercise_worker(&worker, KIND_FAKE_ALLOW, br#"{"mode":"oid"}"#);
    assert_eq!(oid.code, expect("fake-oid"));
    assert!(!oid.syntax_allow);
    assert!(!oid.oid_acl_rls_proven);
    assert!(oid.reaped);
    assert!(oid.workdir_removed);
}
