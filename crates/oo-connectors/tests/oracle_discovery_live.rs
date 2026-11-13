#![cfg(feature = "oracle")]
//! Opt-in: Oracle catalog discovery against a throwaway container.
//! Launched by testing/engines/oracle/discovery.py (OO_ORA_DISCOVERY_*).
use oo_connectors::engine::*;
use oo_connectors::oracle::engine::{OracleConnectConfig, OracleEngine};

fn find<'a>(found: &'a [DiscoveredTable], name: &str) -> &'a DiscoveredTable {
    found
        .iter()
        .find(|t| t.table.schema == "OO_APP" && t.table.name == name)
        .unwrap_or_else(|| panic!("OO_APP.{name} not discovered"))
}

#[tokio::test]
#[ignore = "requires the owned Oracle discovery runner"]
async fn discovers_tables_and_services() {
    let port: u16 = std::env::var("OO_ORA_DISCOVERY_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let service = std::env::var("OO_ORA_DISCOVERY_SERVICE").unwrap();
    let password =
        std::fs::read_to_string(std::env::var("OO_ORA_DISCOVERY_PASSWORD").unwrap()).unwrap();
    let mut engine = OracleEngine::connect(OracleConnectConfig::new(
        "127.0.0.1",
        port,
        &service,
        "oo_app",
        password.into(),
    ))
    .unwrap();
    let budget = ReadBudget::default();

    let services = engine.list_services(&budget).unwrap();
    assert!(
        services.iter().any(|s| s.eq_ignore_ascii_case(&service)),
        "{services:?}"
    );

    let found = engine
        .discover_catalog(&budget)
        .unwrap_or_else(|e| panic!("{e:?} ORA-{:?}", engine.last_ora_code()));
    for system in ["SYS", "SYSTEM", "XDB", "MDSYS", "CTXSYS", "AUDSYS"] {
        assert!(
            found.iter().all(|t| t.table.schema != system),
            "{system} table listed"
        );
    }
    let customers = find(&found, "CUSTOMERS");
    assert!(customers.has_primary_key && !customers.has_unique_key);
    assert_eq!(customers.estimated_rows, Some(300));
    let orders = find(&found, "ORDERS");
    assert!(orders.has_primary_key);
    assert_eq!(
        orders.references,
        vec![
            TableRef {
                schema: "OO_APP".into(),
                name: "CUSTOMERS".into()
            },
            TableRef {
                schema: "OO_APP".into(),
                name: "REGIONS".into()
            },
        ]
    );
    let notes = find(&found, "NOTES");
    assert!(!notes.has_primary_key && notes.has_unique_key);
    assert!(
        !found.iter().any(|t| t.table.name == "SCRATCH"),
        "temporary table listed"
    );

    let names: Vec<TableRef> = found.iter().map(|t| t.table.clone()).collect();
    assert_eq!(engine.discover_tables(&budget).await.unwrap(), names);
    let tight = ReadBudget {
        max_chunk_rows: 2,
        ..budget
    };
    let err = engine.discover_catalog(&tight).unwrap_err();
    assert_eq!(err.fact(), Some("catalog_table_limit"));
    println!("oracle: services {services:?}, {} tables", found.len());
}
