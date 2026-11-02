//! Explicit macOS service cutover after an isolated candidate passes real queries.
//! Keeps the old database and runtime intact; failed local verification rolls back.
use crate::io::*;
use crate::refresh_gateway::{archive, process_identity, verified_candidate};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn replace_private(path: &Path, data: &[u8]) -> Result<()> {
    require(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "cutover_symlink",
    )?;
    let tmp = path.with_extension(format!("refresh-{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = (|| -> Result<()> {
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        fs::File::open(path.parent().ok_or("cutover_parent")?)?.sync_all()?;
        Ok(())
    })();
    drop(file);
    // An unsuccessful write must not poison the rollback's next replacement.
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn writable_parent(path: &Path) -> Result<()> {
    let probe = path.with_extension(format!("refresh-{}.probe", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&probe)?;
    let result = file.sync_all();
    drop(file);
    fs::remove_file(&probe)?;
    Ok(result?)
}

fn loaded_service(output: &str, service: &str, binary: &str, root: &Path) -> Result<Option<u64>> {
    let mut lines = output.lines().map(str::trim);
    require(
        lines.next() == Some(format!("{service} = {{").as_str()),
        "cutover_loaded_label",
    )?;
    let mut depth = 1usize;
    let mut program = None;
    let mut pid = None;
    let mut arguments = None;
    while let Some(line) = lines.next() {
        if depth == 1 {
            if let Some(value) = line.strip_prefix("program = ") {
                require(program.replace(value).is_none(), "cutover_loaded_program")?;
            } else if let Some(value) = line.strip_prefix("pid = ") {
                require(
                    pid.replace(value.parse::<u64>()?).is_none(),
                    "cutover_loaded_pid",
                )?;
            } else if line == "arguments = {" {
                require(arguments.is_none(), "cutover_loaded_arguments")?;
                let mut args = Vec::new();
                loop {
                    let argument = lines.next().ok_or("cutover_loaded_arguments")?;
                    if argument == "}" {
                        break;
                    }
                    args.push(argument);
                }
                arguments = Some(args);
                continue;
            }
        }
        if line.ends_with('{') {
            depth += 1;
        } else if line == "}" {
            depth = depth.checked_sub(1).ok_or("cutover_loaded_output")?;
            if depth == 0 {
                break;
            }
        }
    }
    require(
        depth == 0
            && program == Some(binary)
            && arguments == Some(vec![binary, root.to_str().ok_or("cutover_root")?]),
        "cutover_loaded_service_identity",
    )?;
    if let Some(pid) = pid {
        require(pid > 1 && pid <= i32::MAX as u64, "cutover_loaded_pid")?;
    }
    Ok(pid)
}

fn live_identity(calls: &mut Calls, service: &str, binary: &str, root: &Path) -> Result<Value> {
    let raw = calls.run(
        "loaded-service",
        &strings(&["/bin/launchctl", "print", service]),
        None,
    )?;
    let pid = loaded_service(&String::from_utf8(raw)?, service, binary, root)?
        .ok_or("cutover_loaded_pid")?;
    process_identity(pid, binary, root)
}

fn rollback_step(errors: &mut Vec<Value>, step: &str, action: impl FnOnce() -> Result<()>) -> bool {
    match action() {
        Ok(()) => true,
        Err(error) => {
            errors.push(json!({"step":step,"error":error.to_string()}));
            false
        }
    }
}

fn service_program(
    plist: &Value,
    label: &str,
    old_binary: &str,
    new_binary: &str,
    old_root: &str,
    new_root: &str,
) -> Result<Value> {
    require(
        plist["Label"] == label
            && plist["ProgramArguments"] == json!([old_binary, old_root])
            && (plist["Program"].is_null() || plist["Program"] == old_binary),
        "cutover_service_identity",
    )?;
    let mut next = plist.clone();
    next["ProgramArguments"] = json!([new_binary, new_root]);
    if !plist["Program"].is_null() {
        next["Program"] = json!(new_binary);
    }
    Ok(next)
}

pub fn run(config_path: &Path) -> Result<Value> {
    let config = load(private(config_path)?)?;
    require(
        config["protocol"] == "oo.library.refresh-cutover/v1",
        "cutover_protocol",
    )?;
    let gateway_path = private(text(&config, "gateway_config")?)?;
    let gateway_config = load(&gateway_path)?;
    let candidate_root =
        crate::refresh::output_path(Path::new(text(&gateway_config, "candidate_root")?))?;
    let root = candidate_root.as_path();
    require(
        gateway_config["expected_pack_digest"] == config["expected_pack_digest"]
            && gateway_config["schema_scope_digest"] == config["schema_scope_digest"],
        "cutover_candidate_binding",
    )?;
    let plist_path = private(text(&config, "launchd_plist")?)?;
    let old_plist = fs::read(&plist_path)?;
    require(
        sha(&old_plist) == text(&config, "expected_plist_sha256")?,
        "cutover_plist_drift",
    )?;
    let label = text(&config, "launchd_label")?;
    require(
        !label.is_empty() && !label.contains('/') && !label.chars().any(char::is_whitespace),
        "cutover_label",
    )?;
    let binary = text(&gateway_config["gateway_binary"], "path")?;
    let old_binary_pin = config.get("baseline_gateway_binary")
        .unwrap_or(&gateway_config["gateway_binary"]);
    let old_binary = text(old_binary_pin, "path")?;
    require(Path::new(old_binary).is_absolute()
        && digest(old_binary)? == text(old_binary_pin, "sha256")?, "cutover_baseline_binary_pin")?;
    let old_root = text(&gateway_config, "baseline_root")?;
    let out = crate::refresh::output_path(Path::new(text(&config, "output_dir")?))?;
    dir(&out)?;
    let mut calls = Calls::new(&out.join("calls"))?;
    let raw = calls.run(
        "read-launchd",
        &strings(&[
            "/usr/bin/plutil",
            "-convert",
            "json",
            "-o",
            "-",
            plist_path.to_str().ok_or("cutover_plist")?,
        ]),
        None,
    )?;
    let current: Value = serde_json::from_slice(&raw)?;
    let next = service_program(
        &current,
        label,
        old_binary,
        binary,
        old_root,
        root.to_str().ok_or("cutover_root")?,
    )?;
    save(out.join("new-service.json"), &next)?;
    put(out.join("previous-service.plist"), &old_plist)?;
    calls.run(
        "encode-launchd",
        &strings(&[
            "/usr/bin/plutil",
            "-convert",
            "xml1",
            "-o",
            out.join("new-service.plist")
                .to_str()
                .ok_or("cutover_path")?,
            out.join("new-service.json")
                .to_str()
                .ok_or("cutover_path")?,
        ]),
        None,
    )?;
    let old_config = load(Path::new(old_root).join("config.json"))?;
    let candidate_path = root.join("config.json");
    let original_candidate = fs::read(private(&candidate_path)?)?;
    let mut candidate_config = load(&candidate_path)?;
    require(
        candidate_config["listen"] == gateway_config["listen"],
        "cutover_candidate_listener",
    )?;
    let old_listener = text(&old_config, "listen")?;
    let uid =
        String::from_utf8(calls.run("operator-id", &strings(&["/usr/bin/id", "-u"]), None)?)?;
    let domain = format!("gui/{}", uid.trim());
    let service = format!("{domain}/{label}");
    let baseline = live_identity(&mut calls, &service, old_binary, Path::new(old_root))?;
    archive(&out, "baseline-service.json")?;
    save(out.join("baseline-service.json"), &baseline)?;
    let token = fs::read(private(text(&gateway_config, "token_file")?)?)?;
    let targets = array(&config, "token_targets")?
        .iter()
        .map(|v| private(v.as_str().ok_or("cutover_token_target")?))
        .collect::<Result<Vec<_>>>()?;
    require(!targets.is_empty(), "cutover_token_targets")?;
    let mut old_tokens = Vec::new();
    for (index, path) in targets.iter().enumerate() {
        writable_parent(path)?;
        let bytes = fs::read(path)?;
        put(out.join(format!("previous-client-{index}.token")), &bytes)?;
        old_tokens.push(bytes);
    }
    archive(&out, "rollback.json")?;
    // Reuse the real-query probe, never authorize a stop from an old marker alone.
    crate::refresh_gateway::probe(&gateway_path)?;
    require(
        load(&gateway_path)? == gateway_config,
        "cutover_gateway_config_drift",
    )?;
    let proof = verified_candidate(&gateway_config, root)?;
    let pid = proof["process"]["pid"].as_u64().ok_or("cutover_pid")?;
    require(
        live_identity(&mut calls, &service, old_binary, Path::new(old_root))? == baseline
            && digest(old_binary)? == text(old_binary_pin, "sha256")?
            && fs::read(&plist_path)? == old_plist
            && fs::read(&candidate_path)? == original_candidate
            && fs::read(private(text(&gateway_config, "token_file")?)?)? == token,
        "cutover_preflight_drift",
    )?;
    for (target, previous) in targets.iter().zip(&old_tokens) {
        require(fs::read(target)? == *previous, "cutover_token_drift")?;
    }
    let mut candidate_stopped = false;
    let mut live_stop_attempted = false;
    let mut replacement_start_attempted = false;
    let result = json!({"status":"succeeded","pack_digest":config["expected_pack_digest"],
        "schema_scope_digest":config["schema_scope_digest"],"live_gateway_switched":true,
        "local_manifest_verified":true,"local_queries_verified":true,
        "public_gateway_verified":false,"old_agentdb_retained":true});
    let switched = (|| -> Result<()> {
        require(
            verified_candidate(&gateway_config, root)? == proof,
            "cutover_candidate_drift",
        )?;
        require(
            process_identity(pid, binary, root)? == proof["process"],
            "cutover_candidate_owner",
        )?;
        calls.run(
            "stop-candidate",
            &strings(&["/bin/kill", "-TERM", &pid.to_string()]),
            None,
        )?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = Command::new("/bin/ps")
                .args(["-p", &pid.to_string(), "-o", "stat="])
                .output()?;
            let state_text = String::from_utf8(state.stdout)?;
            // A candidate started by this invocation can remain an unreaped zombie.
            if (!state.status.success() && state_text.trim().is_empty() && state.stderr.is_empty())
                || (state.status.success() && state_text.trim().starts_with('Z'))
            {
                break;
            }
            require(Instant::now() < deadline, "cutover_candidate_stop_timeout")?;
            std::thread::sleep(Duration::from_millis(100));
        }
        candidate_stopped = true;
        for name in ["candidate-process.json", "verified.json", "listen.addr"] {
            archive(root, name)?;
        }
        candidate_config["listen"] = json!(old_listener);
        replace_private(&candidate_path, &bytes(&candidate_config)?)?;
        require(
            live_identity(&mut calls, &service, old_binary, Path::new(old_root))? == baseline
                && digest(old_binary)? == text(old_binary_pin, "sha256")?,
            "cutover_baseline_process_drift",
        )?;
        live_stop_attempted = true;
        calls.run(
            "stop-live",
            &strings(&["/bin/launchctl", "bootout", &service]),
            None,
        )?;
        replace_private(&plist_path, &fs::read(out.join("new-service.plist"))?)?;
        for target in &targets {
            replace_private(target, &token)?;
        }
        replacement_start_attempted = true;
        calls.run(
            "start-live",
            &strings(&[
                "/bin/launchctl",
                "bootstrap",
                &domain,
                plist_path.to_str().ok_or("cutover_path")?,
            ]),
            None,
        )?;
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let ready = fs::read_to_string(root.join("listen.addr")).unwrap_or_default();
            if ready.trim() == old_listener {
                break;
            }
            require(Instant::now() < deadline, "cutover_live_start_timeout")?;
            std::thread::sleep(Duration::from_millis(200));
        }
        live_identity(&mut calls, &service, binary, root)?;
        let mut local = gateway_config.clone();
        local["listen"] = json!(old_listener);
        crate::refresh_gateway::verify_requests(&local, root, false)?;
        save(out.join("cutover.json"), &result)?;
        Ok(())
    })();
    if let Err(error) = switched {
        let mut errors = Vec::new();
        if replacement_start_attempted {
            rollback_step(&mut errors, "stop-replacement", || {
                // A failed bootstrap does not authorize stopping a different loaded job.
                let raw = calls.run(
                    "rollback-loaded-service",
                    &strings(&["/bin/launchctl", "print", &service]),
                    None,
                )?;
                if let Some(replacement_pid) =
                    loaded_service(&String::from_utf8(raw)?, &service, binary, root)?
                {
                    let owned = process_identity(replacement_pid, binary, root)?;
                    require(
                        live_identity(&mut calls, &service, binary, root)? == owned,
                        "cutover_replacement_drift",
                    )?;
                }
                // An owned, registered job with no PID must also be unloaded before bootstrap.
                calls.run(
                    "rollback-stop",
                    &strings(&["/bin/launchctl", "bootout", &service]),
                    None,
                )?;
                Ok(())
            });
        }
        if candidate_stopped {
            rollback_step(&mut errors, "restore-candidate-config", || {
                replace_private(&candidate_path, &original_candidate)
            });
            for name in ["candidate-process.json", "verified.json", "listen.addr"] {
                rollback_step(&mut errors, &format!("archive-{name}"), || {
                    archive(root, name)
                });
            }
        }
        let mut restored = false;
        if live_stop_attempted {
            let plist_restored = rollback_step(&mut errors, "restore-plist", || {
                replace_private(&plist_path, &old_plist)
            });
            for (index, (target, previous)) in targets.iter().zip(&old_tokens).enumerate() {
                rollback_step(&mut errors, &format!("restore-token-{index}"), || {
                    replace_private(target, previous)
                });
            }
            // Always attempt bootstrap, even if any file restoration failed.
            rollback_step(&mut errors, "bootstrap-previous", || {
                let backup = out.join("previous-service.plist");
                let bootstrap = if plist_restored { &plist_path } else { &backup };
                require(
                    fs::read(bootstrap)? == old_plist
                        && digest(old_binary)? == text(old_binary_pin, "sha256")?,
                    "cutover_rollback_plist_drift",
                )?;
                calls.run(
                    "rollback-bootstrap",
                    &strings(&[
                        "/bin/launchctl",
                        "bootstrap",
                        &domain,
                        bootstrap.to_str().ok_or("cutover_path")?,
                    ]),
                    None,
                )?;
                Ok(())
            });
            restored = rollback_step(&mut errors, "verify-previous-service", || {
                live_identity(&mut calls, &service, old_binary, Path::new(old_root))?;
                Ok(())
            });
        }
        save(
            out.join("rollback.json"),
            &json!({"reason":error.to_string(),
                "previous_runtime_restored":restored,"live_stop_attempted":live_stop_attempted,
                "errors":errors,"source_unchanged":true}),
        )?;
        return Err("cutover_failed_check_rollback_receipt".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    #[test]
    fn loaded_service_rejects_other_label_program_arguments_and_nested_pid() {
        let output = "gui/501/example = {\n program = /bin/gateway\n arguments = {\n /bin/gateway\n /old\n }\n pid = 42\n environment = {\n pid = 99\n }\n}\n";
        assert_eq!(
            loaded_service(output, "gui/501/example", "/bin/gateway", Path::new("/old")).unwrap(),
            Some(42)
        );
        for changed in [
            output.replace("gui/501/example", "gui/501/other"),
            output.replace("program = /bin/gateway", "program = /bin/other"),
            output.replace("\n /old\n", "\n /other\n"),
            output.replace(" pid = 42\n", " pid = 0\n"),
            output.replace(" pid = 42\n", " pid = 42\n pid = 99\n"),
        ] {
            assert!(
                loaded_service(
                    &changed,
                    "gui/501/example",
                    "/bin/gateway",
                    Path::new("/old")
                )
                .is_err()
            );
        }
        assert_eq!(
            loaded_service(
                &output.replace(" pid = 42\n", ""),
                "gui/501/example",
                "/bin/gateway",
                Path::new("/old")
            )
            .unwrap(),
            None,
        );
    }

    #[test]
    fn rollback_continues_after_plist_and_token_failures() {
        let mut errors = Vec::new();
        let mut attempted = Vec::new();
        for step in [
            "restore-plist",
            "restore-token-0",
            "restore-token-1",
            "bootstrap-previous",
        ] {
            rollback_step(&mut errors, step, || {
                attempted.push(step);
                if step == "restore-plist" || step == "restore-token-0" {
                    return Err("injected_write_failure".into());
                }
                Ok(())
            });
        }
        assert_eq!(attempted.last(), Some(&"bootstrap-previous"));
        assert_eq!(attempted.len(), 4);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[1]["step"], "restore-token-0");
        let receipt = json!({"reason":"initial_failure","errors":errors});
        assert_eq!(receipt["errors"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn parent_probe_preserves_token_and_failed_replace_cleans_its_temporary() {
        let serial = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("cutover-token-{}-{serial}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let token = root.join("client.token");
        put(&token, b"original").unwrap();
        writable_parent(&token).unwrap();
        assert_eq!(fs::read(&token).unwrap(), b"original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        let target = root.join("not-a-file");
        fs::create_dir(&target).unwrap();
        assert!(replace_private(&target, b"replacement").is_err());
        assert!(
            !target
                .with_extension(format!("refresh-{}.tmp", std::process::id()))
                .exists()
        );
        fs::remove_dir(&target).unwrap();
        put(&target, b"before").unwrap();
        replace_private(&target, b"after").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"after");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_exact_owned_service_arguments_can_switch() {
        let before =
            json!({"Label":"example","ProgramArguments":["/bin/gateway","/old"],"KeepAlive":true});
        let after = service_program(&before, "example", "/bin/gateway", "/bin/gateway", "/old", "/new").unwrap();
        assert_eq!(after["ProgramArguments"], json!(["/bin/gateway", "/new"]));
        assert_eq!(before["ProgramArguments"], json!(["/bin/gateway", "/old"]));
        assert!(service_program(&before, "other", "/bin/gateway", "/bin/new", "/old", "/new").is_err());
        assert!(service_program(&before, "example", "/unrelated", "/bin/new", "/old", "/new").is_err());
        let mut explicit = before.clone();
        explicit["Program"] = json!("/bin/gateway");
        let upgraded = service_program(&explicit, "example", "/bin/gateway", "/bin/new", "/old", "/new").unwrap();
        assert_eq!(upgraded["Program"], "/bin/new");
        assert_eq!(upgraded["ProgramArguments"], json!(["/bin/new", "/new"]));
        assert_eq!(upgraded["KeepAlive"], true);
        assert_eq!(explicit["Program"], "/bin/gateway");
    }
}
