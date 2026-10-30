#[test]
fn create_and_upgrade_constraints_allow_only_the_emitted_c5_stages() {
    let sql = include_str!("../native_schedule.sql");
    let checks: Vec<_> = sql
        .lines()
        .filter(|line| line.contains("CHECK(stage IN ("))
        .map(|line| {
            line.split_once("CHECK(")
                .unwrap()
                .1
                .rsplit_once(')')
                .unwrap()
                .0
        })
        .collect();
    assert_eq!(
        checks.len(),
        2,
        "check both creation and installed-schema upgrade"
    );
    let allowed = [
        "c5_initialize_full_if_absent",
        "c5_initialize_multipart_if_absent",
        "c5_initialize_published_if_absent",
        "c5_initialize_empty_if_absent",
        "c5_begin_scan",
        "c5_observe_partial",
        "c5_export",
        "c5_export_before",
        "c5_export_after",
        "c5_export_hydrated",
        "c5_observe",
        "c5_hydrate_multipart_0",
        "c5_hydrate_multipart_9",
        "c5_hydrate_multipart_10",
        "c5_hydrate_multipart_9999",
    ];
    let refused = [
        "c5_arbitrary",
        "c5_initialize_full",
        "c5_initialize_published",
        "c5_initialize_empty",
        "c5_export_fake",
        "c5_begin_scan_0",
        "c5_observe_partial_untrusted",
        "c5_hydrate_multipart",
        "c5_hydrate_multipart_-1",
        "c5_hydrate_multipart_00",
        "c5_hydrate_multipart_01",
        "c5_hydrate_multipart_10000",
        "c5_hydrate_multipart_0_extra",
    ];
    let output = std::process::Command::new("python3")
        .args([
            "-I",
            "-c",
            r#"
import json, re, sqlite3, sys
checks, allowed, refused = map(json.loads, sys.argv[1:4])
limit = int(sys.argv[4])
allowed += ["c5_hydrate_multipart_" + str(index) for index in range(limit)]
refused += ["c5_hydrate_multipart_" + str(limit)]
for check in checks:
    with sqlite3.connect(":memory:") as db:
        db.create_function("regexp", 2, lambda pattern, value: re.fullmatch(pattern, value) is not None)
        db.execute("CREATE TABLE attempts (stage TEXT NOT NULL CHECK(" + check.replace("stage ~", "stage REGEXP") + "))")
        for stage in allowed:
            db.execute("INSERT INTO attempts VALUES (?)", (stage,))
        for stage in refused:
            try:
                db.execute("INSERT INTO attempts VALUES (?)", (stage,))
            except sqlite3.IntegrityError:
                continue
            raise AssertionError("unexpected stage accepted: " + stage)
print("both constraints verified")
"#,
            &serde_json::to_string(&checks).unwrap(),
            &serde_json::to_string(&allowed).unwrap(),
            &serde_json::to_string(&refused).unwrap(),
            &job_worker::native_schedule::c5_state_adapter::MAX_BASELINE_PARTS.to_string(),
        ])
        .output()
        .expect("python3 with stdlib sqlite3 is required for the in-memory SQL fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"both constraints verified\n");
}
