use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("oo-dl01w1-09-cli-{name}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_mode(path: &std::path::Path, contents: &str, mode: u32) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = fs::remove_file(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(contents.as_bytes()).unwrap();
}

fn customer(dir: &std::path::Path, extra: &str) -> PathBuf {
    let secret = dir.join("reader");
    let ca = dir.join("ca.pem");
    write_mode(&secret, "reader-secret\n", 0o600);
    fs::write(&ca, "pem-bytes").unwrap();
    let path = dir.join("customer.json");
    let body = format!(
        r#"{{
            "protocol": "oo.mssql-preview.customer/v1",
            "source_id": "sqlserver_customer",
            "secret_ref": "{secret}",
            "endpoint": {{"host": "db.example", "port": 1433, "database": "appdb", "username": "reader"}},
            "encrypt": "required",
            "trust_server_certificate": false,
            "ca_file": "{ca}",
            "approved_objects": [{{"schema": "dbo", "name": "orders"}}],
            "budget": {{"unit": "Rows", "hard_limit": 10, "confidence": "declared"}},
            "permissions_expected": ["SELECT"]
            {extra}
        }}"#,
        secret = secret.display(),
        ca = ca.display(),
        extra = extra,
    );
    fs::write(&path, body).unwrap();
    path
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_mssql-preview"))
        .args(args)
        .output()
        .unwrap();
    let code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    (code, stdout, stderr)
}

#[test]
fn driver_pin_is_tiberius_without_default_features() {
    let cargo = include_str!("../Cargo.toml");
    assert!(cargo.contains("version = \"=0.13.0\""));
    assert!(cargo.contains("default-features = false"));
    assert!(cargo.contains("\"tds73\""));
    assert!(cargo.contains("\"rustls\""));
    assert!(cargo.contains("\"rust_decimal\""));
    assert!(cargo.contains("\"rustls-webpki-roots\""));
    assert!(!cargo.contains("chrono"));
    assert!(!cargo.contains("native-tls"));
    assert!(!cargo.contains("tds80"));
    assert!(!cargo.contains("integrated-auth"));
}

#[test]
fn commands_reject_secrets_and_do_not_approve_the_source() {
    let dir = scratch("cmd");
    let config = customer(&dir, "");
    let (code, stdout, stderr) = run(&["preflight", "--config", config.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("\"socket_opened\": false"));
    assert!(stdout.contains("\"admission_invoked\": false"));
    assert!(stdout.contains("\"source_acceptance\": \"pending\""));
    assert!(!stdout.contains("\"admitted\""));
    assert!(!stdout.contains("reader-secret"));

    let bad = dir.join("inline.json");
    fs::write(&bad, r#"{"password":"reader-secret","protocol":"oo.mssql-preview.customer/v1"}"#).unwrap();
    let (code, _, stderr) = run(&["preflight", "--config", bad.to_str().unwrap()]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("inline_secret"));
    assert!(!stderr.contains("reader-secret"));

    let foreign = customer(&dir, "");
    let (code, _, stderr) = run(&[
        "preflight",
        "--config",
        foreign.to_str().unwrap(),
        "--object",
        "dbo.other",
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("object_not_approved"));

    let past = customer(&dir, r#", "connect_deadline_unix_ms": 1"#);
    let started = std::time::Instant::now();
    let (code, _, stderr) = run(&[
        "catalog",
        "--config",
        past.to_str().unwrap(),
        "--out",
        dir.join("catalog.json").to_str().unwrap(),
    ]);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(code, 3, "{stderr}");
    assert!(stderr.contains("deadline_exceeded"));

    let seal_config = customer(
        &dir,
        r#", "seal": {
            "run_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "owner": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "source_generation": 1,
            "target_generation": 1,
            "snapshot_kind": "declared",
            "snapshot_value": "unverified"
        }"#,
    );
    let request = dir.join("request.json");
    let (code, _, stderr) = run(&[
        "seal-request",
        "--config",
        seal_config.to_str().unwrap(),
        "--out",
        request.to_str().unwrap(),
    ]);
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.contains("native_engine_unrecognized"));
    let body = fs::read_to_string(&request).unwrap();
    assert!(body.contains("oo.pipeline.native-sealed-catalog/v1"));
    assert!(!body.contains("\"admitted\""));
    assert!(!body.contains("\"engine\""));
    assert!(!body.contains("\"source_engine\""));
    assert!(!body.contains("reader-secret"));

    let (code, stdout, _) = run(&["gates"]);
    assert_eq!(code, 5);
    assert!(stdout.contains("\"passed\": false"));
    assert!(stdout.contains("\"whole_source\": \"pending\""));
    assert!(!stdout.contains("\"admitted\""));
    assert!(stdout.contains("cdc_ct_claimed"));

    let fixture = dir.join("fixture.json");
    fs::write(
        &fixture,
        r#"{
            "protocol": "oo.mssql-preview.encode-fixture/v1",
            "native_source": false,
            "gold": false,
            "columns": [
                {"name": "name_ci", "sql_type": "nvarchar", "collation": "Latin1_General_CI_AS"},
                {"name": "dt7", "sql_type": "datetime2", "scale": 7}
            ],
            "rows": [[
                {"kind": "nvarchar", "value": "A "},
                {"kind": "datetime2", "days": 738959, "increments": 452961234567, "scale": 7}
            ]]
        }"#,
    )
    .unwrap();
    let arrow = dir.join("batch.arrow");
    let (code, _, stderr) = run(&[
        "encode-arrow",
        "--fixture",
        fixture.to_str().unwrap(),
        "--out",
        arrow.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let manifest = fs::read_to_string(dir.join("batch.arrow.manifest.json")).unwrap();
    assert!(manifest.contains("\"native_source\": false"));
    assert!(manifest.contains("\"executed_against_sqlserver\": false"));
    assert!(manifest.contains("\"pg_timestamp6\": \"refused\""));
    assert!(!manifest.contains("\"admitted\""));
    assert!(arrow.metadata().unwrap().len() > 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn capture_commands_refuse_missing_approval_and_invalid_chunk_without_socket() {
    let dir=scratch("capture-approval");
    let path=customer(&dir, "");
    let out=dir.join("never-created");
    let (code,_,stderr)=run(&["snapshot-full","--config",path.to_str().unwrap(),"--out",out.to_str().unwrap()]);
    assert_eq!(code,2);assert!(stderr.contains("ct_approval"));assert!(!out.exists());
    let (code,_,stderr)=run(&["snapshot-full","--config",path.to_str().unwrap(),"--out",out.to_str().unwrap(),"--chunk-rows","0"]);
    assert_eq!(code,2);assert!(stderr.contains("chunk_size"));assert!(!out.exists());
    let _=fs::remove_dir_all(dir);
}
