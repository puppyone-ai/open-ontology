use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, CompiledPolicy, LogicalTable, PiiClass,
    PolicyCatalog, PolicyIdentity, TlsEndpoint, compile_column_acl, connect_require_tls,
    guard_select, rustls_connector,
    mapping_reader_access::provision_mapping_reader_native_vector,
    policy::catalog::{CatalogLimits, UnknownRows, load_policy_catalog_for_source_vector},
    query_executor::{Admission, AuditAdmission, Cell, CompletionRecord, DedicatedSession,
        ExecError, SelectRequest},
};
use release_coordinator as rc;
use serde_json::{Value, json};
use std::{error::Error, fs, io::Write, path::{Path, PathBuf}, time::Duration};
use tokio_postgres::Client;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const SOURCE: &str = "gateway_visibility";
const ROLE: &str = "oo_r_gateway_sidecar";
const LOGIN: &str = "oo_gateway_sidecar";
const FRESH_LOGIN: &str = "oo_gateway_fresh";

fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    fs::write(root.join(format!("{name}.json")), serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

async fn assert_fresh_metadata_absent(admin: &Client) -> Result<()> {
    let row = admin.query_one(
        "SELECT has_schema_privilege($1::text,'_oo','USAGE'), \
         has_table_privilege($1::text,'_oo.physical_mappings','SELECT')", &[&FRESH_LOGIN],
    ).await?;
    assert!(!row.get::<_,bool>(0));
    assert!(!row.get::<_,bool>(1));
    Ok(())
}

async fn provisioning_refused(admin: &mut Client, policy: &ColumnAclPolicy, out: &Path, label: &str) -> Result<()> {
    assert_fresh_metadata_absent(admin).await?;
    let error=provision_mapping_reader_native_vector(admin,FRESH_LOGIN,std::slice::from_ref(policy)).await.unwrap_err();
    assert_eq!(error,oo_gateway::mapping_reader_access::MappingReaderError::GrantMismatch);
    assert_fresh_metadata_absent(admin).await?;
    save(out,label,&json!({"outcome":"refused","error":"GrantMismatch","metadata_grants_unchanged":true}))
}

struct Audit(PathBuf);
impl Audit {
    fn append(&self, value: Value) -> std::io::Result<()> {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&self.0)?;
        writeln!(f, "{value}")?;
        f.sync_all()
    }
}
impl AuditAdmission for Audit {
    async fn admit(&mut self, a: &Admission<'_>) -> std::result::Result<(), ExecError> {
        self.append(json!({"phase":"admit","role":a.role(),"project":a.project(),
            "backend_pid":a.backend_pid(),"original_sha256":a.original_sha256(),
            "executed_sha256":a.executed_sha256()})).map_err(|_| ExecError::AuditUnavailable)
    }
    async fn complete(&mut self, c: &CompletionRecord) -> std::result::Result<(), ExecError> {
        self.append(json!({"phase":"complete","status":format!("{:?}",c.status()),
            "rows":c.returned_rows(),"sqlstate":c.sqlstate(),"rollback_confirmed":c.rollback_confirmed(),
            "original_sha256":c.original_sha256(),"executed_sha256":c.executed_sha256()}))
            .map_err(|_| ExecError::AuditUnavailable)
    }
}

struct Gateway {
    endpoint: TlsEndpoint,
    password: String,
    ca: Vec<u8>,
    identity: PolicyIdentity,
    policies: Vec<CompiledPolicy>,
    vector: Value,
    catalog_binding: Value,
    evidence: PathBuf,
}

impl Gateway {
    async fn query(&self, label: &str, sql: &str, cached: Option<&PolicyCatalog>) -> Result<(Value, PolicyCatalog)> {
        let (mut client, mut driver) = connect_require_tls(&self.endpoint, &self.password, &self.ca).await?;
        let limits = CatalogLimits { max_tables: 4, max_columns: 16, max_functions: 1,
            max_operators: 128, large_table_row_threshold: 1_000_000, unknown_rows: UnknownRows::Refuse };
        let catalog = if let Some(cached) = cached { cached.clone() } else {
            let loaded = {
                let load = load_policy_catalog_for_source_vector(
                    &mut client, &self.identity, &self.policies, &limits, SOURCE,
                    Some(&self.vector), Some(&self.catalog_binding));
                tokio::pin!(load);
                tokio::select! {
                    result = &mut load => result,
                    _ = &mut driver => return Err("Gateway TLS connection ended during catalog load".into()),
                }
            };
            match loaded {
                Ok(catalog) => catalog,
                Err(error) => {
                    save(&self.evidence,label,&json!({"phase":"policy_catalog","error":format!("{error:?}")}))?;
                    return Err(error.into());
                }
            }
        };
        let relations: Vec<Value> = catalog.relations.iter().map(|r| json!({
            "logical_namespace":r.schema,"logical_table":r.name,
            "physical_namespace":r.physical_schema,"physical_name":r.physical_name,
            "estimated_rows":r.estimated_rows,
            "columns":r.columns.iter().map(|c|c.name.clone()).collect::<Vec<_>>(),
        })).collect();
        let mut evidence = json!({"sql":sql,"catalog_source":if cached.is_some() {"cached"} else {"live_native_vector"},
            "relations":relations,"tls_required":true,"rows":[]});
        if let Err(error) = guard_select(&self.identity, &catalog, sql) {
            evidence["phase"] = json!("guard");
            evidence["error"] = json!(format!("{error:?}"));
            save(&self.evidence,label,&evidence)?;
            return Ok((evidence,catalog));
        }
        let mut session = DedicatedSession::bind(client, driver, rustls_connector(&self.ca)?).await?;
        let request = SelectRequest { identity:&self.identity,catalog:&catalog,original_sql:sql,
            role:ROLE,max_rows:20,max_result_bytes:16384,wall_clock:Duration::from_secs(5),
            read_quality:true,quality_vector:Some(&self.vector) };
        let result = session.execute_trusted_select(request,Audit(self.evidence.join("gateway-audit.jsonl"))).await;
        let mut stream = match result {
            Ok(stream) => stream,
            Err(error) => {
                evidence["phase"] = json!("execute");
                evidence["error"] = json!(format!("{error:?}"));
                evidence["sqlstate"] = json!(error.sqlstate());
                save(&self.evidence,label,&evidence)?;
                return Ok((evidence,catalog));
            }
        };
        let mut rows = Vec::new();
        loop {
            match stream.next_row().await {
                Ok(Some(cells)) => rows.push(cells.iter().map(|cell| match cell {
                    Cell::Int(v) => json!({"int":v}), Cell::Text(v) => json!({"text":v}),
                    Cell::Null => Value::Null, other => json!(format!("{other:?}")),
                }).collect::<Vec<_>>()),
                Ok(None) => break,
                Err(error) => {
                    evidence["error"] = json!(format!("{error:?}"));
                    evidence["sqlstate"] = json!(error.sqlstate());
                    break;
                }
            }
        }
        let quality = stream.quality();
        evidence["quality"] = json!({"relations":quality.relations,"capture_method":quality.capture_method,
            "delete_detection":quality.delete_detection,"warnings":format!("{:?}",quality.warnings),
            "freshness":format!("{:?}",quality.freshness),"watermarks":format!("{:?}",quality.watermarks)});
        drop(stream);
        session.wait_idle().await?;
        let summary = session.summary().ok_or("missing Gateway query summary")?;
        assert!(summary.rollback_confirmed(), "Gateway left an unconfirmed transaction");
        assert!(summary.end_audit_ok(), "Gateway completion audit was not durable");
        evidence["phase"] = json!("complete");
        evidence["status"] = json!(format!("{:?}",summary.status()));
        evidence["rollback_confirmed"] = json!(summary.rollback_confirmed());
        evidence["audit_complete"] = json!(summary.end_audit_ok());
        evidence["rows"] = json!(rows);
        evidence["returned_rows"] = json!(summary.returned_rows());
        save(&self.evidence,label,&evidence)?;
        session.close();
        Ok((evidence,catalog))
    }
}

async fn connect(endpoint: &rc::Endpoint, user: &str, password: &str, ca: &[u8]) -> Result<Client> {
    let (client, driver) = tokio_postgres::Config::new().host(&endpoint.host).port(endpoint.port)
        .dbname(&endpoint.database).user(user).password(password)
        .ssl_mode(tokio_postgres::config::SslMode::Require)
        .connect(rc::rustls_connector(ca).map_err(|e|e.to_string())?).await?;
    tokio::spawn(async move { let _=driver.await; });
    Ok(client)
}

fn compile_pack(out: &Path) -> Result<oo_builder::RolePack> {
    let effective=json!({"protocol":"oo.builder.effective-oos/v1","project":SOURCE,"ontology_version":"c5-v1",
        "catalog_version":"c5-v1","policy":{"id":"fixture-policy","version":"1"},"knowledge":{"id":"fixture-knowledge","version":"1"},
        "ontology":{"spec_version":"oos/v1","name":"fixture","locale":"en-US","include_schemas":["app"],
            "default_tier_preference":["file"],"publish":{"include_unreviewed":[]},"classification":{"auto_accept":"rule_high_confidence"},
            "lifecycle":{"deprecation_min_releases":2},"compile":{"per_role":true,"budgets":{"readme_tokens":2000,"table_file_tokens":3000},
                "sample_values":{"default":"masked","max_per_column":0}}},
        "datasets":[{"schema_version":"oos/v1","dataset":"app.orders","label":"Owned fixture","tier":"file",
            "grain":"one row per key","primary_key":["k1"],"columns":(["k1","value"].iter().map(|n|json!({
                "name":n,"pii":"none","classification":{"state":"confirmed"}})).collect::<Vec<_>>())}]});
    let visibility=json!({"protocol":"oo.builder.role-visibility/v1","project":SOURCE,"principal":"fixture","role":ROLE,
        "ontology_version":"c5-v1","catalog_version":"c5-v1","policy":{"id":"fixture-policy","version":"1"},
        "knowledge":{"id":"fixture-knowledge","version":"1"},"issued_for_compiler":"oo-builder/role-pack/v1",
        "datasets":[{"dataset":"app.orders","row_security":"rls","global_stats":"denied","columns":["k1","value"]}],
        "limits":{"max_file_bytes":262144,"max_total_bytes":1048576,"max_entries":128}});
    let pack=oo_builder::compile_role_pack(&effective.to_string(),&visibility.to_string())?;
    fs::write(out.join("role-pack.json"),&pack.artifact)?;
    save(&out,"effective",&effective)?; save(&out,"visibility",&visibility)?;
    Ok(pack)
}

async fn sealed_facts(admin: &Client, run: &str) -> Result<Value> {
    let facts: Value = serde_json::from_str(&admin.query_one(
        "SELECT _oo.full_import_native_catalog(r.run_id,r.source_id,r.owner_token,r.config_digest,\
         r.source_generation,r.target_generation,r.schema_digest,r.type_digest,r.input_digest)::text \
         FROM _oo.full_import_run r WHERE r.run_id=$1::text::uuid",&[&run]).await?.get::<_,String>(0))?;
    Ok(facts)
}

async fn publication_for(admin: &Client, ctx: &Value, facts: &Value) -> Result<(rc::Publication,Value)> {
    let physical=ctx["physical"].as_str().ok_or("physical")?;
    let run=ctx["run"].as_str().ok_or("run")?;
    let generation=ctx["generation"].as_i64().ok_or("generation")?;
    let target_generation=ctx["fence"].as_i64().ok_or("fence")?;
    let w = admin.query_one("SELECT position_lineage,consistency_grade FROM _oo.source_watermark WHERE source=$1",&[&SOURCE]).await?;
    let lineage:String=w.get(0); let grade:String=w.get(1);
    let receipt = json!({"protocol":"oo.pipeline.native-publication-receipt/v1","collection_mode":"per_table",
        "source_id":SOURCE,"source_generation":generation,"target_generation":target_generation,
        "position_codec":"snapshot-zero/v1","position_lineage":lineage,"applied_pos":"0","consistency":"per_table",
        "consistency_grade":grade,"cross_table_consistent":false,"run_id":run,"run_ids":[run],
        "tables":[{"schema":"app","table":"orders","physical_name":physical,"accepted_rows":3,
            "canonical_digest":facts["tables"][0]["canonical_digest"],"rejects":0,"run_id":run,
            "source_generation":generation,"target_generation":target_generation,"applied_pos":"0",
            "position_lineage":lineage,"capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":grade}]});
    let publication_context = json!({"publication":{"protocol":"oo.pipeline.native-publication-receipt/v1",
        "run_id":run,"receipt_sha256":rc::sha256_hex(&serde_json::to_vec(&receipt)?),"receipt":receipt}});
    let publication = rc::native_publication_context(&publication_context).map_err(|e|e.to_string())?.ok_or("publication")?;
    Ok((publication,publication_context))
}

fn policy_for(config_sha: &str,generation: i64) -> ColumnAclPolicy {
    ColumnAclPolicy {project:SOURCE.into(),config_digest:config_sha.into(),generation,
        role:"gateway_sidecar".into(),login:LOGIN.into(),row_filter:None,
        table:LogicalTable {namespace:"app".into(),name:"orders".into(),columns:["k1","value"].iter().map(|n|ColumnSpec {
            name:(*n).into(),status:ClassStatus::Confirmed,semantic_type:"value".into(),pii:PiiClass::None}).collect()}}
}

#[tokio::main]
async fn main() -> Result<()> {
    let config: Value = serde_json::from_slice(&fs::read(std::env::args().nth(1).ok_or("fixture JSON")?)?)?;
    assert_eq!(config["scope"],"owned_synthetic_only");
    assert_eq!(config["host"],"127.0.0.1");
    assert_eq!(config["database"],"visibility_fixture");
    let out = PathBuf::from(config["evidence"].as_str().ok_or("evidence")?);
    let password = fs::read_to_string(config["password_file"].as_str().ok_or("password file")?)?;
    let ca = fs::read(config["ca_file"].as_str().ok_or("CA")?)?;
    let endpoint = rc::Endpoint { pack_ceilings:None,host:"127.0.0.1".into(),
        port:config["port"].as_u64().ok_or("port")? as u16,database:"visibility_fixture".into() };
    let conn = rc::TargetConn { endpoint:&endpoint,user:"oo_target_admin",password:password.trim(),ca_pem:&ca };
    let mut admin = connect(&endpoint,"oo_target_admin",password.trim(),&ca).await?;
    macro_rules! step {
        ($name:expr,$future:expr) => {{
            let r=$future.await.map_err(|e|e.to_string())?;
            save(&out,$name,&r.body)?;
            assert_eq!(r.code,0,"{}: {}",$name,r.body);
            r
        }};
    }
    let pack=compile_pack(&out)?;
    let base:Value=serde_json::from_str(&admin.query_one("SELECT ctx::text FROM gateway_fixture.saved WHERE label='base'",&[]).await?.get::<_,String>(0))?;
    let base_run=base["run"].as_str().ok_or("base run")?;
    let base_facts=sealed_facts(&admin,base_run).await?;
    let (base_publication,base_input)=publication_for(&admin,&base,&base_facts).await?;
    save(&out,"00-baseline-publication-input",&base_input)?;
    let base_columns:Vec<String>=serde_json::from_value(base["pcs"].clone())?;
    let base_fence=rc::FenceRef {schema_name:"_oo",relation_name:base["physical"].as_str().ok_or("base physical")?,role_name:ROLE};
    step!("00-baseline-install",rc::install_target(&conn,&base_fence));
    step!("00-baseline-close",rc::close_target(&conn,&base_fence));
    step!("00-baseline-stage",rc::grant_target(&conn,&base_fence,&base_columns));
    step!("00-baseline-publish",rc::open_native_target(&conn,&base_fence,&base_columns,&pack.pack_digest,"base_v1","c5_gateway_base",&base_publication));
    let base_policy=policy_for(base_facts["config_digest"].as_str().ok_or("base config")?,base["generation"].as_i64().ok_or("base generation")?);
    admin.batch_execute("ALTER FUNCTION _oo.c5_overlay_visible(text,name) RENAME TO c5_overlay_visible_fixture_hidden").await?;
    let provision=provision_mapping_reader_native_vector(&mut admin,LOGIN,&[base_policy]).await?;
    admin.batch_execute("ALTER FUNCTION _oo.c5_overlay_visible_fixture_hidden(text,name) RENAME TO c5_overlay_visible").await?;
    save(&out,"00-baseline-reader-provision",&json!({"effect":format!("{:?}",provision.effect),"role":provision.role,"login":provision.login,"overlay_api_absent":true}))?;
    step!("00-baseline-retire",rc::close_target(&conn,&base_fence));
    admin.batch_execute("BEGIN; INSERT INTO gateway_fixture.saved SELECT 'view',gateway_fixture.load_range('gateway_visibility','orders','0','[[\"1\",\"updated\"],[\"3\",\"keep\"],[\"4\",\"inserted\"]]'); COMMIT;").await?;
    let ctx: Value = serde_json::from_str(&admin.query_one(
        "SELECT ctx::text FROM gateway_fixture.saved WHERE label='view'",&[]).await?.get::<_,String>(0))?;
    let physical = ctx["physical"].as_str().ok_or("physical")?;
    let run = ctx["run"].as_str().ok_or("run")?;
    let generation = ctx["generation"].as_i64().ok_or("generation")?;
    let target_generation = ctx["fence"].as_i64().ok_or("fence")?;
    let columns:Vec<String> = serde_json::from_value(ctx["pcs"].clone())?;
    let facts=sealed_facts(&admin,run).await?;
    save(&out,"sealed-catalog-facts",&facts)?;
    let config_sha = facts["config_digest"].as_str().ok_or("config digest")?;
    let native_columns:Vec<Value> = facts["tables"][0]["columns"].as_array().ok_or("columns")?
        .iter().map(|c|json!({"logical_name":c["logical_column"],"stored_physical":c["physical_name"],
            "oid":c["atttypid"],"typmod":c["atttypmod"],"caveat":null})).collect();
    let mut entry = facts.clone();
    entry["tables"] = json!([{"logical_schema":"app","logical_table":"orders",
        "stored_namespace":facts["tables"][0]["stored_namespace"],"stored_physical":physical,"columns":native_columns}]);
    let mut vector_entry = facts.clone();
    vector_entry["dataset"] = json!("app.orders");
    vector_entry["stored_namespace"] = facts["tables"][0]["stored_namespace"].clone();
    vector_entry["stored_physical"] = json!(physical);
    let vector = json!([vector_entry]);
    let catalog_binding = json!({"entries":[entry]});
    save(&out,"native-vector",&vector)?;
    save(&out,"native-catalog-binding",&catalog_binding)?;
    let (publication,publication_context)=publication_for(&admin,&ctx,&facts).await?;
    save(&out,"publication-input",&publication_context)?;
    let fence=rc::FenceRef {schema_name:"_oo",relation_name:physical,role_name:ROLE};
    step!("01-install",rc::install_target(&conn,&fence));
    step!("02-close-before-publish",rc::close_target(&conn,&fence));
    step!("03-stage-before-publish",rc::grant_target(&conn,&fence,&columns));
    let policy=policy_for(config_sha,generation);
    let fresh_policy=ColumnAclPolicy {login:FRESH_LOGIN.into(),..policy.clone()};
    provisioning_refused(&mut admin,&fresh_policy,&out,"03b-unpublished-reader-refused").await?;
    step!("04-native-publish",rc::open_native_target(&conn,&fence,&columns,&pack.pack_digest,"v1","c5_gateway",&publication));
    let definition=admin.query_one("SELECT pg_get_viewdef(view_oid,false),delta_name FROM _oo.c5_overlay WHERE physical_name=$1",&[&physical]).await?;
    let original:String=definition.get(0); let delta:String=definition.get(1);
    // Keep the real published cut, role ACLs, column types and view OID unchanged.
    admin.batch_execute(&format!("CREATE OR REPLACE VIEW _oo.\"{physical}\" WITH (security_barrier=true) AS \
        SELECT * FROM _oo.\"{delta}\" UNION ALL SELECT * FROM _oo.\"{delta}\" WHERE false")).await?;
    provisioning_refused(&mut admin,&fresh_policy,&out,"04b-arbitrary-view-reader-refused").await?;
    admin.batch_execute(&format!("CREATE OR REPLACE VIEW _oo.\"{physical}\" WITH (security_barrier=true) AS {original}")).await?;
    admin.batch_execute("ALTER FUNCTION _oo.c5_overlay_visible(text,name) RENAME TO c5_overlay_visible_fixture_hidden").await?;
    provisioning_refused(&mut admin,&fresh_policy,&out,"04c-ungated-api-reader-refused").await?;
    admin.batch_execute("ALTER FUNCTION _oo.c5_overlay_visible_fixture_hidden(text,name) RENAME TO c5_overlay_visible").await?;
    let gate=format!("_oo.\"pv_{}\"()",&physical[3..]);
    admin.batch_execute(&format!("REVOKE EXECUTE ON FUNCTION {gate} FROM {ROLE}")).await?;
    provisioning_refused(&mut admin,&fresh_policy,&out,"04d-closed-gate-reader-refused").await?;
    admin.batch_execute(&format!("GRANT EXECUTE ON FUNCTION {gate} TO {ROLE}")).await?;
    assert_fresh_metadata_absent(&admin).await?;
    let provision=provision_mapping_reader_native_vector(&mut admin,FRESH_LOGIN,std::slice::from_ref(&fresh_policy)).await?;
    assert_eq!(provision.effect,oo_gateway::mapping_reader_access::MappingReaderEffect::Installed);
    let overlay_provisioning=json!({"outcome":"passed","effect":"Installed","login":FRESH_LOGIN,
        "prior_metadata_grants":false,"arbitrary_view_refused":true,"missing_gate_api_refused":true,
        "closed_gate_refused":true,"unpublished_refused":true});
    save(&out,"05-overlay-reader-provision",&overlay_provisioning)?;
    let replay=provision_mapping_reader_native_vector(&mut admin,FRESH_LOGIN,std::slice::from_ref(&fresh_policy)).await?;
    assert_eq!(replay.effect,oo_gateway::mapping_reader_access::MappingReaderEffect::Replay);
    save(&out,"05b-overlay-reader-replay",&json!({"effect":"Replay","login":FRESH_LOGIN}))?;
    let gateway=Gateway {endpoint:TlsEndpoint {host:"127.0.0.1".into(),port:endpoint.port,database:endpoint.database.clone(),user:LOGIN.into()},
        password:password.trim().into(),ca:ca.clone(),identity:PolicyIdentity {project:SOURCE.into(),role:ROLE.into(),policy_id:config_sha.into()},
        policies:vec![compile_column_acl(&policy)?],vector,catalog_binding,evidence:out.clone()};
    let query="SELECT k1, value FROM app.orders ORDER BY k1";
    let fresh_gateway=Gateway {endpoint:TlsEndpoint {user:FRESH_LOGIN.into(),..gateway.endpoint.clone()},
        password:gateway.password.clone(),ca:ca.clone(),identity:gateway.identity.clone(),
        policies:vec![compile_column_acl(&fresh_policy)?],vector:gateway.vector.clone(),
        catalog_binding:gateway.catalog_binding.clone(),evidence:out.clone()};
    let (fresh_rows,_)=fresh_gateway.query("05c-fresh-reader-gateway-query",query,None).await?;
    assert_eq!(fresh_rows["status"],"Succeeded");
    assert_eq!(fresh_rows["returned_rows"],3);
    let (opened,cached)=gateway.query("06-initial-open-query",query,None).await?;
    assert_eq!(opened["status"],"Succeeded");
    assert_eq!(opened["rows"],json!([[{"int":"1"},{"text":"updated"}],[{"int":"3"},{"text":"keep"}],[{"int":"4"},{"text":"inserted"}]]));
    assert_eq!(cached.relations[0].physical_name,physical);
    assert_eq!(cached.relations[0].estimated_rows,3);
    let reader=connect(&endpoint,LOGIN,password.trim(),&ca).await?;
    let reader_tls:bool=reader.query_one("SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()",&[]).await?.get(0);
    assert!(reader_tls);
    reader.batch_execute(&format!("SET ROLE {ROLE}")).await?;
    let role_facts=reader.query_one("SELECT current_user::text,session_user::text, \
        current_setting('transaction_read_only'),r.rolsuper,r.rolbypassrls, \
        pg_has_role(session_user,'oo_pipeline_runtime','MEMBER'), \
        pg_has_role(session_user,'oo_target_owner','MEMBER') \
        FROM pg_roles r WHERE rolname=current_user",&[]).await?;
    assert_eq!(role_facts.get::<_,String>(0),ROLE);
    assert_eq!(role_facts.get::<_,String>(1),LOGIN);
    assert_eq!(role_facts.get::<_,String>(2),"on");
    for i in 3..7 { assert!(!role_facts.get::<_,bool>(i)); }
    let hidden=reader.query_one(&format!("SELECT _oo_pos FROM _oo.\"{physical}\""),&[]).await.unwrap_err();
    let raw=reader.query_one(&format!("SELECT * FROM _oo.\"{}\"",base_fence.relation_name),&[]).await.unwrap_err();
    assert_eq!(hidden.code().map(|c|c.code()),Some("42501"));
    assert_eq!(raw.code().map(|c|c.code()),Some("42501"));
    save(&out,"06b-reader-identity-acl",&json!({"current_user":ROLE,"session_user":LOGIN,"tls":true,
        "read_only":true,"superuser":false,"bypassrls":false,"pipeline_member":false,"owner_member":false,
        "hidden_column_sqlstate":"42501","raw_root_sqlstate":"42501"}))?;
    drop(reader);
    step!("07-close",rc::close_target(&conn,&fence));
    let (closed,_)=gateway.query("08-closed-query",query,None).await?;
    assert_eq!(closed["phase"],"guard");
    let (stale,_)=gateway.query("09-closed-cached-catalog-query",query,Some(&cached)).await?;
    assert_ne!(stale["status"],"Succeeded");
    assert!(stale["rows"].as_array().is_some_and(Vec::is_empty));
    step!("10-stage",rc::grant_target(&conn,&fence,&columns));
    step!("10b-observe-staged",rc::observe_target(&conn,&fence,&columns,LOGIN,password.trim()));
    let (staged,_)=gateway.query("11-staged-query",query,None).await?;
    assert_eq!(staged["status"],"Succeeded"); assert_eq!(staged["returned_rows"],0);
    step!("12-native-replay-open",rc::open_native_target(&conn,&fence,&columns,&pack.pack_digest,"v1","c5_gateway",&publication));
    rc::install_visibility_reader(&conn,LOGIN).await.map_err(|e|e.to_string())?;
    let mut visibility_columns = columns.clone();
    visibility_columns.sort();
    let bindings=vec![("_oo".to_owned(),physical.to_owned(),ROLE.to_owned(),visibility_columns)];
    let mapping=json!({"datasets":[{"dataset":"app.orders","source":SOURCE,"config_digest":config_sha,"generation":generation}],
        "publication":publication_context["publication"]});
    let barrier=rc::primary_visibility_barrier(&conn,LOGIN,password.trim(),rc::VisibilityRef {
        bindings:&bindings,version:"v1",pack:&pack.pack_digest,candidate:"c5_gateway",mapping:Some(&mapping),
    }).await.map_err(|e|e.to_string())?;
    assert_eq!(barrier["barrier"],"visible");
    save(&out,"12b-coordinator-reader-snapshot",&barrier)?;
    let (reopened,_)=gateway.query("13-reopened-query",query,None).await?;
    assert_eq!(reopened["rows"],opened["rows"]);
    let (hidden,_)=gateway.query("14-hidden-column-query","SELECT _oo_pos FROM app.orders",None).await?;
    assert_eq!(hidden["phase"],"guard");
    step!("15-close-for-removal",rc::close_target(&conn,&fence));
    step!("16-remove-value",rc::open_target(&conn,&fence,&columns[..1],&pack.pack_digest,"v2","c5_gateway_v2"));
    let (removed,_)=gateway.query("17-removed-column-query","SELECT value FROM app.orders",None).await?;
    assert_eq!(removed["phase"],"guard");
    step!("18-close-for-regrant",rc::close_target(&conn,&fence));
    let regrant=step!("19-regrant-refuses-history",rc::open_target(&conn,&fence,&columns,&pack.pack_digest,"v3","c5_gateway_v3"));
    assert_eq!(regrant.body["revoked_columns"],json!([columns[1]]));
    let (still_removed,_)=gateway.query("20-regrant-column-query","SELECT value FROM app.orders",None).await?;
    assert_eq!(still_removed["phase"],"guard");
    let (key_only,_)=gateway.query("21-retained-column-query","SELECT k1 FROM app.orders ORDER BY k1",None).await?;
    assert_eq!(key_only["returned_rows"],3);
    step!("22-close-before-revoke",rc::close_target(&conn,&fence));
    step!("23-revoke",rc::revoke_target(&conn,&fence));
    let (revoked,_)=gateway.query("24-revoked-query",query,None).await?;
    assert_eq!(revoked["phase"],"guard");
    let (revoked_cached,_)=gateway.query("25-revoked-cached-query",query,Some(&cached)).await?;
    assert_ne!(revoked_cached["status"],"Succeeded");
    let definition=admin.query_one("SELECT pg_get_viewdef(view_oid,false),delta_name FROM _oo.c5_overlay WHERE physical_name=$1",&[&physical]).await?;
    let original:String=definition.get(0); let delta:String=definition.get(1);
    admin.batch_execute(&format!("CREATE OR REPLACE VIEW _oo.\"{physical}\" WITH (security_barrier=true) AS \
        SELECT * FROM _oo.\"{delta}\" UNION ALL SELECT * FROM _oo.\"{delta}\" WHERE false")).await?;
    assert!(gateway.query("26-forged-view-query",query,None).await.is_err());
    admin.batch_execute(&format!("CREATE OR REPLACE VIEW _oo.\"{physical}\" WITH (security_barrier=true) AS {original}")).await?;
    let db=admin.query_one("SELECT r.state,c.relkind::text,_oo.c5_overlay_is_valid(t.physical_name), \
        (SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()) \
        FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id) \
        JOIN pg_class c ON c.oid=to_regclass(format('_oo.%I',t.physical_name)) WHERE r.run_id=$1::text::uuid",&[&run]).await?;
    assert_eq!(db.get::<_,String>(0),"published"); assert_eq!(db.get::<_,String>(1),"v");
    assert!(db.get::<_,bool>(2)); assert!(db.get::<_,bool>(3));
    let result=json!({"outcome":"passed",
        "query_acceptance":"passed","scope":"owned synthetic C5 coordinator + Gateway native-vector catalog/guard/executor over TLS",
        "publication":"published","overlay_reader_provisioning":overlay_provisioning,"relkind":"v","registered_view_valid":true,"open_rows":3,"staged_rows":0,
        "closed_denied":true,"revoked_denied":true,"cached_catalog_cannot_bypass":true,"hidden_column_denied":true,
        "permanent_revoke_preserved":true,"forged_view_catalog_denied":true,"coordinator_reader_snapshot":"visible",
        "pack_digest":pack.pack_digest,"run_id":run,
        "source_generation":generation,"target_generation":target_generation,"database":endpoint.database,
        "customer_or_eip_traffic":false,"http_token_routing":"not_run","worker_producer_e2e":"main_owned_not_run",
        "evidence":out});
    save(&out,"result",&result)?;
    println!("{result}");
    Ok(())
}
