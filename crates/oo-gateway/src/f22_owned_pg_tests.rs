//! Opt-in mechanism acceptance on a new local cluster, never a business oracle.
use super::*;
use crate::http_gateway::{CurrentTuple, Endpoint, IssueGrant, TokenKind};
use crate::policy::LogicalTable;
use crate::publication_binding::{ACTUAL, ACTUAL_VECTOR, EXPECTED, EXPECTED_VECTOR};
use axum::body::to_bytes;
use axum::http::Request;
use serde_json::json;
use tower::ServiceExt;

const SQL: &str = "SELECT amount FROM app.items";
const ROLE: &str = "oo_r_f22";

struct Fixture {
    root: PathBuf,
    password: String,
    ca: Vec<u8>,
    endpoint: TlsEndpoint,
    admin: tokio_postgres::Client,
    driver: BindingDriver,
    plane: Arc<Plane>,
    pack_digest: String,
    physical: String,
    column: String,
    state: AppState,
    token: String,
    token_hash: String,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_dates(false).await
    }

    async fn with_dates(dates: bool) -> Self {
        let path = std::env::var("OO_F22_OWNED_FIXTURE").expect("run tests/f22_owned_pg.py");
        let config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(config["owned_fixture"], "f22-owned/v1");
        assert_eq!(config["host"], "127.0.0.1");
        assert_eq!(config["database"], "f22_owned");
        let root = PathBuf::from(config["work"].as_str().unwrap());
        assert!(
            root.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("f22-owned-")
        );
        let pid: i64 = fs::read_to_string(root.join("pg/postmaster.pid"))
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(config["postmaster_pid"], pid);
        let password = fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
        let ca = fs::read(config["ca_file"].as_str().unwrap()).unwrap();
        let endpoint = TlsEndpoint {
            host: "127.0.0.1".into(),
            port: config["port"].as_u64().unwrap() as u16,
            database: "f22_owned".into(),
            user: "postgres".into(),
        };
        let (admin, connection) = connect_require_tls(&endpoint, &password, &ca)
            .await
            .unwrap();
        let driver = BindingDriver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        assert_eq!(
            admin
                .query_one("SELECT current_database()", &[])
                .await
                .unwrap()
                .get::<_, String>(0),
            "f22_owned"
        );
        let physical = format!("pt_{}", "a".repeat(60));
        let column = format!("pc_{}", "b".repeat(60));
        let pack_digest = pack_with_dates(&root, dates);
        let other_pack = dates.then(|| pack_for_role(&root, dates, "oo_r_f22_other"));
        let packs = PackCatalog::load_dir(&root.join("packs")).unwrap();
        admin.batch_execute(&format!(
            "CREATE SCHEMA _oo; CREATE SCHEMA oo_publish_target;
             CREATE TABLE _oo.{physical}({column} int8, secret text);
             INSERT INTO _oo.{physical} VALUES (10,'hidden');
             CREATE FUNCTION public.f22_read_probe() RETURNS boolean LANGUAGE plpgsql VOLATILE AS
                 $probe$ BEGIN RAISE LOG 'F22_USER_ROW_READ'; RETURN true; END $probe$;
             ALTER TABLE _oo.{physical} ENABLE ROW LEVEL SECURITY;
             ALTER TABLE _oo.{physical} FORCE ROW LEVEL SECURITY;
             CREATE POLICY f22_read_probe ON _oo.{physical} FOR SELECT TO {ROLE} USING (public.f22_read_probe());
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
             INSERT INTO _oo.table_freshness VALUES('f22','app','items','{physical}',1,1,'ready',NULL,now());
             CREATE TABLE oo_publish_target.fence(role_name text,schema_name text,relation_name text,
                 state text,opened_pack_digest text,opened_version text);
             INSERT INTO oo_publish_target.fence VALUES('{ROLE}','_oo','{physical}','open','{pack_digest}','release1');
             GRANT USAGE ON SCHEMA _oo TO {ROLE},oo_f22_login;
             GRANT USAGE ON SCHEMA oo_publish_target TO oo_f22_login;
             GRANT SELECT ({column}) ON _oo.{physical} TO {ROLE};
             GRANT SELECT ON _oo.physical_mappings,_oo.table_freshness,oo_publish_target.fence TO oo_f22_login;
             ALTER ROLE oo_f22_login SET default_transaction_read_only=on;
             REVOKE ALL ON FUNCTION pg_catalog.pg_advisory_lock(bigint),pg_catalog.pg_advisory_xact_lock(bigint) FROM PUBLIC;",
             digest="c".repeat(64))).await.unwrap();
        let mut policy = ColumnAclPolicy {
            project: "f22".into(),
            config_digest: "c".repeat(64),
            generation: 1,
            role: "f22".into(),
            login: "oo_f22_login".into(),
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
        if dates {
            let date_column = format!("pc_{}", "c".repeat(60));
            admin.batch_execute(&format!("ALTER TABLE _oo.{physical} ADD COLUMN {date_column} timestamp;
                CREATE OR REPLACE FUNCTION public.f22_read_probe() RETURNS boolean LANGUAGE plpgsql VOLATILE AS
                    $fn$ BEGIN RETURN true; END $fn$;
                TRUNCATE _oo.{physical};
                INSERT INTO _oo.{physical} SELECT CASE WHEN i%13=0 THEN NULL WHEN i%11=0 THEN -i ELSE 9007199254740993+i END,
                    'hidden', TIMESTAMP '2026-10-20' + (i%11)*interval '1 day' FROM generate_series(1,140000) i;
                INSERT INTO _oo.physical_mappings VALUES('f22','{digest}',1,'c','app','items','event_time','{date_column}');
                GRANT SELECT ({date_column}) ON _oo.{physical} TO {ROLE}; ANALYZE _oo.{physical};", digest="c".repeat(64)))
                .await.unwrap();
            policy.table.columns.push(ColumnSpec {
                name: "event_time".into(),
                status: ClassStatus::Confirmed,
                semantic_type: "event_time".into(),
                pii: PiiClass::None,
            });
        }
        if let Some(pack) = &other_pack {
            admin.batch_execute(&format!("CREATE ROLE oo_r_f22_other NOLOGIN NOINHERIT;
                GRANT oo_r_f22_other TO oo_f22_login WITH INHERIT FALSE, SET TRUE;
                GRANT USAGE ON SCHEMA _oo TO oo_r_f22_other;
                GRANT SELECT ({column},pc_{date_suffix}) ON _oo.{physical} TO oo_r_f22_other;
                CREATE POLICY other_read ON _oo.{physical} FOR SELECT TO oo_r_f22_other USING (true);
                INSERT INTO oo_publish_target.fence VALUES('oo_r_f22_other','_oo','{physical}','open','{pack}','release1');",
                date_suffix="c".repeat(60))).await.unwrap();
        }
        let mut runtime_endpoint = endpoint.clone();
        runtime_endpoint.user = "oo_f22_login".into();
        fs::create_dir(root.join("wal")).unwrap();
        let plane = Arc::new(Plane {
            deployment_mode: oo_core::config::DeploymentMode::AllInOne,
            query_timeout_ms: 5_000,
            query_capacity: Some(tokio::sync::Semaphore::new(2)),
            endpoint: runtime_endpoint,
            password: password.clone(),
            ca_pem: ca.clone(),
            connector: rustls_connector(&ca).unwrap(),
            roles: ReviewedRoles::fixed({
                let published = Published {
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
                };
                let mut roles = vec![published];
                if dates {
                    let mut other = roles[0].clone();
                    other.role = "oo_r_f22_other".into();
                    for policy in &mut other.policies {
                        policy.role = "f22_other".into();
                    }
                    roles.push(other);
                }
                roles
            }),
            wal_dir: root.join("wal"),
            collector_dir: None,
            sessions: Mutex::new(HashMap::new()),
            opening: Mutex::new(()),
            inflight: Mutex::new(HashMap::new()),
        });
        let auth_endpoint = Endpoint {
            host: endpoint.host.clone(),
            port: endpoint.port,
            database: endpoint.database.clone(),
        };
        crate::http_gateway::harness_install(auth_endpoint.clone(), &password, &ca)
            .await
            .unwrap();
        let (token, token_hash) = crate::http_gateway::harness_issue(
            auth_endpoint,
            &password,
            &ca,
            IssueGrant {
                kind: TokenKind::Client,
                client_ttl_ms: None,
                current: tuple("alice"),
            },
        )
        .await
        .unwrap();
        let endpoint_path = root.join("endpoint.json");
        fs::write(
            &endpoint_path,
            json!({"host":endpoint.host,"port":endpoint.port,"database":endpoint.database})
                .to_string(),
        )
        .unwrap();
        let state = AppState::for_owned_binding_test(
            packs,
            endpoint_path,
            password.clone(),
            ca.clone(),
            plane.clone(),
        );
        Self {
            root,
            password,
            ca,
            endpoint,
            admin,
            driver,
            plane,
            pack_digest,
            physical,
            column,
            state,
            token,
            token_hash,
        }
    }

    async fn http(
        &self,
        method: &str,
        path: &str,
        sql: Option<&str>,
        id: Option<&str>,
        vector: Option<&str>,
        token: &str,
    ) -> (u16, HeaderMap, Value) {
        let body = sql
            .map(|sql| {
                if path == "/v1/query" {
                    json!({"sql":sql,"format":"json"})
                } else if path == "/v1/knowledge/search" {
                    json!({"q":sql})
                } else {
                    json!({"sql":sql})
                }
            })
            .unwrap_or_else(|| json!({}));
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json");
        if let Some(id) = id {
            request = request.header(EXPECTED, id);
        }
        if let Some(vector) = vector {
            request = request.header(EXPECTED_VECTOR, vector);
        }
        let response = crate::http_gateway::router(self.state.clone())
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            !text.contains(&self.token)
                && !text.contains(&self.password)
                && !text.contains("hidden")
        );
        (status, headers, serde_json::from_slice(&bytes).unwrap())
    }

    async fn query(&self, id: &str) -> (u16, HeaderMap, Value) {
        self.http("POST", "/v1/query", Some(SQL), Some(id), None, &self.token)
            .await
    }

    async fn publish_data(&self, amount: i64, generation: i64) {
        self.admin.batch_execute(&format!("BEGIN; UPDATE _oo.{} SET {}={amount};
            UPDATE _oo.table_freshness SET target_generation={generation},applied_at=clock_timestamp();
            UPDATE oo_publish_target.fence SET opened_version='release{generation}'; COMMIT",
            self.physical, self.column)).await.unwrap();
    }

    async fn user_calls(&self) -> i64 {
        // Rolled-back portals are not reliably charged to pg_stat_statements.
        // A volatile RLS probe logs actual row execution, never LIMIT 0 probes.
        fs::read_to_string(self.root.join("postgres.log"))
            .unwrap()
            .lines()
            .filter(|line| line.contains("LOG:  F22_USER_ROW_READ"))
            .count() as i64
    }
}

fn tuple(principal: &str) -> CurrentTuple {
    CurrentTuple {
        project_id: "f22".into(),
        principal_id: principal.into(),
        role: ROLE.into(),
        claims: vec![],
        policy_version: "pv1".into(),
        knowledge_version: "kv1".into(),
        audience_url: "https://gateway.loopback.invalid/v1".into(),
        sandbox_id: None,
        session_id: None,
    }
}

fn pack_with_dates(root: &Path, dates: bool) -> String {
    pack_for_role(root, dates, ROLE)
}

fn pack_for_role(root: &Path, dates: bool, role: &str) -> String {
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
    let mut visibility = json!({"protocol":"oo.builder.role-visibility/v1","project":"f22","ontology_version":"oos-1",
        "catalog_version":"cat-1","policy":{"id":"policy","version":"pv1"},"knowledge":{"id":"knowledge","version":"kv1"},
        "principal":"alice","role":role,"issued_for_compiler":"oo-builder/role-pack/v1",
        "datasets":[{"dataset":"app.items","columns":["amount"],"row_security":"none","global_stats":"denied"}],
        "limits":{"max_file_bytes":262144,"max_total_bytes":1048576,"max_entries":128}});
    if dates {
        effective["datasets"][0]["columns"].as_array_mut().unwrap().push(json!({
            "name":"event_time", "data_type":"timestamp", "semantic_type":"event_time", "pii":"none",
            "classification":{"state":"confirmed","by":"fixture/test"},"provenance":"declared"}));
        visibility["datasets"][0]["columns"]
            .as_array_mut()
            .unwrap()
            .push(json!("event_time"));
    }
    let pack =
        oo_builder::compile_role_pack(&effective.to_string(), &visibility.to_string()).unwrap();
    fs::create_dir_all(root.join("packs")).unwrap();
    fs::write(root.join(format!("packs/{role}.json")), &pack.artifact).unwrap();
    pack.pack_digest
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "owned runtime preparation only; tests/f22_owned_pg.py --handoff"]
async fn f22_owned_pg_prepare_runtime() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new().await;
    fs::write(f.root.join("client.token"), &f.token).unwrap();
    fs::set_permissions(
        f.root.join("client.token"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let reviewed = f.root.join("reviewed/reader");
    fs::create_dir_all(&reviewed).unwrap();
    fs::write(reviewed.join("policy.json"), json!({
        "protocol":"oo.operator-preview.policy/v1","project":"f22","role":"f22",
        "login":"oo_f22_login","config_digest":"c".repeat(64),"generation":1,
        "table":{"namespace":"app","name":"items","columns":[
            {"name":"amount","semantic_type":"amount","status":"confirmed","pii":{"class":"none"}}
        ]}
    }).to_string()).unwrap();
    fs::write(
        reviewed.join("visibility.json"),
        json!({
            "protocol":"oo.builder.role-visibility/v1","project":"f22","role":ROLE,
            "knowledge":{"version":"kv1"},"policy":{"version":"pv1"},
            "datasets":[{"dataset":"app.items","columns":["amount"]}]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(f.root.join("config.json"),json!({
        "listen":"127.0.0.1:0","audience":"https://gateway.loopback.invalid/v1",
        "runtime_user":"oo_token_runtime","password_file":"password","ca_file":"ca.crt",
        "endpoint_file":"endpoint.json","pack_dir":"packs","body_limit":1048576,
        "request_timeout_ms":10000,"global_concurrency":2,"deployment_mode":"all_in_one",
        "data_endpoint_file":"endpoint.json","data_password_file":"password",
        "data_ca_file":"ca.crt","data_user":"oo_f22_login","wal_dir":"wal","reviewed_dir":"reviewed"
    }).to_string()).unwrap();
    crate::http_gateway::load_operator_dir(&f.root)
        .unwrap_or_else(|reason| panic!("operator config: {reason}"));
    fs::write(f.root.join("prepared.json"),json!({
        "protocol":"oo.f22.owned-runtime-prepared/v1","pack_digest":f.pack_digest,
        "query":"SELECT amount FROM app.items","initial_amount":10,
        "config_dir":f.root,"token_file":f.root.join("client.token"),
        "owner":"gateway F22; root may read, acquire bindings and use the controlled mutation/stop hook",
        "business_oracle":false
    }).to_string()).unwrap();
}

async fn cli(f: &Fixture, origin: &str, id: &str, vector: Option<&str>) -> (i32, Value) {
    use std::os::unix::fs::PermissionsExt;
    let binary = PathBuf::from(std::env::var("OO_F22_CLI").expect("runner builds the owned CLI"));
    assert!(binary.is_absolute() && binary.is_file());
    let token = f.root.join("cli-token");
    fs::write(&token, &f.token).unwrap();
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    let mut args: Vec<String> = [
        "query",
        "--json",
        "--gateway",
        origin,
        "--transport",
        "local-development",
        "--auth",
        "external",
        "--token-file",
        token.to_str().unwrap(),
        "--expected-publication-id",
        id,
        SQL,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if let Some(vector) = vector {
        args.extend(["--expected-data-vector-sha256".into(), vector.into()]);
    }
    let root = f.root.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(binary)
            .args(args)
            .env_clear()
            .env("HOME", &root)
            .current_dir(root)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains(&f.token) && !stdout.contains(&f.password));
    let value: Value = serde_json::from_str(&stdout).expect("CLI JSON receipt");
    (output.status.code().unwrap(), value)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "new isolated PG only: run tests/f22_owned_pg.py"]
async fn f22_owned_pg_gateway_and_executor() {
    let f = Fixture::new().await;
    metadata_snapshot_contract(&f).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = crate::http_gateway::router(f.state.clone());
    let _http = BindingDriver(tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    }));
    let (status, headers, first) = f.query("current").await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["rows"], json!([["10"]]));
    let id = headers[ACTUAL].to_str().unwrap().to_owned();
    let vector = headers[ACTUAL_VECTOR].to_str().unwrap().to_owned();
    let (exit, acquired) = cli(&f, &origin, "current", None).await;
    assert_eq!(exit, 0, "{acquired}");
    assert_eq!(acquired["publication_binding"]["publication_id"], id);
    assert_eq!(
        acquired["publication_binding"]["data_vector_sha256"],
        vector
    );
    fs::write(f.root.join("cli-acquire.json"), acquired.to_string()).unwrap();
    assert_eq!(cli(&f, &origin, &id, Some(&vector)).await.0, 0);
    assert_eq!(f.query(&id).await.0, 200);
    retired_idle_sessions(&f, &id, &vector).await;
    let calls = f.user_calls().await;
    assert!(calls >= 2, "real user SQL must execute");
    for (method, path, sql) in [
        ("GET", "/v1/tables", None),
        ("GET", "/v1/tables/app.items", None),
        ("POST", "/v1/sql/validate", Some(SQL)),
        ("POST", "/v1/knowledge/search", Some("items")),
        ("GET", "/v1/freshness", None),
    ] {
        let (status, h, body) = f
            .http(method, path, sql, Some(&id), Some(&vector), &f.token)
            .await;
        assert_eq!(status, 200, "{path}: {body}");
        assert_eq!(h[ACTUAL], id);
    }
    f.publish_data(20, 2).await;
    let (status, headers, drift) = f.query(&id).await;
    assert_eq!(status, 409, "{drift}");
    assert_eq!(drift["error"]["code"], "CONFLICT");
    assert_eq!(drift["error"]["retryable"], false);
    assert!(
        drift["error"]["query_id"]
            .as_str()
            .unwrap()
            .starts_with("q_")
    );
    assert!(
        drift["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("RECOMPUTE_WHOLE_QUESTION")
    );
    assert!(!headers.contains_key(ACTUAL));
    assert_eq!(
        f.user_calls().await,
        calls,
        "mismatch must not execute user SQL"
    );
    let (exit, refused) = cli(&f, &origin, &id, Some(&vector)).await;
    assert_eq!(exit, 2, "{refused}");
    assert_eq!(refused["code"], "CONFLICT");
    assert_eq!(refused["retryable"], false);
    assert!(refused["query_id"].as_str().unwrap().starts_with("q_"));
    assert!(refused.get("rows").is_none() && refused.get("publication_binding").is_none());
    assert_eq!(
        f.user_calls().await,
        calls,
        "real CLI drift cannot execute SQL"
    );
    fs::write(f.root.join("cli-drift.json"), refused.to_string()).unwrap();
    assert_eq!(
        f.http(
            "POST",
            "/v1/sql/validate",
            Some(SQL),
            Some(&id),
            None,
            &f.token
        )
        .await
        .0,
        409
    );
    assert_eq!(
        f.http(
            "GET",
            "/v1/tables/app.items",
            None,
            Some(&id),
            None,
            &f.token
        )
        .await
        .0,
        409
    );
    let (status, headers, second) = f.query("current").await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["rows"], json!([["20"]]));
    assert_eq!(first["knowledge_version"], second["knowledge_version"]);
    assert_eq!(first["policy_version"], second["policy_version"]);
    let id2 = headers[ACTUAL].to_str().unwrap().to_owned();
    let vector2 = headers[ACTUAL_VECTOR].to_str().unwrap().to_owned();
    assert_ne!(id, id2);
    assert_ne!(vector, headers[ACTUAL_VECTOR]);
    assert_eq!(f.query(&format!("pub1_{}", "0".repeat(64))).await.0, 409);
    assert_eq!(
        f.http(
            "POST",
            "/v1/query",
            Some(SQL),
            Some(&id2),
            Some(&"0".repeat(64)),
            &f.token
        )
        .await
        .0,
        409
    );
    assert_eq!(
        f.http(
            "POST",
            "/v1/query",
            Some(SQL),
            Some("current"),
            None,
            "ooct_forged"
        )
        .await
        .0,
        401
    );
    assert_eq!(
        f.http("POST", "/v1/query", Some(SQL), None, None, &f.token)
            .await
            .0,
        200
    );
    // C5 can reuse the generation and Pack: published data observations still change.
    f.admin
        .batch_execute(&format!(
            "BEGIN; UPDATE _oo.{} SET {}=21;
        UPDATE _oo.table_freshness SET applied_at=clock_timestamp(); COMMIT",
            f.physical, f.column
        ))
        .await
        .unwrap();
    let calls = f.user_calls().await;
    assert_eq!(f.query(&id2).await.0, 409);
    assert_eq!(f.user_calls().await, calls);
    let endpoint = Endpoint {
        host: f.endpoint.host.clone(),
        port: f.endpoint.port,
        database: f.endpoint.database.clone(),
    };
    let (other, _) = crate::http_gateway::harness_issue(
        endpoint,
        &f.password,
        &f.ca,
        IssueGrant {
            kind: TokenKind::Client,
            client_ttl_ms: None,
            current: tuple("bob"),
        },
    )
    .await
    .unwrap();
    let (_, latest, _) = f.query("current").await;
    assert_eq!(
        f.http(
            "POST",
            "/v1/query",
            Some(SQL),
            Some(latest[ACTUAL].to_str().unwrap()),
            None,
            &other
        )
        .await
        .0,
        409
    );
    let before_missing_pack = f.user_calls().await;
    let mut unpublished = tuple("alice");
    unpublished.knowledge_version = "kv2".into();
    let (unpublished_token, _) = crate::http_gateway::harness_issue(
        Endpoint {
            host: f.endpoint.host.clone(),
            port: f.endpoint.port,
            database: f.endpoint.database.clone(),
        },
        &f.password,
        &f.ca,
        IssueGrant {
            kind: TokenKind::Client,
            client_ttl_ms: None,
            current: unpublished,
        },
    )
    .await
    .unwrap();
    let (unavailable, headers, body) = f
        .http(
            "POST",
            "/v1/query",
            Some(SQL),
            Some(latest[ACTUAL].to_str().unwrap()),
            None,
            &unpublished_token,
        )
        .await;
    assert_eq!(unavailable, 409, "{body}");
    assert_eq!(body["error"]["code"], "CONFLICT");
    assert!(!headers.contains_key(ACTUAL) && body.get("rows").is_none());
    assert_eq!(f.user_calls().await, before_missing_pack);
    assert_eq!(
        f.http(
            "POST",
            "/v1/query",
            Some(SQL),
            None,
            None,
            &unpublished_token
        )
        .await
        .0,
        400
    );
    let before_schema = f.user_calls().await;
    f.admin
        .batch_execute(&format!(
            "ALTER TABLE _oo.{} ALTER COLUMN {} TYPE int4",
            f.physical, f.column
        ))
        .await
        .unwrap();
    assert_eq!(f.query(latest[ACTUAL].to_str().unwrap()).await.0, 409);
    assert_eq!(f.user_calls().await, before_schema);
    f.admin
        .batch_execute(&format!(
            "ALTER TABLE _oo.{} ALTER COLUMN {} TYPE int8",
            f.physical, f.column
        ))
        .await
        .unwrap();
    // Fence semantics can change while knowledge and policy labels remain fixed.
    f.admin
        .batch_execute("UPDATE oo_publish_target.fence SET opened_version='semantic2'")
        .await
        .unwrap();
    assert_eq!(f.query(&id2).await.0, 409);
    let (_, h, _) = f.query("current").await;
    let id3 = h[ACTUAL].to_str().unwrap().to_owned();
    let before_revoke = f.user_calls().await;
    f.admin
        .batch_execute(&format!(
            "REVOKE SELECT ({}) ON _oo.{} FROM {ROLE}",
            f.column, f.physical
        ))
        .await
        .unwrap();
    let (denied, headers, body) = f.query(&id3).await;
    assert_ne!(denied, 200);
    assert!(!headers.contains_key(ACTUAL) && body.get("rows").is_none());
    assert_eq!(f.user_calls().await, before_revoke);
    f.admin
        .batch_execute(&format!(
            "GRANT SELECT ({}) ON _oo.{} TO {ROLE}",
            f.column, f.physical
        ))
        .await
        .unwrap();

    executor_races(&f).await;

    // A cached successful unbound lookup must not defeat revocation on a bound call.
    crate::http_gateway::harness_revoke_token(
        Endpoint {
            host: f.endpoint.host.clone(),
            port: f.endpoint.port,
            database: f.endpoint.database.clone(),
        },
        &f.password,
        &f.ca,
        "f22".into(),
        f.token_hash.clone(),
    )
    .await
    .unwrap();
    assert_eq!(f.query("current").await.0, 401);
    let (_, headers, failed) = f.query("current").await;
    assert!(!headers.contains_key(ACTUAL));
    assert!(failed.get("rows").is_none());
    fs::write(f.root.join("acceptance.json"), json!({
        "protocol":"oo.f22.owned-acceptance/v1","same_version":true,"data_only_drift":true,
        "unchanged_knowledge":true,"unchanged_policy":true,"pre_sql_refusal":true,
        "real_gateway_authentication":true,"revocation_bypasses_positive_cache":true,
        "actual_cli_over_loopback_http_to_owned_pg":true,
        "first": {"publication_id":id,"data_vector_sha256":vector},
        "second": {"publication_id":id2,"data_vector_sha256":vector2},
        "data_only_same_generation_and_pack_refused":true,"cross_principal_binding_refused":true,
        "physical_schema_drift_without_label_change_refused":true,
        "unavailable_old_pack_is_explicit_drift_without_sql":true,
        "scope":"local mechanism fixture, not customer/business/public acceptance"
    }).to_string()).unwrap();
    let _ = &f.driver;
}

async fn metadata_snapshot_contract(f: &Fixture) {
    let caller = PublicCaller {
        tuple: tuple("alice"),
        scope: "client",
        auth_remaining_ms: 0,
        auth_cache_hit: false,
    };
    let binding = f
        .plane
        .binding_check(
            &f.plane.roles.snapshot().unwrap()[0],
            &caller,
            &f.pack_digest,
            None,
            Expected {
                publication_id: "current".into(),
                data_vector_sha256: None,
            },
        )
        .unwrap_or_else(|_| panic!("metadata binding"));
    let (mut client, connection) = f.plane.connect().await.unwrap();
    let _driver = BindingDriver(tokio::spawn(async move {
        let _ = connection.await;
    }));
    for (isolation, read_only) in [
        (tokio_postgres::IsolationLevel::ReadCommitted, true),
        (tokio_postgres::IsolationLevel::RepeatableRead, false),
    ] {
        let tx = client
            .build_transaction()
            .isolation_level(isolation)
            .read_only(read_only)
            .start()
            .await
            .unwrap();
        assert!(
            binding.metadata_scope(&tx).await.is_err(),
            "wrong snapshot mode was admitted"
        );
        tx.rollback().await.unwrap();
    }
    let tx = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .unwrap();
    let (receipt, catalog) = binding.metadata_scope(&tx).await.unwrap();
    let query_binding = f
        .plane
        .binding_check(
            &f.plane.roles.snapshot().unwrap()[0],
            &caller,
            &f.pack_digest,
            Some(&catalog),
            Expected {
                publication_id: "current".into(),
                data_vector_sha256: None,
            },
        )
        .unwrap_or_else(|_| panic!("query binding"));
    assert_eq!(receipt, query_binding.verify(&tx).await.unwrap());
    assert_eq!(
        receipt,
        query_binding.reference_verify(&tx).await.unwrap(),
        "single-snapshot metadata must preserve the original binding bytes"
    );
    assert!(
        query_binding.metadata_scope(&tx).await.is_err(),
        "metadata must not reuse a catalog admitted outside its snapshot"
    );
    tx.rollback().await.unwrap();
    fs::write(f.root.join("metadata-single-snapshot.json"), json!({
        "actual_owned_pg":true,"wrong_isolation_refused":true,"read_write_refused":true,
        "catalog_loaded_inside_response_snapshot":true,"binding_matches_query_and_original_reference":true,
        "pre_admitted_catalog_not_accepted_for_metadata":true
    }).to_string()).unwrap();
}

struct TestAudit {
    mutation: Option<(tokio_postgres::Client, BindingDriver, String)>,
    fail: bool,
}

impl AuditAdmission for TestAudit {
    async fn admit(&mut self, _: &Admission<'_>) -> Result<(), ExecError> {
        if self.fail {
            return Err(ExecError::AuditUnavailable);
        }
        if let Some((client, _, sql)) = &self.mutation {
            client.batch_execute(sql).await.unwrap();
        }
        Ok(())
    }
    async fn complete(&mut self, _: &CompletionRecord) -> Result<(), ExecError> {
        Ok(())
    }
}

fn quiet_audit() -> TestAudit {
    TestAudit {
        mutation: None,
        fail: false,
    }
}

async fn start(
    f: &Fixture,
    session: &mut DedicatedSession,
    catalog: &crate::PolicyCatalog,
    sql: &str,
    binding: BindingCheck,
    audit: TestAudit,
) -> Result<SelectStream, ExecError> {
    if !session.is_open() {
        let (client, connection) = f.plane.connect().await.unwrap();
        *session = DedicatedSession::bind(client, connection, f.plane.connector.clone())
            .await
            .unwrap();
    }
    session
        .execute_bound_select(
            SelectRequest {
                identity: &catalog.identity,
                catalog,
                original_sql: sql,
                role: ROLE,
                max_rows: 2000,
                max_result_bytes: MAX_QUERY_BYTES,
                wall_clock: Duration::from_secs(5),
                read_quality: false,
                quality_vector: None,
            },
            audit,
            Some(binding),
        )
        .await
}

fn check(f: &Fixture, catalog: &crate::PolicyCatalog, id: &str) -> BindingCheck {
    f.plane
        .binding_check(
            &f.plane.roles.snapshot().unwrap()[0],
            &PublicCaller {
                tuple: tuple("alice"),
                scope: "client",
                auth_remaining_ms: 0,
                auth_cache_hit: false,
            },
            &f.pack_digest,
            Some(catalog),
            Expected {
                publication_id: id.into(),
                data_vector_sha256: None,
            },
        )
        .unwrap_or_else(|_| panic!("binding"))
}

async fn no_locks(f: &Fixture, session: &DedicatedSession) {
    let row = f.admin.query_one(
        "SELECT (SELECT count(*) FROM pg_locks WHERE pid=$1 AND relation=('_oo.'||$2)::regclass), \
         (SELECT state FROM pg_stat_activity WHERE pid=$1)",
        &[&session.backend_pid(), &f.physical]).await.unwrap();
    assert_eq!(row.get::<_, i64>(0), 0, "binding relation lock leaked");
    assert_ne!(
        row.get::<_, Option<String>>(1).as_deref(),
        Some("idle in transaction")
    );
}

async fn retired_idle_sessions(f: &Fixture, id: &str, vector: &str) {
    let mut cases = Vec::new();
    for _ in 0..3 {
        let old = f.plane.session_for(ROLE).await.unwrap();
        let pid = old.lock().await.backend_pid();
        let calls = f.user_calls().await;
        assert!(
            f.admin
                .query_one("SELECT pg_terminate_backend($1,5000)", &[&pid])
                .await
                .unwrap()
                .get::<_, bool>(0)
        );
        let (status, headers, answer) = f
            .http(
                "POST",
                "/v1/query",
                Some(SQL),
                Some(id),
                Some(vector),
                &f.token,
            )
            .await;
        assert_eq!(
            status, 200,
            "first query after backend retirement: {answer}"
        );
        assert_eq!(answer["rows"], json!([["10"]]));
        assert_eq!(headers[ACTUAL], id);
        assert_eq!(headers[ACTUAL_VECTOR], vector);
        assert_eq!(f.user_calls().await, calls + 1, "no user SQL replay");
        let current = f.plane.session_for(ROLE).await.unwrap();
        let next_pid = current.lock().await.backend_pid();
        assert!(!Arc::ptr_eq(&old, &current));
        assert!(!old.lock().await.is_open());
        assert_ne!(next_pid, pid);
        cases.push(json!({"terminated_pid":pid,"replacement_pid":next_pid,
            "first_request_status":status,"user_sql_calls":1,"same_publication":true}));
    }
    fs::write(
        f.root.join("retired-idle-sessions.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

async fn executor_races(f: &Fixture) {
    let (_, headers, _) = f.query("current").await;
    let id = headers[ACTUAL].to_str().unwrap();
    let catalog = f
        .plane
        .catalog_for(&f.plane.roles.snapshot().unwrap()[0])
        .await
        .unwrap_or_else(|_| panic!("catalog"));
    let session = f.plane.session_for(ROLE).await.unwrap();
    let mut session = session.lock().await;
    let calls = f.user_calls().await;

    // Deterministically publish AFTER guard/catalog admission but BEFORE BEGIN.
    let (writer, connection) = connect_require_tls(&f.endpoint, &f.password, &f.ca)
        .await
        .unwrap();
    let driver = BindingDriver(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let mutation = format!(
        "BEGIN; UPDATE _oo.{} SET {}=30;
        UPDATE _oo.table_freshness SET target_generation=3;
        UPDATE oo_publish_target.fence SET opened_version='release3'; COMMIT",
        f.physical, f.column
    );
    let mut stream = start(
        f,
        &mut session,
        &catalog,
        SQL,
        check(f, &catalog, id),
        TestAudit {
            mutation: Some((writer, driver, mutation)),
            fail: false,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        stream.next_row().await,
        Err(ExecError::Protocol {
            step: "publication_drift"
        })
    ));
    assert!(
        stream.result_columns().is_empty(),
        "drift must precede user prepare"
    );
    drop(stream);
    session.wait_idle().await.unwrap();
    assert_eq!(f.user_calls().await, calls);
    no_locks(f, &session).await;

    // Real ACL revocation between admission and snapshot, not a fake policy label.
    let before_revoke = f.user_calls().await;
    let (writer, connection) = connect_require_tls(&f.endpoint, &f.password, &f.ca)
        .await
        .unwrap();
    let driver = BindingDriver(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let mutation = format!(
        "REVOKE SELECT ({}) ON _oo.{} FROM {ROLE}",
        f.column, f.physical
    );
    let mut denied = start(
        f,
        &mut session,
        &catalog,
        SQL,
        check(f, &catalog, "current"),
        TestAudit {
            mutation: Some((writer, driver, mutation)),
            fail: false,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        denied.next_row().await,
        Err(ExecError::Protocol {
            step: "publication_drift"
        })
    ));
    assert!(denied.result_columns().is_empty());
    drop(denied);
    session.wait_idle().await.unwrap();
    assert_eq!(f.user_calls().await, before_revoke);
    no_locks(f, &session).await;
    f.admin
        .batch_execute(&format!(
            "GRANT SELECT ({}) ON _oo.{} TO {ROLE}",
            f.column, f.physical
        ))
        .await
        .unwrap();

    // Policy/knowledge changes are part of the identity, even with the same data.
    for semantic in [true, false] {
        let mut acquire = start(
            f,
            &mut session,
            &catalog,
            SQL,
            check(f, &catalog, "current"),
            quiet_audit(),
        )
        .await
        .unwrap();
        while acquire.next_row().await.unwrap().is_some() {}
        let receipt = acquire.publication().unwrap();
        drop(acquire);
        session.wait_idle().await.unwrap();
        let mut binding = check(f, &catalog, &receipt.publication_id);
        if semantic {
            binding.knowledge_version = "kv2".into();
        } else {
            binding.policy_version = "pv2".into();
        }
        let mut denied = start(f, &mut session, &catalog, SQL, binding, quiet_audit())
            .await
            .unwrap();
        assert!(matches!(
            denied.next_row().await,
            Err(ExecError::Protocol {
                step: "publication_drift"
            })
        ));
        assert!(denied.result_columns().is_empty());
        drop(denied);
        session.wait_idle().await.unwrap();
    }

    // DML publication after snapshot capture cannot change the active portal.
    f.admin
        .batch_execute(&format!(
            "INSERT INTO _oo.{} SELECT n,'hidden' FROM generate_series(31,50) n;
        UPDATE _oo.table_freshness SET target_generation=4",
            f.physical
        ))
        .await
        .unwrap();
    let mut stream = start(
        f,
        &mut session,
        &catalog,
        SQL,
        check(f, &catalog, "current"),
        quiet_audit(),
    )
    .await
    .unwrap();
    let first = stream.next_row().await.unwrap().unwrap();
    let held = stream.publication().unwrap();
    let mut rows = vec![first];
    let lock_count: i64 = f.admin.query_one(
        "SELECT count(*) FROM pg_locks WHERE pid=$1 AND relation=('_oo.'||$2)::regclass AND granted",
        &[&session.backend_pid(), &f.physical]).await.unwrap().get(0);
    assert!(lock_count > 0);
    f.publish_data(100, 5).await;
    while let Some(row) = stream.next_row().await.unwrap() {
        rows.push(row);
    }
    assert_eq!(rows.len(), 21);
    assert!(
        !rows.iter().any(|row| row == &vec![Cell::Int("100".into())]),
        "mixed MVCC snapshot"
    );
    assert_eq!(stream.publication().unwrap(), held);
    drop(stream);
    session.wait_idle().await.unwrap();
    no_locks(f, &session).await;
    let mut refused = start(
        f,
        &mut session,
        &catalog,
        SQL,
        check(f, &catalog, &held.publication_id),
        quiet_audit(),
    )
    .await
    .unwrap();
    assert!(matches!(
        refused.next_row().await,
        Err(ExecError::Protocol {
            step: "publication_drift"
        })
    ));
    assert!(refused.result_columns().is_empty());
    drop(refused);
    session.wait_idle().await.unwrap();

    // An error and consumer cancellation release both the role transaction and locks.
    let mut failed = start(
        f,
        &mut session,
        &catalog,
        "SELECT amount / 0 FROM app.items",
        check(f, &catalog, "current"),
        quiet_audit(),
    )
    .await
    .unwrap();
    assert!(
        matches!(failed.next_row().await,Err(ExecError::Database {sqlstate}) if sqlstate=="22012")
    );
    drop(failed);
    session.wait_idle().await.unwrap();
    no_locks(f, &session).await;
    let mut canceled = start(
        f,
        &mut session,
        &catalog,
        SQL,
        check(f, &catalog, "current"),
        quiet_audit(),
    )
    .await
    .unwrap();
    assert!(canceled.next_row().await.unwrap().is_some());
    drop(canceled);
    session.wait_idle().await.unwrap();
    no_locks(f, &session).await;
    let mut explicit = start(
        f,
        &mut session,
        &catalog,
        SQL,
        check(f, &catalog, "current"),
        quiet_audit(),
    )
    .await
    .unwrap();
    assert!(explicit.next_row().await.unwrap().is_some());
    let cancel = explicit.cancel_handle();
    assert_eq!(
        cancel.cancel(&f.admin).await.unwrap(),
        crate::query_executor::CancelOutcome::Sent
    );
    drop(explicit);
    session.wait_idle().await.unwrap();
    no_locks(f, &session).await;
    assert_eq!(
        cancel.cancel(&f.admin).await.unwrap(),
        crate::query_executor::CancelOutcome::NotSent
    );
    if !session.is_open() {
        let (client, connection) = f.plane.connect().await.unwrap();
        *session = DedicatedSession::bind(client, connection, f.plane.connector.clone())
            .await
            .unwrap();
    }
    // Binding lock waits consume the existing SQL wall budget and clean up.
    let (mut writer, connection) = connect_require_tls(&f.endpoint, &f.password, &f.ca)
        .await
        .unwrap();
    let _driver = BindingDriver(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let tx = writer.transaction().await.unwrap();
    tx.batch_execute(&format!(
        "LOCK TABLE _oo.{} IN ACCESS EXCLUSIVE MODE",
        f.physical
    ))
    .await
    .unwrap();
    let before_timeout = f.user_calls().await;
    let started = Instant::now();
    let mut timed = session
        .execute_bound_select(
            SelectRequest {
                identity: &catalog.identity,
                catalog: &catalog,
                original_sql: SQL,
                role: ROLE,
                max_rows: 2000,
                max_result_bytes: MAX_QUERY_BYTES,
                wall_clock: Duration::from_millis(500),
                read_quality: false,
                quality_vector: None,
            },
            quiet_audit(),
            Some(check(f, &catalog, "current")),
        )
        .await
        .unwrap();
    assert!(matches!(
        timed.next_row().await,
        Err(ExecError::Timeout { .. })
    ));
    assert!(timed.result_columns().is_empty());
    drop(timed);
    session.wait_idle().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(f.user_calls().await, before_timeout);
    no_locks(f, &session).await;
    tx.rollback().await.unwrap();
    let fail = TestAudit {
        mutation: None,
        fail: true,
    };
    assert!(matches!(
        start(
            f,
            &mut session,
            &catalog,
            SQL,
            check(f, &catalog, "current"),
            fail
        )
        .await,
        Err(ExecError::AuditUnavailable)
    ));
    no_locks(f, &session).await;

    fs::write(f.root.join("executor-races.json"),json!({
        "pre_snapshot_cutover_refused_before_prepare":true,"active_portal_same_mvcc_snapshot":true,
        "real_acl_revocation_after_admission_no_user_prepare_or_execution":true,
        "data_only_cutover_after_snapshot_old_result_then_drift":true,"permission_and_semantic_identity":true,
        "consumer_cancellation_locks_released":true,"database_error_locks_released":true,
        "explicit_cancel_locks_released_and_stale_cancel_refused":true,
        "binding_lock_wait_uses_existing_timeout_budget_and_releases":true,
        "audit_failure_no_sql":true,"sql_allowlist_unchanged":true,"timeouts_unchanged":true
    }).to_string()).unwrap();
}

#[path = "query_lifecycle_owned_pg_tests.rs"]
mod lifecycle;
