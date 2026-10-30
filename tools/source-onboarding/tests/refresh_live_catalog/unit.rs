use super::*;

fn fixture() -> (Value, Value) {
    let t = json!({"schema":"app","table":"items","run_id":"00000000-0000-4000-8000-000000000001",
        "physical_name":"pt_example","source_generation":1,"target_generation":2,
        "accepted_rows":3,"canonical_digest":"c".repeat(64),"applied_pos":"0"});
    let r = json!({"protocol":"oo.pipeline.native-publication-receipt/v1","source_id":"source",
        "run_id":t["run_id"],"run_ids":[t["run_id"]],"consistency":"per_table","tables":[t]});
    let manifest = json!({"protocol":"oo.library.membership/v1","library_id":"library",
        "sources":[{"source_id":"source","schema":"app","tables":["items"]}]});
    let receipt = json!({"library":{"protocol":"oo.pipeline.native-library-publication/v1",
        "library_id":"library","manifest_sha256":sha(&bytes(&manifest).unwrap()),"manifest":manifest,
        "shards":["source"],"expected_tables":1},
        "shards":[{"source_id":"source","anchor_run":t["run_id"],
            "receipt_sha256":sha(&bytes(&r).unwrap()),"receipt":r}]});
    let p = json!({"protocol":"oo.pipeline.native-library-publication/v1",
        "receipt_sha256":sha(&bytes(&receipt).unwrap()),"receipt":receipt,"run_id":t["run_id"]});
    let m = json!({"head":{"library_id":"library","state":"ready","receipt_sha256":p["receipt_sha256"],
        "manifest_sha256":receipt["library"]["manifest_sha256"]},"receipt":receipt,
        "rows":[{"source":"source","table":t,"apply_match":true,"fence_match":true,
            "run":{"run_id":t["run_id"],"source_id":"source",
            "source_generation":1,"target_generation":2,"config_digest":"a".repeat(64),
            "type_digest":"b".repeat(64),"input_digest":"d".repeat(64),"schema_digest":"e".repeat(64),
            "sealed":true,"state":"published"},"import_table":{"run_id":t["run_id"],
            "schema_ident":"app","table_ident":"items","physical_name":"pt_example","exhausted":true,
            "applied_rows":3,"applied_pos":"0","canonical_digest":"c".repeat(64)}}]});
    (p, m)
}

#[test]
fn exact_immutable_table_supplies_all_six_fields() {
    let (p, m) = fixture();
    assert_eq!(
        membership(&p).unwrap(),
        BTreeSet::from(["app.items".into()])
    );
    assert_eq!(
        bindings(&p, &m).unwrap(),
        json!([{"dataset":"app.items","source":"source",
        "generation":1,"config_digest":"a".repeat(64),"type_digest":"b".repeat(64),
        "input_digest":"d".repeat(64)}])
    );
    // Input digests describe data, not schema; a new legitimate cut can change them.
    let mut next = m.clone();
    next["rows"][0]["run"]["input_digest"] = json!("f".repeat(64));
    assert_eq!(
        bindings(&p, &next).unwrap()[0]["input_digest"],
        "f".repeat(64)
    );
}

#[test]
fn refuses_wrong_head_missing_fields_and_wrong_run_table() {
    let (p, m) = fixture();
    for pointer in [
        "/head/state",
        "/head/receipt_sha256",
        "/head/manifest_sha256",
        "/receipt/library/manifest_sha256",
        "/rows/0/run/type_digest",
        "/rows/0/run/input_digest",
        "/rows/0/run/config_digest",
        "/rows/0/run/source_id",
        "/rows/0/run/run_id",
        "/rows/0/run/source_generation",
        "/rows/0/run/target_generation",
        "/rows/0/run/sealed",
        "/rows/0/run/state",
        "/rows/0/import_table/exhausted",
        "/rows/0/import_table/canonical_digest",
        "/rows/0/import_table/physical_name",
        "/rows/0/import_table/applied_pos",
        "/rows/0/import_table/applied_rows",
        "/rows/0/import_table/run_id",
        "/rows/0/apply_match",
        "/rows/0/fence_match",
    ] {
        let mut bad = m.clone();
        *bad.pointer_mut(pointer).unwrap() = Value::Null;
        assert!(bindings(&p, &bad).is_err(), "{pointer}");
    }
    let mut bad = m.clone();
    bad["rows"]
        .as_array_mut()
        .unwrap()
        .push(m["rows"][0].clone());
    assert!(bindings(&p, &bad).is_err());
    bad["rows"] = json!([]);
    assert!(bindings(&p, &bad).is_err());
}

#[test]
fn raw_receipt_hash_and_exact_manifest_are_not_reconstructed() {
    let (p, _) = fixture();
    for pointer in [
        "/receipt/library/expected_tables",
        "/receipt/library/manifest/sources/0/tables/0",
        "/receipt/shards/0/receipt/tables/0/source_generation",
        "/receipt/shards/0/anchor_run",
    ] {
        let mut bad = p.clone();
        *bad.pointer_mut(pointer).unwrap() = json!("truncated");
        assert!(membership(&bad).is_err(), "{pointer}");
        bad["receipt_sha256"] = json!(sha(&bytes(&bad["receipt"]).unwrap()));
        assert!(membership(&bad).is_err(), "{pointer}");
    }
}

#[test]
fn psql_argv_never_accepts_commands_shells_or_credentials() {
    assert!(
        metadata_argv(
            &json!({"metadata_argv":["/bin/psql","-h","127.0.0.1","-qAt","-v","ON_ERROR_STOP=1"]})
        )
        .is_ok()
    );
    for a in [
        json!(["/bin/sh", "-c", "psql"]),
        json!(["psql"]),
        json!(["/bin/psql", "-c", "DELETE FROM t"]),
        json!(["/bin/psql", "-f", "arbitrary.sql"]),
        json!(["/bin/psql", "-d", "postgres://user:password@host/db"]),
        json!(["/bin/psql", "-d", "dbname=db password=secret"]),
        json!(["/bin/psql", "-v", "ON_ERROR_STOP=0"]),
    ] {
        assert!(metadata_argv(&json!({"metadata_argv":a})).is_err());
    }
}

#[test]
fn byte_and_inode_cas_and_os_lock() {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "live-catalog-unit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    dir(&root).unwrap();
    let path = root.join("catalog.json");
    put(&path, b"old").unwrap();
    let original = snapshot(&path).unwrap();
    cas(&path, &original).unwrap();
    fs::write(&path, b"new").unwrap();
    assert!(cas(&path, &original).is_err());
    let original = snapshot(&path).unwrap();
    put(root.join("replacement"), b"new").unwrap();
    fs::rename(root.join("replacement"), &path).unwrap();
    assert!(cas(&path, &original).is_err());
    let a = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let b = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    a.try_lock().unwrap();
    assert!(b.try_lock().is_err());
    drop(a);
    b.try_lock().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn help_and_short_arguments_do_not_panic() {
    assert!(dispatch(&[]).is_err());
    assert!(dispatch(&["--config".into()]).is_err());
    assert!(
        HELP.contains("--dry-run")
            && HELP.contains("expected_manifest_sha256")
            && HELP.contains("not activation authorization")
    );
}

#[test]
fn duplicate_json_keys_are_rejected_before_hash_or_authority_checks() {
    assert!(
        parse(br#"{"release":{"knowledge_published":false,"knowledge_published":true}}"#).is_err()
    );
    assert!(parse(br#"{"protocol":"one","protocol":"two"}"#).is_err());
}
