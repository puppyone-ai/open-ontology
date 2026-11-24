//! No path, name, endpoint, bucket, credential or content reaches a log line
//! (B-S4-05, 10 §9, §10.1).
//!
//! The static test scans the source for log calls with free form fields. The
//! live one (`--ignored`, needs a database) runs the real engine binary with
//! `OO_DOCS_LOG=debug` through a whole life (drive, files of every kind,
//! failures, API errors) and scans what it wrote to stderr.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test log_redaction -- --ignored`

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use common::engine::{ChildEngine, TOKEN};
use common::{installed_db, pool_runtime};
use serde_json::{Value, json};

/// Keys a log line may have (10 §10.1 plus the few counters of lifecycle lines).
const ALLOWED_KEYS: &[&str] = &[
    "ts",
    "level",
    "event",
    "request_id",
    "job_id",
    "kind",
    "drive_id",
    "file_id",
    "path_h",
    "stage",
    "attempt",
    "duration_ms",
    "code",
    "route",
    "status",
    "reason",
    "count",
    "ontology_version",
];

/// Words that must never be a key of a log call.
const FORBIDDEN_KEYS: &[&str] = &[
    "\"path\"",
    "\"raw_key\"",
    "\"root\"",
    "\"endpoint\"",
    "\"bucket\"",
    "\"url\"",
    "\"token\"",
    "\"secret\"",
    "\"password\"",
    "\"name\"",
    "\"config\"",
    "\"content\"",
    "\"text\"",
    "\"key_ref\"",
    "\"alias\"",
    "\"quote\"",
];

fn src_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            src_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn log_calls_use_the_fixed_vocabulary_only() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    src_files(&root, &mut files);
    let mut calls = 0;
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        // Skip the logging module's own tests and docs.
        if f.ends_with("obs.rs") {
            continue;
        }
        for needle in ["obs::log(", "obs::emit(", "obs::emit_debug(", "fn fail("] {
            let mut rest = text.as_str();
            while let Some(i) = rest.find(needle) {
                calls += 1;
                let after = &rest[i..];
                let window: String = after.chars().take(700).collect();
                // up to the end of the statement
                let call = window.split(";\n").next().unwrap_or(&window);
                for bad in FORBIDDEN_KEYS {
                    assert!(
                        !call.contains(bad),
                        "{}: a log call has the key {bad}:\n{call}",
                        f.display()
                    );
                }
                // Free text may only go through the scrubber.
                if call.contains("\"reason\"") {
                    assert!(
                        call.contains("scrub("),
                        "{}: reason without scrub:\n{call}",
                        f.display()
                    );
                }
                rest = &after[needle.len()..];
            }
        }
    }
    assert!(
        calls >= 5,
        "the scan found {calls} log calls; is it looking in the right place?"
    );
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_whole_engine_life_logs_no_path_name_endpoint_bucket_or_content() {
    let db = "logs_scan";
    {
        let _g = common::CLUSTER_LOCK.lock().await;
        installed_db(db).await;
    }
    let state = std::env::temp_dir().join(format!("oo-docs-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).unwrap();
    let root = state.join("SECRETROOT_drive");
    std::fs::create_dir_all(root.join("SECRETDIR_sub")).unwrap();
    std::fs::write(
        root.join("SECRETDIR_sub/SECRETFILE_a.md"),
        "---\ntype: project\nname: SECRETBODY_proj\nowner: SECRETBODY_owner\n---\n# SECRETBODY_head\n\n负责人：SECRETBODY_owner\n",
    )
    .unwrap();
    std::fs::write(root.join("SECRETFILE_b.txt"), "SECRETBODY_plain text\n").unwrap();
    std::fs::write(root.join("SECRETFILE_bin.txt"), [0u8, 1, 2, 0, 255]).unwrap();
    std::fs::write(root.join("SECRETFILE_gone.md"), "# SECRETBODY_gone\n").unwrap();

    let engine = ChildEngine::spawn(
        env!("CARGO_BIN_EXE_oo-docs-engine"),
        db,
        &state,
        &[("OO_DOCS_LOG", "debug")],
    );
    let auth = format!("Bearer {TOKEN}");
    let call = |method: &'static str, path: String, body: Option<Value>| {
        let auth = auth.clone();
        let addr = engine.addr;
        async move {
            let key = common::engine::key();
            let mut h = vec![("Authorization", auth.as_str())];
            if method == "POST" {
                h.push(("Idempotency-Key", key.as_str()));
            }
            let text = body.map(|b| b.to_string());
            common::http::request(addr, method, &path, &h, text.as_deref()).await
        }
    };

    // A drive, and the whole pipeline over files of every kind.
    let r = call(
        "POST",
        "/v1/docs/drives".into(),
        Some(json!({
            "name": "SECRETDRIVE_name", "backend": "fs", "config": {"root": root.to_str().unwrap()}
        })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let did = r.json()["id"].as_str().unwrap().to_owned();
    let pool = pool_runtime(db, 4).await;
    let mut ok = false;
    for _ in 0..300 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.file WHERE state IN ('published', 'parse_failed', 'unsupported')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if n == 4 {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ok, "files settled");
    // A delete, a retry, reads of every kind, and requests that fail.
    std::fs::remove_file(root.join("SECRETFILE_gone.md")).unwrap();
    let _ = call(
        "POST",
        format!("/v1/docs/drives/{did}:sync"),
        Some(json!({})),
    )
    .await;
    let fid: String =
        sqlx::query_scalar("SELECT id::text FROM _oo_docs.file WHERE path LIKE '%SECRETFILE_a.md'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let _ = call(
        "POST",
        "/v1/docs/files:retry".into(),
        Some(json!({"file_ids": [fid]})),
    )
    .await;
    for p in [
        format!("/v1/docs/files/{fid}?include=history,entities,facts,shares"),
        format!("/v1/docs/files/{fid}/blocks"),
        format!("/v1/docs/files/{fid}/content"),
        format!("/v1/docs/drives/{did}/files?q=SECRETFILE"),
        format!("/v1/docs/drives/{did}/tree?path=SECRETDIR_sub"),
        "/v1/docs/metrics".to_owned(),
        "/v1/docs/health".to_owned(),
        "/v1/docs/files/00000000-0000-4000-8000-000000000000".to_owned(),
        "/v1/docs/drives/00000000-0000-4000-8000-000000000000/files?path=SECRETDIR_x".to_owned(),
    ] {
        let _ = call("GET", p, None).await;
    }
    let bad_drives = [
        json!({"name": "x", "backend": "fs", "config": {"root": "/SECRETMISSING_root/x"}}),
        json!({"name": "SECRETDRIVE_s3", "backend": "s3", "config": {
            "endpoint": "https://SECRETENDPOINT.example.com", "bucket": "SECRETBUCKET_name", "region": "x"}}),
    ];
    for b in bad_drives {
        let r = call("POST", "/v1/docs/drives".into(), Some(b)).await;
        assert!(r.status >= 400, "{}", String::from_utf8_lossy(&r.body));
    }
    tokio::time::sleep(Duration::from_millis(1500)).await; // publish, freshness
    pool.close().await;
    let stderr_path = engine.stderr_path.clone();
    let status = engine.stop();
    assert!(status.success(), "clean stop: {status:?}");

    // The scan.
    let log = std::fs::read_to_string(&stderr_path).unwrap();
    assert!(
        !log.trim().is_empty(),
        "the engine logged nothing; the scan would prove nothing"
    );
    for marker in [
        "SECRETROOT",
        "SECRETDIR",
        "SECRETFILE",
        "SECRETBODY",
        "SECRETDRIVE",
        "SECRETENDPOINT",
        "SECRETBUCKET",
        "SECRETMISSING",
        TOKEN,
        "oo_docs_it_runtime_login",
        &state.to_string_lossy(),
    ] {
        assert!(!log.contains(marker), "the log contains {marker}:\n{log}");
    }
    let mut events = BTreeSet::new();
    let allowed: BTreeSet<&str> = ALLOWED_KEYS.iter().copied().collect();
    for line in log.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value =
            serde_json::from_str(line).unwrap_or_else(|_| panic!("not a JSON line: {line}"));
        for k in v.as_object().expect("object").keys() {
            assert!(
                allowed.contains(k.as_str()),
                "key {k} is not in the vocabulary: {line}"
            );
        }
        assert!(
            v["ts"].is_string() && v["level"].is_string() && v["event"].is_string(),
            "{line}"
        );
        events.insert(v["event"].as_str().unwrap().to_owned());
    }
    for must in [
        "job.claimed",
        "job.completed",
        "publish.version",
        "api.request",
    ] {
        assert!(events.contains(must), "no {must} line in {events:?}");
    }
    let _ = std::fs::remove_dir_all(&state);
    common::drop_db(db).await;
}
