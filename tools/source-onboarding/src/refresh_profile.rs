//! Execute one installed full-capture profile and verify its durable table ledger.
use crate::io::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn prepare(config_path: &Path, out: &Path) -> Result<Value> {
    let config = load(private(config_path)?)?;
    let required = [
        "protocol",
        "inventory",
        "source_counts",
        "target_empty",
        "profiles",
        "runtime",
        "round",
        "pack_digest",
    ];
    let object = config.as_object().ok_or("refresh_config")?;
    require(
        required.iter().all(|k| object.contains_key(*k))
            && object
                .keys()
                .all(|k| required.contains(&k.as_str()) || k == "analysis_additions")
            && config["protocol"] == "oo.library.refresh-profiles/v1",
        "refresh_config",
    )?;
    let round = config["round"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or("refresh_round")?;
    let inventory = array(&config, "inventory")?;
    let mut expected = BTreeSet::new();
    let mut sources = BTreeSet::new();
    for table in inventory {
        let source = text(table, "source_id")?;
        require(
            !source.contains("..")
                && source
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
            "refresh_source_id",
        )?;
        sources.insert(source.to_owned());
        require(
            expected.insert(format!(
                "{}.{}",
                text(table, "schema")?,
                text(table, "table")?
            )),
            "refresh_duplicate_dataset",
        )?;
    }
    let counts = config["source_counts"]
        .as_object()
        .ok_or("refresh_counts")?;
    require(
        !expected.is_empty()
            && counts.keys().cloned().collect::<BTreeSet<_>>() == expected
            && counts.values().all(|v| v.as_u64().is_some()),
        "refresh_count_coverage",
    )?;
    let profiles = config["profiles"].as_object().ok_or("refresh_profiles")?;
    require(
        profiles.keys().cloned().collect::<BTreeSet<_>>() == sources,
        "refresh_source_coverage",
    )?;
    let target_empty = array(&config, "target_empty")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or("refresh_empty"))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    require(target_empty.is_subset(&expected), "refresh_empty_scope")?;
    let additions = config
        .get("analysis_additions")
        .cloned()
        .unwrap_or(json!({}));
    require(
        additions.is_object()
            && additions
                .as_object()
                .unwrap()
                .keys()
                .all(|k| expected.contains(k)),
        "refresh_addition_scope",
    )?;
    let empty = expected
        .iter()
        .filter(|name| {
            counts[*name] == 0 && target_empty.contains(*name) && additions.get(*name).is_none()
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let runtime = &config["runtime"];
    let out = crate::refresh::output_path(out)?;
    require(!out.exists(), "refresh_output_exists")?;
    fs::create_dir_all(out.parent().ok_or("refresh_output_parent")?)?;
    fs::create_dir(&out)?;
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
    let scope_digest = sha(&bytes(&json!(expected))?);
    let mut jobs = Vec::new();
    let mut ignored = Vec::new();
    for source in sources {
        let reference = &profiles[&source];
        let original = load(private(text(reference, "operator")?)?)?;
        let mut manifest = load(private(text(reference, "manifest")?)?)?;
        require(
            original["onboarding"]["full"]["source_id"] == source,
            "refresh_source_identity",
        )?;
        let old_pipeline = &original["onboarding"]["full"]["pipeline"];
        require(
            original["onboarding"].is_object()
                && original["onboarding"]["full"].is_object()
                && original["onboarding"]["audit"].is_object()
                && old_pipeline.is_object()
                && ["target", "catalog_target", "audit_target"]
                    .iter()
                    .all(|k| old_pipeline[*k].is_object()),
            "refresh_operator_shape",
        )?;
        let names = inventory
            .iter()
            .filter(|t| t["source_id"] == source)
            .map(|t| Ok(format!("{}.{}", text(t, "schema")?, text(t, "table")?)))
            .collect::<Result<BTreeSet<_>>>()?;
        let mut selected = Vec::new();
        let mut covered = BTreeSet::new();
        for table in array(&manifest, "tables")? {
            let name = format!("{}.{}", text(table, "schema")?, text(table, "table")?);
            if !names.contains(&name) {
                ignored.push(
                    json!({"source":source,"dataset":name,"reason":"not_current_membership"}),
                );
                continue;
            }
            require(
                covered.insert(name.clone()),
                "refresh_duplicate_profile_table",
            )?;
            if empty.contains(&name) {
                continue;
            }
            let mut table = table.clone();
            if let Some(added) = additions.get(&name) {
                let columns = table["columns"].as_array_mut().ok_or("refresh_columns")?;
                let mut seen = columns
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned).ok_or("refresh_column"))
                    .collect::<std::result::Result<BTreeSet<_>, _>>()?;
                for col in added.as_array().ok_or("refresh_addition")? {
                    let col = col
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or("refresh_addition")?;
                    require(seen.insert(col.to_owned()), "refresh_duplicate_addition")?;
                    columns.push(json!(col));
                }
            }
            selected.push(table);
        }
        require(covered == names, "refresh_profile_coverage")?;
        if selected.is_empty() {
            continue;
        }
        require(
            selected.iter().all(|t| t["schema"] == original["database"]),
            "refresh_database_binding",
        )?;
        selected.sort_by_key(|t| t["table"].as_str().unwrap_or("").to_owned());
        let root = out.join(&source);
        dir(&root)?;
        let mut operator = original.clone();
        let onboarding = &mut operator["onboarding"];
        require(
            runtime["runner_container"] != onboarding["container"]
                && runtime["control_container"]
                    != onboarding["full"]["pipeline"]["control_container"],
            "isolated_refresh_runtime_required",
        )?;
        let secret_root = PathBuf::from(text(onboarding, "host_secret_root")?).canonicalize()?;
        require(root.starts_with(&secret_root), "refresh_secret_root")?;
        onboarding["container"] = runtime["runner_container"].clone();
        onboarding["worker_config"] = json!(root.join("worker.conf"));
        onboarding["secret_registry"] = json!(root.join("secrets.json"));
        onboarding["full"]["batch_manifest"] = json!(root.join("manifest.json"));
        let pipeline = &mut onboarding["full"]["pipeline"];
        pipeline["control_container"] = runtime["control_container"].clone();
        pipeline["state_root"] = json!(root.join("state"));
        pipeline["target"]["password_file"] = runtime["runtime_password_file"].clone();
        pipeline["catalog_target"]["password_file"] = runtime["admin_password_file"].clone();
        pipeline["catalog_target"]["ca_file"] = runtime["target_ca_file"].clone();
        pipeline["audit_target"]["host"] = runtime["control_container"].clone();
        onboarding["audit"]["allow_unresolved_chunk_fraction"] = json!(0.0);
        manifest["generation"] = json!(round);
        manifest["concurrency"] = json!(1);
        manifest["tables"] = json!(selected);
        manifest["config_sha256"] = json!(sha(&bytes(&operator)?));
        save(root.join("operator.json"), &operator)?;
        save(root.join("manifest.json"), &manifest)?;
        let input: serde_json::Map<_, _> =
            ["type", "host", "port", "database", "user", "secret_ref"]
                .into_iter()
                .map(|key| (key.to_owned(), operator[key].clone()))
                .collect();
        save(root.join("input.json"), &Value::Object(input))?;
        let registry: serde_json::Map<_, _> = std::iter::once((
            text(&operator, "secret_ref")?.to_owned(),
            operator["password_file"].clone(),
        ))
        .collect();
        save(root.join("secrets.json"), &Value::Object(registry))?;
        let worker = fs::read_to_string(private(text(&original["onboarding"], "worker_config")?)?)?;
        let mut lines = Vec::new();
        for line in worker.lines() {
            let name = line.split('=').next().unwrap_or("");
            let replacement = match name {
                "runtime_password_file" | "target_runtime_password_file" => {
                    runtime["runtime_password_file"].as_str()
                }
                "admin_password_file" => runtime["admin_password_file"].as_str(),
                _ => None,
            };
            lines.push(
                replacement
                    .map(|v| format!("{name}={v}"))
                    .unwrap_or_else(|| line.to_owned()),
            );
        }
        put(
            &root.join("worker.conf"),
            (lines.join("\n") + "\n").as_bytes(),
        )?;
        save(
            root.join("run.json"),
            &json!({"runner":runtime["runner_container"],
            "binary":reference["binary"],"root":root,"pack_digest":config["pack_digest"],
            "schema_scope_digest":scope_digest,"timeout_seconds":21600}),
        )?;
        let rows = names.difference(&empty).try_fold(0u64, |sum, n| {
            sum.checked_add(counts[n].as_u64().ok_or("refresh_rows")?)
                .ok_or("refresh_rows_overflow")
        })?;
        jobs.push(json!({"key":source,"profile":root.join("run.json"),
            "tables":selected.len(),"source_rows":rows}));
    }
    let result = json!({"protocol":"oo.library.refresh-profiles-result/v1",
        "pack_digest":config["pack_digest"],"schema_scope_digest":scope_digest,
        "scope":expected,"empty_verified_nochange":empty,"capture_profiles":jobs,
        "source_ids_preserved":true,"analysis_only_additions":additions,
        "semantic_review_required":!additions.as_object().unwrap().is_empty(),
        "ignored_legacy_template_tables":ignored,"published":false});
    save(out.join("profiles.json"), &result)?;
    Ok(result)
}

fn conclusions(manifest: &Value, ledger: &Value) -> Result<(u64, u64)> {
    let expected = array(manifest, "tables")?
        .iter()
        .map(|v| Ok((text(v, "schema")?.to_owned(), text(v, "table")?.to_owned())))
        .collect::<Result<BTreeSet<_>>>()?;
    require(
        ledger["protocol"] == "oo.mysql.full-ledger/v1",
        "refresh_ledger_protocol",
    )?;
    let records = ledger["records"].as_object().ok_or("refresh_ledger")?;
    let mut actual = BTreeSet::new();
    let mut rows = 0u64;
    for row in records.values() {
        require(
            actual.insert((
                text(row, "schema")?.to_owned(),
                text(row, "table")?.to_owned(),
            )) && row["status"] == "succeeded"
                && row["comparison"] == "equal",
            "refresh_capture_incomplete",
        )?;
        rows = rows
            .checked_add(row["rows"].as_u64().ok_or("refresh_rows")?)
            .ok_or("refresh_rows_overflow")?;
    }
    require(
        !expected.is_empty() && expected == actual,
        "refresh_capture_coverage",
    )?;
    Ok((actual.len() as u64, rows))
}

pub fn verify_profiles(path: &Path, prepared_only: bool) -> Result<Value> {
    let plan = load(private(path)?)?;
    require(
        plan["protocol"] == "oo.library.refresh-profiles-result/v1",
        "refresh_plan_protocol",
    )?;
    let scope = array(&plan, "scope")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or("refresh_scope"))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    require(
        scope.len() == array(&plan, "scope")?.len() && !scope.is_empty(),
        "refresh_scope",
    )?;
    let mut covered = array(&plan, "empty_verified_nochange")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or("refresh_empty"))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    let empty = covered.len();
    let mut rows = 0u64;
    let mut captured = 0u64;
    for profile in array(&plan, "capture_profiles")? {
        let run = load(private(text(profile, "profile")?)?)?;
        require(
            run["pack_digest"] == plan["pack_digest"]
                && run["schema_scope_digest"] == plan["schema_scope_digest"],
            "refresh_plan_binding",
        )?;
        let root = Path::new(text(&run, "root")?);
        let operator = load(private(root.join("operator.json"))?)?;
        let manifest = load(private(root.join("manifest.json"))?)?;
        require(
            operator["onboarding"]["full"]["source_id"] == profile["key"]
                && manifest["config_sha256"] == sha(&bytes(&operator)?)
                && digest(text(&run["binary"], "path")?)? == text(&run["binary"], "sha256")?,
            "refresh_profile_binding",
        )?;
        for t in array(&manifest, "tables")? {
            require(
                covered.insert(format!("{}.{}", text(t, "schema")?, text(t, "table")?)),
                "refresh_duplicate_coverage",
            )?;
        }
        if !prepared_only {
            let (tables, count) =
                conclusions(&manifest, &load(private(root.join("run/ledger.json"))?)?)?;
            captured += tables;
            rows = rows.checked_add(count).ok_or("refresh_rows_overflow")?;
        }
    }
    require(covered == scope, "refresh_plan_coverage")?;
    Ok(
        json!({"status":"succeeded","pack_digest":plan["pack_digest"],
        "schema_scope_digest":plan["schema_scope_digest"],"tables":scope.len(),
        "prepared_only":prepared_only,"captured_tables":captured,"captured_rows":rows,
        "verified_empty_retained":empty,"source_counts_rechecked_by_this_command":false,
        "published":false}),
    )
}

pub(crate) fn parallel_capture<F, W>(
    profiles: &[Value],
    concurrency: usize,
    runner: &F,
    mut record: W,
) -> Result<()>
where
    F: Fn(&Value) -> std::result::Result<Value, String> + Sync,
    W: FnMut(usize, &Value, std::result::Result<Value, String>) -> Result<()>,
{
    require((1..=4).contains(&concurrency), "refresh_concurrency")?;
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        let start = |index: usize| {
            let tx = tx.clone();
            scope.spawn(move || {
                let outcome =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner(&profiles[index])))
                        .unwrap_or_else(|_| Err("refresh_capture_thread".into()));
                let _ = tx.send((index, outcome));
            });
        };
        let mut next = concurrency.min(profiles.len());
        let mut active = next;
        let mut failed = false;
        let mut record_error = None;
        for index in 0..next {
            start(index);
        }
        while active > 0 {
            let (index, outcome) = rx.recv().map_err(|_| "refresh_capture_channel")?;
            active -= 1;
            failed |= outcome.is_err();
            if let Err(error) = record(index, &profiles[index], outcome) {
                failed = true;
                if record_error.is_none() {
                    record_error = Some(error);
                }
            }
            // Drain owned captures on failure; never abandon an unconfirmed remote task.
            if !failed && next < profiles.len() {
                start(next);
                next += 1;
                active += 1;
            }
        }
        if let Some(error) = record_error {
            return Err(error);
        }
        require(!failed, "refresh_capture_failed")
    })
}

pub fn capture_profiles(path: &Path, concurrency: usize) -> Result<Value> {
    require((1..=4).contains(&concurrency), "refresh_concurrency")?;
    verify_profiles(path, true)?;
    let plan = load(private(path)?)?;
    let mut profiles = array(&plan, "capture_profiles")?.clone();
    profiles.sort_by_key(|p| std::cmp::Reverse(p["source_rows"].as_u64().unwrap_or(0)));
    let root = path.parent().ok_or("refresh_plan_parent")?;
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    parallel_capture(
        &profiles,
        concurrency,
        &|profile| {
            run(Path::new(profile["profile"].as_str().unwrap_or(""))).map_err(|e| e.to_string())
        },
        |index, profile, outcome| {
            let receipt = match outcome {
                Ok(result) => json!({"source":profile["key"],"status":"succeeded","result":result}),
                Err(reason) => json!({"source":profile["key"],"status":"failed","reason":reason}),
            };
            save(
                root.join(format!("capture-{serial}-{index:04}.json")),
                &receipt,
            )
        },
    )?;
    verify_profiles(path, false)
}

pub fn wait_profiles(path: &Path, seconds: u64) -> Result<Value> {
    require((1..=43200).contains(&seconds), "refresh_wait_budget")?;
    verify_profiles(path, true)?;
    let plan = load(private(path)?)?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let mut pending = false;
        for profile in array(&plan, "capture_profiles")? {
            let run = load(private(text(profile, "profile")?)?)?;
            let root = Path::new(text(&run, "root")?);
            require(
                !root.join("capture-remote-state-unconfirmed.json").exists(),
                "refresh_capture_remote_state_requires_reconciliation",
            )?;
            let ledger = root.join("run/ledger.json");
            if !ledger.exists() {
                pending = true;
                continue;
            }
            let ledger = load(private(ledger)?)?;
            for row in ledger["records"]
                .as_object()
                .ok_or("refresh_ledger")?
                .values()
            {
                require(row["status"] != "failed", "refresh_capture_failed")?;
                pending |= row["status"] != "succeeded";
            }
        }
        if !pending {
            return verify_profiles(path, false);
        }
        require(Instant::now() < deadline, "refresh_wait_deadline")?;
        std::thread::sleep(Duration::from_secs(5));
    }
}

pub fn run(path: &Path) -> Result<Value> {
    let config = load(private(path)?)?;
    let root = PathBuf::from(text(&config, "root")?);
    let root_meta = fs::symlink_metadata(&root)?;
    require(
        root.is_absolute()
            && root_meta.is_dir()
            && !root_meta.file_type().is_symlink()
            && root_meta.permissions().mode() & 0o077 == 0,
        "refresh_private_output",
    )?;
    require(
        !root.ancestors().any(|p| p.join(".git").exists()),
        "refresh_private_output",
    )?;
    let binary = PathBuf::from(text(&config["binary"], "path")?);
    let binary_meta = fs::symlink_metadata(&binary)?;
    require(
        binary.is_absolute()
            && binary_meta.is_file()
            && !binary_meta.file_type().is_symlink()
            && binary_meta.permissions().mode() & 0o022 == 0,
        "refresh_binary",
    )?;
    require(
        digest(&binary)? == text(&config["binary"], "sha256")?,
        "refresh_binary_drift",
    )?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join("refresh-profile.lock"))?;
    lock.try_lock()?;
    require(
        !root.join("capture-remote-state-unconfirmed.json").exists(),
        "refresh_capture_remote_state_requires_reconciliation",
    )?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let log = |kind: &str| -> Result<fs::File> {
        Ok(OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join(format!("capture-rust-{suffix}.{kind}")))?)
    };
    let timeout = config["timeout_seconds"]
        .as_u64()
        .ok_or("refresh_timeout")?;
    require((1..=21600).contains(&timeout), "refresh_timeout")?;
    let mut child = Command::new("docker")
        .args([
            "exec",
            text(&config, "runner")?,
            binary.to_str().ok_or("refresh_binary")?,
            "full",
        ])
        .arg("--input")
        .arg(root.join("input.json"))
        .arg("--operator")
        .arg(root.join("operator.json"))
        .arg("--out")
        .arg(root.join("run"))
        .stdin(Stdio::null())
        .stdout(log("stdout")?)
        .stderr(log("stderr")?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        if let Some(status) = child.try_wait()? {
            require(status.success(), "refresh_capture_failed")?;
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            // A killed Docker exec client is not proof that its remote task stopped.
            save(
                root.join("capture-remote-state-unconfirmed.json"),
                &json!({
                    "reason":"docker_exec_client_timeout","runner":config["runner"],
                    "profile":path,"automatic_retry_allowed":false,
                    "required_action":"reconcile owned remote process and durable worker receipts",
                }),
            )?;
            return Err("refresh_capture_timeout_remote_state_unconfirmed".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let (tables, rows) = conclusions(
        &load(root.join("manifest.json"))?,
        &load(root.join("run/ledger.json"))?,
    )?;
    Ok(
        json!({"status":"succeeded","pack_digest":config["pack_digest"],
        "schema_scope_digest":config["schema_scope_digest"],"tables":tables,"rows":rows,
        "loader_and_independent_comparison":true,"published":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_exit_requires_complete_independently_compared_ledger() {
        let manifest = json!({"tables":[{"schema":"app","table":"orders"}]});
        let mut ledger = json!({"protocol":"oo.mysql.full-ledger/v1","records":{
            "key":{"schema":"app","table":"orders","status":"succeeded","comparison":"equal","rows":4}}});
        assert_eq!(conclusions(&manifest, &ledger).unwrap(), (1, 4));
        ledger["records"]["key"]["comparison"] = json!("not_run");
        assert!(conclusions(&manifest, &ledger).is_err());
        ledger["records"]["key"]["comparison"] = json!("equal");
        ledger["records"]["key"]["table"] = json!("different");
        assert!(conclusions(&manifest, &ledger).is_err());
    }

    #[test]
    fn capture_pool_is_bounded_and_records_each_result() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let items = [json!(0), json!(1), json!(2), json!(3), json!(4)];
        let runner = |item: &Value| {
            let n = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(10));
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(item.clone())
        };
        let mut recorded = BTreeSet::new();
        parallel_capture(&items, 2, &runner, |index, item, result| {
            assert_eq!(result.unwrap(), *item);
            assert!(recorded.insert(index));
            Ok(())
        })
        .unwrap();
        assert_eq!(recorded.len(), items.len());
        assert!(peak.load(Ordering::SeqCst) <= 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(capture_profiles(Path::new("/not-read"), 0).is_err());
    }

    #[test]
    fn capture_pool_refills_before_slow_peer_finishes() {
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::sync::Mutex::new(rx);
        let runner = |item: &Value| {
            if *item == 0 {
                rx.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|_| "idle_slot_not_refilled".to_string())?;
            } else if *item == 2 {
                tx.send(()).unwrap();
            }
            Ok(item.clone())
        };
        parallel_capture(&[json!(0), json!(1), json!(2)], 2, &runner, |_, _, _| Ok(()))
            .unwrap();
    }

    #[test]
    fn capture_failure_drains_inflight_and_stops_new_dispatch() {
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::sync::Mutex::new(rx);
        let mut recorded = BTreeSet::new();
        let result = parallel_capture(
            &[json!(0), json!(1), json!(2)],
            2,
            &|item| {
                if *item == 0 {
                    return Err("capture_failed".into());
                }
                assert_eq!(*item, 1);
                rx.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(item.clone())
            },
            |index, _, outcome| {
                recorded.insert(index);
                if index == 0 {
                    assert!(outcome.is_err());
                    tx.send(()).unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(recorded, BTreeSet::from([0, 1]));
    }

    #[test]
    fn capture_receipt_failure_stops_dispatch_and_panic_is_recorded() {
        let mut calls = 0;
        let result = parallel_capture(
            &[json!(0), json!(1)],
            1,
            &|item| Ok(item.clone()),
            |_, _, _| {
                calls += 1;
                Err("disk_write_failed".into())
            },
        );
        assert_eq!(result.unwrap_err().to_string(), "disk_write_failed");
        assert_eq!(calls, 1);
        let mut recorded = None;
        assert!(
            parallel_capture(
                &[json!(0)],
                1,
                &|_| panic!("capture panic"),
                |_, _, outcome| {
                    recorded = Some(outcome);
                    Ok(())
                },
            )
            .is_err()
        );
        assert_eq!(recorded, Some(Err("refresh_capture_thread".into())));
    }
}
