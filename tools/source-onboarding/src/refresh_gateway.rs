//! Build and probe an isolated Gateway from sealed refresh artifacts.
//! Does not switch the live listener or issue credentials.
use crate::io::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) fn archive(root: &Path, name: &str) -> Result<()> {
    let source = root.join(name);
    let metadata = match fs::symlink_metadata(&source) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    require(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "refresh_archive_file",
    )?;
    if name != "listen.addr" {
        private(&source)?;
    }
    let history = root.join("refresh-history");
    dir(&history)?;
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let destination = history.join(format!("{name}-{serial}-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&destination)?;
    fs::rename(&source, destination.join(name))?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}

fn parse_process(pid: u64, output: &str, binary: &str, root: &Path) -> Result<Value> {
    require(pid > 1 && pid <= i32::MAX as u64, "refresh_candidate_pid")?;
    let mut command = output.trim();
    require(!command.contains('\n'), "refresh_process_output")?;
    let mut start = Vec::new();
    for _ in 0..5 {
        let (part, rest) = command
            .split_once(char::is_whitespace)
            .ok_or("refresh_process_start")?;
        start.push(part);
        command = rest.trim_start();
    }
    require(
        command == format!("{binary} {}", root.display()),
        "refresh_process_not_owned",
    )?;
    Ok(json!({"pid":pid,"lstart":start.join(" "),"command":command}))
}

pub(crate) fn process_identity(pid: u64, binary: &str, root: &Path) -> Result<Value> {
    require(pid > 1 && pid <= i32::MAX as u64, "refresh_candidate_pid")?;
    let output = Command::new("/bin/ps")
        .env("LC_ALL", "C")
        .args([
            "-ww",
            "-p",
            &pid.to_string(),
            "-o",
            "lstart=",
            "-o",
            "command=",
        ])
        .output()?;
    require(output.status.success(), "refresh_process_not_alive")?;
    parse_process(pid, &String::from_utf8(output.stdout)?, binary, root)
}

fn runtime_files(root: &Path) -> Result<Value> {
    fn collect(root: &Path, path: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        require(
            !metadata.file_type().is_symlink(),
            "refresh_runtime_symlink",
        )?;
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                collect(root, &entry?.path(), files)?;
            }
        } else {
            private(path)?;
            files.insert(
                path.strip_prefix(root)?
                    .to_str()
                    .ok_or("refresh_runtime_path")?
                    .into(),
                digest(path)?,
            );
        }
        Ok(())
    }
    let runtime = load(private(root.join("config.json"))?)?;
    let mut files = BTreeMap::new();
    for name in ["config.json", "gateway-client.pem", "gateway-tls-ca.pem"] {
        collect(root, &root.join(name), &mut files)?;
    }
    for field in [
        "password_file",
        "ca_file",
        "endpoint_file",
        "data_endpoint_file",
        "data_password_file",
        "data_ca_file",
        "tls_key_file",
        "tls_chain_file",
        "client_ca_file",
        "pack_ceilings_file",
    ] {
        if let Some(name) = runtime[field].as_str() {
            require(
                Path::new(name).components().count() == 1
                    && matches!(
                        Path::new(name).components().next(),
                        Some(std::path::Component::Normal(_))
                    ),
                "refresh_relative_runtime_file",
            )?;
            collect(root, &root.join(name), &mut files)?;
        }
    }
    // Only immutable input trees; WAL, audit output and process logs are excluded.
    for (field, name) in [("reviewed_dir", "reviewed"), ("pack_dir", "packs")] {
        require(runtime[field] == name, "refresh_runtime_directory")?;
        collect(root, &root.join(name), &mut files)?;
    }
    Ok(json!(files))
}

fn candidate_binding(c: &Value, root: &Path) -> Result<Value> {
    let saved = load(private(root.join("candidate-process.json"))?)?;
    let binary = text(&c["gateway_binary"], "path")?;
    let pid = saved["pid"].as_u64().ok_or("refresh_candidate_pid")?;
    let process = process_identity(pid, binary, root)?;
    let files = runtime_files(root)?;
    require(
        saved["process"] == process
            && saved["root"] == json!(root)
            && saved["listen"] == c["listen"]
            && saved["runtime_files"] == files
            && saved["binary_sha256"] == c["gateway_binary"]["sha256"],
        "refresh_candidate_process_drift",
    )?;
    for key in ["gateway_binary", "cli_binary"] {
        require(
            digest(text(&c[key], "path")?)? == text(&c[key], "sha256")?,
            "refresh_gateway_binary_pin",
        )?;
    }
    require(
        load(root.join("config.json"))?["listen"] == c["listen"],
        "refresh_candidate_listener",
    )?;
    Ok(json!({
        "config_sha256":sha(&bytes(c)?),"candidate_root":root,
        "pack_digest":c["expected_pack_digest"],"schema_scope_digest":c["schema_scope_digest"],
        "runtime_files":files,"process":process,
        "token_sha256":digest(private(text(c, "token_file")?)?)?,
    }))
}

pub(crate) fn verified_candidate(c: &Value, root: &Path) -> Result<Value> {
    let proof = load(private(root.join("verified.json"))?)?;
    let binding = candidate_binding(c, root)?;
    check_proof(&proof, &binding, c)?;
    Ok(binding)
}

fn check_proof(proof: &Value, binding: &Value, c: &Value) -> Result<()> {
    require(
        proof["binding"] == *binding
            && proof["verification"]["candidate_verified"] == true
            && proof["verification"]["manifest_sha256"] == c["expected_manifest_sha256"]
            && proof["verification"]["queries"]
                .as_array()
                .is_some_and(|q| !q.is_empty()),
        "refresh_candidate_proof_drift",
    )
}

fn listener_isolated(candidate: &str, baseline: &str) -> bool {
    candidate
        .parse::<std::net::SocketAddr>()
        .is_ok_and(|address| {
            address.ip() == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                && address.port() != 0
                && candidate != baseline
        })
}

fn checked_query(response: &Value, expected: &Value) -> bool {
    response["error"].is_null()
        && response["truncated"] == false
        && response["rows"] == *expected
        && response["query_id"]
            .as_str()
            .is_some_and(|s| s.starts_with("q_"))
}

fn config(path: &Path) -> Result<Value> {
    let value = load(private(path)?)?;
    require(
        value["protocol"] == "oo.library.refresh-gateway/v1",
        "refresh_gateway_protocol",
    )?;
    for key in ["gateway_binary", "cli_binary"] {
        require(
            digest(text(&value[key], "path")?)? == text(&value[key], "sha256")?,
            "refresh_gateway_binary_pin",
        )?;
    }
    let baseline = load(Path::new(text(&value, "baseline_root")?).join("config.json"))?;
    require(
        listener_isolated(text(&value, "listen")?, text(&baseline, "listen")?),
        "refresh_candidate_listener",
    )?;
    Ok(value)
}

fn output_path(c: &Value) -> Result<PathBuf> {
    crate::refresh::output_path(Path::new(text(c, "candidate_root")?))
}

fn result(c: &Value, detail: Value) -> Value {
    json!({"status":"succeeded","pack_digest":c["expected_pack_digest"],
        "schema_scope_digest":c["schema_scope_digest"],"detail":detail,
        "live_gateway_switched":false})
}

pub fn prepare(path: &Path) -> Result<Value> {
    let c = config(path)?;
    let out = output_path(&c)?;
    let receipt = load(private(text(&c, "publication_receipt")?)?)?;
    let publication = &receipt["prepared"]["native_mapping"]["publication"];
    require(
        receipt["release"]["knowledge_published"] == true
            && receipt["release"]["pack_digest"] == c["expected_pack_digest"],
        "refresh_gateway_publication_required",
    )?;
    let package = Path::new(text(&c, "package_dir")?);
    let producer = load(private(package.join("pack.json"))?)?;
    let effective = load(private(package.join("effective-oos.json"))?)?;
    let visibility = load(private(package.join("visibility.json"))?)?;
    let review = load(private(package.join("effective-review-input.json"))?)?;
    require(
        producer["pack_digest"] == c["expected_pack_digest"]
            && producer["effective_sha256"] == sha(&bytes(&effective)?)
            && producer["visibility_sha256"] == sha(&bytes(&visibility)?)
            && producer["review_sha256"] == sha(&bytes(&review)?)
            && producer["published"] == false
            && producer["business_owner_signed"] == false,
        "refresh_gateway_package_binding",
    )?;
    dir(&out)?;
    save(
        out.join("refresh-input.json"),
        &json!({"config_sha256":sha(&bytes(&c)?)}),
    )?;
    let mut calls = Calls::new(&out.join("preparation-calls"))?;
    let sql = "BEGIN READ ONLY; SET LOCAL statement_timeout='15s'; SELECT json_agg(json_build_object('dataset',f.schema_name||'.'||f.table_name,'source',r.source_id,'config_digest',r.config_digest,'generation',r.source_generation,'type_digest',r.type_digest,'input_digest',r.input_digest) ORDER BY f.schema_name,f.table_name) FROM _oo.table_freshness f JOIN _oo.full_import_run r ON r.source_id=f.source AND r.source_generation=f.source_generation AND r.target_generation=f.target_generation JOIN _oo.full_import_table t ON t.run_id=r.run_id AND t.physical_name=f.physical_name; ROLLBACK;";
    let mut argv = array(&c, "metadata_argv")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or("refresh_metadata_argv"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    require(!argv.is_empty(), "refresh_metadata_argv")?;
    argv.push("-c".into());
    argv.push(sql.into());
    let bindings: Value =
        serde_json::from_slice(&calls.run("refresh-current-bindings", &argv, None)?)?;
    let mut names = array(&visibility, "datasets")?
        .iter()
        .map(|t| text(t, "dataset").map(str::to_owned))
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    let mut bound_names = bindings
        .as_array()
        .ok_or("refresh_bindings")?
        .iter()
        .map(|t| text(t, "dataset").map(str::to_owned))
        .collect::<Result<Vec<_>>>()?;
    bound_names.sort();
    require(
        names == bound_names && sha(&bytes(&json!(names))?) == c["schema_scope_digest"],
        "refresh_gateway_scope",
    )?;
    let baseline = Path::new(text(&c, "baseline_root")?);
    let mut gw_config = load(baseline.join("config.json"))?;
    let old_target = load(baseline.join(text(&gw_config, "data_endpoint_file")?))?;
    require(
        c["target_endpoint"] != old_target,
        "refresh_candidate_target_is_live",
    )?;
    for field in [
        "password_file",
        "ca_file",
        "endpoint_file",
        "data_password_file",
        "data_ca_file",
        "tls_key_file",
        "tls_chain_file",
        "client_ca_file",
        "pack_ceilings_file",
    ] {
        if let Some(name) = gw_config[field].as_str() {
            require(
                Path::new(name).components().count() == 1,
                "refresh_relative_runtime_file",
            )?;
            put(out.join(name), &fs::read(baseline.join(name))?)?;
        }
    }
    // Client mTLS material is for the operator probe, never sent in the Pack.
    for name in ["gateway-client.pem", "gateway-tls-ca.pem"] {
        put(out.join(name), &fs::read(baseline.join(name))?)?;
    }
    gw_config["listen"] = c["listen"].clone();
    gw_config["data_endpoint_file"] = json!("data.json");
    gw_config["reviewed_dir"] = json!("reviewed");
    gw_config["pack_dir"] = json!("packs");
    gw_config["wal_dir"] = json!("wal");
    gw_config["audit_collector_dir"] = json!("collector");
    for name in ["reviewed", "reviewed/library", "packs", "wal", "collector"] {
        dir(out.join(name))?;
    }
    save(out.join("data.json"), &c["target_endpoint"])?;
    save(out.join("config.json"), &gw_config)?;
    let bundle = json!({"protocol":"oo.gateway.library-reviewed/v1",
        "envelope":{"contract":"oo.operator.library-pack/v1","operation":"prepareLocalBuildPack",
            "management_identity":"operator_local","producer_authenticated":false,
            "published":false,"active":false,"compiled_pack":"not_published","data_grants":"none",
            "producer":producer,"output_sha256":sha(&bytes(&producer)?),"review":review},
        "effective":effective,"visibility":visibility,"publication":publication,"bindings":bindings,
        "login":gw_config["data_user"],"ceilings":load(baseline.join("ceilings.json"))?});
    save(out.join("reviewed/library/library.json"), &bundle)?;
    save(out.join("packs/library.json"), &producer["artifact"])?;
    Ok(result(
        &c,
        json!({"candidate_prepared":true,"published_data_binding":true}),
    ))
}

pub fn probe(path: &Path) -> Result<Value> {
    let input = load(private(path)?)?;
    let root = output_path(&input)?;
    // Revoke before any validation or request can fail; retain the previous evidence.
    archive(&root, "verified.json")?;
    let c = config(path)?;
    require(c == input, "refresh_gateway_config_drift")?;
    require(
        root.join("config.json").is_file(),
        "refresh_gateway_not_prepared",
    )?;
    let binary = text(&c["gateway_binary"], "path")?;
    let pid_file = root.join("candidate-process.json");
    if pid_file.exists() {
        candidate_binding(&c, &root)?;
    } else {
        let files = runtime_files(&root)?;
        for name in ["serve.stdout", "serve.stderr", "listen.addr"] {
            archive(&root, name)?;
        }
        let stdout = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("serve.stdout"))?;
        let stderr = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("serve.stderr"))?;
        let mut child = Command::new(binary)
            .arg(&root)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?;
        let recorded = (|| -> Result<()> {
            let process = process_identity(u64::from(child.id()), binary, &root)?;
            save(
                &pid_file,
                &json!({"pid":child.id(),"process":process,"runtime_files":files,"binary_sha256":c["gateway_binary"]["sha256"],
                                  "root":root,"listen":c["listen"]}),
            )
        })();
        if let Err(error) = recorded {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        // Keep the isolated process available for an explicit operator cutover.
        drop(child);
    }
    let deadline = Instant::now() + Duration::from_secs(90);
    let started = load(private(&pid_file)?)?;
    let pid = started["pid"].as_u64().ok_or("refresh_candidate_pid")?;
    while !root.join("listen.addr").exists() {
        require(process_identity(pid, binary, &root)? == started["process"],
            "refresh_candidate_process_drift")?;
        require(Instant::now() < deadline, "refresh_candidate_start_timeout")?;
        std::thread::sleep(Duration::from_millis(200));
    }
    let binding = candidate_binding(&c, &root)?;
    let proof = verify_requests(&c, &root, false)?;
    require(
        binding == candidate_binding(&c, &root)?,
        "refresh_candidate_changed_during_probe",
    )?;
    save(
        root.join("verified.json"),
        &json!({"binding":binding,"verification":proof["detail"]}),
    )?;
    Ok(proof)
}

pub fn public_probe(path: &Path) -> Result<Value> {
    let c = config(path)?;
    verify_requests(&c, &output_path(&c)?, true)
}

pub(crate) fn verify_requests(c: &Value, root: &Path, public: bool) -> Result<Value> {
    let mut calls = Calls::new(&root.join(if public {
        "public-probe-calls"
    } else {
        "probe-calls"
    }))?;
    let endpoint = if public {
        let endpoint = text(c, "public_gateway")?;
        require(
            endpoint.starts_with("https://") && !endpoint.contains('@'),
            "refresh_public_gateway",
        )?;
        endpoint.to_owned()
    } else {
        format!("https://{}", text(c, "listen")?)
    };
    let mut common = vec![
        "--json".to_owned(),
        "--auth".into(),
        "external".into(),
        "--transport".into(),
        "production".into(),
        "--gateway".into(),
        endpoint,
        "--token-file".into(),
        text(c, "token_file")?.into(),
    ];
    if public {
        for (field, flag) in [
            ("public_connect_ip", "--connect-ip"),
            ("public_interface", "--interface"),
        ] {
            if let Some(value) = c[field].as_str() {
                common.extend([flag.into(), value.into()]);
            }
        }
    } else {
        common.extend([
            "--ca-file".into(),
            root.join("gateway-tls-ca.pem")
                .to_string_lossy()
                .into_owned(),
            "--client-pem".into(),
            root.join("gateway-client.pem")
                .to_string_lossy()
                .into_owned(),
        ]);
    }
    let mut argv = vec![
        text(&c["cli_binary"], "path")?.to_owned(),
        "knowledge".into(),
        "cat".into(),
        "manifest.json".into(),
    ];
    argv.extend(common.clone());
    let manifest: Value = serde_json::from_slice(&calls.run("candidate-manifest", &argv, None)?)?;
    require(
        manifest["sha256"] == c["expected_manifest_sha256"],
        "refresh_candidate_manifest",
    )?;
    let mut checked = Vec::new();
    for (n, query) in array(&c, "queries")?.iter().enumerate() {
        let mut argv = vec![text(&c["cli_binary"], "path")?.to_owned(), "query".into()];
        argv.extend(common.clone());
        argv.push(text(query, "sql")?.to_owned());
        let response: Value =
            serde_json::from_slice(&calls.run(&format!("candidate-query-{n}"), &argv, None)?)?;
        require(
            checked_query(&response, &query["expected_rows"]),
            "refresh_candidate_query_mismatch",
        )?;
        checked.push(
            json!({"query_id":response["query_id"],"rows_sha256":sha(&bytes(&response["rows"])?)}),
        );
    }
    require(!checked.is_empty(), "refresh_candidate_queries_required")?;
    Ok(result(
        &c,
        json!({"candidate_verified":!public,"public_verified":public,"queries":checked,
                        "manifest_sha256":manifest["sha256"]}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let serial = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("refresh-{name}-{}-{serial}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        root
    }

    #[test]
    fn failed_probe_revokes_old_proof_and_keeps_each_history() {
        let root = temporary("proof");
        let config = root.join("input.json");
        save(
            &config,
            &json!({"protocol":"invalid","candidate_root":root}),
        )
        .unwrap();
        for attempt in 0..2 {
            save(root.join("verified.json"), &json!({"old":attempt})).unwrap();
            assert!(probe(&config).is_err());
            assert!(!root.join("verified.json").exists());
        }
        let mut previous = fs::read_dir(root.join("refresh-history"))
            .unwrap()
            .map(|entry| {
                load(entry.unwrap().path().join("verified.json")).unwrap()["old"]
                    .as_u64()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        previous.sort();
        assert_eq!(previous, vec![0, 1]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_binding_covers_inputs_but_not_wal_or_logs() {
        let root = temporary("inputs");
        for name in ["reviewed", "packs", "wal"] {
            dir(root.join(name)).unwrap();
        }
        save(
            root.join("config.json"),
            &json!({
                "reviewed_dir":"reviewed","pack_dir":"packs","password_file":"password",
            }),
        )
        .unwrap();
        for name in [
            "gateway-client.pem",
            "gateway-tls-ca.pem",
            "password",
            "packs/library.json",
        ] {
            put(root.join(name), b"fixture").unwrap();
        }
        let before = runtime_files(&root).unwrap();
        put(root.join("wal/entry"), b"mutable").unwrap();
        put(root.join("serve.stdout"), b"log").unwrap();
        assert_eq!(before, runtime_files(&root).unwrap());
        fs::write(root.join("password"), b"changed").unwrap();
        assert_ne!(before, runtime_files(&root).unwrap());
        fs::write(root.join("password"), b"fixture").unwrap();
        put(root.join("reviewed/new.json"), b"new").unwrap();
        assert_ne!(before, runtime_files(&root).unwrap());
        std::os::unix::fs::symlink(root.join("password"), root.join("packs/link")).unwrap();
        assert!(runtime_files(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_binding_rejects_reused_pid_and_other_command() {
        let first = parse_process(
            42,
            "Sun Oct  4 12:00:00 2026 /bin/gateway /candidate\n",
            "/bin/gateway",
            Path::new("/candidate"),
        )
        .unwrap();
        let restarted = parse_process(
            42,
            "Sun Oct  4 12:01:00 2026 /bin/gateway /candidate\n",
            "/bin/gateway",
            Path::new("/candidate"),
        )
        .unwrap();
        assert_ne!(first, restarted);
        assert_eq!(first["lstart"], "Sun Oct 4 12:00:00 2026");
        assert!(
            parse_process(
                42,
                "Sun Oct 4 12:00:00 2026 /bin/gateway /other",
                "/bin/gateway",
                Path::new("/candidate")
            )
            .is_err()
        );
        assert!(
            parse_process(
                0,
                "Sun Oct 4 12:00:00 2026 /bin/gateway /candidate",
                "/bin/gateway",
                Path::new("/candidate")
            )
            .is_err()
        );
    }

    #[test]
    fn proof_requires_exact_files_process_and_real_query_evidence() {
        let c = json!({"expected_manifest_sha256":"manifest"});
        let binding =
            json!({"runtime_files":{"config.json":"sha"},"process":{"pid":42,"lstart":"start"}});
        let proof = json!({"binding":binding,"verification":{
            "candidate_verified":true,"manifest_sha256":"manifest","queries":[{"query_id":"q_actual"}],
        }});
        assert!(check_proof(&proof, &binding, &c).is_ok());
        for changed in [
            json!({"runtime_files":{"config.json":"changed"},"process":{"pid":42,"lstart":"start"}}),
            json!({"runtime_files":{"config.json":"sha"},"process":{"pid":42,"lstart":"restarted"}}),
        ] {
            assert!(check_proof(&proof, &changed, &c).is_err());
        }
        let mut empty = proof;
        empty["verification"]["queries"] = json!([]);
        assert!(check_proof(&empty, &binding, &c).is_err());
    }

    #[test]
    fn candidate_must_not_replace_live_listener() {
        assert!(listener_isolated("127.0.0.1:19491", "127.0.0.1:19481"));
        assert!(!listener_isolated("127.0.0.1:19481", "127.0.0.1:19481"));
        assert!(!listener_isolated("0.0.0.0:19491", "127.0.0.1:19481"));
        assert!(!listener_isolated("127.0.0.1:0", "127.0.0.1:19481"));
    }

    #[test]
    fn actual_rows_query_id_and_no_truncation_are_required() {
        let expected = json!([["4"]]);
        let mut result = json!({"rows":expected,"query_id":"q_actual","truncated":false});
        assert!(checked_query(&result, &expected));
        result["truncated"] = json!(true);
        assert!(!checked_query(&result, &expected));
        result["truncated"] = json!(false);
        result["error"] = json!({"code":"INTERNAL"});
        assert!(!checked_query(&result, &expected));
        result.as_object_mut().unwrap().remove("error");
        assert!(!checked_query(&result, &json!([["0"]])));
        result["query_id"] = Value::Null;
        assert!(!checked_query(&result, &expected));
    }
}
