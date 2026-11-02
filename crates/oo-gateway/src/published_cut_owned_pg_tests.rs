//! Target-fixture C5 staging on a real owned PG and the existing F22 Gateway harness.
//! No C5/source ingestion API is simulated as a product acceptance claim.
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
    admin: tokio_postgres::Client,
    _driver: BindingDriver,
    plane: Arc<Plane>,
    pack_digest: String,
    physical: String,
    column: String,
    state: AppState,
    token: String,
    _token_hash: String,
}

impl Fixture {
    async fn new() -> Self {
        let path = std::env::var("OO_PUBLISHED_CUT_FIXTURE").expect("run tests/f22_owned_pg.py");
        let config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(config["owned_fixture"], "published-cut-owned/v1");
        assert_eq!(config["host"], "127.0.0.1");
        assert_eq!(config["database"], "published_cut_owned");
        let root = PathBuf::from(config["work"].as_str().unwrap());
        assert!(
            root.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("published-cut-owned-")
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
            database: "published_cut_owned".into(),
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
            "published_cut_owned"
        );
        let physical = format!("pt_{}", "a".repeat(60));
        let column = format!("pc_{}", "b".repeat(60));
        let pack_digest = pack(&root);
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
        let policy = ColumnAclPolicy {
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
            admin,
            _driver: driver,
            plane,
            pack_digest,
            physical,
            column,
            state,
            token,
            _token_hash: token_hash,
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

fn hash(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

impl Fixture {
    async fn install_native_fixture(&self) {
        // Minimal target metadata fixture, not a run of the source/C5 pipeline.
        self.admin.batch_execute("
            CREATE TABLE _oo.full_import_run(run_id uuid PRIMARY KEY,source_id text,
                source_generation int8,target_generation int8,config_digest text,schema_digest text,
                type_digest text,input_digest text,shard_id text,sealed_at timestamptz,state text);
            CREATE TABLE _oo.full_import_table(run_id uuid,schema_ident text,table_ident text,
                physical_name text,exhausted bool,applied_rows int8,applied_pos numeric,canonical_digest text);
            CREATE TABLE _oo.apply_receipts(source text,table_generation text,config_digest text,
                schema_digest text,type_digest text,fence_shard text,fence_generation int8,
                applied_pos numeric,position_codec text,position_lineage text,source_epoch int4,
                cross_table_consistent bool,partial_txn bool,target_commit_at timestamptz);
            CREATE TABLE _oo.full_import_quality(physical_name text,kind text,rejected_rows int8,accepted_rows int8);
            CREATE TABLE _oo.source_watermark(source text PRIMARY KEY,source_generation int8,
                target_generation int8,state text,applied_at timestamptz);
            CREATE TABLE _oo.table_quality(schema_name text,table_name text,capture_method text);
            CREATE TABLE _oo.native_library_publication(receipt_sha256 text PRIMARY KEY,
                library_id text,library jsonb,shards jsonb);
            CREATE TABLE _oo.native_library_head(library_id text PRIMARY KEY,receipt_sha256 text,
                manifest_sha256 text,state text);
            CREATE VIEW _oo.native_library_source_status AS
                SELECT s->>'source_id' AS source_id,s->>'receipt_sha256' AS receipt_sha256,
                    h.library_id,h.receipt_sha256 AS library_receipt_sha256
                FROM _oo.native_library_head h JOIN _oo.native_library_publication p USING(receipt_sha256)
                CROSS JOIN LATERAL jsonb_array_elements(p.shards) s;
            CREATE FUNCTION _oo.native_library_source_ready(text,text) RETURNS bool LANGUAGE sql STABLE AS
                $$ SELECT NOT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1 AND source_generation<>1) $$;
            CREATE FUNCTION _oo.fixture_immutable() RETURNS trigger LANGUAGE plpgsql AS
                $$ BEGIN RAISE EXCEPTION 'immutable receipt'; END $$;
            CREATE TRIGGER immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON _oo.native_library_publication
                FOR EACH STATEMENT EXECUTE FUNCTION _oo.fixture_immutable();
            GRANT SELECT ON ALL TABLES IN SCHEMA _oo,oo_publish_target TO oo_f22_login;
            GRANT SELECT ON _oo.physical_mappings,_oo.full_import_run,_oo.full_import_table,
                _oo.apply_receipts,_oo.full_import_quality,_oo.native_library_publication,
                _oo.native_library_head TO oo_r_f22;
        ").await.unwrap();
    }

    async fn stage(&self, generation: i64) -> Published {
        let physical = if generation == 1 {
            self.physical.clone()
        } else {
            format!("pt_{:060x}", generation)
        };
        let column = &self.column;
        let run = format!("00000000-0000-4000-8000-{generation:012}");
        let d = "c".repeat(64);
        if generation > 1 {
            self.admin
                .batch_execute(&format!(
                    "CREATE TABLE _oo.{physical}({column} int8,secret text);
                 INSERT INTO _oo.{physical} VALUES({},'hidden');",
                    generation * 10
                ))
                .await
                .unwrap();
        }
        self.admin.batch_execute(&format!(
            "INSERT INTO _oo.physical_mappings VALUES
                ('f22','{d}',{generation},'n','app','','','pn_{ns}');
             INSERT INTO _oo.full_import_run VALUES('{run}','f22',{generation},{generation},
                '{d}','{d}','{d}','{d}','fixture',now(),'awaiting_publish');
             INSERT INTO _oo.full_import_table VALUES('{run}','app','items','{physical}',true,1,0,'{d}');
             INSERT INTO _oo.apply_receipts VALUES('f22','{physical}','{d}','{d}','{d}','fixture',
                {generation},0,'snapshot-zero/v1','full-import.snapshot',0,false,false,
                '2026-10-20T00:00:0{generation}Z');
             INSERT INTO _oo.full_import_quality VALUES('{physical}','batch',0,1);
             INSERT INTO _oo.source_watermark VALUES('f22',{generation},{generation},'pending',now())
                ON CONFLICT(source) DO UPDATE SET source_generation=EXCLUDED.source_generation,
                    target_generation=EXCLUDED.target_generation,state='pending',applied_at=now();
             UPDATE _oo.table_freshness SET physical_name='{physical}',source_generation={generation},
                target_generation={generation},state='pending',source_committed_at='2099-01-01Z',applied_at=now();
             INSERT INTO _oo.table_quality VALUES('app','items','pending-poison');",
            ns="a".repeat(60)
        )).await.unwrap();
        if generation > 1 {
            self.admin
                .batch_execute(&format!(
                    "INSERT INTO _oo.physical_mappings VALUES
                ('f22','{d}',{generation},'t','app','items','','{physical}'),
                ('f22','{d}',{generation},'c','app','items','amount','{column}');"
                ))
                .await
                .unwrap();
        }
        let table = json!({"schema":"app","table":"items","run_id":run,"physical_name":physical,
            "source_generation":generation,"target_generation":generation,"accepted_rows":1,
            "canonical_digest":d,"applied_pos":"0","capture_method":"c5_diff",
            "delete_detection":"reconcile","consistency_grade":"B"});
        let receipt = json!({"protocol":"oo.pipeline.native-publication-receipt/v1",
            "collection_mode":"per_table","run_id":run,"run_ids":[run],"source_id":"f22",
            "source_generation":generation,"target_generation":generation,"position_codec":"snapshot-zero/v1",
            "position_lineage":"configured.mysql","applied_pos":"0","consistency":"per_table",
            "consistency_grade":"B","cross_table_consistent":false,"tables":[table]});
        let manifest = json!({"protocol":"oo.library.membership/v1","library_id":"fixture",
            "sources":[{"source_id":"f22","schema":"app","tables":["items"]}]});
        let library = json!({"protocol":"oo.pipeline.native-library-publication/v1",
            "library_id":"fixture","manifest_sha256":hash(&manifest),"manifest":manifest,
            "shards":["f22"],"expected_tables":1});
        let shards = json!([{"source_id":"f22","anchor_run":run,"receipt_sha256":hash(&receipt),"receipt":receipt}]);
        let sha = hash(&json!({"library":library,"shards":shards}));
        // Prepared evidence stays on disk until activate(), never in the active head.
        fs::write(
            self.root.join(format!("staged-{generation}.json")),
            json!({"library":library,"shards":shards,"sha":sha}).to_string(),
        )
        .unwrap();
        let vector = json!([{"dataset":"app.items","source_id":"f22","config_digest":d,
            "source_generation":generation,"target_generation":generation,"stored_namespace":"_oo",
            "stored_physical":physical,"run_id":run,"type_digest":d,"input_digest":d,
            "expected_columns":[{"name":"amount","data_type":"bigint"}]}]);
        let mut published = self.plane.roles.snapshot().unwrap()[0].clone();
        published.mapping_source = Some(String::new());
        published.policies[0].generation = generation;
        published.native_vector = Some(vector.clone());
        published.quality_vector = Some(vector.clone());
        published.native_catalog = Some(
            json!({"protocol":"oo.gateway.library-binding/v1","entries":vector,
            "library_id":"fixture","receipt_sha256":sha,"pack_digest":self.pack_digest,
            "limits":{"max_tables":64,"max_columns":512}}),
        );
        published
    }

    async fn activate(&self, published: Published) {
        let generation = published.policies[0].generation;
        let staged: Value = serde_json::from_slice(
            &fs::read(self.root.join(format!("staged-{generation}.json"))).unwrap(),
        )
        .unwrap();
        let vector = &published.native_vector.as_ref().unwrap()[0];
        self.admin.batch_execute(&format!(
            "BEGIN;
             UPDATE _oo.full_import_run SET state='published' WHERE run_id='{}';
             INSERT INTO _oo.native_library_publication VALUES('{}','fixture','{}'::jsonb,'{}'::jsonb);
             INSERT INTO _oo.native_library_head VALUES('fixture','{}','{}','ready')
                ON CONFLICT(library_id) DO UPDATE SET receipt_sha256=EXCLUDED.receipt_sha256,state='ready';
             GRANT SELECT ({}) ON _oo.{} TO {ROLE};
             INSERT INTO oo_publish_target.fence VALUES('{ROLE}','_oo','{}','open','{}','release{generation}')
                ;
             COMMIT;",
            vector["run_id"].as_str().unwrap(), staged["sha"].as_str().unwrap(),
            staged["library"],staged["shards"],staged["sha"].as_str().unwrap(),
            staged["library"]["manifest_sha256"].as_str().unwrap(),self.column,
            vector["stored_physical"].as_str().unwrap(),vector["stored_physical"].as_str().unwrap(),
            self.pack_digest,
        )).await.unwrap();
        // The test replaces the same reviewed-role cache after target activation,
        // analogous to reviewed file reload; neither Plane nor session is replaced.
        self.plane.roles.current.lock().unwrap().roles = Arc::new(vec![published]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "new owned PG only; tests/published_cut_owned_pg.py"]
async fn published_cut_survives_pending_generation() {
    let f = Fixture::new().await;
    f.install_native_fixture().await;
    f.admin
        .batch_execute("DELETE FROM oo_publish_target.fence")
        .await
        .unwrap();
    let one = f.stage(1).await;
    f.activate(one.clone()).await;
    let (status, headers, first) = f.query("current").await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["rows"], json!([["10"]]));
    let id = headers[ACTUAL].to_str().unwrap().to_owned();
    let vector = headers[ACTUAL_VECTOR].to_str().unwrap().to_owned();
    let session = f.plane.session_for(ROLE).await.unwrap();
    let before_freshness = f
        .http(
            "GET",
            "/v1/freshness",
            None,
            Some(&id),
            Some(&vector),
            &f.token,
        )
        .await;
    assert_eq!(before_freshness.0, 200, "{}", before_freshness.2);
    assert_eq!(before_freshness.2["source_time"], "unmeasured");
    assert_eq!(before_freshness.2["publication"], "live");
    let two = f.stage(2).await;
    assert!(
        !f.admin
            .query_one(
                "SELECT _oo.native_library_source_ready('f22','unused')",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let (status, after_headers, retained) = f.query(&id).await;
    assert_eq!(status, 200, "{retained}");
    assert_eq!(retained["rows"], first["rows"]);
    for key in [
        "sources",
        "warnings",
        "freshness",
        "knowledge_version",
        "policy_version",
        "hints",
    ] {
        assert_eq!(
            retained[key], first[key],
            "changed old-cut provenance: {key}"
        );
    }
    assert_eq!(after_headers[ACTUAL], id);
    assert_eq!(after_headers[ACTUAL_VECTOR], vector);
    assert!(Arc::ptr_eq(
        &session,
        &f.plane.session_for(ROLE).await.unwrap()
    ));
    let mut evidence = json!({"scope":"owned target-fixture; real PostgreSQL/TLS/Gateway/session; not source C5 or customer acceptance",
        "first":first,"retained":retained,"old_publication_id":id,"old_vector":vector});
    for (method, path, sql) in [
        ("GET", "/v1/tables", None),
        ("GET", "/v1/tables/app.items", None),
        ("POST", "/v1/sql/validate", Some(SQL)),
        ("POST", "/v1/knowledge/search", Some("items")),
        ("GET", "/v1/freshness", None),
    ] {
        let result = f
            .http(method, path, sql, Some(&id), Some(&vector), &f.token)
            .await;
        assert_eq!(result.0, 200, "{path}: {}", result.2);
        assert_eq!(result.1[ACTUAL], id);
        if path == "/v1/freshness" {
            assert_eq!(result.2, before_freshness.2);
            evidence["retained_freshness"] = result.2;
        }
    }
    let plain = f
        .http("POST", "/v1/query", Some(SQL), None, None, &f.token)
        .await;
    assert_eq!(plain.0, 200, "{}", plain.2);
    assert_eq!(plain.2["rows"], json!([["10"]]));
    let plain_freshness = f
        .http("GET", "/v1/freshness", None, None, None, &f.token)
        .await;
    assert_eq!(plain_freshness.0, 200, "{}", plain_freshness.2);
    assert_eq!(plain_freshness.2, before_freshness.2);
    f.admin
        .batch_execute("UPDATE oo_publish_target.fence SET state='closed'")
        .await
        .unwrap();
    let revoked_old = f.query("current").await;
    assert_ne!(revoked_old.0, 200, "revoked active gen1 fence admitted");
    evidence["revoked_old_fence"] = json!({"status":revoked_old.0,"body":revoked_old.2});
    f.admin
        .batch_execute("UPDATE oo_publish_target.fence SET state='open'")
        .await
        .unwrap();
    assert_eq!(f.query(&id).await.0, 200);
    // A cached plan is refused inside the transaction and may close that
    // session. Data-only publication must retain the recovered healthy one.
    let session = f.plane.session_for(ROLE).await.unwrap();
    f.activate(two.clone()).await;
    let second = f.query("current").await;
    assert_eq!(second.0, 200, "{}", second.2);
    assert_eq!(second.2["rows"], json!([["20"]]));
    assert_ne!(second.1[ACTUAL], id);
    assert_ne!(second.1[ACTUAL_VECTOR], vector);
    assert!(Arc::ptr_eq(
        &session,
        &f.plane.session_for(ROLE).await.unwrap()
    ));
    assert_eq!(f.query(&id).await.0, 409);
    let active_physical = two.native_vector.as_ref().unwrap()[0]["stored_physical"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut refusals = Vec::new();
    for (name, sql, undo) in [
        (
            "closed_fence",
            "UPDATE oo_publish_target.fence SET state='closed'".into(),
            "UPDATE oo_publish_target.fence SET state='open'".into(),
        ),
        (
            "wrong_fence_digest",
            "UPDATE oo_publish_target.fence SET opened_pack_digest=repeat('0',64)".into(),
            format!(
                "UPDATE oo_publish_target.fence SET opened_pack_digest='{}'",
                f.pack_digest
            ),
        ),
        (
            "foreign_fence_role",
            "UPDATE oo_publish_target.fence SET role_name='oo_r_foreign'".into(),
            format!("UPDATE oo_publish_target.fence SET role_name='{ROLE}'"),
        ),
        (
            "revoked_acl",
            format!(
                "REVOKE SELECT ({}) ON _oo.{} FROM {ROLE}",
                f.column, active_physical
            ),
            format!(
                "GRANT SELECT ({}) ON _oo.{} TO {ROLE}",
                f.column, active_physical
            ),
        ),
        (
            "retired_run",
            "UPDATE _oo.full_import_run SET state='retired' WHERE source_generation=2".into(),
            "UPDATE _oo.full_import_run SET state='published' WHERE source_generation=2".into(),
        ),
        (
            "wrong_input",
            "UPDATE _oo.full_import_run SET input_digest=repeat('0',64) WHERE source_generation=2"
                .into(),
            "UPDATE _oo.full_import_run SET input_digest=repeat('c',64) WHERE source_generation=2"
                .into(),
        ),
        (
            "wrong_type",
            "UPDATE _oo.full_import_run SET type_digest=repeat('0',64) WHERE source_generation=2"
                .into(),
            "UPDATE _oo.full_import_run SET type_digest=repeat('c',64) WHERE source_generation=2"
                .into(),
        ),
        (
            "wrong_schema",
            "UPDATE _oo.full_import_run SET schema_digest=repeat('0',64) WHERE source_generation=2"
                .into(),
            "UPDATE _oo.full_import_run SET schema_digest=repeat('c',64) WHERE source_generation=2"
                .into(),
        ),
        (
            "wrong_generation",
            "UPDATE _oo.full_import_run SET target_generation=99 WHERE source_generation=2".into(),
            "UPDATE _oo.full_import_run SET target_generation=2 WHERE source_generation=2".into(),
        ),
        (
            "wrong_table_digest",
            "UPDATE _oo.full_import_table SET canonical_digest=repeat('0',64)".into(),
            "UPDATE _oo.full_import_table SET canonical_digest=repeat('c',64)".into(),
        ),
        (
            "closed_head",
            "UPDATE _oo.native_library_head SET state='closed'".into(),
            "UPDATE _oo.native_library_head SET state='ready'".into(),
        ),
        (
            "pending_head",
            "UPDATE _oo.native_library_head SET state='pending'".into(),
            "UPDATE _oo.native_library_head SET state='ready'".into(),
        ),
        (
            "ddl_type",
            format!(
                "ALTER TABLE _oo.{} ALTER COLUMN {} TYPE text",
                active_physical, f.column
            ),
            format!(
                "ALTER TABLE _oo.{} ALTER COLUMN {} TYPE bigint USING {}::bigint",
                active_physical, f.column, f.column
            ),
        ),
    ] {
        f.admin.batch_execute(&sql).await.unwrap();
        let denied = f.query("current").await;
        assert_ne!(denied.0, 200, "accepted {name}: {}", denied.2);
        refusals.push(json!({"name":name,"status":denied.0,"body":denied.2}));
        f.admin.batch_execute(&undo).await.unwrap();
    }
    // A retained gen1 vector cannot impersonate the now-active gen2 receipt.
    let mut forged = one.clone();
    forged.native_catalog.as_mut().unwrap()["receipt_sha256"] =
        two.native_catalog.as_ref().unwrap()["receipt_sha256"].clone();
    f.plane.roles.current.lock().unwrap().roles = Arc::new(vec![forged]);
    assert_ne!(f.query("current").await.0, 200);
    let mut wrong_pack = two.clone();
    wrong_pack.native_catalog.as_mut().unwrap()["pack_digest"] = json!("0".repeat(64));
    f.plane.roles.current.lock().unwrap().roles = Arc::new(vec![wrong_pack]);
    assert_ne!(f.query("current").await.0, 200);
    f.plane.roles.current.lock().unwrap().roles = Arc::new(vec![two]);
    // An unregistered replacement view must not be admitted as a sealed table.
    f.admin.batch_execute(&format!(
        "ALTER TABLE _oo.{} RENAME TO held_heap; CREATE VIEW _oo.{} AS SELECT * FROM _oo.held_heap;
         GRANT SELECT ({}) ON _oo.{} TO {ROLE};", active_physical,active_physical,f.column,active_physical
    )).await.unwrap();
    assert_ne!(f.query("current").await.0, 200);
    f.admin
        .batch_execute(&format!(
            "DROP VIEW _oo.{}; ALTER TABLE _oo.held_heap RENAME TO {}",
            active_physical, active_physical
        ))
        .await
        .unwrap();
    evidence["second"] = second.2;
    evidence["new_publication_id"] = json!(second.1[ACTUAL].to_str().unwrap());
    evidence["refusals"] = json!(refusals);
    evidence["same_gateway_and_session"] = json!(true);
    fs::write(
        f.root.join("acceptance.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
