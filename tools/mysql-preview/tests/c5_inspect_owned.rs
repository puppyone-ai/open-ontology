use mysql_async::{Conn, OptsBuilder, prelude::Queryable};
use oo_connectors::mysql;
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Command,
};

const BIN: &str = env!("CARGO_BIN_EXE_mysql-preview");
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn private(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}

fn inspect(path: &Path) -> Result<Value> {
    let output = Command::new(BIN)
        .args(["c5-inspect", "--config"])
        .arg(path)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    Ok(serde_json::from_slice(&output.stdout)?)
}

async fn connection(user: &str, password: &str) -> Result<Conn> {
    Ok(Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23308)
            .user(Some(user))
            .pass(Some(password))
            .prefer_socket(false)
            .ssl_opts(None::<mysql_async::SslOpts>),
    )
    .await?)
}

#[test]
fn inspection_cli_requires_exact_arguments_without_opening_a_source() {
    for args in [
        vec!["c5-inspect"],
        vec!["c5-inspect", "--config"],
        vec!["c5-inspect", "--sql", "SELECT 1"],
        vec!["c5-inspect", "--config", "unused", "--out", "unused"],
    ] {
        let output = Command::new(BIN).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[tokio::test]
#[ignore = "owned MySQL 5.7.44 loopback fixture only"]
async fn owned_inspect_cli_metadata_matches_full_and_empty_helpers() -> Result<()> {
    let root_password = std::env::var("OO_C5_CODEC_FIXTURE_PASSWORD")?;
    let mut admin = connection("root", &root_password).await?;
    let version: String = admin
        .query_first("SELECT VERSION()")
        .await?
        .ok_or("version")?;
    assert_eq!(version, "5.7.44");
    admin
        .query_drop("CREATE DATABASE IF NOT EXISTS codec_fixture")
        .await?;
    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let keyed = format!("inspect_{tag}");
    let dated = format!("inspect_date_{tag}");
    let user = format!("inspect_{tag}");
    let password = format!("owned-inspection-{tag}");
    admin
        .query_drop(format!(
            "CREATE USER '{user}'@'%' IDENTIFIED BY '{password}'"
        ))
        .await?;
    admin
        .query_drop(format!("GRANT SELECT ON codec_fixture.* TO '{user}'@'%'"))
        .await?;
    admin
        .query_drop(format!(
            "CREATE TABLE codec_fixture.`{keyed}` (
        a BIGINT NOT NULL,b VARCHAR(16) CHARACTER SET gbk COLLATE gbk_bin NOT NULL,
        note VARCHAR(128),PRIMARY KEY(b,a)) ENGINE=InnoDB"
        ))
        .await?;
    admin.query_drop(format!("INSERT INTO codec_fixture.`{keyed}` VALUES (1,'x','owned-payload-must-not-be-read-or-returned')")).await?;
    admin
        .query_drop(format!(
            "CREATE TABLE codec_fixture.`{dated}` (v DATE PRIMARY KEY) ENGINE=InnoDB"
        ))
        .await?;
    admin
        .query_drop(format!(
            "INSERT INTO codec_fixture.`{dated}` VALUES ('2026-10-26')"
        ))
        .await?;
    let out = std::env::temp_dir().join(format!("c5-inspection-qualification-{tag}"));
    fs::create_dir(&out)?;
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
    let secret = out.join("reader-secret");
    private(&secret, password.as_bytes())?;
    let base = json!({"schema":"codec_fixture","connection":{"host":"127.0.0.1","port":23308,
        "user":user,"secret_file":secret,"tls":"disabled","plaintext_authorized":true}});
    let mut single = base.clone();
    single["table"] = json!(keyed);
    single["columns"] = json!(["note", "b", "a"]);
    let single_path = out.join("single.json");
    private(&single_path, &serde_json::to_vec(&single)?)?;
    let one = inspect(&single_path)?;
    assert_eq!(one["schema_fingerprint_codec"], "mysql-full-catalog/v1");
    assert_eq!(one["selected_columns"], json!(["note", "b", "a"]));
    assert_eq!(one["key_columns"], json!(["b", "a"]));
    assert_eq!(
        one["source_key_columns"],
        json!([
            {"name":"b","kind":"Text","collation":"gbk_bin"},{"name":"a","kind":"Int","collation":null}
        ])
    );
    let mut reader = connection(&user, &password).await?;
    let catalog = mysql::catalog_columns(
        &mut reader,
        &mut vec![],
        &mysql::SelectedTable {
            schema: "codec_fixture".into(),
            name: keyed.clone(),
            columns: vec![],
        },
    )
    .await?;
    let historical = catalog.iter().map(|c|json!({
        "name":c.name,"ordinal":c.ordinal,"data_type":c.data_type,"column_type":c.column_type,
        "nullable":c.nullable,"char_len":c.char_len,"precision":c.precision,"scale":c.scale,
        "charset":c.charset,"column_key":c.column_key,
    })).collect::<Vec<_>>();
    assert_eq!(
        one["schema_fingerprint"],
        sha256_hex(&serde_json::to_vec(&historical)?)
    );
    let mut reported = one["source_columns"].as_array().unwrap().clone();
    for column in &mut reported {
        column.as_object_mut().unwrap().remove("collation");
    }
    assert_eq!(reported, historical);
    assert_eq!(one["source_columns"][0]["name"], "a");
    assert_eq!(one["source_columns"][1]["collation"], "gbk_bin");
    let mut bulk = base.clone();
    bulk["tables"] = json!([
        {"table":keyed,"columns":["b","a"]},{"table":dated,"empty_only":true},
    ]);
    let bulk_path = out.join("bulk.json");
    private(&bulk_path, &serde_json::to_vec(&bulk)?)?;
    let all = inspect(&bulk_path)?;
    assert_eq!(all["tables"].as_array().unwrap().len(), 2);
    assert_eq!(
        all["tables"][0]["schema_fingerprint"],
        one["schema_fingerprint"]
    );
    assert_eq!(
        all["tables"][0]["source_columns"], one["source_columns"],
        "full catalog metadata must not shrink to the selected projection"
    );
    let empty = &all["tables"][1];
    assert_eq!(
        empty["schema_fingerprint_codec"],
        "mysql-empty-catalog-keys/v1"
    );
    assert_eq!(
        empty["schema_fingerprint"],
        mysql_preview::c5_layered::empty::schema_fingerprint(&mut reader, "codec_fixture", &dated)
            .await?
    );
    assert_eq!(empty["key_columns"], json!([]));
    assert_eq!(empty["range_key_available"], false);
    assert_eq!(empty["source_columns"][0]["column_type"], "date");
    assert_eq!(empty["source_columns"][0]["column_key"], "PRI");
    for result in [&one, empty, &all] {
        assert_eq!(result["metadata_only"], true);
        assert_eq!(result["business_rows_read"], 0);
        assert_eq!(result["historical_baseline_verified"], false);
        assert_eq!(result["emptiness_verified"], false);
        let serialized = serde_json::to_string(result)?;
        assert!(!serialized.contains(&password));
        assert!(!serialized.contains("owned-payload-must-not-be-read-or-returned"));
    }
    let mut root = single.clone();
    let root_secret = out.join("root-secret");
    private(&root_secret, root_password.as_bytes())?;
    root["connection"]["user"] = json!("root");
    root["connection"]["secret_file"] = json!(root_secret);
    let root_config = out.join("denied.json");
    private(&root_config, &serde_json::to_vec(&root)?)?;
    let denied = Command::new(BIN)
        .args(["c5-inspect", "--config"])
        .arg(&root_config)
        .output()?;
    assert!(
        !denied.status.success(),
        "write-capable grants must fail closed"
    );
    assert!(denied.stdout.is_empty());
    let evidence = json!({"scope":"owned fixture metadata-only CLI; no customer access",
        "mysql_version":version,"single":one,"bulk":all,"write_capable_grants_refused":true});
    private(
        &out.join("qualification.json"),
        &serde_json::to_vec_pretty(&evidence)?,
    )?;
    println!(
        "C5_INSPECTION_QUALIFICATION={}",
        out.join("qualification.json").display()
    );
    reader.disconnect().await?;
    admin.disconnect().await?;
    Ok(())
}
