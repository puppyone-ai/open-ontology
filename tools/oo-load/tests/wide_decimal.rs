//! Run with the owned test-wide-decimal.py fixture, never a customer source.
use oo_connectors::engine::{ReadBudget, SourceEngine, TableRef};
use oo_connectors::mysql::{EngineOptions, MysqlConfig, MysqlEngine};
use oo_connectors::{
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SourceId, TableSelection,
};
use oo_pipeline::engine_load::load;
use oo_pipeline::engine_load_pg::PgTarget;
use sqlx::{
    Connection,
    postgres::{PgConnectOptions, PgConnection, PgSslMode},
};

#[tokio::test]
#[ignore = "requires the owned MySQL 8.4 or PostgreSQL fixture"]
async fn wide_decimals_load_byte_exact() {
    let path = std::path::PathBuf::from(
        std::env::var("OO_WIDE_DECIMAL_CONFIG").expect("private config path"),
    );
    let root = path.parent().unwrap();
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    if config["engine"] == "postgres" {
        let table = TableRef {
            schema: "public".into(),
            name: "wide_decimal".into(),
        };
        let cfg = PgSourceConfig::try_new(
            SourceId::new("wide_fixture").unwrap(),
            PgEndpoint::new(
                "127.0.0.1",
                config["source_port"].as_u64().unwrap() as u16,
                "fixture",
            )
            .unwrap(),
            PgCredentials::new(
                "fixture_reader",
                std::fs::read_to_string(root.join("source-password")).unwrap(),
            )
            .unwrap(),
            SecretRef::new("secret://fixture-reader").unwrap(),
            PgTlsConfig::from_ca_pem(std::fs::read(root.join("source-ca")).unwrap()).unwrap(),
            PgBudget::new(
                std::time::Duration::from_secs(10),
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(1),
                100_000,
                32 * 1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(vec![SelectedTable::new("public", "wide_decimal").unwrap()])
                .unwrap(),
        )
        .unwrap();
        verify(
            &mut oo_connectors::postgres::PostgresEngine::new(cfg),
            root,
            &config,
            table,
        )
        .await;
        return;
    }
    let mut engine = MysqlEngine::new(
        MysqlConfig {
            mysql_host: "127.0.0.1".into(),
            mysql_port: config["source_port"].as_u64().unwrap() as u16,
            mysql_database: "fixture".into(),
            mysql_user: "fixture_reader".into(),
            mysql_ca_file: root.join("source-ca"),
            mysql_tls_server_name: None,
            secret_ref: SecretRef::new("secret://fixture-reader").unwrap(),
            connect_timeout_ms: 10000,
        },
        zeroize::Zeroizing::new(std::fs::read_to_string(root.join("source-password")).unwrap()),
        EngineOptions::default(),
    );
    verify(
        &mut engine,
        root,
        &config,
        TableRef {
            schema: "fixture".into(),
            name: "wide_decimal".into(),
        },
    )
    .await;
}

async fn verify<E: SourceEngine>(
    engine: &mut E,
    root: &std::path::Path,
    config: &serde_json::Value,
    table: TableRef,
) {
    let password = std::fs::read_to_string(root.join("target-password")).unwrap();
    let mut conn = PgConnection::connect_with(
        &PgConnectOptions::new()
            .host("127.0.0.1")
            .port(config["target_port"].as_u64().unwrap() as u16)
            .username("postgres")
            .database("postgres")
            .password(password.trim())
            .ssl_mode(PgSslMode::Disable),
    )
    .await
    .unwrap();
    let budget = ReadBudget::default();
    let shape = engine.column_shape(&table, &budget).await.unwrap().unwrap();
    assert_eq!(shape[1].target_type, "numeric(40,12)");
    assert_eq!(shape[2].target_type, "numeric(65,30)");
    let mut target = PgTarget::new(&mut conn, "wide", "wide-1", &table)
        .await
        .unwrap();
    target.source_columns(Some(shape));
    load(engine, &table, &budget, oo_load::IpcSink::new, &mut target)
        .await
        .unwrap();
    let relation = target.relation();
    drop(target);
    let rows: Vec<(i64, Option<String>, Option<String>)> =
        sqlx::query_as(sqlx_core::sql_str::AssertSqlSafe(format!(
            "SELECT id,a::text,b::text FROM {relation} ORDER BY id"
        )))
        .fetch_all(&mut conn)
        .await
        .unwrap();
    let source: Vec<(i64, Option<String>, Option<String>)> =
        serde_json::from_slice(&std::fs::read(root.join("source-rows.json")).unwrap()).unwrap();
    assert_eq!(rows, source);
    conn.close().await.unwrap();
}
