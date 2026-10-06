//! One int8 quantity table through import, ACL, mapping, analyze, and query.
//! Policy generation is the stored mapping run generation. The target fence
//! is recorded and is not written onto the policy.

use std::fs;
use std::path::Path;

use oo_gateway::{connect_require_tls, TlsEndpoint};
use serde_json::{json, Value};
use tokio_postgres::Client;

use crate::catalog::{self, decision, fingerprints_of, parse_policies, run_policy, Col, Identity, TableSpec, AMOUNT_I8};
use crate::proc::{self, Spawn, Stop};

const SOURCE_ID: &str = "a259qty";
const SOURCE_DB: &str = "oo_a259_src";
const TARGET_DB: &str = "oo_a259_dst";
const READER: &str = "oo_a259_reader";
const RUNTIME: &str = "oo_a259_runtime";
const LOGIN: &str = "oo_a259_login";
const ADMIN: &str = "oo_fixture_admin";
const SHARD: &str = "a259main";
const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a259";
const SOURCE_CUT: i64 = 48;

struct Live {
    host: String,
    port: u16,
    admin_password: String,
    reader_password: String,
    runtime_password: String,
    login_password: String,
    ca: Vec<u8>,
    evidence: std::path::PathBuf,
}

pub async fn run() -> Result<(), Stop> {
    let live = load().await?;
    let steps = live.evidence.join("steps");
    fs::create_dir_all(&steps).map_err(|_| Stop::new("evidence", "mkdir"))?;
    let secrets = [
        live.admin_password.as_str(),
        live.reader_password.as_str(),
        live.runtime_password.as_str(),
        live.login_password.as_str(),
    ];
    prepare(&live).await?;
    let early = command("OO_A259_PIPELINE", &["import-pg"], &import_request(&live)?)?;
    save_checked(&steps, "import-pg-not-ready", &early, &secrets)?;
    let early_err = String::from_utf8_lossy(&early.stderr);
    if early.code != 1 || !early.stdout.is_empty() || !early_err.contains("target schema is not ready") {
        return Err(Stop::new("import-pg-not-ready", format!("exit {}", early.code)));
    }
    let init = command("OO_A259_INIT_PG", &[], &init_request(&live)?)?;
    save_checked(&steps, "init-pg", &init, &secrets)?;
    if init.code != 0 || !init.stderr.is_empty() {
        return Err(Stop::new("init-pg", format!("exit {}", init.code)));
    }
    let init_doc = parse_stdout("init-pg", &init.stdout)?;
    if init_doc["protocol"] != "oo.pipeline.init-pg-result/v1"
        || init_doc["install"] != "bootstrapped"
        || init_doc["full_import"] != "installed"
        || init_doc["historical_v2"] != false
    {
        return Err(Stop::new("init-pg", "result"));
    }
    grant_runtime(&live).await?;
    let imported = command("OO_A259_PIPELINE", &["import-pg"], &import_request(&live)?)?;
    save_checked(&steps, "import-pg", &imported, &secrets)?;
    if imported.code != 0 || !imported.stderr.is_empty() {
        return Err(Stop::new("import-pg", format!("exit {}", imported.code)));
    }
    let doc = parse_stdout("import-pg", &imported.stdout)?;
    let facts = read_generations(&live, &doc).await?;
    fs::write(
        live.evidence.join("generations.txt"),
        format!(
            "source_generation={}\ntarget_generation={}\nrun_generation={}\nfence_generation={}\nconfig_digest={}\n",
            facts.source_generation, facts.target_generation, facts.run_generation, facts.fence, facts.digest
        ),
    )
    .map_err(|_| Stop::new("generations", "write"))?;
    if facts.source_generation != SOURCE_CUT || facts.run_generation != facts.source_generation {
        return Err(Stop::new("mapping", "run_generation is not the requested source cut"));
    }
    if facts.fence != facts.target_generation || facts.run_generation == facts.fence {
        return Err(Stop::new("mapping", "target fence was not kept distinct from the source cut"));
    }
    let stdout = emit_policy(&facts)?;
    fs::write(live.evidence.join("reviewed-policy.stdout"), &stdout).map_err(|_| Stop::new("reviewed-policy", "write"))?;
    let policies = parse_policies(&stdout)?;
    if policies.len() != 1 || policies[0].generation != facts.run_generation || policies[0].generation == facts.fence {
        return Err(Stop::new("reviewed-policy", "policy generation is not the mapping run generation"));
    }
    if policies[0].config_digest != facts.digest {
        return Err(Stop::new("reviewed-policy", "policy digest is not the mapping digest"));
    }
    let amount = catalog::column(&policies[0], "amount")?;
    if amount.semantic_type != "quantity" || amount.pii != oo_gateway::PiiClass::None {
        return Err(Stop::new("reviewed-policy", "amount classification"));
    }
    let policy_doc: Value = serde_json::from_str(&stdout).map_err(|_| Stop::new("reviewed-policy", "json"))?;
    install_and_provision(&live, &policy_doc, &secrets).await?;
    analyze(&live, &facts, &secrets)?;
    query(&live, &facts, &policy_doc, &secrets)?;
    let outcome = json!({
        "fixed_commit": "e447249ab1a7f4c82df4729090b1285c9ef4a4ec",
        "parent_acceptance": false,
        "development_unit": false,
        "component_approval": false,
        "m1_i01": "not_accepted",
        "m1_i02": "not_accepted",
        "source_generation": facts.source_generation,
        "target_generation": facts.target_generation,
        "run_generation": facts.run_generation,
        "fence_generation": facts.fence,
        "policy_generation": policies[0].generation,
        "import_run_id": facts.run_id,
        "policy_generation_is_fence": false,
        "amount_semantic_type": "quantity",
        "query_amounts": AMOUNT_I8,
        "authentication": "not_claimed",
        "authorization": "not_claimed",
        "wal": "missing",
        "publication": "missing"
    });
    fs::write(
        live.evidence.join("outcome.json"),
        format!("{outcome}\n"),
    )
    .map_err(|_| Stop::new("outcome", "write"))?;
    println!("bound=ok");
    Ok(())
}

struct Facts {
    source_generation: i64,
    target_generation: i64,
    run_generation: i64,
    fence: i64,
    digest: String,
    run_id: String,
}

async fn load() -> Result<Live, Stop> {
    let port: u16 = proc::env_var("OO_A259_PORT")?
        .parse()
        .map_err(|_| Stop::new("env", "OO_A259_PORT"))?;
    let ca_path = proc::env_var("OO_A259_CA_FILE")?;
    Ok(Live {
        host: proc::env_var("OO_A259_HOST")?,
        port,
        admin_password: proc::read_hex(&proc::env_var("OO_A259_ADMIN_PASSWORD_FILE")?)?,
        reader_password: proc::read_hex(&proc::env_var("OO_A259_READER_PASSWORD_FILE")?)?,
        runtime_password: proc::read_hex(&proc::env_var("OO_A259_RUNTIME_PASSWORD_FILE")?)?,
        login_password: proc::read_hex(&proc::env_var("OO_A259_LOGIN_PASSWORD_FILE")?)?,
        ca: fs::read(&ca_path).map_err(|_| Stop::new("ca", "unreadable"))?,
        evidence: proc::evidence()?.join("live"),
    })
}

async fn dial(live: &Live, user: &str, password: &str, database: &str) -> Result<Client, Stop> {
    let endpoint = TlsEndpoint {
        host: live.host.clone(),
        port: live.port,
        database: database.to_string(),
        user: user.to_string(),
    };
    let (client, conn) = connect_require_tls(&endpoint, password, &live.ca)
        .await
        .map_err(|_| Stop::new("tls", "connect"))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Ok(client)
}

fn db(step: &str, err: tokio_postgres::Error) -> Stop {
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_else(|| "none".to_string());
    Stop::new(step, format!("sqlstate {state}"))
}

async fn exec(client: &Client, step: &str, sql: &str) -> Result<(), Stop> {
    client.simple_query(sql).await.map_err(|err| db(step, err))?;
    Ok(())
}

async fn texts(client: &Client, step: &str, sql: &str) -> Result<Vec<Vec<String>>, Stop> {
    let rows = client.query(sql, &[]).await.map_err(|err| db(step, err))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells = Vec::new();
        for index in 0..row.len() {
            cells.push(row.try_get::<_, String>(index).map_err(|err| db(step, err))?);
        }
        out.push(cells);
    }
    Ok(out)
}

async fn prepare(live: &Live) -> Result<(), Stop> {
    let admin = dial(live, ADMIN, &live.admin_password, "postgres").await?;
    let ssl = texts(
        &admin,
        "tls",
        "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
    )
    .await?;
    let version = texts(&admin, "tls", "SHOW server_version").await?;
    if ssl.first().and_then(|row| row.first()).map(String::as_str) != Some("t")
        && ssl.first().and_then(|row| row.first()).map(String::as_str) != Some("true")
    {
        return Err(Stop::new("tls", "ssl is not true"));
    }
    fs::write(
        live.evidence.join("server.txt"),
        format!(
            "ssl={}\nserver_version={}\n",
            ssl[0][0],
            version.first().and_then(|row| row.first()).map(String::as_str).unwrap_or("")
        ),
    )
    .map_err(|_| Stop::new("server", "write"))?;
    for (name, password, inherit) in [
        (READER, live.reader_password.as_str(), "INHERIT"),
        (RUNTIME, live.runtime_password.as_str(), "INHERIT"),
        (LOGIN, live.login_password.as_str(), "NOINHERIT"),
    ] {
        exec(
            &admin,
            "role",
            &format!(
                "CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION {inherit}"
            ),
        )
        .await?;
    }
    for name in [SOURCE_DB, TARGET_DB] {
        exec(
            &admin,
            "database",
            &format!("CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"),
        )
        .await?;
    }
    exec(&admin, "grant", &format!("GRANT CONNECT ON DATABASE {SOURCE_DB} TO {READER}")).await?;
    exec(
        &admin,
        "grant",
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}"),
    )
    .await?;
    drop(admin);
    let fixture = fs::read_to_string(proc::env_var("OO_A259_FIXTURE")?).map_err(|_| Stop::new("fixture", "read"))?;
    for needle in AMOUNT_I8 {
        if !fixture.contains(needle) {
            return Err(Stop::new("fixture", "int8 extreme is absent"));
        }
    }
    let source = dial(live, ADMIN, &live.admin_password, SOURCE_DB).await?;
    exec(&source, "fixture", &fixture).await?;
    drop(source);
    let reader = dial(live, READER, &live.reader_password, SOURCE_DB).await?;
    let rows = texts(
        &reader,
        "source",
        "SELECT ticket_id::text, amount::text FROM qty.ticket ORDER BY ticket_id",
    )
    .await?;
    let expect = vec![
        vec!["1".to_string(), AMOUNT_I8[0].to_string()],
        vec!["2".to_string(), AMOUNT_I8[1].to_string()],
        vec!["3".to_string(), AMOUNT_I8[2].to_string()],
    ];
    if rows != expect {
        return Err(Stop::new("source", "int8 extremes were not stored"));
    }
    Ok(())
}

async fn grant_runtime(live: &Live) -> Result<(), Stop> {
    let admin = dial(live, ADMIN, &live.admin_password, "postgres").await?;
    exec(
        &admin,
        "grant",
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}"),
    )
    .await?;
    exec(&admin, "grant", &format!("GRANT oo_pipeline_runtime TO {RUNTIME}")).await?;
    Ok(())
}

fn source_object(live: &Live) -> Value {
    json!({
        "source_id": SOURCE_ID,
        "host": live.host,
        "port": live.port,
        "database": SOURCE_DB,
        "username": READER,
        "secret_ref": "secret://pg/a259/reader",
        "password_file": "/secrets/reader.pass",
        "ca_file": "/secrets/ca.crt",
        "tables": [{"schema": "qty", "table": "ticket"}]
    })
}

fn import_request(live: &Live) -> Result<Vec<u8>, Stop> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.import-pg/v1",
        "source": source_object(live),
        "target": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "runtime_principal": RUNTIME,
            "password_file": "/secrets/runtime.pass",
            "ca_file": "/secrets/ca.crt"
        },
        "shard": SHARD,
        "owner": OWNER,
        "source_generation": SOURCE_CUT
    }))
    .map_err(|_| Stop::new("import-pg", "encode"))
}

fn init_request(live: &Live) -> Result<Vec<u8>, Stop> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "connect_ms": 30000,
        "target": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "administrator": ADMIN,
            "password_file": "/secrets/admin.pass",
            "ca_file": "/secrets/ca.crt"
        }
    }))
    .map_err(|_| Stop::new("init-pg", "encode"))
}

fn command(env_name: &str, args: &[&str], stdin_text: &[u8]) -> Result<Spawn, Stop> {
    let program = proc::bin(env_name)?;
    proc::spawn(&program, args, stdin_text)
}

fn save_checked(dir: &Path, name: &str, output: &Spawn, secrets: &[&str]) -> Result<(), Stop> {
    proc::reject_secrets(&output.stdout, secrets)?;
    proc::reject_secrets(&output.stderr, secrets)?;
    proc::save_step(dir, name, output)
}

fn parse_stdout(step: &str, stdout: &[u8]) -> Result<Value, Stop> {
    let text = proc::utf8(step, stdout)?;
    let trimmed = text.trim_end();
    if !text.ends_with('\n') || trimmed.is_empty() {
        return Err(Stop::new(step, "stdout shape"));
    }
    serde_json::from_str(trimmed).map_err(|_| Stop::new(step, "json"))
}

async fn read_generations(live: &Live, doc: &Value) -> Result<Facts, Stop> {
    if doc["protocol"] != "oo.pipeline.import-pg-result/v1"
        || doc["state"] != "awaiting_publish"
        || doc["table_count"].as_i64() != Some(1)
        || doc["tables"][0]["name"] != "ticket"
        || doc["tables"][0]["rows"].as_i64() != Some(3)
    {
        return Err(Stop::new("import-pg", "result"));
    }
    let source_generation = doc["source_generation"].as_i64().ok_or_else(|| Stop::new("import-pg", "source_generation"))?;
    let target_generation = doc["target_generation"].as_i64().ok_or_else(|| Stop::new("import-pg", "target_generation"))?;
    let run_id = doc["run_id"].as_str().ok_or_else(|| Stop::new("import-pg", "run_id"))?.to_string();
    if run_id.len() != 36 || !run_id.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-') {
        return Err(Stop::new("import-pg", "run_id shape"));
    }
    let admin = dial(live, ADMIN, &live.admin_password, TARGET_DB).await?;
    let sealed = texts(
        &admin,
        "import-pg",
        &format!("SELECT source_generation::text, target_generation::text FROM _oo.full_import_run WHERE run_id = '{run_id}'"),
    )
    .await?;
    if sealed.len() != 1 || sealed[0].len() != 2 {
        return Err(Stop::new("import-pg", "sealed row"));
    }
    let mapping = texts(
        &admin,
        "mapping",
        "SELECT DISTINCT config_digest, run_generation::text FROM _oo.physical_mappings WHERE source = 'a259qty'",
    )
    .await?;
    if mapping.len() != 1 {
        return Err(Stop::new("mapping", "digest and run_generation were not unique"));
    }
    let fence_rows = texts(
        &admin,
        "fence",
        "SELECT generation::text FROM _oo.target_generations WHERE shard_id = 'a259main' AND state = 'adopted'",
    )
    .await?;
    if fence_rows.len() != 1 {
        return Err(Stop::new("fence", "adopted generation"));
    }
    let run_generation: i64 = mapping[0][1].parse().map_err(|_| Stop::new("mapping", "run_generation"))?;
    let fence: i64 = fence_rows[0][0].parse().map_err(|_| Stop::new("fence", "generation"))?;
    let sealed_source: i64 = sealed[0][0].parse().map_err(|_| Stop::new("import-pg", "sealed source"))?;
    let sealed_target: i64 = sealed[0][1].parse().map_err(|_| Stop::new("import-pg", "sealed target"))?;
    if sealed_source != source_generation || sealed_target != target_generation || fence != sealed_target {
        return Err(Stop::new("import-pg", "sealed generations disagree with the import result"));
    }
    Ok(Facts {
        source_generation,
        target_generation,
        run_generation,
        fence,
        digest: mapping[0][0].clone(),
        run_id,
    })
}

fn emit_policy(facts: &Facts) -> Result<String, Stop> {
    let id = Identity {
        project: SOURCE_ID,
        source_id: SOURCE_ID,
        database: SOURCE_DB,
        digest: facts.digest.clone(),
        generation: facts.run_generation,
        role: "clerk",
        login: LOGIN,
    };
    if id.generation == facts.fence {
        return Err(Stop::new("reviewed-policy", "refusing to stamp the target fence"));
    }
    let table = TableSpec {
        schema: "qty",
        name: "ticket",
        columns: vec![
            Col { attnum: 1, name: "ticket_id", oid: 20, type_name: "int8" },
            Col { attnum: 2, name: "amount", oid: 20, type_name: "int8" },
        ],
        pk: vec![(1, "ticket_id")],
    };
    let catalog_doc = catalog::catalog_json(&table, &id);
    let artifact_json = proc::run_ok(
        "OO_A259_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog_doc).map_err(|_| Stop::new("catalog", "encode"))?,
    )?;
    let artifact: Value = serde_json::from_str(&artifact_json).map_err(|_| Stop::new("captured-pg-draft", "json"))?;
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "schema hash"))?
        .to_string();
    let prints = fingerprints_of(&artifact)?;
    let decisions = vec![
        decision(&prints, "qty.ticket", &schema_hash, "ticket_id", "none", "identifier")?,
        decision(&prints, "qty.ticket", &schema_hash, "amount", "none", "quantity")?,
    ];
    let chain = catalog::compile_table(&table, &id, decisions)?;
    let command: Value = serde_json::from_str(&chain.command_json).map_err(|_| Stop::new("reviewed-effective", "json"))?;
    let amount = command["effective_oos"]["datasets"][0]["columns"]
        .as_array()
        .ok_or_else(|| Stop::new("reviewed-effective", "columns"))?
        .iter()
        .find(|item| item["name"] == "amount")
        .ok_or_else(|| Stop::new("reviewed-effective", "amount"))?;
    if amount["semantic_type"] != "quantity" {
        return Err(Stop::new("reviewed-effective", "amount was reclassified"));
    }
    let text = catalog::operator_for(
        &chain,
        &id,
        "qty.ticket",
        &[("ticket_id", false), ("amount", false)],
    )?;
    let out = run_policy(&text)?;
    if out.code != 0 || !out.stderr.is_empty() {
        return Err(Stop::new(
            "reviewed-policy",
            format!("exit {} {}", out.code, String::from_utf8_lossy(&out.stderr)),
        ));
    }
    let again = run_policy(&text)?;
    if again.stdout != out.stdout {
        return Err(Stop::new("reviewed-policy", "stdout was not repeatable"));
    }
    proc::utf8("reviewed-policy", &out.stdout)
}

async fn install_and_provision(live: &Live, policy_doc: &Value, secrets: &[&str]) -> Result<(), Stop> {
    let policies = policy_doc["policies"].clone();
    let install = json!({
        "protocol": "oo.gateway.install-column-acl-cli/v1",
        "policies": policies,
        "endpoint": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "user": ADMIN
        },
        "password_file": "/secrets/admin.pass",
        "ca_file": "/secrets/ca.crt"
    });
    let install_bytes = serde_json::to_vec(&install).map_err(|_| Stop::new("install-column-acl", "encode"))?;
    proc::reject_secrets(&install_bytes, secrets)?;
    fs::write(live.evidence.join("install-column-acl.request.json"), &install_bytes)
        .map_err(|_| Stop::new("install-column-acl", "request"))?;
    let installed = command("OO_A259_INSTALL_COLUMN_ACL", &[], &install_bytes)?;
    save_checked(&live.evidence.join("steps"), "install-column-acl", &installed, secrets)?;
    if installed.code != 0 || !installed.stderr.is_empty() {
        return Err(Stop::new("install-column-acl", format!("exit {}", installed.code)));
    }
    let install_doc = serde_json::from_slice::<Value>(&installed.stdout).map_err(|_| Stop::new("install-column-acl", "json"))?;
    if install_doc["effect"] != "installed" || install_doc["role"] != "oo_r_clerk" {
        return Err(Stop::new("install-column-acl", "effect"));
    }
    let provision = json!({
        "protocol": "oo.gateway.provision-mapping-reader-cli/v1",
        "policies": policies,
        "login": LOGIN,
        "endpoint": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "user": ADMIN
        },
        "password_file": "/secrets/admin.pass",
        "ca_file": "/secrets/ca.crt"
    });
    let provision_bytes = serde_json::to_vec(&provision).map_err(|_| Stop::new("provision-mapping-reader", "encode"))?;
    proc::reject_secrets(&provision_bytes, secrets)?;
    fs::write(live.evidence.join("provision-mapping-reader.request.json"), &provision_bytes)
        .map_err(|_| Stop::new("provision-mapping-reader", "request"))?;
    let provisioned = command("OO_A259_PROVISION", &[], &provision_bytes)?;
    save_checked(&live.evidence.join("steps"), "provision-mapping-reader", &provisioned, secrets)?;
    if provisioned.code != 0 || !provisioned.stderr.is_empty() {
        return Err(Stop::new("provision-mapping-reader", format!("exit {}", provisioned.code)));
    }
    let provision_doc = serde_json::from_slice::<Value>(&provisioned.stdout).map_err(|_| Stop::new("provision-mapping-reader", "json"))?;
    if provision_doc["effect"] != "installed" || provision_doc["login"] != LOGIN || provision_doc["role"] != "oo_r_clerk" {
        return Err(Stop::new("provision-mapping-reader", "effect"));
    }
    Ok(())
}

fn analyze(live: &Live, facts: &Facts, secrets: &[&str]) -> Result<(), Stop> {
    if facts.run_generation == facts.fence {
        return Err(Stop::new("analyze-pg", "refusing to send the target fence as source_generation"));
    }
    let request = json!({
        "protocol": "oo.pipeline.analyze-pg/v1",
        "project": SOURCE_ID,
        "source": SOURCE_ID,
        "config_digest": facts.digest,
        "source_generation": facts.run_generation,
        "tables": [{"namespace": "qty", "name": "ticket"}],
        "statement_timeout_ms": 5000,
        "lock_timeout_ms": 5000,
        "connect_ms": 30000,
        "target": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "operator": ADMIN,
            "password_file": "/secrets/admin.pass",
            "ca_file": "/secrets/ca.crt"
        }
    });
    let bytes = serde_json::to_vec(&request).map_err(|_| Stop::new("analyze-pg", "encode"))?;
    proc::reject_secrets(&bytes, secrets)?;
    fs::write(live.evidence.join("analyze-pg.request.json"), &bytes).map_err(|_| Stop::new("analyze-pg", "request"))?;
    let output = command("OO_A259_ANALYZE", &[], &bytes)?;
    save_checked(&live.evidence.join("steps"), "analyze-pg", &output, secrets)?;
    if output.code != 0 || !output.stderr.is_empty() {
        return Err(Stop::new("analyze-pg", format!("exit {}", output.code)));
    }
    let receipt = parse_stdout("analyze-pg", &output.stdout)?;
    if receipt["source_generation"].as_i64() != Some(facts.run_generation)
        || receipt["source_generation"].as_i64() == Some(facts.fence)
        || receipt["published"] != false
        || receipt["estimate_is_exact"] != false
        || receipt["config_digest"] != facts.digest
    {
        return Err(Stop::new("analyze-pg", "receipt used the target fence or changed the cut"));
    }
    Ok(())
}

fn query(live: &Live, facts: &Facts, policy_doc: &Value, secrets: &[&str]) -> Result<(), Stop> {
    let mut policies = policy_doc["policies"]
        .as_array()
        .ok_or_else(|| Stop::new("query-pg", "policies"))?
        .clone();
    for policy in &mut policies {
        if let Some(object) = policy.as_object_mut() {
            object.remove("row_filter");
        }
    }
    let audit = live.evidence.join("query-ticket.jsonl");
    let request = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "user": LOGIN
        },
        "password_file": "/secrets/login.pass",
        "ca_file": "/secrets/ca.crt",
        "audit_file": audit,
        "identity": {
            "project": SOURCE_ID,
            "role": "oo_r_clerk",
            "policy_id": facts.digest
        },
        "policies": policies,
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1000000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": "stdin",
            "max_rows": 20,
            "max_result_bytes": 8192,
            "wall_clock_ms": 30000
        }
    });
    let bytes = serde_json::to_vec(&request).map_err(|_| Stop::new("query-pg", "encode"))?;
    proc::reject_secrets(&bytes, secrets)?;
    let path = live.evidence.join("query-ticket.request.json");
    fs::write(&path, &bytes).map_err(|_| Stop::new("query-pg", "request"))?;
    let sql = json!({
        "version": "oo.gateway.query-pg-turn/v1",
        "sql": "SELECT ticket_id, amount FROM qty.ticket ORDER BY ticket_id"
    });
    let mut stdin = serde_json::to_string(&sql).map_err(|_| Stop::new("query-pg", "sql"))?;
    stdin.push('\n');
    let program = proc::bin("OO_A259_QUERY")?;
    let output = proc::spawn(
        &program,
        &["--request", &path.to_string_lossy()],
        stdin.as_bytes(),
    )?;
    save_checked(&live.evidence.join("steps"), "query-ticket", &output, secrets)?;
    if output.code != 0 || !output.stderr.is_empty() {
        return Err(Stop::new("query-pg", format!("exit {} {}", output.code, String::from_utf8_lossy(&output.stderr))));
    }
    expect_amounts(&String::from_utf8_lossy(&output.stdout))?;
    Ok(())
}

fn expect_amounts(stdout: &str) -> Result<(), Stop> {
    let expect = [("1", AMOUNT_I8[0]), ("2", AMOUNT_I8[1]), ("3", AMOUNT_I8[2])];
    let mut got = Vec::new();
    let mut summaries = 0;
    for line in stdout.lines() {
        let value: Value = serde_json::from_str(line).map_err(|_| Stop::new("query-pg", "row json"))?;
        if value["record"] == "row" {
            let cells = value["cells"].as_array().ok_or_else(|| Stop::new("query-pg", "cells"))?;
            if cells.len() != 2 || cells[0]["kind"] != "int" || cells[1]["kind"] != "int" {
                return Err(Stop::new("query-pg", "cell shape"));
            }
            got.push((
                cells[0]["value"].as_str().unwrap_or("").to_string(),
                cells[1]["value"].as_str().unwrap_or("").to_string(),
            ));
        } else if value["record"] == "summary" {
            summaries += 1;
            if value["status"] != "succeeded"
                || value["returned_rows"].as_u64() != Some(3)
                || value["end_audit_ok"] != true
            {
                return Err(Stop::new("query-pg", "summary"));
            }
        } else {
            return Err(Stop::new("query-pg", "record"));
        }
    }
    if summaries != 1 || got.len() != 3 {
        return Err(Stop::new("query-pg", "row count"));
    }
    for (index, (ticket, amount)) in expect.iter().enumerate() {
        if got[index].0 != *ticket || got[index].1 != *amount {
            return Err(Stop::new(
                "query-pg",
                format!("amount text changed ticket {} amount {}", got[index].0, got[index].1),
            ));
        }
    }
    Ok(())
}
