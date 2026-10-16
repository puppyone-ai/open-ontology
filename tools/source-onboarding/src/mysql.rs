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
pub fn source_sql(c: &Value, calls: &mut Calls, sql: &str) -> Result<String> {
    let p = &c["onboarding"]["product"];
    let container = text(p, "source_container")?;
    require(
        container.starts_with("oo-dl01w1-06-") || container == "oo-mysql57-reference-20261016",
        "owned_fixture_only",
    )?;
    Ok(String::from_utf8(calls.docker(
        "profile-sql",
        container,
        &strings(&[
            "mysql",
            &format!("--defaults-file={}", text(p, "source_client_file")?),
            "--default-character-set=utf8mb4",
            "--batch",
            "--raw",
            "--skip-column-names",
            "-e",
            sql,
        ]),
        None,
    )?)?)
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

pub fn follow(c: &Value, out: &Path) -> Result<Value> {
    let mut calls = Calls::new(out)?;
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
    let request = load(exported.join("request.json"))?;
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
        let expected = [
            "native/request.json",
            "native/load.json",
            "native/ack.json",
            "catalog.json",
            "release-status.json",
            "gateway-ready.json",
            "publication.json",
        ];
        require(
            reuse["sha256"]
                .as_object()
                .is_some_and(|m| m.len() == expected.len()),
            "published_cut_pins",
        )?;
        for name in expected {
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
        let ready = load(prior.join("gateway-ready.json"))?;
        require(
            status["status"] == "succeeded"
                && status["half_published"] == false
                && ready["ready"] == true
                && ready["readonly_replica"] == true
                && ready["source_generation"] == generation
                && ready["pack_digest"] == status["pack_digest"],
            "published_cut_not_ready",
        )?;
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
            &json!({"original":prior,"pinned_sha256":reuse["sha256"],"target_published_identity_and_types_verified":true}),
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
    let previous = Path::new(text(p, "previous_gateway")?);
    let before = load(previous.join("reviewed/mysql_full/native-input.json"))?;
    let vector = array(catalog, "binding_vector")?;
    require(
        array(&before["catalog"], "binding_vector")?
            .iter()
            .map(|v| v["source_id"].to_string())
            .collect::<BTreeSet<_>>()
            == vector
                .iter()
                .map(|v| v["source_id"].to_string())
                .collect::<BTreeSet<_>>(),
        "previous_gateway_source",
    )?;
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
            require(
                response["publication"] == "live"
                    && array(&response, "freshness")?.len() == vector.len()
                    && !array(&response, "sources")?.is_empty()
                    && array(&response, "freshness")?
                        .iter()
                        .all(|r| r["state"] == "live"),
                "gateway_not_ready",
            )?;
        }
    }
    save(
        calls.out.join("gateway-ready.json"),
        &json!({"source_generation":generation,"pack_digest":native["pack_digest"],"role":role,"token_sha256":digest(gateway.join("current-client.token"))?,"gateway_container":container,"private_root":gateway,"readonly_replica":true,"ready":true}),
    )?;
    Ok(gateway)
}

#[cfg(test)]
mod tests {
    use super::*;
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
