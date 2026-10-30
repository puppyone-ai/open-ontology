//! Synthetic local PG mechanism/performance fixture, never a customer oracle.
use super::*;
use crate::policy::LogicalTable;
use serde_json::json;
use std::collections::BTreeMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TABLES: usize = 128;
const ROLES: &[&str] = &["oo_r_perf_a", "oo_r_perf_b", "oo_r_perf_c"];

struct Fixture {
    root: PathBuf,
    endpoint: TlsEndpoint,
    password: String,
    ca: Vec<u8>,
    admin: tokio_postgres::Client,
    _driver: BindingDriver,
    _proxy: Option<BindingDriver>,
    plane: Arc<Plane>,
}

fn physical(i: usize) -> String {
    format!("pt_{i:060x}")
}
fn column() -> String {
    format!("pc_{}", "b".repeat(60))
}
fn digest() -> String {
    "c".repeat(64)
}
fn run_id(i: usize) -> String {
    format!("00000000-0000-4000-8000-{i:012}")
}

impl Fixture {
    async fn new() -> Self {
        Self::with_tables(TABLES).await
    }

    async fn with_tables(table_count: usize) -> Self {
        let config: Value = serde_json::from_slice(
            &fs::read(std::env::var("OO_PERF_OWNED_FIXTURE").expect("owned fixture required"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(config["protocol"], "oo.gateway-performance.owned/v1");
        assert_eq!(config["host"], "127.0.0.1");
        assert_eq!(config["database"], "gateway_perf");
        let root = PathBuf::from(config["work"].as_str().unwrap());
        assert!(
            root.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("perf-owned-")
        );
        assert_eq!(
            fs::read_to_string(root.join("pg/postmaster.pid"))
                .unwrap()
                .lines()
                .next()
                .unwrap(),
            config["postmaster_pid"].as_i64().unwrap().to_string()
        );
        let password = fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
        let ca = fs::read(config["ca_file"].as_str().unwrap()).unwrap();
        let endpoint = TlsEndpoint {
            host: "127.0.0.1".into(),
            port: config["port"].as_u64().unwrap() as u16,
            database: "gateway_perf".into(),
            user: "postgres".into(),
        };
        let (admin, connection) = connect_require_tls(&endpoint, &password, &ca)
            .await
            .unwrap();
        let driver = BindingDriver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let mut sql = format!("CREATE SCHEMA _oo; CREATE SCHEMA oo_publish_target;
            CREATE TABLE _oo.physical_mappings(source text,config_digest text,run_generation int8,
                level text,logical_namespace text,logical_table text,logical_column text,physical_name text);
            CREATE INDEX ON _oo.physical_mappings(source,config_digest,run_generation,level,logical_namespace,logical_table,logical_column);
            CREATE FUNCTION _oo.lookup_physical_mapping(text,text,int8,text,text,text,text) RETURNS text
            LANGUAGE sql STABLE AS $fn$ SELECT physical_name FROM _oo.physical_mappings WHERE
                source=$1 AND config_digest=$2 AND run_generation=$3 AND level=left($4,1)
                AND logical_namespace=$5 AND logical_table=$6 AND logical_column=$7 $fn$;
            INSERT INTO _oo.physical_mappings VALUES('perf','{}',1,'n','app','','','pn_{}');
            CREATE TABLE _oo.full_import_run(run_id uuid PRIMARY KEY,source_id text,source_generation int8,
                target_generation int8,config_digest text,type_digest text,input_digest text,sealed_at timestamptz,state text);
            CREATE TABLE _oo.full_import_table(run_id uuid,schema_ident text,table_ident text,physical_name text);
            CREATE INDEX ON _oo.full_import_table(run_id);
            CREATE TABLE _oo.table_freshness(source text,schema_name text,table_name text,physical_name text,
                source_generation int8,target_generation int8,state text,applied_at timestamptz);
            CREATE INDEX ON _oo.table_freshness(source,schema_name,table_name);
            CREATE TABLE _oo.source_watermark(source text PRIMARY KEY,source_generation int8,target_generation int8,applied_at timestamptz);
            INSERT INTO _oo.source_watermark VALUES('perf',1,1,now());
            CREATE TABLE oo_publish_target.fence(role_name text,schema_name text,relation_name text,state text,opened_pack_digest text,
                PRIMARY KEY(role_name,schema_name,relation_name));
            CREATE TABLE _oo.native_library_head(library_id text PRIMARY KEY,receipt_sha256 text,state text);
            INSERT INTO _oo.native_library_head VALUES('perf','{}','ready');
            CREATE TABLE _oo.native_library_source_status(library_id text,source_id text,receipt_sha256 text);
            INSERT INTO _oo.native_library_source_status VALUES('perf','perf','{}');
            CREATE FUNCTION _oo.native_library_source_ready(text,text) RETURNS bool LANGUAGE sql STABLE AS
                $fn$ SELECT EXISTS(SELECT 1 FROM _oo.native_library_head WHERE library_id='perf' AND receipt_sha256=$2 AND state='ready')
                    AND EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1) $fn$;
            GRANT USAGE ON SCHEMA _oo,oo_publish_target TO oo_perf_login;
            ALTER ROLE oo_perf_login SET default_transaction_read_only=on;",
            digest(),"a".repeat(60),"d".repeat(64),"d".repeat(64));
        for role in ROLES {
            sql += &format!("GRANT USAGE ON SCHEMA _oo TO {role};");
        }
        for i in 0..table_count {
            let physical = physical(i);
            let column = column();
            let run = run_id(i);
            sql += &format!("CREATE TABLE _oo.{physical}({column} int8, secret text);
                INSERT INTO _oo.{physical} SELECT n,'hidden' FROM generate_series(1,32) n;
                INSERT INTO _oo.physical_mappings VALUES
                    ('perf','{d}',1,'t','app','t{i}','','{physical}'),
                    ('perf','{d}',1,'c','app','t{i}','amount','{column}');
                INSERT INTO _oo.full_import_run VALUES('{run}','perf',1,1,'{d}','{d}','{d}',now(),'published');
                INSERT INTO _oo.full_import_table VALUES('{run}','app','t{i}','{physical}');
                INSERT INTO _oo.table_freshness VALUES('perf','app','t{i}','{physical}',1,1,'live',now());",d=digest());
            for role in ROLES {
                sql += &format!("GRANT SELECT ({column}) ON _oo.{physical} TO {role};
                    INSERT INTO oo_publish_target.fence VALUES('{role}','_oo','{physical}','open','{}');","e".repeat(64));
            }
        }
        sql += "GRANT SELECT ON ALL TABLES IN SCHEMA _oo,oo_publish_target TO oo_perf_login;";
        admin.batch_execute(&sql).await.unwrap();
        let mut runtime_endpoint = endpoint.clone();
        runtime_endpoint.user = "oo_perf_login".into();
        // A local TLS byte relay adds a measured, deterministic response-chunk
        // delay. It is a sensitivity measurement, NOT a claimed WAN latency.
        let delay = config["response_chunk_delay_ms"].as_u64().unwrap_or(0);
        let proxy = if delay > 0 {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            runtime_endpoint.port = listener.local_addr().unwrap().port();
            let port = endpoint.port;
            Some(BindingDriver(tokio::spawn(async move {
                loop {
                    let Ok((client, _)) = listener.accept().await else {
                        break;
                    };
                    tokio::spawn(async move {
                        let server = tokio::net::TcpStream::connect(("127.0.0.1", port))
                            .await
                            .unwrap();
                        client.set_nodelay(true).unwrap();
                        server.set_nodelay(true).unwrap();
                        let (mut cr, mut cw) = client.into_split();
                        let (mut sr, mut sw) = server.into_split();
                        let upstream = tokio::io::copy(&mut cr, &mut sw);
                        let downstream = async {
                            let mut buf = vec![0; 65536];
                            loop {
                                let n = sr.read(&mut buf).await?;
                                if n == 0 {
                                    break;
                                }
                                tokio::time::sleep(Duration::from_millis(delay)).await;
                                cw.write_all(&buf[..n]).await?;
                            }
                            Ok::<(), std::io::Error>(())
                        };
                        tokio::select! {_=upstream=>{},_=downstream=>{}}
                    });
                }
            })))
        } else {
            None
        };
        let roles = ReviewedRoles::fixed(
            ROLES
                .iter()
                .map(|role| published(role, table_count))
                .collect(),
        );
        fs::create_dir(root.join("wal")).unwrap();
        let plane = Arc::new(Plane {
            deployment_mode: oo_core::config::DeploymentMode::AllInOne,
            query_timeout_ms: if table_count > TABLES { 20_000 } else { 5000 },
            query_capacity: Some(tokio::sync::Semaphore::new(4)),
            endpoint: runtime_endpoint,
            password: password.clone(),
            ca_pem: ca.clone(),
            connector: rustls_connector(&ca).unwrap(),
            roles,
            wal_dir: root.join("wal"),
            collector_dir: None,
            sessions: Mutex::new(HashMap::new()),
            opening: Mutex::new(()),
            inflight: Mutex::new(HashMap::new()),
        });
        Self {
            root,
            endpoint,
            password,
            ca,
            admin,
            _driver: driver,
            _proxy: proxy,
            plane,
        }
    }

    fn log_mark(&self) -> usize {
        fs::read(self.root.join("postgres.log")).unwrap().len()
    }
    fn profile(&self, from: usize) -> Value {
        let bytes = fs::read(self.root.join("postgres.log")).unwrap();
        let text = String::from_utf8_lossy(&bytes[from..]);
        let mut categories = BTreeMap::<String, usize>::new();
        let mut queries = Vec::new();
        for line in text.lines().filter(|l| {
            l.contains("oo-gateway-all-in-one")
                && (l.contains(" execute ") || l.contains(" statement: "))
        }) {
            let category = if line.contains("has_column_privilege") {
                "column_acl"
            } else if line.contains("full_import_run") {
                "published_run"
            } else if line.contains("oo_publish_target.fence") {
                "fence"
            } else if line.contains("pg_attribute") {
                "attributes"
            } else if line.contains("pg_class") {
                "relations"
            } else if line.contains("max(run_generation)") {
                "generation"
            } else if line.contains("lookup_physical_mapping") {
                "mappings"
            } else if line.contains("pg_operator") {
                "operators"
            } else if line.contains("pg_proc") {
                "functions"
            } else {
                "other"
            };
            *categories.entry(category.into()).or_default() += 1;
            queries.push(line.to_owned());
        }
        json!({"statements":queries.len(),"categories":categories,"raw_lines":queries,
            "counting_rule":"completed PG execute/statement log records; one simple-query batch is one record, not one SQL statement or network RTT"})
    }
}

fn published(role: &str, count: usize) -> Published {
    let policies = (0..count)
        .map(|i| ColumnAclPolicy {
            project: "perf".into(),
            config_digest: digest(),
            generation: 1,
            role: role.strip_prefix("oo_r_").unwrap().into(),
            login: "oo_perf_login".into(),
            row_filter: None,
            table: LogicalTable {
                namespace: "app".into(),
                name: format!("t{i}"),
                columns: vec![ColumnSpec {
                    name: "amount".into(),
                    status: ClassStatus::Confirmed,
                    semantic_type: "amount".into(),
                    pii: PiiClass::None,
                }],
            },
        })
        .collect();
    let vector:Vec<_>=(0..count).map(|i|json!({"dataset":format!("app.t{i}"),"source_id":"perf","config_digest":digest(),
        "source_generation":1,"target_generation":1,"stored_namespace":"_oo","stored_physical":physical(i),
        "run_id":run_id(i),"type_digest":digest(),"input_digest":digest(),
        "expected_columns":[{"name":"amount","data_type":"bigint"}]})).collect();
    Published {
        mapping_source: Some(String::new()),
        policies,
        role: role.into(),
        project: "perf".into(),
        policy_version: "pv1".into(),
        knowledge_version: "kv1".into(),
        config_digest: digest(),
        native_vector: Some(json!(vector)),
        native_catalog: Some(json!({"protocol":"oo.gateway.library-binding/v1",
            "library_id":"perf","receipt_sha256":"d".repeat(64),"pack_digest":"e".repeat(64),"entries":vector,
            "limits":{"max_tables":count.max(TABLES),"max_columns":512}})),
        quality_vector: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "new owned PG only; tests/performance_owned_pg.py"]
async fn performance_owned_pg_catalog_and_roles() {
    let f = Fixture::new().await;
    let phase = std::env::var("OO_PERF_PHASE").expect("baseline or optimized");
    let cold = Instant::now();
    let a = f.plane.session_for(ROLES[0]).await.unwrap();
    let cold_session_ms = cold.elapsed().as_secs_f64() * 1000.0;
    let mut warm_session_ms = Vec::new();
    for _ in 0..8 {
        let started = Instant::now();
        assert!(Arc::ptr_eq(
            &a,
            &f.plane.session_for(ROLES[0]).await.unwrap()
        ));
        warm_session_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let a_guard = a.lock().await;
    let waiter_plane = f.plane.clone();
    let same = tokio::spawn(async move { waiter_plane.session_for(ROLES[0]).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    let started = Instant::now();
    let other =
        tokio::time::timeout(Duration::from_millis(250), f.plane.session_for(ROLES[1])).await;
    let blocked = other.is_err();
    let other_ms = started.elapsed().as_secs_f64() * 1000.0;
    drop(a_guard);
    let same = same.await.unwrap().unwrap();
    assert!(Arc::ptr_eq(&a, &same), "same role must retain one session");
    if phase == "baseline" {
        assert!(blocked, "baseline defect must reproduce");
    } else {
        assert!(
            !blocked,
            "busy A must not hold global locks while B connects"
        );
    }
    let b = f.plane.session_for(ROLES[1]).await.unwrap();
    assert_ne!(a.lock().await.backend_pid(), b.lock().await.backend_pid());
    let mut simultaneous = Vec::new();
    for _ in 0..8 {
        let plane = f.plane.clone();
        simultaneous.push(tokio::spawn(async move {
            plane.session_for(ROLES[2]).await.unwrap()
        }));
    }
    let first = simultaneous.remove(0).await.unwrap();
    for future in simultaneous {
        assert!(Arc::ptr_eq(&first, &future.await.unwrap()));
    }
    first.lock().await.close();
    let reopened = f.plane.session_for(ROLES[2]).await.unwrap();
    assert!(!Arc::ptr_eq(&first, &reopened));
    assert_eq!(
        f.plane.query_capacity.as_ref().unwrap().available_permits(),
        4
    );
    let mut samples = Vec::new();
    for size in [1, 16, 64, 128] {
        let published = published(ROLES[0], size);
        for iteration in 0..3 {
            let mark = f.log_mark();
            let started = Instant::now();
            let catalog = f
                .plane
                .catalog_for(&published)
                .await
                .unwrap_or_else(|e| panic!("catalog size={size}: {:?}", e.code));
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(catalog.relations.len(), size);
            assert!(catalog.relations.iter().all(|r| r.columns.len() == 1
                && r.columns[0].name == "amount"
                && r.pending_unclassified == 1));
            let profile = f.profile(mark);
            samples.push(json!({"tables":size,"iteration":iteration,"connection":"fresh TLS per call",
                "cache_state":if iteration==0 {"first at this size; DB buffer cache not flushed"} else {"warm DB buffers; no catalog cache"},
                "elapsed_ms":elapsed,"profile":profile}));
        }
    }
    let started = Instant::now();
    let mut parallel = Vec::new();
    for role in ROLES {
        let plane = f.plane.clone();
        let p = published(role, 64);
        parallel.push(tokio::spawn(async move {
            let started = Instant::now();
            let catalog = plane
                .catalog_for(&p)
                .await
                .unwrap_or_else(|_| panic!("parallel catalog"));
            (catalog, started.elapsed().as_secs_f64() * 1000.0)
        }));
    }
    let mut parallel_ms = Vec::new();
    for task in parallel {
        let (catalog, ms) = task.await.unwrap();
        assert_eq!(catalog.relations.len(), 64);
        parallel_ms.push(ms);
    }
    let parallel_wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    let scope_samples = if phase == "optimized" {
        scope_samples(&f).await
    } else {
        Vec::new()
    };
    let checks = if phase == "optimized" {
        negative_checks(&f).await
    } else {
        json!({"status":"not part of initial timing baseline"})
    };
    let report = json!({"protocol":"oo.gateway-performance.measurement/v1","phase":phase,"tables":TABLES,"roles":ROLES.len(),
        "fixture":"synthetic owned PG; no source or customer data; no business oracle",
        "cross_role":{"blocked_250ms":blocked,"role_b_wait_ms":other_ms,"same_role_single_session":true,
            "concurrent_open_single_session":true,"closed_replacement":true,"capacity_unchanged":true,
            "cold_session_ms":cold_session_ms,"warm_session_ms":warm_session_ms},
        "samples":samples,"parallel_3_roles_64_tables":{"wall_ms":parallel_wall_ms,"individual_ms":parallel_ms},
        "native_catalog_regressions":checks,"bound_execution_samples":scope_samples});
    fs::write(
        f.root.join("measurement.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    for sample in report["samples"].as_array().unwrap() {
        eprintln!(
            "PERF tables={} iteration={} ms={} statements={}",
            sample["tables"],
            sample["iteration"],
            sample["elapsed_ms"],
            sample["profile"]["statements"]
        );
    }
    let _ = (&f.endpoint, &f.password, &f.ca, &f.admin);
}

struct PerfAudit;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "fresh owned PG only; tests/performance_owned_pg.py planning"]
async fn planning_catalog_is_not_authorization() {
    let f = Fixture::with_tables(658).await;
    let mut publications: Vec<_> = ROLES.iter().map(|r| published(r, 658)).collect();
    let mut sql = String::new();
    for i in 0..658 {
        let physical = physical(i);
        for j in 1..(24 + usize::from(i < 7)) {
            let name = format!("field_{j}");
            let encoded = format!("pc_{j:060x}");
            sql += &format!(
                "ALTER TABLE _oo.{physical} ADD COLUMN {encoded} int8;
                 INSERT INTO _oo.physical_mappings VALUES
                 ('perf','{}',1,'c','app','t{i}','{name}','{encoded}');",
                digest(),
            );
            for (p, role) in publications.iter_mut().zip(ROLES) {
                sql += &format!("GRANT SELECT ({encoded}) ON _oo.{physical} TO {role};");
                p.policies[i].table.columns.push(ColumnSpec {
                    name: name.clone(),
                    status: ClassStatus::Confirmed,
                    semantic_type: "amount".into(),
                    pii: PiiClass::None,
                });
                p.native_vector.as_mut().unwrap()[i]["expected_columns"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"name":name,"data_type":"bigint"}));
            }
        }
    }
    f.admin.batch_execute(&sql).await.unwrap();
    // Current receipt-shaped metadata for the synthetic target, not a source
    // pipeline or customer-publication acceptance fixture.
    f.admin.batch_execute(&format!(
        "ALTER TABLE _oo.full_import_run ADD COLUMN schema_digest text DEFAULT '{d}';
         ALTER TABLE _oo.full_import_run ADD COLUMN shard_id text DEFAULT 'perf';
         ALTER TABLE _oo.full_import_table ADD COLUMN exhausted bool DEFAULT true;
         ALTER TABLE _oo.full_import_table ADD COLUMN canonical_digest text DEFAULT '{d}';
         ALTER TABLE _oo.full_import_table ADD COLUMN applied_rows int8 DEFAULT 32;
         ALTER TABLE _oo.full_import_table ADD COLUMN applied_pos numeric DEFAULT 0;
         CREATE TABLE _oo.apply_receipts(source text,table_generation text,config_digest text,
            schema_digest text,type_digest text,fence_generation int8,fence_shard text,partial_txn bool);
         INSERT INTO _oo.apply_receipts SELECT 'perf',physical_name,'{d}','{d}','{d}',1,'perf',false
            FROM _oo.full_import_table;
         ALTER TABLE _oo.native_library_head ADD COLUMN manifest_sha256 text DEFAULT '{d}';
         CREATE TABLE _oo.native_library_publication(receipt_sha256 text PRIMARY KEY,
            library_id text,library jsonb,shards jsonb);
         GRANT SELECT ON _oo.apply_receipts,_oo.native_library_publication TO oo_perf_login;",
        d=digest(),
    )).await.unwrap();
    let tables: Vec<_> = (0..658).map(|i| json!({
        "schema":"app","table":format!("t{i}"),"run_id":run_id(i),"physical_name":physical(i),
        "source_generation":1,"target_generation":1,"accepted_rows":32,
        "canonical_digest":digest(),"applied_pos":"0",
    })).collect();
    let library = json!({"library_id":"perf","manifest_sha256":digest()});
    let shards = json!([{"source_id":"perf","receipt_sha256":digest(),
        "receipt":{"consistency":"per_table","tables":tables}}]);
    use sha2::{Digest, Sha256};
    let cut_sha = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!({"library":library,"shards":shards})).unwrap())
    );
    f.admin.execute("INSERT INTO _oo.native_library_publication VALUES($1,'perf',$2::text::jsonb,$3::text::jsonb)",
        &[&cut_sha,&library.to_string(),&shards.to_string()]).await.unwrap();
    f.admin
        .execute(
            "UPDATE _oo.native_library_head SET receipt_sha256=$1",
            &[&cut_sha],
        )
        .await
        .unwrap();
    for p in &mut publications {
        p.native_catalog.as_mut().unwrap()["entries"] = p.native_vector.clone().unwrap();
        p.native_catalog.as_mut().unwrap()["receipt_sha256"] = json!(cut_sha);
    }
    f.plane.roles.current.lock().unwrap().roles = Arc::new(publications.clone());
    let p = &publications[0];
    assert_eq!(
        p.policies
            .iter()
            .map(|p| p.table.columns.len())
            .sum::<usize>(),
        15799
    );
    let start = Instant::now();
    f.plane.warm_planning_catalogs().await;
    let warmup_ms = start.elapsed().as_millis();
    let mark = f.log_mark();
    let start = Instant::now();
    let catalog = f
        .plane
        .planning_catalog_for(p)
        .await
        .unwrap_or_else(|_| panic!("planning"));
    let planning_ms = start.elapsed().as_millis();
    assert_eq!(
        f.profile(mark)["statements"],
        0,
        "warm planning must not reopen PG"
    );
    assert_eq!(catalog.relations.len(), 658);

    async fn verify(
        f: &Fixture,
        p: &Published,
        catalog: &crate::sql_guard::PolicyCatalog,
    ) -> Result<BindingReceipt, ExecError> {
        let binding = f
            .plane
            .binding_check(
                p,
                &caller(&p.role),
                &"e".repeat(64),
                Some(catalog),
                Expected {
                    publication_id: "current".into(),
                    data_vector_sha256: None,
                },
            )
            .unwrap_or_else(|_| panic!("binding"));
        let (mut client, connection) = f.plane.connect().await.unwrap();
        let _driver = BindingDriver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let tx = client
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await
            .unwrap();
        let result = match binding.lock_relations(&tx).await {
            Ok(()) => binding.verify(&tx).await,
            Err(error) => Err(error),
        };
        tx.rollback().await.unwrap();
        result
    }
    let first = verify(&f, p, &catalog).await.unwrap();
    let physical = physical(0);
    let column = column();
    let mut refusals = Vec::new();
    for (name, change, restore) in [
        ("closed_fence", format!("UPDATE oo_publish_target.fence SET state='closed' WHERE role_name='{}' AND relation_name='{physical}'", ROLES[0]),
            "UPDATE oo_publish_target.fence SET state='open'".into()),
        ("revoked_column", format!("REVOKE SELECT ({column}) ON _oo.{physical} FROM {}", ROLES[0]),
            format!("GRANT SELECT ({column}) ON _oo.{physical} TO {}", ROLES[0])),
        ("duplicate_mapping", "INSERT INTO _oo.physical_mappings SELECT * FROM _oo.physical_mappings WHERE level='t' AND logical_table='t0'".into(),
            "DELETE FROM _oo.physical_mappings WHERE ctid IN (SELECT max(ctid) FROM _oo.physical_mappings WHERE level='t' AND logical_table='t0')".into()),
        ("run_not_published", "UPDATE _oo.full_import_run SET state='awaiting_publish' WHERE run_id='00000000-0000-4000-8000-000000000000'".into(),
            "UPDATE _oo.full_import_run SET state='published'".into()),
    ] {
        f.admin.batch_execute(&change).await.unwrap();
        assert_eq!(f.plane.planning_catalog_for(p).await.unwrap_or_else(|_| panic!("cached")), catalog);
        assert!(verify(&f, p, &catalog).await.is_err(), "cached planning must not authorize {name}");
        f.admin.batch_execute(&restore).await.unwrap();
        refusals.push(name);
    }
    let helper: String = f.admin.query_one(
        "SELECT pg_get_functiondef('_oo.lookup_physical_mapping(text,text,bigint,text,text,text,text)'::regprocedure)", &[])
        .await.unwrap().get(0);
    f.admin.batch_execute("CREATE OR REPLACE FUNCTION _oo.lookup_physical_mapping(text,text,int8,text,text,text,text)
        RETURNS text LANGUAGE sql STABLE AS $$ SELECT 'wrong'::text $$").await.unwrap();
    assert!(
        verify(&f, p, &catalog).await.is_err(),
        "same-signature helper mutation must refuse"
    );
    f.admin.batch_execute(&helper).await.unwrap();
    refusals.push("helper_body_mutation");
    let after = verify(&f, p, &catalog).await.unwrap();
    let fresh = f
        .plane
        .catalog_for(p)
        .await
        .unwrap_or_else(|_| panic!("fresh"));
    assert_eq!(catalog, fresh);
    assert_eq!(after, verify(&f, p, &fresh).await.unwrap());
    assert_eq!(after.data_vector_sha256, first.data_vector_sha256);
    let mut changed = p.clone();
    changed.native_vector.as_mut().unwrap()[0]["target_generation"] = json!(2);
    assert!(
        f.plane.planning_catalog_for(&changed).await.is_err(),
        "a different reviewed data generation cannot hit the cache"
    );
    let other = f
        .plane
        .planning_catalog_for(&publications[1])
        .await
        .unwrap_or_else(|_| panic!("other role"));
    assert_ne!(other.identity.role, catalog.identity.role);
    f.plane.forget_planning_catalog(p).await;
    assert!(
        f.plane
            .role_slot(&p.role)
            .await
            .unwrap()
            .planning
            .lock()
            .await
            .is_none()
    );
    assert!(
        f.plane
            .role_slot(&publications[1].role)
            .await
            .unwrap()
            .planning
            .lock()
            .await
            .is_some()
    );
    fs::write(f.root.join("planning-receipt.json"), serde_json::to_vec_pretty(&json!({
        "tables":658,"columns":15799,"warmup_ms":warmup_ms,"planning_ms":planning_ms,
        "warm_planning_database_statements":0,"cached_catalog_is_not_authorization":true,
        "refusals":refusals,"cached_and_fresh_receipts_equal":true,"publication_revision_miss":true,
        "roles_isolated":true,"invalidation_is_role_local":true,
        "customer_data":false,"real_c5_overlay_fixture":false
    })).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "new owned PG only; metadata-margin native regression runner"]
async fn metadata_margin_owned_regressions() {
    let f = Fixture::new().await;
    let checks = negative_checks(&f).await;
    let mut invalid = published(ROLES[0], 16);
    let canonical = invalid.native_vector.as_ref().unwrap()[0]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    for alias in [canonical.replace('-', ""), format!("{{{canonical}}}")] {
        invalid.native_vector.as_mut().unwrap()[0]["run_id"] = json!(alias);
        assert!(
            f.plane.catalog_for(&invalid).await.is_err(),
            "UUID coercion must not accept a noncanonical run binding"
        );
    }
    invalid.native_vector.as_mut().unwrap()[0]["run_id"] = json!(canonical);
    assert!(f.plane.catalog_for(&invalid).await.is_ok());
    let mut duplicate = published(ROLES[0], 16);
    let first = duplicate.native_vector.as_ref().unwrap()[0].clone();
    duplicate.native_vector.as_mut().unwrap()[1] = first;
    assert!(f.plane.catalog_for(&duplicate).await.is_err());
    fs::write(f.root.join("margin-regressions.json"), serde_json::to_vec_pretty(&json!({
        "native_negative_matrix":checks,"canonical_uuid_required":true,"duplicate_vector_rejected":true,
        "timing_baseline_not_rerun":true,"business_oracle":false
    })).unwrap()).unwrap();
}

impl AuditAdmission for PerfAudit {
    async fn admit(&mut self, _: &Admission<'_>) -> Result<(), ExecError> {
        Ok(())
    }
    async fn complete(&mut self, _: &CompletionRecord) -> Result<(), ExecError> {
        Ok(())
    }
}

fn caller(role: &str) -> PublicCaller {
    PublicCaller {
        tuple: crate::http_gateway::CurrentTuple {
            project_id: "perf".into(),
            principal_id: format!("principal_{role}"),
            role: role.into(),
            claims: vec![],
            policy_version: "pv1".into(),
            knowledge_version: "kv1".into(),
            audience_url: "https://performance.invalid/v1".into(),
            sandbox_id: None,
            session_id: None,
        },
        scope: "client",
        auth_remaining_ms: 0,
        auth_cache_hit: false,
    }
}

async fn scope_samples(f: &Fixture) -> Vec<Value> {
    let mut samples = Vec::new();
    for size in [1, 16, 64, 128] {
        let published = published(ROLES[0], size);
        let catalog = f
            .plane
            .catalog_for(&published)
            .await
            .unwrap_or_else(|_| panic!("catalog"));
        let binding = f
            .plane
            .binding_check(
                &published,
                &caller(ROLES[0]),
                &"e".repeat(64),
                Some(&catalog),
                Expected {
                    publication_id: "current".into(),
                    data_vector_sha256: None,
                },
            )
            .unwrap_or_else(|_| panic!("binding"));
        let session = f.plane.session_for(ROLES[0]).await.unwrap();
        let mut session = session.lock().await;
        let started = Instant::now();
        let mark = f.log_mark();
        let mut stream = session
            .execute_bound_select(
                SelectRequest {
                    identity: &catalog.identity,
                    catalog: &catalog,
                    original_sql: "SELECT SUM(amount) FROM app.t0",
                    role: ROLES[0],
                    max_rows: 10,
                    max_result_bytes: MAX_QUERY_BYTES,
                    wall_clock: Duration::from_millis(5000),
                    read_quality: false,
                    quality_vector: None,
                },
                PerfAudit,
                Some(binding),
            )
            .await
            .unwrap();
        let result = read_stream(&mut stream).await;
        let receipt = stream.publication();
        drop(stream);
        session.wait_idle().await.unwrap();
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        match &result {
            Ok(rows) => {
                assert_eq!(rows.len(), 1);
                assert!(receipt.is_some());
            }
            Err(error) => assert!(
                matches!(error, ExecError::Timeout { .. }),
                "unexpected bound error: {error}"
            ),
        }
        samples.push(json!({"tables":size,"elapsed_ms":elapsed,"sql_wall_budget_ms":5000,"success":result.is_ok(),
            "error":result.err().map(|e|e.to_string()),"profile":f.profile(mark)}));
    }
    samples
}

async fn negative_checks(f: &Fixture) -> Value {
    let p = published(ROLES[0], 16);
    let load = || async { f.plane.catalog_for(&p).await };
    let expected = load().await.unwrap_or_else(|_| panic!("expected catalog"));
    let physical = physical(0);
    let column = column();
    for (name,change,restore) in [
        ("generation","UPDATE _oo.physical_mappings SET run_generation=2 WHERE logical_table='t0'".to_string(),
            "UPDATE _oo.physical_mappings SET run_generation=1 WHERE logical_table='t0'".into()),
        ("duplicate_mapping","INSERT INTO _oo.physical_mappings SELECT * FROM _oo.physical_mappings WHERE level='t' AND logical_table='t0'".into(),
            "DELETE FROM _oo.physical_mappings WHERE ctid IN (SELECT max(ctid) FROM _oo.physical_mappings WHERE level='t' AND logical_table='t0')".into()),
        ("closed_fence",format!("UPDATE oo_publish_target.fence SET state='closed' WHERE role_name='{}' AND relation_name='{physical}'",ROLES[0]),
            "UPDATE oo_publish_target.fence SET state='open'".into()),
        ("run_not_published","UPDATE _oo.full_import_run SET state='awaiting_publish' WHERE run_id='00000000-0000-4000-8000-000000000000'".into(),
            "UPDATE _oo.full_import_run SET state='published'".into()),
        ("duplicate_run_table","INSERT INTO _oo.full_import_table SELECT * FROM _oo.full_import_table WHERE table_ident='t0'".into(),
            "DELETE FROM _oo.full_import_table WHERE ctid IN (SELECT max(ctid) FROM _oo.full_import_table WHERE table_ident='t0')".into()),
        ("wrong_target","UPDATE _oo.table_freshness SET target_generation=2 WHERE table_name='t0'".into(),
            "UPDATE _oo.table_freshness SET target_generation=1".into()),
        ("closed_head","UPDATE _oo.native_library_head SET state='closed'".into(),
            "UPDATE _oo.native_library_head SET state='ready'".into()),
        ("unsupported_type",format!("ALTER TABLE _oo.{physical} ALTER COLUMN {column} TYPE real"),
            format!("ALTER TABLE _oo.{physical} ALTER COLUMN {column} TYPE int8")),
        ("schema_ambiguity",format!("CREATE SCHEMA pn_{}; CREATE TABLE pn_{}.{physical}(id int8)","a".repeat(60),"a".repeat(60)),
            format!("DROP SCHEMA pn_{} CASCADE","a".repeat(60))),
        ("arbitrary_view",format!("ALTER TABLE _oo.{physical} RENAME TO saved_perf_heap; CREATE VIEW _oo.{physical} AS SELECT * FROM _oo.saved_perf_heap"),
            format!("DROP VIEW _oo.{physical}; ALTER TABLE _oo.saved_perf_heap RENAME TO {physical}")),
    ] {
        f.admin.batch_execute(&change).await.unwrap();
        assert!(load().await.is_err(),"{name} must fail closed");
        f.admin.batch_execute(&restore).await.unwrap();
        assert_eq!(load().await.unwrap_or_else(|_|panic!("restore {name}")),expected);
    }
    f.admin
        .batch_execute(&format!(
            "REVOKE SELECT ({column}) ON _oo.{physical} FROM {}",
            ROLES[0]
        ))
        .await
        .unwrap();
    let revoked = load().await.unwrap_or_else(|_| panic!("revoked catalog"));
    assert_eq!(revoked.relations.len(), 15);
    assert!(!revoked.relations.iter().any(|r| r.name == "t0"));
    f.admin
        .batch_execute(&format!(
            "GRANT SELECT ({column}) ON _oo.{physical} TO {}",
            ROLES[0]
        ))
        .await
        .unwrap();
    assert_eq!(
        load().await.unwrap_or_else(|_| panic!("restore ACL")),
        expected
    );
    for key in [
        "source_id",
        "config_digest",
        "type_digest",
        "input_digest",
        "stored_namespace",
        "stored_physical",
        "run_id",
    ] {
        let mut invalid = published(ROLES[0], 16);
        invalid.native_vector.as_mut().unwrap()[0][key] = json!("wrong");
        assert!(f.plane.catalog_for(&invalid).await.is_err(), "vector {key}");
    }
    let mut invalid = published(ROLES[0], 16);
    invalid.native_vector.as_mut().unwrap()[0]["expected_columns"][0]["data_type"] =
        json!("integer");
    assert!(f.plane.catalog_for(&invalid).await.is_err());

    // Verify the library proof and ACL are reread on one real transaction,
    // then execute the admitted SQL with the same role as the regular path.
    let mut totals = Vec::new();
    for role in ROLES {
        let p = published(role, 16);
        let catalog = f
            .plane
            .catalog_for(&p)
            .await
            .unwrap_or_else(|_| panic!("catalog"));
        let caller = PublicCaller {
            tuple: crate::http_gateway::CurrentTuple {
                project_id: "perf".into(),
                principal_id: format!("principal_{role}"),
                role: (*role).into(),
                claims: vec![],
                policy_version: "pv1".into(),
                knowledge_version: "kv1".into(),
                audience_url: "https://performance.invalid/v1".into(),
                sandbox_id: None,
                session_id: None,
            },
            scope: "client",
            auth_remaining_ms: 0,
            auth_cache_hit: false,
        };
        let mut binding = f
            .plane
            .binding_check(
                &p,
                &caller,
                &"e".repeat(64),
                Some(&catalog),
                Expected {
                    publication_id: "current".into(),
                    data_vector_sha256: None,
                },
            )
            .unwrap_or_else(|_| panic!("binding"));
        let (mut client, connection) = f.plane.connect().await.unwrap();
        let _driver = BindingDriver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let tx = client
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await
            .unwrap();
        binding.lock_relations(&tx).await.unwrap();
        let receipt = binding.verify(&tx).await.unwrap();
        assert_eq!(
            receipt,
            binding.reference_verify(&tx).await.unwrap(),
            "batching must preserve the frozen F22 publication/vector bytes"
        );
        let plan = guard_select(
            &catalog.identity,
            &catalog,
            "SELECT SUM(amount) FROM app.t0",
        )
        .unwrap();
        tx.batch_execute(&format!("SET LOCAL ROLE \"{role}\""))
            .await
            .unwrap();
        let rows = tx
            .query(&format!("SELECT ({})::text", plan.executed_sql), &[])
            .await
            .unwrap();
        let sum: String = rows[0].get(0);
        assert_eq!(sum, "528");
        totals.push(sum);
        tx.rollback().await.unwrap();
        binding.expected = Expected {
            publication_id: receipt.publication_id,
            data_vector_sha256: Some(receipt.data_vector_sha256),
        };
        f.admin
            .batch_execute(
                "UPDATE _oo.table_freshness SET applied_at=clock_timestamp() WHERE table_name='t0'",
            )
            .await
            .unwrap();
        let tx = client
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await
            .unwrap();
        binding.lock_relations(&tx).await.unwrap();
        assert!(
            binding.verify(&tx).await.is_err(),
            "data-only must not reuse old binding"
        );
        assert!(binding.reference_verify(&tx).await.is_err());
        tx.rollback().await.unwrap();
    }
    assert!(f.plane.session_for("oo_r_unknown").await.is_err());
    // An aborted same-role waiter cannot replace a busy generation or hold B.
    let a = f.plane.session_for(ROLES[0]).await.unwrap();
    let held = a.lock().await;
    let plane = f.plane.clone();
    let waiter = tokio::spawn(async move { plane.session_for(ROLES[0]).await });
    tokio::time::sleep(Duration::from_millis(10)).await;
    waiter.abort();
    let _ = waiter.await;
    let b = tokio::time::timeout(Duration::from_millis(250), f.plane.session_for(ROLES[1]))
        .await
        .unwrap()
        .unwrap();
    assert_ne!(held.backend_pid(), b.lock().await.backend_pid());
    drop(held);
    assert!(Arc::ptr_eq(
        &a,
        &f.plane.session_for(ROLES[0]).await.unwrap()
    ));
    let baseline_connections: i64 = f
        .admin
        .query_one(
            "SELECT count(*) FROM pg_stat_activity WHERE usename='oo_perf_login'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let plane = f.plane.clone();
    let task = tokio::spawn(async move { plane.catalog_for(&published(ROLES[0], 128)).await });
    let mut observed = false;
    for _ in 0..100 {
        let count: i64 = f
            .admin
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE usename='oo_perf_login'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        if count > baseline_connections {
            observed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(observed, "observe real catalog connection before abort");
    task.abort();
    let _ = task.await;
    let mut released = false;
    for _ in 0..100 {
        let count: i64 = f
            .admin
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE usename='oo_perf_login'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        if count == baseline_connections {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(released, "aborted catalog TLS driver/session must not leak");
    json!({"generation":true,"duplicate_mapping":true,"closed_fence":true,"run_not_published":true,
        "duplicate_run_table":true,"wrong_target":true,"closed_head":true,"unsupported_type":true,
        "schema_ambiguity":true,"arbitrary_view":true,"live_acl_and_restoration":true,"tampered_vector":true,
        "three_roles_actual_sql_sum":totals,"same_transaction_data_only_drift":true,
        "frozen_f22_receipt_hashes_equal_reference":true,
        "aborted_catalog_tls_driver_released":true,
        "unknown_role_no_slot":true,"aborted_waiter_does_not_replace_session_or_block_other_role":true})
}
