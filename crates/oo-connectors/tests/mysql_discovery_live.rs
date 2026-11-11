#![cfg(feature = "mysql")]
//! Opt-in: MySQL catalog discovery against a throwaway TLS container.
//! Launched by testing/engines/mysql/discovery.py (OO_MY_DISCOVERY_*).
use oo_connectors::engine::*;
use oo_connectors::mysql::{EngineOptions, MysqlConfig, MysqlEngine};
use std::path::PathBuf;

fn find<'a>(found: &'a [DiscoveredTable], name: &str) -> &'a DiscoveredTable {
    found
        .iter()
        .find(|t| t.table.schema == "shop" && t.table.name == name)
        .unwrap_or_else(|| panic!("shop.{name} not discovered"))
}

#[tokio::test]
#[ignore = "requires the owned MySQL discovery runner"]
async fn discovers_databases_and_tables() {
    let dir = PathBuf::from(std::env::var("OO_MY_DISCOVERY_DIR").expect("owned runner"));
    let port: u16 = std::env::var("OO_MY_DISCOVERY_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let mut engine = MysqlEngine::new(
        MysqlConfig {
            mysql_host: "localhost".into(),
            mysql_port: port,
            mysql_database: "shop".into(),
            mysql_user: "oo_reader".into(),
            mysql_ca_file: dir.join("ca.pem"),
            mysql_tls_server_name: None,
            secret_ref: oo_connectors::SecretRef::new("secret://fixture/reader").unwrap(),
            connect_timeout_ms: 10_000,
        },
        std::fs::read_to_string(dir.join("reader")).unwrap().into(),
        EngineOptions {
            source_id: "my_discovery".into(),
            ..Default::default()
        },
    );
    let budget = ReadBudget::default();

    let databases = engine.list_databases(&budget).await.unwrap();
    assert_eq!(
        databases,
        vec!["analytics".to_owned(), "ref".into(), "shop".into()]
    );

    let found = engine.discover_catalog(&budget).await.unwrap();
    assert!(found.iter().all(|t| t.table.schema == "shop"));
    let customers = find(&found, "customers");
    assert!(customers.has_primary_key && !customers.has_unique_key);
    assert!(
        customers.estimated_rows.is_some_and(|n| n > 0),
        "{customers:?}"
    );
    let orders = find(&found, "orders");
    assert_eq!(
        orders.references,
        vec![
            TableRef {
                schema: "ref".into(),
                name: "regions".into()
            },
            TableRef {
                schema: "shop".into(),
                name: "customers".into()
            },
        ]
    );
    let notes = find(&found, "notes");
    assert!(!notes.has_primary_key && notes.has_unique_key);
    assert!(
        !found.iter().any(|t| t.table.name == "recent_orders"),
        "view listed"
    );

    let names: Vec<TableRef> = found.iter().map(|t| t.table.clone()).collect();
    assert_eq!(engine.discover_tables(&budget).await.unwrap(), names);
    let tight = ReadBudget {
        max_chunk_rows: 2,
        ..budget
    };
    let err = engine.discover_catalog(&tight).await.unwrap_err();
    assert_eq!(err.fact(), Some("catalog_table_limit"));
    println!("mysql: {databases:?}, {} tables", found.len());
}
