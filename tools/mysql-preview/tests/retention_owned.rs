use mysql_preview::c1_retention;
use oo_connectors::{
    SecretRef,
    engine::RequirementKind,
    mysql::{MysqlConfig, MysqlEngine},
};
use serde_json::{Value, json};
use std::{fs, path::Path};
use zeroize::Zeroizing;

#[tokio::test]
#[ignore = "requires run.py --suite s2-retention; never uses a default endpoint"]
async fn actual_oldest_log_controls_retention() {
    let path = std::env::var("OO_MYSQL_ENGINE_FIXTURE").expect("owned fixture config");
    let v: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let config = MysqlConfig {
        mysql_host: "127.0.0.1".into(),
        mysql_port: v["port"].as_u64().unwrap() as u16,
        mysql_database: "fixture".into(),
        mysql_user: "reader".into(),
        mysql_ca_file: v["ca"].as_str().unwrap().into(),
        mysql_tls_server_name: Some("localhost".into()),
        secret_ref: SecretRef::new("secret://lane-c-fixture").unwrap(),
        connect_timeout_ms: 5000,
    };
    let password =
        Zeroizing::new(fs::read_to_string(v["reader_password"].as_str().unwrap()).unwrap());
    let mut engine = MysqlEngine::new(config.clone(), password.clone(), Default::default());
    let available = c1_retention::inspect(&mut engine, &config, &password, 732, 0, 0)
        .await
        .unwrap();
    assert!(available.oldest_event_unix.is_some());
    assert!(available.rebaseline.is_none());
    let available = available.require_sufficient().unwrap();
    let short = c1_retention::inspect(&mut engine, &config, &password, 732, 86_400, 3_600)
        .await
        .unwrap();
    assert_eq!(available.earliest, short.earliest);
    let requirement = short
        .rebaseline
        .as_ref()
        .expect("explicit new baseline requirement");
    assert_eq!(requirement.kind, RequirementKind::Retention);
    assert!(requirement.name.contains("new_generation_rebaseline"));
    assert!(requirement.dba_statement.is_none());
    assert_eq!(short.required_seconds, 90_000);
    fs::write(
        Path::new(v["out"].as_str().unwrap()).join("retention.json"),
        serde_json::to_vec_pretty(&json!({
            "earliest": short.earliest,
            "oldest_event_unix": short.oldest_event_unix,
            "server_unix": short.server_unix,
            "requirement": requirement,
            "generation_allocation": "pending_agent_db",
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        short.require_sufficient().unwrap_err().kind,
        RequirementKind::Retention
    );
    engine.close().await;
}
