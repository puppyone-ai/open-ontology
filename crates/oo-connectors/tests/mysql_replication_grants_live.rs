//! Read-only admission check; this does not claim a binlog capture or C5/C1 handoff.
#![cfg(feature = "mysql")]

use mysql_async::prelude::Queryable;
use oo_connectors::{SecretRef, mysql::*};
use serde_json::Value;
use std::fs;
use zeroize::Zeroizing;

#[tokio::test]
#[ignore = "requires OO_MYSQL_C1_GRANTS_FIXTURE pointing to an owned TLS source"]
async fn c1_admits_existing_observation_grants_without_using_them() {
    let path = std::env::var("OO_MYSQL_C1_GRANTS_FIXTURE").unwrap();
    let fixture: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(fixture["owned_fixture"], true);
    let text = |key: &str| fixture[key].as_str().unwrap().to_owned();
    let config = MysqlConfig {
        mysql_host: text("host"),
        mysql_port: fixture["port"].as_u64().unwrap().try_into().unwrap(),
        mysql_database: text("database"),
        mysql_user: text("user"),
        mysql_ca_file: text("ca_file").into(),
        mysql_tls_server_name: fixture["tls_server_name"].as_str().map(str::to_owned),
        secret_ref: SecretRef::new("secret://owned/c1-grants").unwrap(),
        connect_timeout_ms: 5000,
    };
    let password = Zeroizing::new(
        fs::read_to_string(text("password_file")).unwrap()
            .trim_end_matches(['\r', '\n']).to_owned(),
    );
    let mut conn = connect(&config, &password).await.unwrap();
    let grants: Vec<String> = conn.query("SHOW GRANTS FOR CURRENT_USER()").await.unwrap();
    let observed = full_read_grants(&grants).unwrap();
    for privilege in ["SELECT", "PROCESS", "LOCK TABLES", "SHOW VIEW",
                      "REPLICATION SLAVE", "REPLICATION CLIENT"] {
        assert!(observed.privileges.contains(privilege), "{privilege}");
    }
    let mut notes = Vec::new();
    let access = inspect_replication(&mut conn, &config.mysql_user, &mut notes).await.unwrap();
    assert!(access.version.starts_with(&text("version_prefix")));
    assert!(!access.ssl_cipher.is_empty());
    assert!(notes.iter().all(|note| matches!(note.verb.as_str(), "SELECT" | "SHOW")));
    // The same genuine grants must not hide a write-capable credential.
    let mut with_write = grants;
    with_write.push("GRANT UPDATE ON owned_fixture.* TO reader".to_owned());
    assert!(replication_grants_ok(&with_write).is_err());
    println!("C1_GRANT_PROOF version={} tls_verified=true metadata_statements={} lock_sql_used=false write_grant_refused=true binlog_capture_tested=false",
        access.version, notes.len());
    conn.disconnect().await.unwrap();
}
