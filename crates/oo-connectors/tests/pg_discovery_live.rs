//! Opt-in: bounded catalog discovery against a throwaway TLS PostgreSQL.
//! Launched by testing/engines/postgres/discovery.py, which owns the
//! container, the fixture and the secrets (OO_PG_DISCOVERY_DIR).
use oo_connectors::engine::*;
use oo_connectors::postgres::PostgresEngine;
use oo_connectors::*;
use std::path::PathBuf;
use std::time::Duration;

fn engine(dir: &std::path::Path, port: u16) -> PostgresEngine {
    PostgresEngine::new(
        PgSourceConfig::try_new(
            SourceId::new("pg_discovery").unwrap(),
            PgEndpoint::new("localhost", port, "logistics").unwrap(),
            PgCredentials::new(
                "oo_reader",
                std::fs::read_to_string(dir.join("reader")).unwrap(),
            )
            .unwrap(),
            SecretRef::new("secret://fixture/reader").unwrap(),
            PgTlsConfig::from_ca_pem(std::fs::read(dir.join("ca.pem")).unwrap()).unwrap(),
            PgBudget::new(
                Duration::from_secs(10),
                Duration::from_secs(30),
                Duration::from_secs(1),
                10_001,
                16 * 1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(Vec::new()).unwrap(),
        )
        .unwrap(),
    )
}

fn find<'a>(found: &'a [DiscoveredTable], schema: &str, name: &str) -> &'a DiscoveredTable {
    found
        .iter()
        .find(|t| t.table.schema == schema && t.table.name == name)
        .unwrap_or_else(|| panic!("{schema}.{name} not discovered"))
}

#[tokio::test]
#[ignore = "requires the owned discovery fixture runner"]
async fn discovers_databases_and_tables() {
    let dir = PathBuf::from(std::env::var("OO_PG_DISCOVERY_DIR").expect("owned runner"));
    let port: u16 = std::env::var("OO_PG_DISCOVERY_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let major: u32 = std::env::var("OO_PG_DISCOVERY_MAJOR")
        .unwrap()
        .parse()
        .unwrap();
    let budget = ReadBudget::default();
    let mut pg = engine(&dir, port);

    let databases = pg.list_databases(&budget).await.unwrap();
    for name in ["analytics", "logistics", "postgres"] {
        assert!(databases.contains(&name.to_owned()), "{databases:?}");
    }
    for hidden in ["restricted", "template0", "template1"] {
        assert!(!databases.contains(&hidden.to_owned()), "{databases:?}");
    }
    let mut sorted = databases.clone();
    sorted.sort();
    assert_eq!(databases, sorted);

    let found = pg.discover_catalog(&budget).await.unwrap();
    assert!(
        found
            .iter()
            .all(|t| !t.table.schema.starts_with("pg_") && t.table.schema != "information_schema"),
        "system tables listed"
    );
    assert!(
        !found.iter().any(|t| t.table.name == "hidden"),
        "unreadable table listed"
    );

    let customers = find(&found, "sales", "customers");
    assert!(customers.has_primary_key && !customers.has_unique_key);
    assert_eq!(customers.estimated_rows, Some(300));
    assert!(customers.references.is_empty());

    let orders = find(&found, "sales", "orders");
    assert!(orders.has_primary_key);
    assert_eq!(
        orders.references,
        vec![
            TableRef {
                schema: "ref".into(),
                name: "regions".into()
            },
            TableRef {
                schema: "sales".into(),
                name: "customers".into()
            },
        ]
    );

    let notes = find(&found, "sales", "notes");
    assert!(!notes.has_primary_key && notes.has_unique_key);
    // Never analyzed: -1 from PostgreSQL 14 on, 0 before.
    if major >= 14 {
        assert_eq!(notes.estimated_rows, None);
    }

    let events = find(&found, "sales", "events");
    assert!(!events.partition_child);
    assert!(find(&found, "sales", "events_2025").partition_child);
    assert!(find(&found, "ref", "regions").has_primary_key);

    let names: Vec<TableRef> = found.iter().map(|t| t.table.clone()).collect();
    let mut ordered = names.clone();
    ordered.sort();
    assert_eq!(names, ordered);
    assert_eq!(pg.discover_tables(&budget).await.unwrap(), names);

    // A budget below the table count is refused, never silently cut.
    let tight = ReadBudget {
        max_chunk_rows: 2,
        ..budget.clone()
    };
    let err = pg.discover_catalog(&tight).await.unwrap_err();
    assert_eq!(err.fact(), Some("catalog_table_limit"));
    println!(
        "pg{major}: {} databases, {} tables",
        databases.len(),
        found.len()
    );
}
