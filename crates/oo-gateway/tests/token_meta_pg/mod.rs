//! Role-scoped meta reads the real publication fence and freshness snapshot.
use super::*;
use axum::http::Request;
use serde_json::json;
use tower::ServiceExt;
const ROLE: &str = "oo_r_meta";

#[tokio::test]
#[ignore = "owned PostgreSQL 17 fixture; token_owned_pg.py"]
async fn current_meta_is_bound_to_publication_and_freshness() {
    let root = PathBuf::from(std::env::var("OO_TOKEN_FIXTURE").expect("owned fixture"));
    let endpoint: Value =
        serde_json::from_slice(&fs::read(root.join("endpoint.json")).unwrap()).unwrap();
    let password = zeroize::Zeroizing::new(fs::read_to_string(root.join("admin.pass")).unwrap());
    let runtime_password =
        zeroize::Zeroizing::new(fs::read_to_string(root.join("runtime.pass")).unwrap());
    let ca = fs::read(root.join("ca.crt")).unwrap();
    let endpoint = TlsEndpoint {
        host: "localhost".into(),
        port: endpoint["port"].as_u64().unwrap() as u16,
        database: "meta".into(),
        user: "oo_token_admin".into(),
    };
    let (admin, connection) = connect_require_tls(&endpoint, &password, &ca)
        .await
        .unwrap();
    let _driver = BindingDriver(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let work = root.join("meta");
    fs::create_dir(&work).unwrap();
    let pack_digest = pack(&work);
    let packs = PackCatalog::load_dir(&work.join("packs")).unwrap();
    let physical = format!("pt_{}", "a".repeat(60));
    let column = format!("pc_{}", "b".repeat(60));
    admin.batch_execute(&format!(
        "CREATE SCHEMA _oo; CREATE SCHEMA oo_publish_target;
         CREATE TABLE _oo.{physical}({column} int8);
         CREATE TABLE _oo.physical_mappings(source text,config_digest text,run_generation int8,
             level text,logical_namespace text,logical_table text,logical_column text,physical_name text);
         INSERT INTO _oo.physical_mappings VALUES
             ('f22','{digest}',1,'t','app','items','','{physical}'),
             ('f22','{digest}',1,'c','app','items','amount','{column}');
         CREATE FUNCTION _oo.lookup_physical_mapping(text,text,int8,text,text,text,text) RETURNS text
         LANGUAGE sql STABLE AS $fn$ SELECT physical_name FROM _oo.physical_mappings
             WHERE source=$1 AND config_digest=$2 AND run_generation=$3
             AND level=left($4,1) AND logical_namespace=$5 AND logical_table=$6 AND logical_column=$7 $fn$;
         CREATE TABLE _oo.table_freshness(source text,schema_name text,table_name text,physical_name text,
             source_generation int8,target_generation int8,state text,source_committed_at timestamptz,applied_at timestamptz);
         INSERT INTO _oo.table_freshness VALUES('f22','app','items','{physical}',1,1,'live',NULL,now());
         CREATE TABLE oo_publish_target.fence(role_name text,schema_name text,relation_name text,
             state text,opened_pack_digest text,opened_version text);
         INSERT INTO oo_publish_target.fence VALUES('{ROLE}','_oo','{physical}','open','{pack_digest}','release1');
         GRANT USAGE ON SCHEMA _oo TO {ROLE};
         GRANT SELECT ({column}) ON _oo.{physical} TO {ROLE};", digest="c".repeat(64))).await.unwrap();
    let policy = ColumnAclPolicy {
        project: "f22".into(),
        config_digest: "c".repeat(64),
        generation: 1,
        role: "meta".into(),
        login: "oo_token_admin".into(),
        row_filter: None,
        table: LogicalTable {
            namespace: "app".into(),
            name: "items".into(),
            columns: vec![ColumnSpec {
                name: "amount".into(),
                status: ClassStatus::Confirmed,
                semantic_type: "amount".into(),
                pii: PiiClass::None,
            }],
        },
    };
    fs::create_dir(work.join("wal")).unwrap();
    let plane = Arc::new(Plane {
        deployment_mode: oo_core::config::DeploymentMode::AllInOne,
        query_timeout_ms: 5000,
        query_capacity: Some(tokio::sync::Semaphore::new(2)),
        endpoint: endpoint.clone(),
        password: password.to_string(),
        ca_pem: ca.clone(),
        connector: rustls_connector(&ca).unwrap(),
        roles: ReviewedRoles::fixed(vec![Published {
            mapping_source: None,
            policies: vec![policy],
            role: ROLE.into(),
            project: "f22".into(),
            policy_version: "pv1".into(),
            knowledge_version: "kv1".into(),
            config_digest: "c".repeat(64),
            native_vector: None,
            native_catalog: None,
            quality_vector: None,
        }]),
        wal_dir: work.join("wal"),
        collector_dir: None,
        sessions: Mutex::new(HashMap::new()),
        opening: Mutex::new(()),
        inflight: Mutex::new(HashMap::new()),
    });
    let auth_endpoint = crate::http_gateway::Endpoint {
        host: endpoint.host.clone(),
        port: endpoint.port,
        database: endpoint.database.clone(),
    };
    crate::http_gateway::harness_install(auth_endpoint.clone(), &password, &ca)
        .await
        .unwrap();
    let (token, _) = crate::http_gateway::harness_issue(
        auth_endpoint,
        &password,
        &ca,
        crate::http_gateway::IssueGrant {
            kind: crate::http_gateway::TokenKind::Client,
            client_ttl_ms: None,
            current: crate::http_gateway::CurrentTuple {
                project_id: "f22".into(),
                principal_id: "alice".into(),
                role: ROLE.into(),
                claims: vec![],
                policy_version: "pv1".into(),
                knowledge_version: "kv1".into(),
                audience_url: "https://gateway.loopback.invalid/v1".into(),
                sandbox_id: None,
                session_id: None,
            },
        },
    )
    .await
    .unwrap();
    let token = zeroize::Zeroizing::new(token);
    fs::write(
        work.join("endpoint.json"),
        json!({"host":endpoint.host,"port":endpoint.port,"database":endpoint.database}).to_string(),
    )
    .unwrap();
    let state = AppState::for_owned_binding_test(
        packs,
        work.join("endpoint.json"),
        runtime_password.to_string(),
        ca,
        plane,
    );
    let get = |path: &str| {
        let request = Request::builder()
            .uri(path)
            .header("authorization", format!("Bearer {}", token.as_str()))
            .header(crate::publication_binding::EXPECTED, "current")
            .body(Body::empty())
            .unwrap();
        crate::http_gateway::router(state.clone()).oneshot(request)
    };
    let response = get("/v1/meta").await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let value: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1048576)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status, 200, "{value}");
    assert!(
        value["publication_id"]
            .as_str()
            .unwrap()
            .starts_with("pub1_")
    );
    assert_eq!(
        value["publication_id"],
        headers[crate::publication_binding::ACTUAL]
            .to_str()
            .unwrap()
    );
    let response = get("/v1/freshness").await.unwrap();
    assert_eq!(response.status(), 200);
    let freshness: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1048576)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["freshness"], freshness);
    assert_eq!(freshness["source_time"], "unmeasured");
    admin.batch_execute("UPDATE oo_publish_target.fence SET opened_version='release2'; UPDATE _oo.table_freshness SET target_generation=2,state='awaiting_publish',applied_at=clock_timestamp()").await.unwrap();
    let response = get("/v1/meta").await.unwrap();
    assert_eq!(response.status(), 200);
    let changed: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1048576)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_ne!(value["publication_id"], changed["publication_id"]);
    assert_ne!(value["freshness"], changed["freshness"]);
    println!(
        "meta: current publication id follows real PostgreSQL fence; freshness matches authenticated snapshot and preserves source_time=unmeasured"
    );
}

fn pack(root: &Path) -> String {
    let mut effective = json!({"protocol":"oo.builder.effective-oos/v1","project":"f22","ontology_version":"oos-1",
        "catalog_version":"cat-1","policy":{"id":"policy","version":"pv1"},"knowledge":{"id":"knowledge","version":"kv1"},
        "ontology":{"spec_version":"oos/v1","name":"f22","locale":"en-US","include_schemas":["app"],
            "default_tier_preference":["raw"],"publish":{"include_unreviewed":[]},"classification":{"auto_accept":"none"},
            "lifecycle":{"deprecation_min_releases":1},"compile":{"per_role":true,"budgets":{"readme_tokens":2000,"table_file_tokens":3000},
                "sample_values":{"default":"none","max_per_column":0}}},
        "datasets":[{"schema_version":"oos/v1","dataset":"app.items","label":"Items","tier":"raw","primary_key":["amount"],
            "columns":[{"name":"amount","data_type":"bigint","semantic_type":"identifier","pii":"none",
                "classification":{"state":"confirmed","by":"fixture/test"},"provenance":"declared"}]}]});
    for name in [
        "relationships",
        "enums",
        "glossary",
        "entities",
        "metrics",
        "examples",
    ] {
        effective[name] = json!([]);
    }
    let visibility = json!({"protocol":"oo.builder.role-visibility/v1","project":"f22","ontology_version":"oos-1",
        "catalog_version":"cat-1","policy":{"id":"policy","version":"pv1"},"knowledge":{"id":"knowledge","version":"kv1"},
        "principal":"alice","role":ROLE,"issued_for_compiler":"oo-builder/role-pack/v1",
        "datasets":[{"dataset":"app.items","columns":["amount"],"row_security":"none","global_stats":"denied"}],
        "limits":{"max_file_bytes":262144,"max_total_bytes":1048576,"max_entries":128}});
    let pack =
        oo_builder::compile_role_pack(&effective.to_string(), &visibility.to_string()).unwrap();
    fs::create_dir(root.join("packs")).unwrap();
    fs::write(root.join("packs/reader.json"), &pack.artifact).unwrap();
    pack.pack_digest
}
