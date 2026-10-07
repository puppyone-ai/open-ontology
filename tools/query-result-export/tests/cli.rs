use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn bin() -> PathBuf {
    let path = option_env!("CARGO_BIN_EXE_query-result-export")
        .or(option_env!("CARGO_BIN_EXE_query_result_export"))
        .expect("query-result-export binary");
    PathBuf::from(path)
}

fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let root = fs::canonicalize(std::env::temp_dir()).unwrap();
    let dir = root.join(format!(
        "oo-qre-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn request(dir: &std::path::Path, source: &std::path::Path, out: &std::path::Path, columns: &str) -> PathBuf {
    let path = dir.join("request.json");
    let body = format!(
        "{{\"version\":\"oo.query-result-export/v1\",\"source_file\":\"{}\",\"output_dir\":\"{}\",\"columns\":{columns}}}",
        source.display(),
        out.display()
    );
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn help_exits_zero_and_usage_exits_two() {
    let help = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("query-result-export --request <path>"));
    assert!(text.contains("U+0027"));
    assert!(text.contains("xlsx is"));
    let usage = Command::new(bin()).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert_eq!(usage.stderr, b"query-result-export: usage\n");
    assert!(usage.stdout.is_empty());
}

#[test]
fn cli_exports_the_compiled_fixture_and_refuses_a_second_write() {
    let root = scratch();
    let source = root.join("clerk.stdout");
    fs::write(
        &source,
        include_bytes!("fixtures/query-clerk-order_doc-star.stdout"),
    )
    .unwrap();
    let out = root.join("out");
    fs::create_dir(&out).unwrap();
    let req = request(
        &root,
        &source,
        &out,
        "[\"order_id\",\"customer_id\",\"memo\",\"posted\",\"amount\",\"clerk_note\"]",
    );
    let first = Command::new(bin())
        .arg("--request")
        .arg(&req)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0), "stderr {}", String::from_utf8_lossy(&first.stderr));
    assert!(first.stdout.is_empty());
    assert!(first.stderr.is_empty());
    assert_eq!(
        fs::read(out.join("export.csv")).unwrap(),
        include_bytes!("fixtures/golden-clerk.csv")
    );
    assert_eq!(
        fs::read(out.join("export.source.jsonl")).unwrap(),
        include_bytes!("fixtures/query-clerk-order_doc-star.stdout")
    );
    let second = Command::new(bin())
        .arg("--request")
        .arg(&req)
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(4));
    assert_eq!(second.stderr, b"query-result-export: path\n");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cli_partial_exit_does_not_claim_completion() {
    let root = scratch();
    let mut text = String::from_utf8(
        include_bytes!("fixtures/query-clerk-order_doc-star.stdout").to_vec(),
    )
    .unwrap();
    text = text.replacen("\"truncated\":false", "\"truncated\":true", 1);
    let source = root.join("partial.stdout");
    fs::write(&source, text).unwrap();
    let out = root.join("out");
    fs::create_dir(&out).unwrap();
    let req = request(
        &root,
        &source,
        &out,
        "[\"order_id\",\"customer_id\",\"memo\",\"posted\",\"amount\",\"clerk_note\"]",
    );
    let run = Command::new(bin())
        .arg("--request")
        .arg(&req)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(run.stderr, b"query-result-export: partial\n");
    assert!(run.stdout.is_empty());
    let side = fs::read_to_string(out.join("export.sidecar.json")).unwrap();
    assert!(side.contains("\"complete\":false"));
    assert!(side.contains("\"partial\":true"));
    assert_eq!(
        fs::read(out.join("export.csv")).unwrap(),
        include_bytes!("fixtures/golden-clerk.csv")
    );
    let _ = fs::remove_dir_all(&root);
}
