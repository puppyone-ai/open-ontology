use oo_gateway::policy::catalog::{
    CatalogLimits, UnknownRows, load_policy_catalog_for_source_vector,
};
use oo_gateway::sql_guard::PolicyIdentity;
use serde_json::Value;
use std::path::PathBuf;

#[tokio::test]
#[ignore = "explicit owned local Gateway root; read-only live catalog verification"]
async fn current_library_catalog_keeps_exact_native_projection() {
    let root = PathBuf::from(std::env::var("OO_OWNED_LIBRARY_ROOT").expect("owned root"));
    assert!(
        root.to_string_lossy()
            .contains("/mysql-layered-handoff-20261030/owned-02/outlet/gateway")
    );
    let read = |name: &str| -> Value {
        serde_json::from_slice(&std::fs::read(root.join(name)).unwrap()).unwrap()
    };
    let library =
        oo_gateway::library_policy::compile_library(&read("reviewed/library/library.json"))
            .unwrap();
    let config = read("config.json");
    let endpoint = read("data.json");
    assert_eq!(endpoint["host"], "127.0.0.1");
    assert_eq!(endpoint["port"], 25487);
    let target = oo_gateway::TlsEndpoint {
        host: "127.0.0.1".into(),
        port: 25487,
        database: endpoint["database"].as_str().unwrap().into(),
        user: config["data_user"].as_str().unwrap().into(),
    };
    assert!(target.database.starts_with("handoff_target_"));
    let password = std::fs::read_to_string(root.join("data.password")).unwrap();
    let ca = std::fs::read(root.join("ca.pem")).unwrap();
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let (mut client, connection) = oo_gateway::connect_require_tls(&target, password.trim(), &ca)
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    let policies = library
        .policies
        .iter()
        .map(oo_gateway::policy::compile_column_acl_native)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let identity = PolicyIdentity {
        project: library.project,
        role: library.role,
        policy_id: library.config_digest,
    };
    let limits = CatalogLimits {
        max_tables: library.catalog["limits"]["max_tables"].as_u64().unwrap() as u32,
        max_columns: library.catalog["limits"]["max_columns"].as_u64().unwrap() as u32,
        max_functions: oo_gateway::policy::catalog::FUNCTION_CATALOG_LIMIT,
        max_operators: 128,
        large_table_row_threshold: 1_000_000_000,
        unknown_rows: UnknownRows::Zero,
    };
    let result = load_policy_catalog_for_source_vector(
        &mut client,
        &identity,
        &policies,
        &limits,
        "",
        Some(&library.vector),
        Some(&library.catalog),
    )
    .await;
    eprintln!("actual_catalog={result:?}");
    assert_eq!(result.unwrap().relations.len(), 2);
    drop(client);
    driver.abort();
}
