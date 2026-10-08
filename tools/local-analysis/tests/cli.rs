use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_local-analysis")
        .or(option_env!("CARGO_BIN_EXE_local_analysis"))
        .expect("local-analysis bin")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a386-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn assert_status(output: std::process::Output, code: i32, token: Option<&str>) {
    assert_eq!(output.status.code(), Some(code));
    assert!(output.stdout.is_empty());
    match token {
        None => assert!(output.stderr.is_empty()),
        Some(token) => {
            let expected = format!("local-analysis: {token}\n");
            assert_eq!(output.stderr, expected.as_bytes());
        }
    }
}

fn request(dir: &std::path::Path, body: &str) -> PathBuf {
    let path = dir.join("request.json");
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn help_is_exit_zero_and_other_shapes_are_usage() {
    let help = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("local-analysis --request <path>"));
    assert!(text.contains("closes_sp09_w05"));
    assert_status(Command::new(bin()).output().unwrap(), 2, Some("usage"));
    assert_status(
        Command::new(bin()).arg("--help").arg("more").output().unwrap(),
        2,
        Some("usage"),
    );
    assert_status(
        Command::new(bin()).arg("--request").output().unwrap(),
        2,
        Some("usage"),
    );
}

#[test]
fn duckdb_without_a_cli_is_engine_before_paths_exist() {
    let dir = scratch("flags");
    let common = r#""export_dir":"missing","lineage_file":"missing","output_dir":"missing","operation":"sum","group_column":0,"value_column":1,"group_header":"g","value_header":"v","chart_title":"t","cancel_file":null"#;
    let engine = request(
        &dir,
        &format!(
            r#"{{"version":"oo.local-analysis/v1",{common},"engine":"duckdb","xlsx":true,"parquet":true}}"#
        ),
    );
    assert_status(
        Command::new(bin()).arg("--request").arg(&engine).output().unwrap(),
        3,
        Some("engine"),
    );
    let wrong = dir.join("not-the-cli");
    fs::write(&wrong, b"not a duckdb binary").unwrap();
    let mismatch = request(
        &dir,
        &format!(
            r#"{{"version":"oo.local-analysis/v1",{common},"engine":"duckdb","xlsx":true,"parquet":true,"duckdb_cli":"{}","font_file":null}}"#,
            wrong.display()
        ),
    );
    assert_status(
        Command::new(bin()).arg("--request").arg(&mismatch).output().unwrap(),
        3,
        Some("engine"),
    );
    assert!(!dir.join("missing").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_field_budget_and_empty_request_have_stable_tokens() {
    let dir = scratch("bad-request");
    let unknown = request(
        &dir,
        r#"{"version":"oo.local-analysis/v1","export_dir":"e","lineage_file":"l","output_dir":"o","operation":"sum","group_column":0,"value_column":0,"group_header":"g","value_header":"v","chart_title":"t","cancel_file":null,"engine":"rust_exact","xlsx":false,"parquet":false,"extra":1}"#,
    );
    assert_status(
        Command::new(bin()).arg("--request").arg(&unknown).output().unwrap(),
        3,
        Some("request"),
    );
    let empty = dir.join("empty.json");
    fs::write(&empty, b"").unwrap();
    assert_status(
        Command::new(bin()).arg("--request").arg(&empty).output().unwrap(),
        3,
        Some("request"),
    );
    let big = dir.join("big.json");
    fs::write(&big, vec![b' '; 262_145]).unwrap();
    assert_status(
        Command::new(bin()).arg("--request").arg(&big).output().unwrap(),
        3,
        Some("budget"),
    );
    let _ = fs::remove_dir_all(&dir);
}
