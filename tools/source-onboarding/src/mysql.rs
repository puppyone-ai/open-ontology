use crate::{api, io::*, product};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn collection(entries: &[Value], expected: &[Value]) -> Result<Value> {
    require(!entries.is_empty(), "empty_collection")?;
    let mut datasets = BTreeSet::new();
    let mut vector = Vec::new();
    for e in entries {
        require(
            e["protocol"] == "oo.pipeline.sealed-import-catalog/v1"
                && e["state"] == "awaiting_publish"
                && e["shadows_queried"] == false,
            "sealed_catalog",
        )?;
        for t in array(e, "tables")? {
            let dataset = format!(
                "{}.{}",
                text(t, "logical_schema")?,
                text(t, "logical_table")?
            );
            require(datasets.insert(dataset.clone()), "duplicate_dataset")?;
            let mut b = json!({"dataset":dataset,"stored_namespace":t["stored_namespace"],"stored_physical":t["stored_physical"]});
            for k in [
                "source_id",
                "run_id",
                "source_generation",
                "target_generation",
                "config_digest",
                "schema_digest",
                "type_digest",
                "input_digest",
            ] {
                b[k] = e[k].clone();
            }
            vector.push(b);
        }
    }
    let wanted = expected
        .iter()
        .map(|s| Ok(format!("{}.{}", text(s, "schema")?, text(s, "table")?)))
        .collect::<Result<BTreeSet<_>>>()?;
    require(
        wanted.len() == expected.len() && datasets == wanted,
        "incomplete_collection",
    )?;
    let mut body = json!({"protocol":"oo.pipeline.sealed-import-collection/v1","entries":entries,"binding_vector":vector});
    body["collection_sha256"] = json!(sha(&bytes(&body)?));
    Ok(body)
}

fn published_types(table: &Value, attributes: &Value) -> Result<()> {
    let mut actual = BTreeMap::new();
    for attribute in attributes.as_array().ok_or("published_target_schema")? {
        let name = attribute[0].as_str().ok_or("published_target_schema")?;
        if !name.starts_with("_oo_") {
            require(
                actual
                    .insert(
                        name.to_owned(),
                        (attribute[1].clone(), attribute[2].clone()),
                    )
                    .is_none(),
                "published_target_schema",
            )?;
        }
    }
    let mut expected = BTreeMap::new();
    for column in array(table, "columns")? {
        require(
            expected
                .insert(
                    text(column, "stored_physical")?.to_owned(),
                    (column["oid"].clone(), column["typmod"].clone()),
                )
                .is_none(),
            "published_target_schema",
        )?;
    }
    require(actual == expected, "published_target_schema")
}
pub fn replace(
    c: &Value,
    calls: &mut Calls,
    container: &str,
    binary: &str,
    runtime: &str,
    label: &str,
) -> Result<()> {
    let helper = text(&c["onboarding"], "runtime_helper_container")?;
    calls.docker(
        &format!("{label}-stop"),
        container,
        &strings(&[helper, "stop-owned-api", binary]),
        None,
    )?;
    let prefix = calls.out.join(format!("{label}-serve"));
    let stdout = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(prefix.with_extension("stdout"))?;
    let stderr = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(prefix.with_extension("stderr"))?;
    use std::os::unix::fs::PermissionsExt;
    stdout.set_permissions(fs::Permissions::from_mode(0o600))?;
    stderr.set_permissions(fs::Permissions::from_mode(0o600))?;
    let mut child = Command::new("docker")
        .args(["exec", container, binary, "serve", "--config", runtime])
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    save(
        prefix.with_extension("handle.json"),
        &json!({"docker_exec_pid":child.id(),"container":container,"binary":binary,"runtime":runtime}),
    )?;
    std::thread::sleep(Duration::from_secs(1));
    require(child.try_wait()?.is_none(), "api_start")
}
/// Budgets for one network statement. The statement budget stays under the 600 s
/// child deadline in `io.rs`. The row and byte caps cover one 4096-row oracle page.
const SOURCE_SQL_STATEMENT_MS: u64 = 540_000;
const SOURCE_SQL_MAX_ROWS: u64 = 100_000;
const SOURCE_SQL_MAX_BYTES: u64 = 67_108_864;

/// How one source statement runs. The legacy path execs the mysql client into an
/// owned fixture container. The network path execs the pinned producer into the
/// trusted runtime container, which connects to the admitted source. Neither
/// path puts the password in argv.
#[derive(Debug, PartialEq)]
struct SourceCall {
    label: &'static str,
    container: String,
    args: Vec<String>,
    input: Option<Vec<u8>>,
}

fn source_call(c: &Value, sql: &str) -> Result<SourceCall> {
    let o = &c["onboarding"];
    let p = &o["product"];
    if p["source_access"] == "network" {
        return Ok(SourceCall {
            label: "source-sql-network",
            container: text(o, "container")?.into(),
            args: strings(&[text(o, "producer")?, "source-sql"]),
            input: Some(bytes(&source_request(c, sql)?)?),
        });
    }
    let container = text(p, "source_container")?;
    require(
        container.starts_with("oo-dl01w1-06-") || container == "oo-mysql57-reference-20261016" || (o["full"].is_object() && (container.starts_with("oo-yjcap-full-") || container.starts_with("oo-wf-batch-") || container.starts_with("oo-wf-mpart-"))),
        "owned_fixture_only",
    )?;
    Ok(SourceCall {
        label: "profile-sql",
        container: container.into(),
        args: strings(&[
            "mysql",
            &format!("--defaults-file={}", text(p, "source_client_file")?),
            "--default-character-set=utf8mb4",
            "--batch",
            "--raw",
            "--skip-column-names",
            "-e",
            sql,
        ]),
        input: None,
    })
}

/// The optional profiling account (`onboarding.product.profile_access`). Only the
/// account and its password file differ from the admitted binding. The endpoint,
/// CA and TLS name always come from the binding. It is used only by network-mode
/// source SQL, and the drift check never reads it.
#[derive(Debug, PartialEq)]
pub struct ProfileAccess {
    pub user: String,
    pub password_file: PathBuf,
}

/// Accepts exactly `{user, secret_ref}`. `secret_ref` resolves through the
/// operator's secret registry to a private file under `host_secret_root`. Naming
/// the CDC account is allowed. The producer's own inspect() still requires a
/// read-only account.
pub fn profile_access(c: &Value) -> Result<Option<ProfileAccess>> {
    let o = &c["onboarding"];
    let Some(access) = o["product"].get("profile_access") else {
        return Ok(None);
    };
    let keys = access
        .as_object()
        .ok_or("profile_access")?
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    require(
        keys == BTreeSet::from(["secret_ref", "user"]),
        "profile_access_keys",
    )?;
    let user = text(access, "user")?;
    require(!user.is_empty(), "profile_access_user")?;
    let registry = load(private(text(o, "secret_registry")?)?)?;
    let password = registry[text(access, "secret_ref")?]
        .as_str()
        .ok_or("profile_secret_ref")?;
    let password_file = private(password)?;
    require(
        password_file.starts_with(text(o, "host_secret_root")?),
        "secret_root",
    )?;
    Ok(Some(ProfileAccess {
        user: user.into(),
        password_file,
    }))
}

/// The `oo.mysql.source-sql/v1` request for the admitted source. `c` is the
/// operator after `connection()` has matched the six fields exactly. The
/// password is referenced by its container path under `/secrets`, the same
/// mapping `capture()` uses. Its contents are never read here. Without
/// `profile_access` the admitted CDC account and password are used.
fn source_request(c: &Value, sql: &str) -> Result<Value> {
    let o = &c["onboarding"];
    let (user, password) = match profile_access(c)? {
        Some(access) => (access.user, access.password_file),
        None => (
            text(c, "user")?.into(),
            PathBuf::from(text(c, "password_file")?),
        ),
    };
    private(&password)?;
    let port = c["port"]
        .as_u64()
        .filter(|port| (1..=65_535).contains(port))
        .ok_or("port")?;
    let relative = password.strip_prefix(text(o, "host_secret_root")?)?;
    let mut connection = json!({
        "host": text(c, "host")?,
        "port": port,
        "database": text(c, "database")?,
        "user": user,
        "password_file": if o["full"]["pipeline"]["runtime_transport"] == "host" {
            password.to_string_lossy().into_owned()
        } else {
            format!("/secrets/{}", relative.display())
        },
    });
    if let Some(policy) = o["full"].get("source_transport") {
        require(
            o["full"].is_object()
                && policy == &json!({"tls":"disabled","plaintext_authorized":true})
                && c.get("ca_file") == Some(&Value::Null)
                && c.get("tls_server_name") == Some(&Value::Null),
            "source_transport",
        )?;
        connection["tls"] = json!("disabled");
        connection["plaintext_authorized"] = json!(true);
        connection["ca_file"] = Value::Null;
        connection["tls_server_name"] = Value::Null;
    } else {
        connection["ca_file"] = json!(text(c, "ca_file")?);
    }
    if !c["tls_server_name"].is_null() {
        connection["tls_server_name"] = json!(text(c, "tls_server_name")?);
    }
    Ok(json!({
        "protocol": "oo.mysql.source-sql/v1",
        "connection": connection,
        "statement_ms": SOURCE_SQL_STATEMENT_MS,
        "max_rows": SOURCE_SQL_MAX_ROWS,
        "max_bytes": SOURCE_SQL_MAX_BYTES,
        "sql": sql,
    }))
}

/// A chunked-typed/v2 final-load request carries `tables[]` (admission, digests) instead of the v1
/// `entries[]` + `native_requests[]`. Shape it like v1 so the catalog and publication steps stay shared.
/// A v1 request is returned unchanged.
pub fn normalize_c1_request(request: Value) -> Result<Value> {
    if request.get("native_requests").is_some() || request.get("tables").is_none() {
        return Ok(request);
    }
    let tables = array(&request, "tables")?;
    let scope_tables = array(&request["baseline_scope"], "tables")?;
    let mut entries = Vec::new();
    let mut natives = Vec::new();
    for item in array(&request, "entries")? {
        let (schema, table) = (text(item, "schema")?, text(item, "table")?);
        let find = |list: &Vec<Value>| -> Result<Value> {
            list.iter()
                .find(|t| t["schema"] == schema && t["table"] == table)
                .cloned()
                .ok_or_else(|| "table_vector".into())
        };
        let t = find(tables)?;
        let scope = find(scope_tables)?;
        entries.push(json!({"schema": schema, "table": table,
            "admission_registry_path": t["admission"]["registry_path"],
            "admission_registry_sha256": t["admission"]["registry_sha256"]}));
        natives.push(json!({"logical_schema": schema, "logical_table": table,
            "source_id": request["source_id"], "source_generation": request["source_generation"],
            "config_digest": request["baseline_scope"]["config_digest"],
            "schema_digest": scope["schema_sha256"], "type_digest": t["type_digest"],
            "input_digest": t["input_digest"],
            "snapshot": {"kind": "mysql_cdc_first", "value": request["baseline_scope"]["l_end"]},
            "target": request["target"]}));
    }
    let mut shaped = request;
    shaped["entries"] = Value::Array(entries);
    shaped["native_requests"] = Value::Array(natives);
    Ok(shaped)
}
pub fn source_sql(c: &Value, calls: &mut Calls, sql: &str) -> Result<String> {
    let call = source_call(c, sql)?;
    let output = if c["onboarding"]["product"]["source_access"] == "network" {
        crate::full_pipeline::runtime(c, calls, call.label, &call.args, call.input.as_deref())?
    } else {
        calls.docker(call.label, &call.container, &call.args, call.input.as_deref())?
    };
    Ok(String::from_utf8(output)?)
}
fn profile(c: &Value, calls: &mut Calls, generation: &Value, source: &Value) -> Result<Value> {
    let selection = load(
        Path::new(text(&c["onboarding"], "prepared_root")?).join("prepared/native-config.json"),
    )?;
    let schema = ident(text(c, "database")?)?;
    require(selection["database"] == schema, "profile_schema")?;
    let before = source_sql(c, calls, "SELECT @@GLOBAL.gtid_executed")?
        .trim()
        .to_owned();
    let mut tables = Vec::new();
    for selected in array(&selection, "tables")? {
        let qualified = selected.as_str().ok_or("table")?;
        let (ns, table) = qualified.split_once('.').ok_or("table")?;
        require(ns == schema, "selected_schema")?;
        ident(table)?;
        let columns = source_sql(
            c,
            calls,
            &format!(
                "SELECT COLUMN_NAME,DATA_TYPE,COLUMN_TYPE,IS_NULLABLE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' ORDER BY ORDINAL_POSITION"
            ),
        )?;
        let mut fields = Vec::new();
        for row in columns.lines() {
            let parts = row.split('\t').collect::<Vec<_>>();
            require(parts.len() == 4, "profile_column")?;
            let name = ident(parts[0])?;
            if !array(&selection["columns"], qualified)?.contains(&json!(name)) {
                continue;
            }
            let binary = [
                "binary",
                "varbinary",
                "blob",
                "tinyblob",
                "mediumblob",
                "longblob",
            ]
            .contains(&parts[1]);
            let min = if binary {
                format!("HEX(MIN(`{name}`))")
            } else {
                format!("CAST(MIN(`{name}`) AS CHAR)")
            };
            let max = if binary {
                format!("HEX(MAX(`{name}`))")
            } else {
                format!("CAST(MAX(`{name}`) AS CHAR)")
            };
            let raw = source_sql(
                c,
                calls,
                &format!(
                    "SELECT JSON_OBJECT('rows',COUNT(*),'nulls',COUNT(*)-COUNT(`{name}`),'distinct',COUNT(DISTINCT BINARY `{name}`),'min',{min},'max',{max}) FROM `{schema}`.`{table}`"
                ),
            )?;
            let mut v: Value = serde_json::from_str(&raw)?;
            require(
                v["nulls"].as_u64() <= v["rows"].as_u64()
                    && v["distinct"].as_u64() <= v["rows"].as_u64(),
                "profile_counts",
            )?;
            v["null_count"] = v["nulls"].clone();
            v["distinct_count"] = v["distinct"].clone();
            v["name"] = json!(name);
            v["source_type"] = json!(parts[2]);
            v["nullable"] = json!(parts[3] == "YES");
            v["aggregation"] = json!("full_unsampled");
            v["min_max_encoding"] = json!(if binary { "hex" } else { "source_text" });
            fields.push(v);
        }
        require(!fields.is_empty(), "profile_empty")?;
        tables.push(json!({"dataset":qualified,"schema":schema,"table":table,"rows":fields[0]["rows"],"full_scan":true,"columns":fields}));
    }
    let after = source_sql(c, calls, "SELECT @@GLOBAL.gtid_executed")?
        .trim()
        .to_owned();
    require(before == after, "profile_source_changed")?;
    Ok(
        json!({"protocol":"oo.source.mysql.profile/v1","source_id":source,"source_gtid":after,"source_generation":generation,"samples":"none","sampled":false,"tables":tables}),
    )
}
fn container_owned(path: &Path, c: &Value) -> Result<String> {
    Ok(format!(
        "/owned/{}",
        path.strip_prefix(text(&c["onboarding"], "host_secret_root")?)?
            .display()
    ))
}

fn published_cut_files(pins: &Value) -> Result<Vec<&'static str>> {
    let mut files = vec![
        "native/request.json",
        "native/load.json",
        "native/ack.json",
        "catalog.json",
        "release-status.json",
        "publication.json",
    ];
    let object = pins.as_object().ok_or("published_cut_pins")?;
    require(
        files.iter().all(|name| object.contains_key(*name))
            && object
                .keys()
                .all(|name| files.contains(&name.as_str()) || name == "gateway-ready.json"),
        "published_cut_pins",
    )?;
    if object.contains_key("gateway-ready.json") {
        files.push("gateway-ready.json");
    }
    Ok(files)
}

#[cfg(test)]
fn published_cut_ready(status: &Value, ready: Option<&Value>, generation: &Value) -> Result<()> {
    published_cut_ready_in_mode(status, ready, generation, oo_core::config::DeploymentMode::PrimaryReplica)
}

fn published_cut_ready_in_mode(status: &Value, ready: Option<&Value>, generation: &Value, mode: oo_core::config::DeploymentMode) -> Result<()> {
    require(
        status["status"] == "succeeded"
            && status["half_published"] == false
            && ready.is_none_or(|ready| {
                mode.matches_ready(ready)
                    && ready["source_generation"] == *generation
                    && ready["pack_digest"] == status["pack_digest"]
            }),
        "published_cut_not_ready",
    )
}

pub fn full_batch(c: &Value, out: &Path) -> Result<Value> {
    let full = &c["onboarding"]["full"];
    let manifest = load(private(text(full, "batch_manifest")?)?)?;
    require(
        manifest["config_sha256"] == sha(&bytes(c)?),
        "batch_config_binding",
    )?;
    if full["pipeline"].is_object() {
        crate::full_pipeline::provision(c, out)?;
    }
    let catalog = if full["pipeline"].is_object() {
        Value::Null
    } else {
        load(private(text(full, "catalog")?)?)?
    };
    let ledger = crate::batch::run(&manifest, out, &|table, attempt| {
        if full["pipeline"].is_object() {
            return crate::full_pipeline::run_table(c, &manifest, table, attempt);
        }
        let item = array(full, "tables")?
            .iter()
            .filter(|item| item["schema"] == table["schema"] && item["table"] == table["table"])
            .collect::<Vec<_>>();
        require(item.len() == 1, "batch_table_binding")?;
        let item = item[0];
        let entries = array(&catalog, "entries")?
            .iter()
            .filter(|entry| {
                entry["tables"].as_array().is_some_and(|tables| {
                    tables.len() == 1
                        && tables[0]["logical_schema"] == table["schema"]
                        && tables[0]["logical_table"] == table["table"]
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        require(
            entries.len() == 1 && entries[0]["source_generation"] == manifest["generation"],
            "batch_generation_binding",
        )?;
        let request = load(private(text(item, "request")?)?)?;
        let columns = array(&request, "columns")?
            .iter()
            .map(|column| column["logical_name"].clone())
            .collect::<Vec<_>>();
        require(json!(columns) == table["columns"], "batch_column_binding")?;
        let single = collection(&entries, std::slice::from_ref(item))?;
        let path = attempt.join("single-catalog.json");
        save(&path, &single)?;
        let mut config = c.clone();
        config["onboarding"]["full"]["catalog"] = json!(path);
        config["onboarding"]["full"]["tables"] = json!([item]);
        config["onboarding"]["full"]
            .as_object_mut()
            .ok_or("full")?
            .remove("batch_manifest");
        self::full(&config, attempt)?;
        let loaded = load(private(text(item, "load")?)?)?;
        Ok(
            json!({"rows":loaded["row_count"],"chunks":1,"bytes":fs::metadata(text(&request,"arrow_path")?)?.len(),"comparison":"equal"}),
        )
    })?;
    require(
        crate::batch::completeness(&manifest, &ledger)?.is_empty(),
        "batch_incomplete",
    )?;
    require(
        !crate::batch::has_failures(&manifest, &ledger)?,
        "batch_table_failed",
    )?;
    Ok(ledger)
}

pub fn full(c: &Value, out: &Path) -> Result<Value> {
    let full = &c["onboarding"]["full"];
    let mut calls = Calls::with_operator(out, &c["onboarding"])?;
    let catalog = load(private(text(full, "catalog")?)?)?;
    let selected = array(full, "tables")?;
    let entries = array(&catalog, "entries")?;
    require(
        !entries.is_empty() && entries.len() == selected.len(),
        "full_selection",
    )?;
    require(
        collection(entries, selected)? == catalog,
        "full_catalog_binding",
    )?;
    let mut targets = json!({});
    let mut evidence = Vec::new();
    let mut captures = Vec::new();
    let evidence_id = sha(&bytes(&catalog)?);
    let evidence_root = out.join("source-evidence").join(&evidence_id);
    dir(out.join("source-evidence"))?;
    dir(&evidence_root)?;
    for (index, entry) in entries.iter().enumerate() {
        let table = &array(entry, "tables")?[0];
        let item = selected
            .iter()
            .find(|item| {
                item["schema"] == table["logical_schema"] && item["table"] == table["logical_table"]
            })
            .ok_or("full_selection")?;
        let dataset = format!(
            "{}.{}",
            text(table, "logical_schema")?,
            text(table, "logical_table")?
        );
        targets[&dataset] = item["target"].clone();
        let mut refs = json!({});
        for kind in ["request", "load", "admission"] {
            let source = private(text(item, kind)?)?;
            let name = format!("{index}-{kind}.json");
            copy(&source, evidence_root.join(&name))?;
            refs[kind] = json!({"path":format!("source-evidence/{evidence_id}/{name}"),"sha256":digest(&source)?});
        }
        let request = load(private(text(item, "request")?)?)?;
        let loaded = load(private(text(item, "load")?)?)?;
        let admission = load(private(text(item, "admission")?)?)?;
        full_admission_hash(&request, &loaded, text(&refs["admission"], "sha256")?)?;
        captures.push((request, loaded, admission));
        evidence.push(refs);
    }
    let generation = json!(entries.iter().filter_map(|e|e["source_generation"].as_i64()).max().ok_or("full_generation")?);
    let target_generation = json!(entries.iter().filter_map(|e|e["target_generation"].as_i64()).max().ok_or("full_generation")?);
    let source = entries[0]["source_id"].clone();
    require(full["source_id"] == source, "full_source_binding")?;
    require(
        entries.iter().all(|e| full_table_binding(e, &c["database"])),
        "full_table_binding",
    )?;
    require(
        entries.iter().all(|entry| {
            entry["source_id"] == source
                && entry["source_generation"].as_i64().is_some_and(|n|n>0)
                && entry["target_generation"].as_i64().is_some_and(|n|n>0)
        }),
        "full_generation",
    )?;
    let publication = full_publication(&catalog, &captures)?;
    let oracle = if captures.iter().any(|(request,_,_)|request.get("multipart_scope").is_some()) {
        let mut tables=Vec::new();
        for (entry,(request,_,_)) in entries.iter().zip(&captures) {
            let single=json!({"entries":[entry]});
            let report=if request.get("multipart_scope").is_some() {crate::audit::compare_chunks(c,&single,&targets,&mut calls,request)?}else{crate::audit::compare_full(c,&single,&targets,&mut calls)?};
            tables.extend(array(&report,"tables")?.iter().cloned());
        }
        json!({"protocol":"oo.mysql.full-typed-oracle/v1","method":"bounded-chunk-cells/v1","tables":tables,"different_cells":0,"unresolved_chunks":0,"common_source_position":null})
    } else {crate::audit::compare_full(c, &catalog, &targets, &mut calls)?};
    save(out.join("typed-oracle.json"), &oracle)?;
    let mut profile = sealed_profile(&catalog, &targets, &mut calls)?;
    save(out.join("catalog.json"), &catalog)?;
    let mut table_generations = json!({});
    let mut methods = json!({});
    for entry in entries {
        for table in array(entry, "tables")? {
            let dataset = format!(
                "{}.{}",
                text(table, "logical_schema")?,
                text(table, "logical_table")?
            );
            methods[&dataset] = json!({"logical_table":dataset,"capture_method":"c5_diff","consistency":captures.iter().find(|(r,_,_)|r["logical_schema"]==table["logical_schema"] && r["logical_table"]==table["logical_table"]).map(|(r,_,_)|r["consistency_grade"].clone()).unwrap_or(json!("B")),"delete_detection":"reconcile","source_generation":entry["source_generation"],"target_generation":entry["target_generation"]});
            table_generations[&dataset]=json!({"source_generation":entry["source_generation"],"target_generation":entry["target_generation"],"run_id":entry["run_id"]});
        }
    }
    let mut semantics = json!({"protocol":"oo.source.mysql.audit/v1","source_id":source,"source_generation":generation,"target_generation":target_generation,"version":text(full,"version")?,"capture_method":"C5","consistency_grade":if entries.len()==1 && captures[0].0["consistency_grade"]=="S0" { "S0" } else { "B" },"delete_detection":"reconcile","cross_table_consistent":false,"position_codec":"snapshot-zero/v1","applied_pos":"0","collection_sha256":digest(out.join("catalog.json"))?,"binding_vector_sha256":sha(&bytes(&catalog["binding_vector"])?),"evidence_set_id":evidence_id,"c5_evidence":evidence,"capture_methods":methods,"quality":{"full_typed_cell_differences":0},"common_source_position":null});
    for document in [&mut profile, &mut semantics] {
        document["source_generation"] = generation.clone();
        document["target_generation"] = target_generation.clone();
        document["generation_semantics"] = json!("high_watermark");
        document["table_generations"] = table_generations.clone();
        document["collection_sha256"] = json!(digest(out.join("catalog.json"))?);
        document["binding_vector_sha256"] = json!(sha(&bytes(&catalog["binding_vector"])?));
    }
    save(out.join("profile.json"), &profile)?;
    save(out.join("source-semantics.json"), &semantics)?;
    save(out.join("publication.json"), &publication)?;
    save(
        out.join("full-receipt.json"),
        &json!({"protocol":"oo.source-onboarding.mysql-full/v1","catalog_sha256":digest(out.join("catalog.json"))?,"profile_sha256":digest(out.join("profile.json"))?,"semantics_sha256":digest(out.join("source-semantics.json"))?,"independent_audit":oracle,"publication":"not_published","review":"not_claimed"}),
    )?;
    Ok(semantics)
}

fn full_table_binding(entry: &Value, database: &Value) -> bool {
    entry["engine"] == "mysql"
        && entry["tables"].as_array().is_some_and(|tables| {
            tables.len() == 1
                && tables[0]["logical_schema"] == *database
                && tables[0]["stored_namespace"].as_str().is_some_and(|namespace| {
                    namespace.strip_prefix("pn_").is_some_and(|suffix| {
                        suffix.len() == 60 && suffix.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    })
                })
        })
}

fn full_admission_hash(request: &Value, loaded: &Value, admission_sha: &str) -> Result<()> {
    require(
        request["admission_expect"]
            .get("declared_receipt_sha256")
            .is_none_or(|sha| sha == admission_sha)
            && loaded["producer_trust"]["receipt_sha256"] == admission_sha
            && loaded["producer_trust"]["document_bound"] == true,
        "full_admission_hash",
    )
}

fn full_publication(
    catalog: &Value,
    captures: &[(Value, Value, Value)],
) -> Result<Value> {
    let entries = array(catalog, "entries")?;
    require(
        !entries.is_empty() && entries.len() == captures.len(),
        "full_publication_binding",
    )?;
    let (anchor_index, anchor) = entries.iter().enumerate().max_by_key(|(_, e)| e["target_generation"].as_i64()).ok_or("full_generation")?;
    require(entries.iter().all(|e|e["source_generation"].as_i64()<=anchor["source_generation"].as_i64()), "full_publication_generation")?;
    let mut tables = Vec::new();
    for (entry, (request, load, admission)) in entries.iter().zip(captures) {
        let table = &array(entry, "tables")?[0];
        require(
            request["capture_method"] == "c5_diff"
                && request["full_snapshot"] == true
                && (request["consistency_grade"] == "S0" || (request["consistency_grade"]=="B" && request["multipart_scope"]["protocol"]=="oo.mysql.c5-multipart/v1" && request["multipart_scope"]==admission["multipart_scope"]))
                && request["cross_table_consistent"] == false
                && request["position_codec"] == "snapshot-zero/v1"
                && request["applied_pos"] == "0"
                && request["position_lineage"].as_str().is_some_and(|lineage| !lineage.is_empty())
                && request["delete_detection"] == "reconcile"
                && request["protocol"] == "oo.pipeline.native-update/v1"
                && request["source_engine"] == "mysql"
                && load["protocol"] == "oo.pipeline.native-update-result/v1"
                && load["source_engine"] == "mysql"
                && admission["schema"] == "oo.source-admission.capture-receipt/v1"
                && admission["source_engine"] == "mysql"
                && admission["write_class"] == "R0"
                && admission["write_attempts"] == 0
                && admission["snapshot_token"] == false
                && admission["snapshot"] == request["snapshot"]
                && request["snapshot"]["value"].as_str().is_some_and(|s| !s.is_empty())
                && admission["capture_method"] == "C5"
                && admission["consistency"] == request["consistency_grade"]
                && admission["source_bound"] == true
                && load["source_bound"] == true
                && load["producer_trust"]["trusted"] == true
                && load["producer_trust"]["document_bound"] == true
                && matches!(load["load_state"].as_str(),Some("awaiting_publish"|"replay"))
                && load["target_generation"] == entry["target_generation"]
                && load["run_id"] == entry["run_id"]
                && admission["run_id"] == request["run_id"] && request["run_id"].as_str().is_some_and(|id| !id.is_empty())
                && load["physical_name"] == table["stored_physical"]
                && load["row_count"] == table["accepted_rows"],
            "full_publication_evidence",
        )?;
        for key in ["source_id", "source_generation"] {
            require(
                request[key] == entry[key]
                    && load[key] == entry[key]
                    && admission[key] == entry[key]
                    && !entry[key].is_null(),
                "full_publication_binding",
            )?;
        }
        for key in ["config_digest", "type_digest", "input_digest"] {
            require(
                request[key] == entry[key]
                    && admission[key] == entry[key]
                    && entry[key].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
                "full_publication_digest",
            )?;
        }
        require(
            request["schema_digest"] == admission["schema_digest"]
                && [request, entry].iter().all(|v| v["schema_digest"].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))),
            "full_publication_schema",
        )?;
        for key in ["logical_schema", "logical_table"] {
            require(
                request[key] == table[key]
                    && load[key] == table[key]
                    && admission[key] == table[key]
                    && !table[key].is_null(),
                "full_publication_binding",
            )?;
        }
        tables.push(json!({"schema":table["logical_schema"],"table":table["logical_table"],"physical_name":table["stored_physical"],"accepted_rows":table["accepted_rows"],"canonical_digest":table["canonical_digest"],"rejects":table["rejects"],"run_id":entry["run_id"],"source_generation":entry["source_generation"],"target_generation":entry["target_generation"],"applied_pos":"0","position_lineage":request["position_lineage"],"capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":request["consistency_grade"]}));
    }
    Ok(
        json!({"protocol":"oo.pipeline.native-publication-receipt/v1","collection_mode":"per_table","source_id":anchor["source_id"],"source_generation":anchor["source_generation"],"target_generation":anchor["target_generation"],"position_codec":"snapshot-zero/v1","position_lineage":captures[anchor_index].0["position_lineage"],"applied_pos":"0","consistency":"per_table","consistency_grade":if entries.len()==1 && captures[0].0["consistency_grade"]=="S0" { "S0" } else { "B" },"cross_table_consistent":false,"run_id":anchor["run_id"],"run_ids":entries.iter().map(|e|e["run_id"].clone()).collect::<Vec<_>>(),"tables":tables}),
    )
}

fn sealed_profile_sql(table: &Value, column: &Value) -> Result<String> {
    let physical = ident(text(table, "stored_physical")?)?;
    let name = ident(text(column, "stored_physical")?)?;
    Ok(format!(
        "SELECT json_build_object('rows',COUNT(*),'nulls',COUNT(*)-COUNT(\"{name}\"),'distinct',COUNT(DISTINCT \"{name}\"::text COLLATE \"C\"),'min',MIN(\"{name}\"::text COLLATE \"C\"),'max',MAX(\"{name}\"::text COLLATE \"C\")) FROM _oo.\"{physical}\""
    ))
}

fn sealed_profile(catalog: &Value, targets: &Value, calls: &mut Calls) -> Result<Value> {
    let entries = array(catalog, "entries")?;
    let mut tables = Vec::new();
    for entry in entries {
        require(entry["state"] == "awaiting_publish", "sealed_profile_state")?;
        for table in array(entry, "tables")? {
            let dataset = format!(
                "{}.{}",
                text(table, "logical_schema")?,
                text(table, "logical_table")?
            );
            let target = &targets[&dataset];
            let mut columns = Vec::new();
            for column in array(table, "columns")? {
                let raw = calls.docker(
                    "sealed-profile",
                    text(target, "host")?,
                    &strings(&[
                        "psql",
                        "-X",
                        "-v",
                        "ON_ERROR_STOP=1",
                        "-U",
                        ident(target["user"].as_str().unwrap_or("oo_admin"))?,
                        "-d",
                        text(target, "database")?,
                        "-Atc",
                        &format!(
                            "BEGIN READ ONLY; SET LOCAL statement_timeout='30s'; {}; COMMIT;",
                            sealed_profile_sql(table, column)?
                        ),
                    ]),
                    None,
                )?;
                let line = std::str::from_utf8(&raw)?
                    .lines()
                    .find(|line| line.starts_with('{'))
                    .ok_or("profile_counts")?;
                let mut stats: Value = serde_json::from_str(line)?;
                require(
                    stats["rows"] == table["accepted_rows"]
                        && stats["nulls"]
                            .as_u64()
                            .zip(stats["rows"].as_u64())
                            .is_some_and(|(n, r)| n <= r)
                        && stats["distinct"]
                            .as_u64()
                            .zip(stats["rows"].as_u64())
                            .is_some_and(|(n, r)| n <= r),
                    "profile_counts",
                )?;
                stats["name"] = column["logical_name"].clone();
                stats["source_type"] = column["source_type"].clone();
                stats["nullable"] = column["nullable"].clone();
                stats["null_count"] = stats["nulls"].clone();
                stats["distinct_count"] = stats["distinct"].clone();
                stats["aggregation"] = json!("sealed_target_full_unsampled");
                stats["min_max_encoding"] = json!("target_text_binary_lexical");
                columns.push(stats);
            }
            tables.push(json!({"dataset":dataset,"schema":table["logical_schema"],"table":table["logical_table"],"rows":table["accepted_rows"],"full_scan":true,"columns":columns,"source_generation":entry["source_generation"],"target_generation":entry["target_generation"],"run_id":entry["run_id"]}));
        }
    }
    Ok(
        json!({"protocol":"oo.source.mysql.profile/v1","source_id":entries[0]["source_id"],"source_generation":entries[0]["source_generation"],"sampled":false,"samples":"none","profile_origin":"sealed_target","tables":tables}),
    )
}

pub fn follow(c: &Value, out: &Path) -> Result<Value> {
    let mut calls = Calls::with_operator(out, &c["onboarding"])?;
    let o = &c["onboarding"];
    let p = &o["product"];
    let identity = load(Path::new(text(o, "prepared_root")?).join("prepared/identity.json"))?;
    let exported = out.join("native");
    crate::worker(
        c,
        &mut calls,
        &strings(&[
            "native-artifacts",
            "--binding",
            text(&identity, "binding_id")?,
            "--out",
            exported.to_str().ok_or("path")?,
        ]),
    )?;
    let request = normalize_c1_request(load(exported.join("request.json"))?)?;
    let result = load(exported.join("load.json"))?;
    let ack = load(exported.join("ack.json"))?;
    let generation = result["source_generation"].clone();
    let source = identity["source_id"].clone();
    require(
        request["source_id"] == source
            && result["source_id"] == source
            && request["binding"] == identity["binding_id"]
            && ack["source_generation"] == generation,
        "cut_identity",
    )?;
    let specs = array(&request, "entries")?;
    let native = array(&request, "native_requests")?;
    let loaded = array(&result, "entries")?;
    require(
        specs.len() == native.len() && native.len() == loaded.len(),
        "table_vector",
    )?;
    let mut entries = Vec::new();
    if !p["reuse_published_cut"].is_null() {
        let reuse = &p["reuse_published_cut"];
        let prior = Path::new(text(reuse, "path")?);
        let expected = published_cut_files(&reuse["sha256"])?;
        for name in &expected {
            require(
                digest(prior.join(name))? == text(&reuse["sha256"], name)?,
                "published_cut_digest",
            )?;
        }
        for name in ["request", "load", "ack"] {
            require(
                fs::read(prior.join("native").join(format!("{name}.json")))?
                    == fs::read(exported.join(format!("{name}.json")))?,
                "published_cut_identity",
            )?;
        }
        let prior_catalog = load(prior.join("catalog.json"))?;
        let status = load(prior.join("release-status.json"))?;
        let ready = expected
            .contains(&"gateway-ready.json")
            .then(|| load(prior.join("gateway-ready.json")))
            .transpose()?;
        let mode = if let Some(ready) = &ready {
            let installed = load(Path::new(text(ready,"private_root")?).join("config.json"))?;
            oo_core::config::DeploymentMode::from_installed(&installed)?
        } else { oo_core::config::DeploymentMode::PrimaryReplica };
        published_cut_ready_in_mode(&status, ready.as_ref(), &generation, mode)?;
        entries = array(&prior_catalog, "entries")?.clone();
        require(
            collection(&entries, specs)? == prior_catalog && entries.len() == native.len(),
            "published_catalog",
        )?;
        for ((entry, n), l) in entries.iter().zip(native).zip(loaded) {
            // Catalog schema digests are per table; the capture schema digest binds the source.
            for k in [
                "source_id",
                "source_generation",
                "config_digest",
                "type_digest",
                "input_digest",
            ] {
                require(entry[k] == n[k], "published_entry_identity")?;
            }
            require(
                entry["run_id"] == l["run_id"]
                    && entry["target_generation"] == result["target_generation"],
                "published_entry_identity",
            )?;
            let run = text(l, "run_id")?;
            require(
                run.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'),
                "run_id",
            )?;
            let table = &array(entry, "tables")?[0];
            let physical = ident(text(table, "stored_physical")?)?;
            let sql = format!(
                "SELECT json_build_object('run',row_to_json(r),'attrs',(SELECT json_agg(json_build_array(attname,atttypid::bigint,atttypmod)) FROM pg_attribute WHERE attrelid='_oo.\"{physical}\"'::regclass AND attnum>0 AND NOT attisdropped)) FROM _oo.full_import_run r WHERE run_id='{run}'"
            );
            let metadata: Value = serde_json::from_slice(&calls.docker(
                "published-target",
                text(&n["target"], "host")?,
                &strings(&[
                    "psql",
                    "-U",
                    "oo_admin",
                    "-d",
                    text(&n["target"], "database")?,
                    "-Atc",
                    &sql,
                ]),
                None,
            )?)?;
            published_types(table, &metadata["attrs"])?;
            let observed = &metadata["run"];
            require(
                observed["state"] == "published"
                    && observed["retired_at"].is_null()
                    && observed["owner_token"] == request["binding"],
                "published_target_state",
            )?;
            for k in [
                "source_id",
                "source_generation",
                "config_digest",
                "schema_digest",
                "type_digest",
                "input_digest",
            ] {
                require(observed[k] == n[k], "published_target_identity")?;
            }
            require(
                observed["target_generation"] == result["target_generation"],
                "published_target_identity",
            )?;
        }
        save(
            out.join("published-catalog-reuse.json"),
            &json!({"original":prior,"pinned_sha256":reuse["sha256"],"prior_gateway_pinned":ready.is_some(),"target_published_identity_and_types_verified":true}),
        )?;
    } else {
        for (index, ((spec, n), l)) in specs.iter().zip(native).zip(loaded).enumerate() {
            require(
                n["logical_schema"] == spec["schema"]
                    && n["logical_table"] == spec["table"]
                    && l["schema"] == spec["schema"]
                    && l["table"] == spec["table"],
                "table_identity",
            )?;
            let mut query = json!({"protocol":"oo.pipeline.native-sealed-catalog/v1","run_id":l["run_id"],"owner":identity["binding_id"],"target_generation":result["target_generation"]});
            for k in [
                "source_id",
                "source_generation",
                "config_digest",
                "schema_digest",
                "type_digest",
                "input_digest",
                "snapshot",
                "target",
            ] {
                query[k] = n[k].clone();
            }
            query["target"]
                .as_object_mut()
                .ok_or("target")?
                .remove("owner");
            query["target"]
                .as_object_mut()
                .ok_or("target")?
                .remove("shard");
            require(
                digest(text(spec, "admission_registry_path")?)?
                    == text(spec, "admission_registry_sha256")?,
                "registry_digest",
            )?;
            let mut args = strings(&[
                "docker",
                "exec",
                "-i",
                "-e",
                &format!("OO_NATIVE_ADMISSION_BIN={}", text(o, "admission")?),
                "-e",
                &format!(
                    "OO_NATIVE_BINDING_REGISTRY={}",
                    text(spec, "admission_registry_path")?
                ),
                "-e",
                &format!(
                    "OO_NATIVE_BINDING_REGISTRY_SHA256={}",
                    text(spec, "admission_registry_sha256")?
                ),
                text(o, "container")?,
                text(o, "loader")?,
                "catalog-import-native",
            ]);
            let entry: Value = serde_json::from_slice(&calls.run(
                &format!("catalog-{index}"),
                &args,
                Some(&bytes(&query)?),
            )?)?;
            args.clear();
            for k in [
                "source_id",
                "source_generation",
                "target_generation",
                "run_id",
                "config_digest",
                "type_digest",
                "input_digest",
            ] {
                require(entry[k] == query[k], "catalog_identity")?;
            }
            entries.push(entry);
        }
    }
    let catalog = collection(&entries, specs)?;
    save(out.join("catalog.json"), &catalog)?;
    let profile = profile(c, &mut calls, &generation, &source)?;
    require(
        profile["source_gtid"] == result["l_end"],
        "source_changed_before_maintenance",
    )?;
    save(out.join("profile.json"), &profile)?;
    let seed = Path::new(text(p, "seed_bundle")?);
    let mut semantics = load(seed.join("source_semantics.json"))?;
    for k in [
        "source_generation",
        "target_generation",
        "applied_pos",
        "source_epoch",
    ] {
        semantics[k] = result[k].clone();
    }
    semantics["collection_sha256"] = json!(digest(out.join("catalog.json"))?);
    semantics["binding_vector_sha256"] = json!(sha(&bytes(&catalog["binding_vector"])?));
    let mut rows = 0;
    let mut columns = 0;
    let mut rejects = 0;
    for e in &entries {
        for t in array(e, "tables")? {
            rows += t["accepted_rows"].as_u64().ok_or("rows")?;
            columns += array(t, "columns")?.len();
            rejects += array(t, "rejects")?.len();
        }
    }
    let oracle = crate::audit::compare(c, &catalog, &result, &mut calls)?;
    save(out.join("typed-oracle.json"), &oracle)?;
    semantics["quality"]["rows"] = json!(rows);
    semantics["quality"]["tables"] = json!(entries.len());
    semantics["quality"]["columns"] = json!(columns);
    semantics["quality"]["zero_date_rejects"] = json!(rejects);
    semantics["quality"]["full_typed_cell_differences"] = oracle["different_cells"].clone();
    semantics["continuous_capture"]["tested_gtid_transactions"]
        .as_array_mut()
        .ok_or("capture")?
        .push(json!(text(&result, "applied_pos")?.parse::<u64>()?));
    let product_config = load(private(text(p, "product_config")?)?)?;
    let retained = load(Path::new(text(p, "reuse_product")?).join("product-receipt.json"))?;
    let build = text(&retained, "build_id")?;
    let mut hashes = String::new();
    for name in ["request", "load", "ack"] {
        hashes.push_str(&digest(exported.join(format!("{name}.json")))?);
    }
    let evidence_id = sha(hashes.as_bytes());
    let build_root = Path::new(text(&product_config, "work_dir_host")?)
        .join("builds")
        .join(build);
    dir(build_root.join("source-evidence"))?;
    let evidence = build_root.join("source-evidence").join(&evidence_id);
    dir(&evidence)?;
    let mut c1 = json!({});
    for name in ["request", "load", "ack"] {
        copy(
            exported.join(format!("{name}.json")),
            evidence.join(format!("{name}.json")),
        )?;
        c1[name] = json!({"path":format!("source-evidence/{evidence_id}/{name}.json"),"sha256":digest(evidence.join(format!("{name}.json")))?});
    }
    semantics["evidence_set_id"] = json!(evidence_id);
    semantics["c1_evidence"] = c1;
    semantics["evidence"] = json!({"oracle_stdout_sha256":digest(out.join("typed-oracle.json"))?,"request_sha256":digest(exported.join("request.json"))?,"load_sha256":digest(exported.join("load.json"))?,"ack_sha256":digest(exported.join("ack.json"))?});
    let mut methods = json!({});
    for b in array(&catalog, "binding_vector")? {
        methods[text(b, "dataset")?] = json!({"logical_table":b["dataset"],"capture_method":"c1_cdc","consistency":"T","delete_detection":"native","source_generation":b["source_generation"],"target_generation":b["target_generation"]});
    }
    semantics["capture_methods"] = methods;
    save(out.join("source-semantics.json"), &semantics)?;
    let key = sha(out.to_str().ok_or("path")?.as_bytes());
    let bundle = Path::new(text(p, "bundle_parent")?).join(format!(
        "rust-bundle-g{}-{}",
        generation,
        &key[..12]
    ));
    dir(&bundle)?;
    for e in fs::read_dir(seed)? {
        let e = e?;
        let name = e.file_name();
        if ![
            "manifest.json",
            "catalog.json",
            "profile.json",
            "source_semantics.json",
        ]
        .contains(&name.to_string_lossy().as_ref())
        {
            copy(e.path(), bundle.join(name))?;
        }
    }
    copy(out.join("catalog.json"), bundle.join("catalog.json"))?;
    copy(out.join("profile.json"), bundle.join("profile.json"))?;
    copy(
        out.join("source-semantics.json"),
        bundle.join("source_semantics.json"),
    )?;
    let mut files = json!({});
    for e in fs::read_dir(&bundle)? {
        let e = e?;
        files[e.file_name().to_str().ok_or("path")?] = json!(digest(e.path())?);
    }
    save(
        bundle.join("manifest.json"),
        &json!({"contract":"oo.selfbuild-control-api/sealed-bundle/v1","files":files}),
    )?;
    let mut runtime = load(text(p, "api_runtime")?)?;
    runtime["projects"][0]["sealed_bundle"] = json!(container_owned(&bundle, c)?);
    let runtime_file = bundle.with_extension("runtime.json");
    save(&runtime_file, &runtime)?;
    replace(
        c,
        &mut calls,
        text(p, "api_container")?,
        text(p, "api_binary_container")?,
        &container_owned(&runtime_file, c)?,
        "selfbuild",
    )?;
    let native = product::maintain(c, &product_config, out, &bundle, &mut calls)?;
    publish(c, &catalog, &result, &native, &product_config, &mut calls)?;
    let gateway = gateway(c, &catalog, &native, &product_config, &mut calls)?;
    let ready = load(out.join("gateway-ready.json"))?;
    save(
        out.join("maintenance-complete.json"),
        &json!({"source_id":source,"source_generation":generation,"s5_reused":true,"catalog_sha256":digest(out.join("catalog.json"))?,"gateway_private":gateway,"maintenance_receipt_sha256":digest(out.join("product/maintenance-consumption.json"))?}),
    )?;
    let pointer = Path::new(text(o, "prepared_root")?).join("rust-current-gateway.json");
    if pointer.exists() {
        crate::io::private(&pointer)?;
    }
    let pending = pointer.with_extension(format!("{}.pending", std::process::id()));
    save(&pending, &ready)?;
    fs::rename(&pending, &pointer)?;
    fs::File::open(pointer.parent().ok_or("pointer_root")?)?.sync_all()?;
    Ok(ready)
}

fn publish(
    c: &Value,
    catalog: &Value,
    result: &Value,
    native: &Value,
    config: &Value,
    calls: &mut Calls,
) -> Result<()> {
    let p = &c["onboarding"]["product"];
    let product = calls.out.join("product");
    let envelope = load(product.join("current-role-response.json"))?["response"].clone();
    let commit = text(native, "artifact_revision")?;
    let wrapper = json!({"protocol":"oo.release-control.native-reviewed-pack/v1","config_commit":commit,"envelope":envelope,"effective":load(product.join("current-effective-oos.json"))?,"visibility":load(product.join("current-visibility.json"))?,"artifact":load(product.join("current-role-pack.json"))?});
    if !p["reuse_published_cut"].is_null() {
        let prior = Path::new(text(&p["reuse_published_cut"], "path")?);
        let old = load(prior.join("release-status.json"))?;
        let mut cfg = config.clone();
        cfg["endpoint"] = p["release_origin"].clone();
        cfg["origin"] = p["release_origin"].clone();
        let active = api::call(
            &cfg,
            "admin",
            &format!(
                "/control/v1/releases/active?project_id={}",
                text(&cfg["create"], "project_id")?
            ),
            None,
            calls,
            "reuse-active",
        )?;
        if old["config_commit"] == commit
            && active["version"] == old["version"]
            && active["pack_digest"] == native["pack_digest"]
            && active["half_published"] == false
        {
            let status = api::call(
                &cfg,
                "admin",
                &format!("/control/v1/releases/{}", text(&old, "operation_id")?),
                None,
                calls,
                "reuse-publish-status",
            )?;
            require(
                status["status"] == "succeeded"
                    && status["half_published"] == false
                    && status["config_commit"] == commit
                    && status["pack_digest"] == native["pack_digest"],
                "published_release_drift",
            )?;
            save(calls.out.join("release-status.json"), &status)?;
            save(calls.out.join("active-release.json"), &active)?;
            copy(
                prior.join("publication.json"),
                calls.out.join("publication.json"),
            )?;
            save(
                calls.out.join("publication-reuse.json"),
                &json!({"original":prior,"fresh_active_and_operation_verified":true,"same_artifact_revision":commit}),
            )?;
            return Ok(());
        }
    }
    let private = Path::new(text(p, "release_private")?);
    save(
        private.join("packs").join(format!("{commit}.json")),
        &wrapper,
    )?;
    save(
        private.join("packs").join(format!("{commit}.gates.json")),
        &json!({"protocol":"oo.release-control.gate-intake/v1","config_commit":commit,"gates":[],"external":[],"checkpoints":[],"skip":[]}),
    )?;
    let mut tables = Vec::new();
    for e in array(catalog, "entries")? {
        for t in array(e, "tables")? {
            tables.push(json!({"schema":t["logical_schema"],"table":t["logical_table"],"physical_name":t["stored_physical"],"accepted_rows":t["accepted_rows"],"canonical_digest":t["canonical_digest"],"rejects":t["rejects"],"run_id":e["run_id"],"source_generation":e["source_generation"],"target_generation":e["target_generation"],"applied_pos":result["applied_pos"]}));
        }
    }
    let publication = json!({"protocol":"oo.pipeline.native-publication-receipt/v1","source_id":result["source_id"],"source_generation":result["source_generation"],"target_generation":result["target_generation"],"source_epoch":result["source_epoch"],"position_codec":"mysql-gtid/v1","position_lineage":result["source_instance"]["measured_id"],"applied_pos":result["applied_pos"],"consistency":"transactional","consistency_grade":"T","cross_table_consistent":true,"run_id":tables[0]["run_id"],"run_ids":tables.iter().map(|t|t["run_id"].clone()).collect::<Vec<_>>(),"tables":tables});
    save(calls.out.join("publication.json"), &publication)?;
    let mut runtime = load(text(p, "release_runtime")?)?;
    runtime["native_mapping"] = json!({"datasets":array(catalog,"binding_vector")?.iter().map(|b|json!({"dataset":b["dataset"],"source":b["source_id"],"config_digest":b["config_digest"],"generation":b["source_generation"]})).collect::<Vec<_>>(),"publication":{"protocol":publication["protocol"],"run_id":publication["run_id"],"receipt_sha256":sha(&bytes(&publication)?),"receipt":publication}});
    let runtime_file = private.join(format!("rust-runtime-{commit}.json"));
    save(&runtime_file, &runtime)?;
    replace(
        c,
        calls,
        text(p, "release_container")?,
        text(p, "release_binary_container")?,
        &container_owned(&runtime_file, c)?,
        "release",
    )?;
    let mut cfg = config.clone();
    cfg["endpoint"] = p["release_origin"].clone();
    cfg["origin"] = p["release_origin"].clone();
    let active_path = format!(
        "/control/v1/releases/active?project_id={}",
        text(&cfg["create"], "project_id")?
    );
    let active = api::call(&cfg, "admin", &active_path, None, calls, "previous-active")?;
    let version = format!(
        "mysql-rust-g{}-{}",
        result["source_generation"],
        &commit[..12]
    );
    let created = api::call(
        &cfg,
        "admin",
        "/control/v1/releases",
        Some(
            json!({"project_id":cfg["create"]["project_id"],"config_commit":commit,"candidate_id":version,"version":version,"base_version":active["version"],"role":wrapper["visibility"]["role"],"scope":"test_scope","expected_version":active["version"]}),
        ),
        calls,
        "publish",
    )?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        let s = api::call(
            &cfg,
            "admin",
            &format!("/control/v1/releases/{}", text(&created, "operation_id")?),
            None,
            calls,
            "publish-status",
        )?;
        if [
            "succeeded",
            "failed",
            "refused",
            "resume_uncertain",
            "canceled",
        ]
        .contains(&text(&s, "status")?)
        {
            break s;
        }
        require(Instant::now() < deadline, "publish_deadline")?;
        std::thread::sleep(Duration::from_secs(1));
    };
    save(calls.out.join("release-status.json"), &status)?;
    require(
        status["status"] == "succeeded"
            && status["half_published"] == false
            && status["pack_digest"] == native["pack_digest"],
        "formal_publication_refused",
    )?;
    let active = api::call(&cfg, "admin", &active_path, None, calls, "current-active")?;
    require(
        active["version"] == version && active["pack_digest"] == native["pack_digest"],
        "formal_active_pack",
    )?;
    save(calls.out.join("active-release.json"), &active)
}

fn current_freshness(response: &Value, vector: &[Value]) -> Result<()> {
    let measured = array(response, "freshness")?;
    let observed = response["target_observations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    require(
        response["publication"] == "live"
            && !array(response, "sources")?.is_empty()
            && measured.len() + observed.len() == vector.len()
            && (observed.is_empty() || response["source_time"] == "unmeasured"),
        "gateway_not_ready",
    )?;
    for binding in vector {
        let rows = measured
            .iter()
            .filter(|row| {
                row["table"] == binding["dataset"]
                    && row["state"] == "live"
                    && row["lag_ms"].as_u64().is_some()
                    && row["as_of"]
                        .as_str()
                        .is_some_and(|time| oo_core::rfc3339::Rfc3339Timestamp::new(time).is_ok())
            })
            .count();
        let observations = observed
            .iter()
            .filter(|row| {
                row["table"] == binding["dataset"]
                    && row["source"] == binding["source_id"]
                    && row["source_generation"] == binding["source_generation"]
                    && row["state"] == "live"
                    && row["source_time"] == "unmeasured"
                    && row.get("source_committed_at").is_some_and(Value::is_null)
                    && row["target_applied_at"]
                        .as_str()
                        .is_some_and(|time| oo_core::rfc3339::Rfc3339Timestamp::new(time).is_ok())
            })
            .count();
        require(rows + observations == 1, "gateway_freshness_coverage")?;
    }
    Ok(())
}

#[cfg(test)]
fn current_gateway_ready(ready: &Value, role: &str, generation: &Value) -> Result<()> {
    current_gateway_ready_in_mode(ready, role, generation, oo_core::config::DeploymentMode::PrimaryReplica)
}

fn current_gateway_ready_in_mode(ready: &Value, role: &str, generation: &Value, mode: oo_core::config::DeploymentMode) -> Result<()> {
    require(
        mode.matches_ready(ready)
            && ready["role"] == role
            && ready["source_generation"].as_u64().is_some_and(|previous| {
                previous > 0
                    && generation
                        .as_u64()
                        .is_some_and(|current| previous <= current)
            }),
        "previous_gateway_identity",
    )
}

fn previous_gateway_source(before: &Value, catalog: &Value) -> Result<()> {
    require(
        array(before, "binding_vector")?
            .iter()
            .map(|v| v["source_id"].to_string())
            .collect::<BTreeSet<_>>()
            == array(catalog, "binding_vector")?
                .iter()
                .map(|v| v["source_id"].to_string())
                .collect::<BTreeSet<_>>(),
        "previous_gateway_source",
    )
}

fn current_gateway_catalog(ready: &Value, catalog: &Value, pack: &Value) -> Result<()> {
    require(
        array(catalog, "binding_vector")?
            .iter()
            .all(|item| item["source_generation"] == ready["source_generation"])
            && pack["pack_digest"].as_str().is_some()
            && pack["pack_digest"] == ready["pack_digest"],
        "previous_gateway_generation_or_pack",
    )
}

fn gateway(
    c: &Value,
    catalog: &Value,
    native: &Value,
    config: &Value,
    calls: &mut Calls,
) -> Result<PathBuf> {
    let o = &c["onboarding"];
    let p = &o["product"];
    let product = calls.out.join("product");
    let visibility = load(product.join("current-visibility.json"))?;
    let effective = load(product.join("current-effective-oos.json"))?;
    let role = text(&visibility, "role")?;
    let login = text(p, "gateway_login")?;
    let pointer = Path::new(text(o, "prepared_root")?).join("rust-current-gateway.json");
    let current = if pointer.exists() {
        let ready = load(crate::io::private(&pointer)?)?;
        let root = PathBuf::from(text(&ready, "private_root")?);
        dir(&root)?;
        require(
            root.canonicalize()?
                .starts_with(Path::new(text(o, "host_secret_root")?).canonicalize()?),
            "previous_gateway_root",
        )?;
        let installed = load(root.join("config.json"))?;
        let mode = oo_core::config::DeploymentMode::from_installed(&installed)?;
        current_gateway_ready_in_mode(&ready, role, &catalog["binding_vector"][0]["source_generation"], mode)?;
        Some((root, ready))
    } else {
        None
    };
    let previous = current
        .as_ref()
        .map(|(root, _)| root.as_path())
        .unwrap_or(Path::new(text(p, "previous_gateway")?));
    let before = load(previous.join("reviewed/mysql_full/native-input.json"))?;
    if let Some((_, ready)) = &current {
        current_gateway_catalog(
            ready,
            &before["catalog"],
            &load(previous.join("packs/mysql_full.json"))?,
        )?;
    }
    let vector = array(catalog, "binding_vector")?;
    previous_gateway_source(&before["catalog"], catalog)?;
    for old in array(&before["catalog"], "binding_vector")? {
        if vector
            .iter()
            .any(|v| v["stored_physical"] == old["stored_physical"])
        {
            continue;
        }
        calls.docker("retire-previous-shadow",text(o,"container")?,&strings(&[text(p,"coordinator_binary")?,"target-close","--config",text(p,"coordinator_config_container")?]),Some(&bytes(&json!({"protocol":"oo.release-coordinator/target-close/v1","schema_name":"_oo","relation_name":old["stored_physical"],"role_name":role}))?))?;
    }
    let mut policies = Vec::new();
    for d in array(&visibility, "datasets")? {
        let context = vector
            .iter()
            .find(|b| b["dataset"] == d["dataset"])
            .ok_or("dataset_binding")?;
        let name = text(d, "dataset")?;
        let (schema, table) = name.split_once('.').ok_or("dataset")?;
        ident(schema)?;
        ident(table)?;
        let ds = array(&effective, "datasets")?
            .iter()
            .find(|v| v["dataset"] == name)
            .ok_or("effective_dataset")?;
        let mut selected = Vec::new();
        for name in array(d, "columns")? {
            let col = array(ds, "columns")?
                .iter()
                .find(|v| v["name"] == *name)
                .ok_or("effective_column")?;
            require(
                col["pii"] == "none" && col["classification"]["state"] == "confirmed",
                "reviewed_column",
            )?;
            selected.push(json!({"name":name,"status":"confirmed","semantic_type":col["semantic_type"],"pii":{"class":"none"}}));
        }
        policies.push(json!({"project":context["source_id"],"config_digest":context["config_digest"],"generation":context["source_generation"],"role":role.trim_start_matches("oo_r_"),"login":login,"table":{"namespace":schema,"name":table,"columns":selected}}));
    }
    let mut mapping = load(text(p, "mapping_template")?)?;
    mapping["policies"] = json!(policies);
    save(calls.out.join("mapping-request.json"), &mapping)?;
    calls.docker(
        "mapping-reader",
        text(o, "container")?,
        &strings(&[text(p, "mapping_binary")?]),
        Some(&bytes(&mapping)?),
    )?;
    let generation = &vector[0]["source_generation"];
    let key = sha(calls.out.to_str().ok_or("path")?.as_bytes());
    let gateway = Path::new(text(p, "bundle_parent")?)
        .join(format!("rust-gateway-g{generation}-{}", &key[..12]));
    dir(&gateway)?;
    dir(gateway.join("packs"))?;
    let folder = gateway.join("reviewed/mysql_full");
    dir(&folder)?;
    let env = load(product.join("current-role-response.json"))?;
    let artifact = Path::new(text(config, "work_dir_host")?)
        .join("builds")
        .join(text(&env["response"], "build_id")?)
        .join("native-maintenance")
        .join(text(native, "artifact_revision")?);
    let command = load(artifact.join("command.json"))?;
    let reviewed = load(artifact.join("reviewed.json"))?;
    let first = &vector[0];
    let allow = array(&visibility, "datasets")?
        .iter()
        .flat_map(|d| {
            d["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |n| json!({"dataset":d["dataset"],"column":n,"explicit_allow":false}))
        })
        .collect::<Vec<_>>();
    let binding = json!({"project":visibility["project"],"ontology_version":effective["ontology_version"],"catalog_version":visibility["catalog_version"],"policy_id":visibility["policy"]["id"],"policy_version":visibility["policy"]["version"],"knowledge_id":visibility["knowledge"]["id"],"knowledge_version":visibility["knowledge"]["version"],"source_id":first["source_id"],"schema":text(&visibility["datasets"][0],"dataset")?.split('.').next(),"schema_hash":reviewed["provenance"]["collection_sha256"],"artifact_sha256":sha(&bytes(&command["catalog"])?),"config_digest":first["config_digest"],"generation":generation,"role":role.trim_start_matches("oo_r_"),"login":login,"allow":allow,"binding_vector":vector});
    copy(
        artifact.join("command.json"),
        folder.join("native-input.json"),
    )?;
    copy(artifact.join("reviewed.json"), folder.join("command.json"))?;
    save(folder.join("binding.json"), &binding)?;
    copy(
        artifact.join("role-pack.json"),
        gateway.join("packs/mysql_full.json"),
    )?;
    for name in [
        "ca.crt",
        "runtime.pass",
        "endpoint.json",
        "data.pass",
        "config.json",
    ] {
        copy(previous.join(name), gateway.join(name))?;
    }
    copy(text(p, "gateway_data_ca")?, gateway.join("data-ca.crt"))?;
    save(
        gateway.join("data-endpoint.json"),
        &p["gateway_data_endpoint"],
    )?;
    dir(gateway.join("wal"))?;
    let cfg = load(gateway.join("config.json"))?;
    let deployment_mode = oo_core::config::DeploymentMode::from_installed(&cfg)?;
    save(
        gateway.join("grant.json"),
        &json!({"kind":"client","project_id":visibility["project"],"principal_id":"fixture-admin","role":role,"policy_version":visibility["policy"]["version"],"knowledge_version":visibility["knowledge"]["version"],"audience_url":cfg["audience"],"claims":[],"client_ttl_ms":3600000}),
    )?;
    let container = format!(
        "{}{}-rust-{}",
        text(p, "gateway_container_prefix")?,
        generation,
        &key[..8]
    );
    let networks = array(p, "gateway_networks")?;
    calls.run(
        "gateway-create",
        &strings(&[
            "docker",
            "run",
            "-d",
            "--name",
            &container,
            "--network",
            networks[0].as_str().ok_or("network")?,
            "-v",
            &format!("{}:/run-current", gateway.display()),
            "-v",
            &format!("{}:/fixed:ro", text(p, "fixed_bin_root")?),
            text(p, "gateway_image")?,
            "sleep",
            "infinity",
        ]),
        None,
    )?;
    for network in &networks[1..] {
        calls.run(
            "gateway-network",
            &strings(&[
                "docker",
                "network",
                "connect",
                network.as_str().ok_or("network")?,
                &container,
            ]),
            None,
        )?;
    }
    let bin = |key: &str| -> Result<String> {
        let path = Path::new(text(p, key)?);
        Ok(format!(
            "/fixed/{}/{}",
            path.parent()
                .and_then(Path::file_name)
                .and_then(|p| p.to_str())
                .ok_or("binary")?,
            path.file_name().and_then(|p| p.to_str()).ok_or("binary")?
        ))
    };
    let admin = crate::io::private(text(p, "token_admin_password_file")?)?;
    let secret_root = Path::new(text(o, "host_secret_root")?);
    require(
        admin
            .canonicalize()?
            .starts_with(secret_root.canonicalize()?),
        "control_secret_root",
    )?;
    let control_path = |path: &Path| -> Result<String> {
        Ok(format!(
            "/secrets/{}",
            path.strip_prefix(secret_root)?.display()
        ))
    };
    let issued = calls.out.join("current-client.issued.token");
    // The existing control container signs; the data-plane mount never contains its admin secret.
    calls.docker(
        "issue-token",
        text(o, "container")?,
        &strings(&[
            text(p, "token_binary")?,
            "issue",
            &control_path(&gateway.join("endpoint.json"))?,
            &control_path(&admin)?,
            &control_path(&gateway.join("ca.crt"))?,
            &control_path(&gateway.join("grant.json"))?,
            issued.to_str().ok_or("issued_token_path")?,
        ]),
        None,
    )?;
    copy(&issued, gateway.join("current-client.token"))?;
    let stdout = fs::File::create(calls.out.join("gateway.serve.stdout"))?;
    let stderr = fs::File::create(calls.out.join("gateway.serve.stderr"))?;
    let mut proc = Command::new("docker")
        .args(["exec", &container, &bin("gateway_binary")?, "/run-current"])
        .stdout(stdout)
        .stderr(stderr)
        .spawn()?;
    save(
        calls.out.join("gateway-handle.json"),
        &json!({"container":container,"docker_exec_pid":proc.id(),"private_root":gateway,"source_generation":generation,"pack_digest":native["pack_digest"],"data_endpoint":p["gateway_data_endpoint"]}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !gateway.join("listen.addr").exists() {
        require(
            proc.try_wait()?.is_none() && Instant::now() < deadline,
            "gateway_start",
        )?;
        std::thread::sleep(Duration::from_millis(100));
    }
    let host = format!(
        "http://{}",
        fs::read_to_string(gateway.join("listen.addr"))?.trim()
    );
    let flags = strings(&[
        "--json",
        "--auth",
        "external",
        "--token-file",
        "/run-current/current-client.token",
        "--gateway",
        &host,
        "--transport",
        "local-development",
    ]);
    let mut checks = vec![
        ("tables", strings(&["tables"])),
        ("freshness", strings(&["freshness"])),
    ];
    for b in vector {
        let dataset = text(b, "dataset")?;
        checks.push(("describe", strings(&["describe", dataset])));
        checks.push((
            "count",
            strings(&["query", &format!("SELECT COUNT(*) AS n FROM {dataset}")]),
        ));
    }
    for sql in array(p, "preflight_sql")? {
        checks.push(("current", strings(&["query", sql.as_str().ok_or("sql")?])));
    }
    for (name, mut args) in checks {
        args.splice(0..0, [bin("cli_binary")?]);
        args.extend(flags.clone());
        let response: Value = serde_json::from_slice(&calls.docker(
            &format!("preflight-{name}"),
            &container,
            &args,
            None,
        )?)?;
        require(
            !response["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|w| w["code"] == "STALE_DATA"),
            "gateway_not_current",
        )?;
        if name == "freshness" {
            current_freshness(&response, vector)?;
        }
    }
    let mut ready = deployment_mode.readiness_record(role, generation.as_u64().ok_or("source_generation")?, text(native,"pack_digest")?, &container, gateway.to_str().ok_or("private_root")?);
    ready["token_sha256"] = json!(digest(gateway.join("current-client.token"))?);
    save(calls.out.join("gateway-ready.json"), &ready)?;
    Ok(gateway)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sealed_profile_reads_only_the_generation_and_marks_lexical_bounds() {
        let sql = sealed_profile_sql(
            &json!({"stored_physical":"pt_sealed"}),
            &json!({"stored_physical":"col_key"}),
        )
        .unwrap();
        assert!(sql.contains("FROM _oo.\"pt_sealed\""));
        assert!(sql.contains("COUNT(DISTINCT"));
        assert!(sql.contains("COLLATE \"C\""));
        assert!(!sql.contains("information_schema"));
        assert!(
            sealed_profile_sql(
                &json!({"stored_physical":"bad; DROP"}),
                &json!({"stored_physical":"id"})
            )
            .is_err()
        );
    }

    #[test]
    fn full_table_binding_accepts_catalog_shadow_namespace_not_storage_schema() {
        let mut entry = json!({"engine":"mysql","tables":[{
            "logical_schema":"yjdv_fixture",
            "stored_namespace":"pn_30c175e52142c1c46a6bc4ceea2b9188807dc3089c78ac21f18a63669077"
        }]});
        let database = json!("yjdv_fixture");
        assert!(full_table_binding(&entry, &database));
        assert!(!full_table_binding(&entry, &json!("other_database")));
        for namespace in ["_oo", "public", "pn_short", "pn_../escape"] {
            entry["tables"][0]["stored_namespace"] = json!(namespace);
            assert!(!full_table_binding(&entry, &database), "{namespace}");
        }
    }

    #[test]
    fn full_admission_hash_links_sealed_receipt_not_raw_capture() {
        let sealed_sha = sha(br#"{"schema":"oo.source-admission.capture-receipt/v1"}"#);
        let raw_sha = sha(br#"{"capture_method":"C5"}"#);
        let request = json!({"admission_path":"/private/sealed/admission-receipt.json"});
        let loaded = json!({"producer_trust":{"receipt_sha256":sealed_sha,"document_bound":true}});
        full_admission_hash(&request, &loaded, &sealed_sha).unwrap();
        assert!(full_admission_hash(&request, &loaded, &raw_sha).is_err());
        let mut wrong = loaded.clone();
        wrong["producer_trust"]["receipt_sha256"] = json!(raw_sha);
        assert!(full_admission_hash(&request, &wrong, &sealed_sha).is_err());
        let mut echoed = request.clone();
        echoed["admission_expect"] = json!({"declared_receipt_sha256":sealed_sha});
        full_admission_hash(&echoed, &loaded, &sealed_sha).unwrap();
        echoed["admission_expect"]["declared_receipt_sha256"] = json!(raw_sha);
        assert!(full_admission_hash(&echoed, &loaded, &sealed_sha).is_err());
    }

    #[test]
    fn full_publication_requires_snapshot_zero_evidence_not_c1() {
        let entry = json!({"source_id":"generic","source_generation":1,"target_generation":1,"run_id":"run","tables":[{"logical_schema":"s","logical_table":"t","stored_physical":"pt_sealed","accepted_rows":2,"canonical_digest":"a".repeat(64),"rejects":[]}]});
        let catalog = json!({"entries":[entry]});
        let request = json!({"protocol":"oo.pipeline.native-update/v1","source_engine":"mysql","snapshot":{"kind":"mysql_consistent_snapshot","value":"capture-run"},"source_id":"generic","source_generation":1,"run_id":"capture-run","logical_schema":"s","logical_table":"t","capture_method":"c5_diff","full_snapshot":true,"consistency_grade":"S0","cross_table_consistent":false,"position_codec":"snapshot-zero/v1","applied_pos":"0","position_lineage":"mysql.snapshot","delete_detection":"reconcile"});
        let load = json!({"protocol":"oo.pipeline.native-update-result/v1","source_id":"generic","source_generation":1,"logical_schema":"s","logical_table":"t","target_generation":1,"run_id":"run","source_engine":"mysql","source_bound":true,"producer_trust":{"trusted":true,"document_bound":true},"load_state":"awaiting_publish","physical_name":"pt_sealed","row_count":2});
        let admission = json!({"schema":"oo.source-admission.capture-receipt/v1","source_engine":"mysql","write_class":"R0","write_attempts":0,"snapshot_token":false,"snapshot":request["snapshot"],"source_id":"generic","source_generation":1,"logical_schema":"s","logical_table":"t","run_id":"capture-run","capture_method":"C5","consistency":"S0","source_bound":true});
        let mut catalog = catalog;
        let mut request = request;
        let mut admission = admission;
        for key in ["config_digest", "type_digest", "input_digest"] {
            catalog["entries"][0][key] = json!("a".repeat(64));
            request[key] = catalog["entries"][0][key].clone();
            admission[key] = request[key].clone();
        }
        catalog["entries"][0]["schema_digest"] = json!("b".repeat(64));
        request["schema_digest"] = json!("c".repeat(64));
        admission["schema_digest"] = request["schema_digest"].clone();
        for key in ["config_digest", "schema_digest", "type_digest", "input_digest"] {
            assert!(load.get(key).is_none());
            let mut wrong = admission.clone();
            wrong[key] = json!("d".repeat(64));
            assert!(full_publication(&catalog, &[(request.clone(), load.clone(), wrong)]).is_err(), "{key}");
        }
        for key in ["source_bound", "document_bound"] {
            let mut wrong = load.clone();
            if key == "source_bound" { wrong[key] = json!(false); }
            else { wrong["producer_trust"][key] = json!(false); }
            assert!(full_publication(&catalog, &[(request.clone(), wrong, admission.clone())]).is_err(), "{key}");
        }
        let mut crossed = load.clone();
        crossed["run_id"] = request["run_id"].clone();
        assert!(full_publication(&catalog, &[(request.clone(), crossed, admission.clone())]).is_err());
        let mut crossed = request.clone();
        crossed["run_id"] = load["run_id"].clone();
        assert!(full_publication(&catalog, &[(crossed, load.clone(), admission.clone())]).is_err());
        let receipt = full_publication(
            &catalog,
            &[(request.clone(), load.clone(), admission.clone())],
        )
        .unwrap();
        let mut replay=load.clone();replay["load_state"]=json!("replay");
        assert!(full_publication(&catalog,&[(request.clone(),replay,admission.clone())]).is_ok());
        assert_eq!(receipt["consistency"], "per_table");
        assert_eq!(receipt["collection_mode"], "per_table");
        assert_eq!(receipt["applied_pos"], "0");
        assert!(receipt.get("source_epoch").is_none());
        let mut second_entry=catalog["entries"][0].clone();second_entry["target_generation"]=json!(2);second_entry["source_generation"]=json!(2);second_entry["run_id"]=json!("second-run");second_entry["tables"][0]["logical_table"]=json!("other");
        let mut second_request=request.clone();second_request["source_generation"]=json!(2);second_request["logical_table"]=json!("other");second_request["position_lineage"]=json!("mysql.other");
        let mut second_load=load.clone();second_load["source_generation"]=json!(2);second_load["logical_table"]=json!("other");second_load["target_generation"]=json!(2);second_load["run_id"]=json!("second-run");
        let mut second_admission=admission.clone();second_admission["source_generation"]=json!(2);second_admission["logical_table"]=json!("other");
        let multiple=json!({"entries":[catalog["entries"][0],second_entry]});
        let receipt=full_publication(&multiple,&[(request.clone(),load.clone(),admission.clone()),(second_request,second_load,second_admission)]).unwrap();
        assert_eq!(receipt["consistency_grade"],"B");assert_eq!(receipt["target_generation"],2);assert_eq!(receipt["run_id"],"second-run");assert_eq!(receipt["position_lineage"],"mysql.other");assert_eq!(receipt["source_generation"],2);assert_eq!(receipt["tables"][0]["target_generation"],1);

        for (key, value) in [
            ("position_codec", json!("mysql-gtid/v1")),
            ("consistency_grade", json!("T")),
            ("cross_table_consistent", json!(true)),
            ("applied_pos", json!("1")),
        ] {
            let mut wrong = request.clone();
            wrong[key] = value;
            assert!(
                full_publication(
                    &catalog,
                    &[(wrong, load.clone(), admission.clone())]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn a_v2_request_is_shaped_like_v1_and_a_v1_request_is_left_alone() {
        let v2 = json!({"source_id":"s","source_generation":1,"target":{"database":"d"},
            "entries":[{"schema":"a","table":"t"}],
            "tables":[{"schema":"a","table":"t","type_digest":"ty","input_digest":"in",
                "admission":{"registry_path":"/r","registry_sha256":"rs"}}],
            "baseline_scope":{"config_digest":"cd","l_end":"u:1-9",
                "tables":[{"schema":"a","table":"t","schema_sha256":"table-schema"}]}});
        let shaped = normalize_c1_request(v2).unwrap();
        assert_eq!(shaped["entries"][0]["admission_registry_path"], "/r");
        assert_eq!(shaped["native_requests"][0]["schema_digest"], "table-schema");
        assert_eq!(shaped["native_requests"][0]["config_digest"], "cd");
        assert_eq!(shaped["native_requests"][0]["logical_table"], "t");
        assert_eq!(shaped["native_requests"][0]["logical_schema"], "a");
        assert_eq!(shaped["native_requests"][0]["snapshot"]["value"], "u:1-9");
        assert_eq!(shaped["native_requests"][0]["target"]["database"], "d");
        let v1 = json!({"entries":[{"x":1}],"native_requests":[{"y":2}]});
        assert_eq!(normalize_c1_request(v1.clone()).unwrap(), v1);
        let missing = json!({"entries":[{"schema":"a","table":"t"}],"tables":[],"baseline_scope":{"tables":[]}});
        assert!(normalize_c1_request(missing).is_err());
    }

    use super::*;
    #[test]
    fn unknown_source_time_requires_exact_valid_target_observation_coverage() {
        let vector = vec![json!({"dataset":"s.t","source_id":"mysql_s","source_generation":1})];
        let response = json!({"publication":"live","sources":[{"source":"mysql_s"}],"freshness":[],"source_time":"unmeasured",
            "target_observations":[{"table":"s.t","source":"mysql_s","source_generation":1,"state":"live","source_time":"unmeasured","source_committed_at":null,"target_applied_at":"2026-10-16T13:00:00Z"}]});
        assert!(current_freshness(&response, &vector).is_ok());
        for key in [
            "table",
            "source",
            "source_generation",
            "state",
            "source_time",
            "target_applied_at",
            "source_committed_at",
        ] {
            let mut wrong = response.clone();
            wrong["target_observations"][0][key] = json!("wrong");
            assert!(current_freshness(&wrong, &vector).is_err(), "{key}");
        }
        let mut duplicate = response.clone();
        duplicate["target_observations"]
            .as_array_mut()
            .unwrap()
            .push(response["target_observations"][0].clone());
        assert!(current_freshness(&duplicate, &vector).is_err());
        let mut missing = response;
        missing["target_observations"] = json!([]);
        assert!(current_freshness(&missing, &vector).is_err());
    }
    #[test]
    fn previous_gateway_requires_ready_current_role_and_nonfuture_generation() {
        let ready = json!({"ready":true,"readonly_replica":true,"role":"oo_r_fixture","source_generation":2});
        assert!(current_gateway_ready(&ready, "oo_r_fixture", &json!(3)).is_ok());
        assert!(current_gateway_ready(&ready, "oo_r_fixture", &json!(2)).is_ok());
        for (key, value) in [
            ("ready", json!(false)),
            ("readonly_replica", json!(false)),
            ("role", json!("oo_r_other")),
            ("source_generation", json!(4)),
            ("source_generation", json!(0)),
        ] {
            let mut wrong = ready.clone();
            wrong[key] = value;
            assert!(current_gateway_ready(&wrong, "oo_r_fixture", &json!(3)).is_err());
        }
        let source = json!({"binding_vector":[{"source_id":"same"}]});
        assert!(previous_gateway_source(&source, &source).is_ok());
        assert!(
            previous_gateway_source(&source, &json!({"binding_vector":[{"source_id":"other"}]}))
                .is_err()
        );
        let ready = json!({"source_generation":2,"pack_digest":"pack2"});
        let catalog = json!({"binding_vector":[{"source_generation":2}]});
        assert!(current_gateway_catalog(&ready, &catalog, &json!({"pack_digest":"pack2"})).is_ok());
        assert!(
            current_gateway_catalog(&ready, &catalog, &json!({"pack_digest":"pack1"})).is_err()
        );
        assert!(
            current_gateway_catalog(
                &ready,
                &json!({"binding_vector":[{"source_generation":1}]}),
                &json!({"pack_digest":"pack2"})
            )
            .is_err()
        );
    }
    #[test]
    fn published_cut_requires_all_release_pins_and_checks_optional_gateway() {
        let mut pins = json!({
            "native/request.json":"sha", "native/load.json":"sha", "native/ack.json":"sha",
            "catalog.json":"sha", "release-status.json":"sha", "publication.json":"sha"
        });
        assert_eq!(published_cut_files(&pins).unwrap().len(), 6);
        pins["gateway-ready.json"] = json!("sha");
        assert_eq!(published_cut_files(&pins).unwrap().len(), 7);
        pins["unknown.json"] = json!("sha");
        assert!(published_cut_files(&pins).is_err());
        pins.as_object_mut().unwrap().remove("unknown.json");
        pins.as_object_mut().unwrap().remove("native/ack.json");
        assert!(published_cut_files(&pins).is_err());
        let status = json!({"status":"succeeded","half_published":false,"pack_digest":"pack"});
        let ready = json!({"ready":true,"readonly_replica":true,"source_generation":1,"pack_digest":"pack"});
        assert!(published_cut_ready(&status, None, &json!(1)).is_ok());
        assert!(published_cut_ready(&status, Some(&ready), &json!(1)).is_ok());
        for (key, value) in [
            ("ready", json!(false)),
            ("readonly_replica", json!(false)),
            ("source_generation", json!(2)),
            ("pack_digest", json!("other")),
        ] {
            let mut invalid = ready.clone();
            invalid[key] = value;
            assert!(published_cut_ready(&status, Some(&invalid), &json!(1)).is_err());
        }
        assert!(published_cut_ready(&json!({"status":"refused"}), None, &json!(1)).is_err());
    }
    #[test]
    fn published_types_refuse_oid_or_decimal_scale_drift() {
        let table = json!({"columns":[{"stored_physical":"pc_price","oid":1700,"typmod":786438}]});
        assert!(
            published_types(
                &table,
                &json!([["pc_price", 1700, 786438], ["_oo_pos", 1700, -1]])
            )
            .is_ok()
        );
        assert!(published_types(&table, &json!([["pc_price", 25, -1]])).is_err());
        assert!(published_types(&table, &json!([["pc_price", 1700, 786439]])).is_err());
        assert!(published_types(&table, &json!([])).is_err());
    }
    #[test]
    fn collection_rejects_duplicates_and_incomplete_input() {
        let e = json!({"protocol":"oo.pipeline.sealed-import-catalog/v1","state":"awaiting_publish","shadows_queried":false,"tables":[{"logical_schema":"s","logical_table":"t"}]});
        assert!(
            collection(
                &[e.clone(), e.clone()],
                &[json!({"schema":"s","table":"t"})]
            )
            .is_err()
        );
        assert!(collection(&[e], &[json!({"schema":"s","table":"other"})]).is_err());
    }
}

#[cfg(test)]
mod source_sql_tests {
    use super::*;
    use std::env;
    use std::os::unix::fs::{PermissionsExt, symlink};

    /// A private scratch root with `secrets/reader.pw` (0600). Callers remove it.
    fn scratch(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("oo-p11-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("secrets")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(root.join("secrets"), fs::Permissions::from_mode(0o700)).unwrap();
        let password = root.join("secrets/reader.pw");
        fs::write(&password, "p11-sentinel-password\n").unwrap();
        fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
        root
    }

    fn operator(root: &Path) -> Value {
        let secrets = root.join("secrets");
        json!({
            "host": "oo-cb-p11-mysql57",
            "port": 3306,
            "database": "oo_cb_p11",
            "user": "oo_reader",
            "secret_ref": "secret://p11-reader",
            "password_file": secrets.join("reader.pw"),
            "ca_file": "/tls/ca.pem",
            "tls_server_name": "mysql57.p11.owned.example",
            "onboarding": {
                "container": "oo-cb-p11-runtime",
                "producer": "/opt/oo/mysql-preview",
                "host_secret_root": secrets,
                "product": {
                    "source_container": "oo-dl01w1-06-mysql",
                    "source_client_file": "/secrets/sol-dba.cnf"
                }
            }
        })
    }

    fn network(mut c: Value) -> Value {
        c["onboarding"]["product"]["source_access"] = json!("network");
        c
    }

    fn request_of(call: &SourceCall) -> Value {
        serde_json::from_slice(call.input.as_deref().expect("network request")).unwrap()
    }

    #[test]
    fn batch_fixture_source_sql_is_full_only() {
        let mut c = json!({"onboarding":{"full":{},"product":{"source_container":"oo-wf-batch-anonymous","source_client_file":"/private/reader.cnf"}}});
        assert_eq!(
            source_call(&c, "SELECT 1").unwrap().container,
            "oo-wf-batch-anonymous"
        );
        c["onboarding"].as_object_mut().unwrap().remove("full");
        assert!(source_call(&c, "SELECT 1").is_err());
    }

    #[test]
    fn full_audit_accepts_only_the_isolated_full_fixture_prefix() {
        let c=json!({"onboarding":{"full":{},"product":{"source_container":"oo-yjcap-full-random","source_client_file":"/private/reader.cnf"}}});
        assert!(source_call(&c,"SELECT 1").is_ok());
        let mut legacy=c.clone();legacy["onboarding"].as_object_mut().unwrap().remove("full");
        assert!(source_call(&legacy,"SELECT 1").is_err());
        let mut other=c;other["onboarding"]["product"]["source_container"]=json!("customer-db");
        assert!(source_call(&other,"SELECT 1").is_err());
    }

    #[test]
    fn multipart_audit_fixture_requires_explicit_full_mode() {
        let mut c=json!({"onboarding":{"full":{},"product":{"source_container":"oo-wf-mpart-m1-mysql","source_client_file":"/private/reader.cnf"}}});
        assert!(source_call(&c,"SELECT 1").is_ok());
        c["onboarding"].as_object_mut().unwrap().remove("full");
        assert!(source_call(&c,"SELECT 1").is_err());
    }

    #[test]
    fn default_mode_is_the_legacy_mysql_client_in_the_owned_container() {
        let root = scratch("default-mode");
        let call = source_call(&operator(&root), "SELECT 1").unwrap();
        assert_eq!(call.label, "profile-sql");
        assert_eq!(call.container, "oo-dl01w1-06-mysql");
        assert_eq!(
            call.args,
            strings(&[
                "mysql",
                "--defaults-file=/secrets/sol-dba.cnf",
                "--default-character-set=utf8mb4",
                "--batch",
                "--raw",
                "--skip-column-names",
                "-e",
                "SELECT 1",
            ])
        );
        assert!(call.input.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_mode_still_refuses_containers_outside_the_fixture_allowlist() {
        let root = scratch("legacy-refuse");
        let mut c = operator(&root);
        c["onboarding"]["product"]["source_container"] = json!("customer-rds-proxy");
        let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
        assert!(error.contains("owned_fixture_only"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_the_exact_network_value_switches_the_mode() {
        let root = scratch("mode-value");
        for value in [json!("docker"), json!("Network"), json!(""), Value::Null] {
            let mut c = operator(&root);
            c["onboarding"]["product"]["source_access"] = value.clone();
            let call = source_call(&c, "SELECT 1").unwrap();
            assert_eq!(call.label, "profile-sql", "{value}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_mode_execs_the_pinned_producer_in_the_runtime_container() {
        let root = scratch("network-mode");
        let call = source_call(&network(operator(&root)), "SELECT 1").unwrap();
        assert_eq!(call.label, "source-sql-network");
        assert_eq!(call.container, "oo-cb-p11-runtime");
        assert_eq!(call.args, strings(&["/opt/oo/mysql-preview", "source-sql"]));
        assert!(!call.args.iter().any(|arg| arg == "mysql"));
        let request = request_of(&call);
        assert_eq!(request["protocol"], "oo.mysql.source-sql/v1");
        assert_eq!(request["connection"]["host"], "oo-cb-p11-mysql57");
        assert_eq!(request["connection"]["port"], 3306);
        assert_eq!(request["connection"]["database"], "oo_cb_p11");
        assert_eq!(request["connection"]["user"], "oo_reader");
        assert_eq!(request["connection"]["password_file"], "/secrets/reader.pw");
        assert_eq!(request["connection"]["ca_file"], "/tls/ca.pem");
        assert_eq!(
            request["connection"]["tls_server_name"],
            "mysql57.p11.owned.example"
        );
        assert!(request["connection"].get("password").is_none());
        assert_eq!(request["statement_ms"], SOURCE_SQL_STATEMENT_MS);
        assert_eq!(request["max_rows"], SOURCE_SQL_MAX_ROWS);
        assert_eq!(request["max_bytes"], SOURCE_SQL_MAX_BYTES);
        assert_eq!(request["sql"], "SELECT 1");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_network_audit_uses_the_same_private_host_secret_as_capture() {
        let root = scratch("host-network");
        let mut config = network(operator(&root));
        config["onboarding"]["full"] = json!({"pipeline":{"runtime_transport":"host"}});
        let request = source_request(&config, "SELECT 1").unwrap();
        assert_eq!(request["connection"]["password_file"], config["password_file"]);
        config["onboarding"]["full"]["pipeline"]["runtime_transport"] = json!("container");
        let request = source_request(&config, "SELECT 1").unwrap();
        assert_eq!(request["connection"]["password_file"], "/secrets/reader.pw");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_request_without_a_tls_name_does_not_send_one() {
        let root = scratch("no-tls-name");
        let mut c = network(operator(&root));
        c["tls_server_name"] = Value::Null;
        let request = request_of(&source_call(&c, "SELECT 1").unwrap());
        assert!(request["connection"].get("tls_server_name").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_request_never_carries_the_password_value() {
        let root = scratch("request-secret");
        let c = network(operator(&root));
        let body = String::from_utf8(source_call(&c, "SELECT 1").unwrap().input.unwrap()).unwrap();
        assert!(!body.contains("p11-sentinel-password"));
        assert!(body.contains("/secrets/reader.pw"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_request_refuses_a_password_outside_the_secret_root() {
        let root = scratch("outside-root");
        let outside = root.join("reader.pw");
        fs::write(&outside, "p11-sentinel-password").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        let mut c = network(operator(&root));
        c["password_file"] = json!(outside);
        assert!(source_call(&c, "SELECT 1").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_request_refuses_a_symlinked_or_group_readable_password() {
        let root = scratch("private-password");
        let secrets = root.join("secrets");
        let mut c = network(operator(&root));
        let password = secrets.join("reader.pw");
        fs::set_permissions(&password, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(source_call(&c, "SELECT 1").is_err());
        fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
        let real = root.join("real.pw");
        fs::rename(&password, &real).unwrap();
        symlink(&real, &password).unwrap();
        c["password_file"] = json!(password);
        assert!(source_call(&c, "SELECT 1").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_legacy_transport_requires_installed_full_policy() {
        let root = scratch("full-transport");
        let mut c = network(operator(&root));
        c["onboarding"]["full"] =
            json!({"source_transport":{"tls":"disabled","plaintext_authorized":true}});
        c["ca_file"] = Value::Null;
        c["tls_server_name"] = Value::Null;
        let request = request_of(&source_call(&c, "SELECT 1").unwrap());
        assert_eq!(request["connection"]["tls"], "disabled");
        assert_eq!(request["connection"]["plaintext_authorized"], true);
        assert!(request["connection"]["ca_file"].is_null());
        assert!(request["connection"]["tls_server_name"].is_null());
        assert_eq!(request["connection"]["password_file"], "/secrets/reader.pw");
        for policy in [
            Value::Null,
            json!({}),
            json!({"tls":"disabled"}),
            json!({"plaintext_authorized":true}),
            json!({"tls":"required","plaintext_authorized":true}),
            json!({"tls":"disabled","plaintext_authorized":false}),
            json!({"tls":"disabled","plaintext_authorized":true,"extra":1}),
        ] {
            let mut bad = c.clone();
            bad["onboarding"]["full"]["source_transport"] = policy;
            assert!(source_call(&bad, "SELECT 1").is_err());
        }
        for (key, value) in [
            ("ca_file", json!("/tls/ca.pem")),
            ("tls_server_name", json!("mysql.example")),
        ] {
            let mut bad = c.clone();
            bad[key] = value;
            assert!(source_call(&bad, "SELECT 1").is_err());
        }
        for key in ["ca_file", "tls_server_name"] {
            let mut bad = c.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(source_call(&bad, "SELECT 1").is_err());
        }
        let mut c1 = c.clone();
        c1["onboarding"].as_object_mut().unwrap().remove("full");
        c1["source_transport"] = json!({"tls":"disabled","plaintext_authorized":true});
        assert!(source_call(&c1, "SELECT 1").is_err());
        c1["ca_file"] = json!("/tls/ca.pem");
        let request = request_of(&source_call(&c1, "SELECT 1").unwrap());
        assert!(request["connection"].get("tls").is_none());
        assert!(request["connection"].get("plaintext_authorized").is_none());
        let mut profiled = with_profile(&root, c.clone());
        let request = request_of(&source_call(&profiled, "SELECT 1").unwrap());
        assert_eq!(request["connection"]["user"], "oo_profile");
        assert_eq!(
            request["connection"]["password_file"],
            "/secrets/profile.pw"
        );
        assert_eq!(request["connection"]["tls"], "disabled");
        profiled["onboarding"]["product"]["profile_access"]["secret_ref"] =
            json!("secret://unknown");
        assert!(source_call(&profiled, "SELECT 1").is_err());
        let mut default_full = c;
        default_full["onboarding"]["full"] = json!({});
        assert!(source_call(&default_full, "SELECT 1").is_err());
        default_full["ca_file"] = json!("/tls/ca.pem");
        let request = request_of(&source_call(&default_full, "SELECT 1").unwrap());
        assert!(request["connection"].get("tls").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_request_requires_the_ca_file() {
        let root = scratch("ca-required");
        let mut c = network(operator(&root));
        c.as_object_mut().unwrap().remove("ca_file");
        assert!(source_call(&c, "SELECT 1").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    /// Adds `onboarding.product.profile_access` for `secret://p12-profile`, which the
    /// operator's secret registry maps to a 0600 `profile.pw`. Its contents are never read.
    fn with_profile(root: &Path, mut c: Value) -> Value {
        let secrets = root.join("secrets");
        let password = secrets.join("profile.pw");
        fs::write(&password, "p12-profile-sentinel\n").unwrap();
        fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
        let registry = root.join("registry.json");
        fs::write(
            &registry,
            json!({"secret://p11-reader": secrets.join("reader.pw"), "secret://p12-profile": password})
                .to_string(),
        )
        .unwrap();
        fs::set_permissions(&registry, fs::Permissions::from_mode(0o600)).unwrap();
        c["onboarding"]["secret_registry"] = json!(registry);
        c["onboarding"]["product"]["profile_access"] =
            json!({"user": "oo_profile", "secret_ref": "secret://p12-profile"});
        c
    }

    #[test]
    fn profile_access_sends_the_profile_account_on_the_admitted_endpoint() {
        let root = scratch("profile-request");
        let call = source_call(&with_profile(&root, network(operator(&root))), "SELECT 1").unwrap();
        assert_eq!(call.args, strings(&["/opt/oo/mysql-preview", "source-sql"]));
        let request = request_of(&call);
        assert_eq!(request["connection"]["user"], "oo_profile");
        assert_eq!(
            request["connection"]["password_file"],
            "/secrets/profile.pw"
        );
        assert_eq!(request["connection"]["host"], "oo-cb-p11-mysql57");
        assert_eq!(request["connection"]["port"], 3306);
        assert_eq!(request["connection"]["database"], "oo_cb_p11");
        assert_eq!(request["connection"]["ca_file"], "/tls/ca.pem");
        assert_eq!(
            request["connection"]["tls_server_name"],
            "mysql57.p11.owned.example"
        );
        let body = String::from_utf8(call.input.unwrap()).unwrap();
        assert!(!body.contains("p12-profile-sentinel"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn without_profile_access_network_requests_keep_the_cdc_identity() {
        let root = scratch("no-profile");
        let request = request_of(&source_call(&network(operator(&root)), "SELECT 1").unwrap());
        assert_eq!(request["connection"]["user"], "oo_reader");
        assert_eq!(request["connection"]["password_file"], "/secrets/reader.pw");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_access_does_not_change_the_legacy_mysql_client_call() {
        let root = scratch("profile-legacy");
        let plain = source_call(&operator(&root), "SELECT 1").unwrap();
        let profiled = source_call(&with_profile(&root, operator(&root)), "SELECT 1").unwrap();
        assert_eq!(plain, profiled);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_access_may_name_the_cdc_account() {
        let root = scratch("profile-same-user");
        let mut c = with_profile(&root, network(operator(&root)));
        c["onboarding"]["product"]["profile_access"]["user"] = json!("oo_reader");
        let request = request_of(&source_call(&c, "SELECT 1").unwrap());
        assert_eq!(request["connection"]["user"], "oo_reader");
        assert_eq!(
            request["connection"]["password_file"],
            "/secrets/profile.pw"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_access_refuses_unknown_or_missing_keys() {
        let root = scratch("profile-keys");
        for (key, value) in [
            ("password", json!("inline")),
            ("password_file", json!("/secrets/profile.pw")),
            ("host", json!("elsewhere")),
        ] {
            let mut c = with_profile(&root, network(operator(&root)));
            c["onboarding"]["product"]["profile_access"][key] = value;
            let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
            assert!(error.contains("profile_access_keys"), "{key}: {error}");
        }
        let mut c = with_profile(&root, network(operator(&root)));
        c["onboarding"]["product"]["profile_access"]
            .as_object_mut()
            .unwrap()
            .remove("secret_ref");
        let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
        assert!(error.contains("profile_access_keys"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_access_refuses_an_empty_user_and_an_unregistered_secret() {
        let root = scratch("profile-fields");
        let mut c = with_profile(&root, network(operator(&root)));
        c["onboarding"]["product"]["profile_access"]["user"] = json!("");
        let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
        assert!(error.contains("profile_access_user"), "{error}");
        let mut c = with_profile(&root, network(operator(&root)));
        c["onboarding"]["product"]["profile_access"]["secret_ref"] = json!("secret://missing");
        let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
        assert!(error.contains("profile_secret_ref"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_access_password_must_be_private_and_inside_the_secret_root() {
        let root = scratch("profile-private");
        let secrets = root.join("secrets");
        let profile = secrets.join("profile.pw");
        let c = with_profile(&root, network(operator(&root)));
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o640)).unwrap();
        let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
        assert!(error.contains("private_file"), "{error}");
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o600)).unwrap();
        let real = root.join("real.pw");
        fs::rename(&profile, &real).unwrap();
        symlink(&real, &profile).unwrap();
        let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
        assert!(error.contains("private_file"), "{error}");
        fs::remove_file(&profile).unwrap();
        fs::rename(&real, &profile).unwrap();
        let outside = root.join("outside.pw");
        fs::write(&outside, "p12-profile-sentinel").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(
            root.join("registry.json"),
            json!({"secret://p12-profile": outside}).to_string(),
        )
        .unwrap();
        let error = source_call(&c, "SELECT 1").unwrap_err().to_string();
        assert!(error.contains("secret_root"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    /// Live: `profile()` in network mode against the owned MySQL 5.7 fixture,
    /// through the trusted runtime container. Needs the OO_CB_P11_* environment.
    /// Run with `--ignored` on a host with the Docker CLI.
    #[test]
    #[ignore = "live: owned MySQL 5.7 fixture and runtime container (OO_CB_P11_*)"]
    fn live_profile_runs_through_the_network_mode() {
        let get = |key: &str| env::var(key).unwrap_or_else(|_| panic!("set {key}"));
        let secrets = PathBuf::from(get("OO_CB_P11_SECRETS"));
        let database = get("OO_CB_P11_DB");
        let table = get("OO_CB_P11_TABLE");
        let root = scratch("live-profile");
        let prepared = root.join("prepared");
        fs::create_dir_all(&prepared).unwrap();
        save(
            prepared.join("native-config.json"),
            &json!({
                "database": database,
                "tables": [format!("{database}.{table}")],
                "columns": {format!("{database}.{table}"): ["id", "note", "bin_col", "amount"]}
            }),
        )
        .unwrap();
        let c = network(json!({
            "host": get("OO_CB_P11_HOST"),
            "port": get("OO_CB_P11_PORT").parse::<u64>().unwrap(),
            "database": database,
            "user": get("OO_CB_P11_USER"),
            "secret_ref": "secret://p11-reader",
            "password_file": secrets.join(get("OO_CB_P11_PW_NAME")),
            "ca_file": get("OO_CB_P11_CA"),
            "tls_server_name": get("OO_CB_P11_TLS_NAME"),
            "onboarding": {
                "container": get("OO_CB_P11_RUNTIME"),
                "producer": get("OO_CB_P11_PRODUCER"),
                "host_secret_root": secrets,
                "prepared_root": root,
                "product": {"source_container": "unused-in-network-mode"}
            }
        }));
        let mut calls = Calls::new(&root.join("calls")).unwrap();
        let profile = profile(&c, &mut calls, &json!(1), &json!("p11")).unwrap();
        let t = &profile["tables"][0];
        assert_eq!(t["rows"], 8, "{profile}");
        let column = |name: &str| {
            t["columns"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == name)
                .unwrap_or_else(|| panic!("column {name}"))
                .clone()
        };
        let id = column("id");
        assert_eq!(
            (id["nulls"].as_u64(), id["distinct"].as_u64()),
            (Some(0), Some(8))
        );
        assert_eq!(
            (id["min"].as_str(), id["max"].as_str()),
            (Some("1"), Some("8"))
        );
        let amount = column("amount");
        assert_eq!(amount["nulls"], 1);
        assert_eq!(amount["distinct"], 5);
        assert_eq!(amount["min"], "-2.25");
        assert_eq!(amount["max"], "99999999.99");
        let bin = column("bin_col");
        assert_eq!(bin["nulls"], 2);
        assert_eq!(bin["distinct"], 5);
        assert_eq!(bin["max"], "80");
        fs::remove_dir_all(root).unwrap();
    }
}
