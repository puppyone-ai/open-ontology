use crate::io::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::Instant,
};

pub fn batches(tables: &[Value]) -> Vec<&[Value]> {
    tables.chunks(32).collect()
}

fn exclusion(value: &Value) -> Result<()> {
    require(
        ["empty_without_key", "unmappable_type", "over_budget"].contains(&text(value, "reason")?)
            && !text(value, "upgrade_condition")?.trim().is_empty(),
        "invalid_exclusion",
    )
}

fn validate(manifest: &Value) -> Result<()> {
    require(
        manifest["generation"].as_u64().is_some_and(|n| n > 0),
        "batch_generation",
    )?;
    let concurrency = match manifest.get("concurrency") {
        Some(value) => value.as_u64().ok_or("batch_concurrency")?,
        None => 1,
    };
    require((1..=32).contains(&concurrency), "batch_concurrency")?;
    let mut names = BTreeSet::new();
    for table in array(manifest, "tables")? {
        let name = format!(
            "{}.{}",
            ident(text(table, "schema")?)?,
            ident(text(table, "table")?)?
        );
        require(names.insert(name), "duplicate_table")?;
        let columns = array(table, "columns")?;
        let mut seen = BTreeSet::new();
        require(!columns.is_empty(), "batch_columns")?;
        for column in columns {
            require(
                seen.insert(ident(column.as_str().ok_or("batch_column")?)?),
                "duplicate_column",
            )?;
        }
        if let Some(value) = table.get("exclusion") {
            exclusion(value)?;
        }
    }
    Ok(())
}

fn key(manifest: &Value, table: &Value) -> Result<String> {
    Ok(format!(
        "{}.{}:{}:{}",
        text(table, "schema")?,
        text(table, "table")?,
        manifest["generation"],
        sha(&bytes(
            &json!({"table":table,"config_sha256":manifest["config_sha256"]})
        )?)
    ))
}

fn atomic(root: &Path, ledger: &Value) -> Result<()> {
    let temp = root.join("ledger.next");
    if temp.exists() {
        require(
            !fs::symlink_metadata(&temp)?.file_type().is_symlink(),
            "ledger_symlink",
        )?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(&bytes(ledger)?)?;
    file.sync_all()?;
    fs::rename(temp, root.join("ledger.json"))?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}

pub fn completeness(manifest: &Value, ledger: &Value) -> Result<Vec<Value>> {
    validate(manifest)?;
    let mut issues = Vec::new();
    for table in array(manifest, "tables")? {
        let record = &ledger["records"][key(manifest, table)?];
        let status = record["status"].as_str().unwrap_or("missing");
        let reason = match status {
            "succeeded"
                if ["rows", "chunks", "bytes"]
                    .iter()
                    .all(|k| record[*k].as_u64().is_some())
                    && record["comparison"] == "equal" =>
            {
                None
            }
            "failed" if record["failure"].as_str().is_some_and(|s| !s.is_empty()) => None,
            "excluded"
                if record["exclusion"] == table["exclusion"]
                    && exclusion(&record["exclusion"]).is_ok() =>
            {
                None
            }
            "excluded" => Some("unjustified_exclusion"),
            _ => Some("missing_conclusion"),
        };
        if let Some(reason) = reason {
            issues.push(json!({"schema":table["schema"],"table":table["table"],"reason":reason}));
        }
    }
    Ok(issues)
}

pub fn has_failures(manifest: &Value, ledger: &Value) -> Result<bool> {
    for table in array(manifest, "tables")? {
        if ledger["records"][key(manifest, table)?]["status"] == "failed" {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn run<F>(manifest: &Value, root: &Path, reader: &F) -> Result<Value>
where
    F: Fn(&Value, &Path) -> Result<Value> + Sync,
{
    validate(manifest)?;
    let concurrency = manifest
        .get("concurrency")
        .and_then(Value::as_u64)
        .unwrap_or(1) as usize;
    dir(root)?;
    let lock_path = root.join("ledger.lock");
    if lock_path.exists() {
        private(&lock_path)?;
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)?;
    lock.try_lock()?;
    let path = root.join("ledger.json");
    let mut ledger = if path.exists() {
        load(private(&path)?)?
    } else {
        json!({"protocol":"oo.mysql.full-ledger/v1","records":{}})
    };
    require(
        ledger["protocol"] == "oo.mysql.full-ledger/v1" && ledger["records"].is_object(),
        "batch_ledger",
    )?;
    for (index, table) in array(manifest, "tables")?.iter().enumerate() {
        let key = key(manifest, table)?;
        if ledger["records"].get(&key).is_none() {
            ledger["records"][&key] = json!({"schema":table["schema"],"table":table["table"],"status":"pending","attempt":0,"rows":null,"chunks":null,"parts":null,"bytes":null,"elapsed_ms":0,"comparison":"not_run","failure":null});
        }
        ledger["records"][&key]["batch_index"] = json!(index / 32);
    }
    atomic(root, &ledger)?;
    for batch in batches(array(manifest, "tables")?) {
        for group in batch.chunks(concurrency) {
            let mut work = Vec::new();
            for table in group {
                let key = key(manifest, table)?;
                if ["succeeded", "excluded"]
                    .contains(&ledger["records"][&key]["status"].as_str().unwrap_or(""))
                {
                    continue;
                }
                if let Some(excluded) = table.get("exclusion") {
                    ledger["records"][&key]["status"] = json!("excluded");
                    ledger["records"][&key]["exclusion"] = excluded.clone();
                    atomic(root, &ledger)?;
                    continue;
                }
                let attempt = ledger["records"][&key]["attempt"]
                    .as_u64()
                    .ok_or("batch_attempt")?
                    .checked_add(1)
                    .ok_or("batch_attempt")?;
                let record = &mut ledger["records"][&key];
                if attempt > 1 {
                    let mut previous = record.clone();
                    previous
                        .as_object_mut()
                        .ok_or("batch_record")?
                        .remove("history");
                    if record.get("history").is_none() {
                        record["history"] = json!([]);
                    }
                    record["history"]
                        .as_array_mut()
                        .ok_or("batch_history")?
                        .push(previous);
                }
                record["attempt"] = json!(attempt);
                record["status"] = json!("running");
                record["failure"] = Value::Null;
                for k in ["rows", "chunks", "bytes"] {
                    record[k] = Value::Null;
                }
                record["comparison"] = json!("not_run");
                let out = root
                    .join(sha(key.as_bytes()))
                    .join(format!("attempt-{attempt}"));
                dir(&out)?;
                work.push((key, table, out));
            }
            atomic(root, &ledger)?;
            std::thread::scope(|scope| -> Result<()> {
                let handles = work
                    .iter()
                    .map(|(key, table, out)| {
                        (
                            key,
                            scope.spawn(move || {
                                let started = Instant::now();
                                let result = reader(table, out).map_err(|e| e.to_string());
                                (started.elapsed().as_millis() as u64, result)
                            }),
                        )
                    })
                    .collect::<Vec<_>>();
                for (key, handle) in handles {
                    let (elapsed, result) = handle
                        .join()
                        .unwrap_or_else(|_| (0, Err("batch_reader_panicked".into())));
                    let record = &mut ledger["records"][key];
                    record["elapsed_ms"] = json!(elapsed);
                    match result {
                        Ok(metrics)
                            if ["rows", "chunks", "bytes"]
                                .iter()
                                .all(|k| metrics[*k].as_u64().is_some())
                                && metrics["comparison"] == "equal" =>
                        {
                            record["status"] = json!("succeeded");
                            if let Some(parts) = metrics.get("parts") {
                                record["parts"] = parts.clone();
                            }
                            for k in ["rows", "chunks", "bytes", "comparison"] {
                                record[k] = metrics[k].clone();
                            }
                        }
                        Ok(_) => {
                            record["status"] = json!("failed");
                            record["failure"] = json!("batch_result_metrics");
                        }
                        Err(error) => {
                            record["status"] = json!("failed");
                            record["failure"] = json!(error);
                        }
                    }
                    atomic(root, &ledger)?;
                }
                Ok(())
            })?;
        }
    }
    Ok(ledger)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(n: usize) -> Value {
        json!({"generation":1,"tables":(0..n).map(|i|json!({"schema":"sample","table":format!("t_{i}"),"columns":["id"]})).collect::<Vec<_>>()})
    }
    #[test]
    fn complete_inventory_has_no_silent_omissions() {
        let m = manifest(672);
        let mut ledger = json!({"records":{}});
        for table in m["tables"].as_array().unwrap() {
            ledger["records"][key(&m, table).unwrap()] =
                json!({"status":"failed","failure":"explicit outcome"});
        }
        assert!(completeness(&m, &ledger).unwrap().is_empty());
        let missing = key(&m, &m["tables"][671]).unwrap();
        ledger["records"].as_object_mut().unwrap().remove(&missing);
        let issues = completeness(&m, &ledger).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["table"], "t_671");
        let mut invalid = m.clone();
        invalid["concurrency"] = json!("2");
        assert!(validate(&invalid).is_err());
    }
    #[test]
    fn kill_child() {
        let Ok(root) = std::env::var("OO_WF_BATCH_KILL_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        run(&manifest(50), &root, &|table, _| {
            if table["table"] == "t_1" {
                fs::write(root.join("ready"), b"running").unwrap();
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
            Ok(json!({"rows":1,"chunks":1,"bytes":20,"comparison":"equal"}))
        })
        .unwrap();
    }
    #[test]
    fn actual_kill_releases_lock_and_resumes_only_unfinished() {
        let root = std::env::temp_dir().join(format!("oo-wf-batch-kill-{}", std::process::id()));
        dir(&root).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "batch::tests::kill_child"])
            .env("OO_WF_BATCH_KILL_ROOT", &root)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(15);
        while !root.join("ready").exists() {
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not start");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let m = manifest(50);
        let ledger = run(&m, &root, &|table, out| {
            assert_ne!(table["table"], "t_0");
            if table["table"] == "t_1" {
                assert!(out.ends_with("attempt-2"));
            }
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(json!({"rows":1,"chunks":1,"bytes":20,"comparison":"equal"}))
        })
        .unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 49);
        assert!(completeness(&m, &ledger).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bounded_parallel_and_exclusions() {
        let root =
            std::env::temp_dir().join(format!("oo-wf-batch-parallel-{}", std::process::id()));
        dir(&root).unwrap();
        let mut m = manifest(33);
        m["concurrency"] = json!(3);
        m["tables"][0]["exclusion"] =
            json!({"reason":"over_budget","upgrade_condition":"raise reviewed budget"});
        let active = std::sync::atomic::AtomicUsize::new(0);
        let peak = std::sync::atomic::AtomicUsize::new(0);
        let ledger = run(&m, &root, &|_, _| {
            let n = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            peak.fetch_max(n, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            Ok(json!({"rows":0,"chunks":1,"bytes":20,"comparison":"equal"}))
        })
        .unwrap();
        assert_eq!(peak.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert!(completeness(&m, &ledger).unwrap().is_empty());
        let mut bad = ledger.clone();
        let k = key(&m, &m["tables"][0]).unwrap();
        bad["records"][&k]["exclusion"] = Value::Null;
        assert_eq!(completeness(&m, &bad).unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ledger_lock_rejects_another_runner() {
        let root = std::env::temp_dir().join(format!("oo-wf-batch-lock-{}", std::process::id()));
        dir(&root).unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(root.join("ledger.lock"))
            .unwrap();
        lock.lock().unwrap();
        assert!(run(&manifest(0), &root, &|_, _| Ok(Value::Null)).is_err());
        drop(lock);
        assert!(run(&manifest(0), &root, &|_, _| Ok(Value::Null)).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn completeness_rejects_unproven_success() {
        let m = manifest(1);
        let key = key(&m, &m["tables"][0]).unwrap();
        let ledger = json!({"records":{key:{"status":"succeeded"}}});
        assert_eq!(completeness(&m, &ledger).unwrap().len(), 1);
    }
    #[test]
    fn boundaries() {
        for (n, expected) in [(0, 0), (1, 1), (32, 1), (33, 2)] {
            assert_eq!(
                batches(array(&manifest(n), "tables").unwrap()).len(),
                expected
            );
        }
    }
    #[test]
    fn exclusion_validation() {
        for reason in ["empty_without_key", "unmappable_type", "over_budget"] {
            assert!(
                exclusion(&json!({"reason":reason,"upgrade_condition":"new capability"})).is_ok()
            );
        }
        assert!(exclusion(&json!({"reason":"other","upgrade_condition":"retry"})).is_err());
        assert!(exclusion(&json!({"reason":"over_budget","upgrade_condition":" "})).is_err());
    }
    #[test]
    fn resume_isolation_and_completeness() {
        let root = std::env::temp_dir().join(format!("oo-wf-batch-test-{}", std::process::id()));
        dir(&root).unwrap();
        let m = manifest(50);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let reader = |table: &Value, _: &Path| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if table["table"] == "t_2" {
                Err("fixture_failure".into())
            } else {
                Ok(json!({"rows":3,"chunks":1,"bytes":20,"comparison":"equal"}))
            }
        };
        let ledger = run(&m, &root, &reader).unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 50);
        assert_eq!(
            ledger["records"][key(&m, &m["tables"][31]).unwrap()]["batch_index"],
            0
        );
        assert_eq!(
            ledger["records"][key(&m, &m["tables"][32]).unwrap()]["batch_index"],
            1
        );
        assert_eq!(
            ledger["records"]
                .as_object()
                .unwrap()
                .values()
                .filter(|r| r["status"] == "failed")
                .count(),
            1
        );
        assert!(completeness(&m, &ledger).unwrap().is_empty());
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let retried = run(&m, &root, &|_, _| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(json!({"rows":3,"chunks":1,"bytes":20,"comparison":"equal"}))
        })
        .unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            retried["records"][key(&m, &m["tables"][2]).unwrap()]["history"][0]["failure"],
            "fixture_failure"
        );
        let mut changed = m.clone();
        changed["generation"] = json!(2);
        assert_eq!(
            completeness(&changed, &load(root.join("ledger.json")).unwrap())
                .unwrap()
                .len(),
            50
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn atomic_and_interrupted_running() {
        let root = std::env::temp_dir().join(format!("oo-wf-batch-atomic-{}", std::process::id()));
        dir(&root).unwrap();
        let m = manifest(1);
        let key = key(&m, &m["tables"][0]).unwrap();
        let ledger = json!({"protocol":"oo.mysql.full-ledger/v1","records":{key.clone():{"status":"running","attempt":1}}});
        atomic(&root, &ledger).unwrap();
        fs::write(root.join("ledger.next"), b"partial").unwrap();
        assert_eq!(load(root.join("ledger.json")).unwrap(), ledger);
        let result = run(&m, &root, &|_, out| {
            assert!(out.ends_with("attempt-2"));
            Ok(json!({"rows":0,"chunks":0,"bytes":0,"comparison":"equal"}))
        })
        .unwrap();
        assert_eq!(result["records"][&key]["status"], "succeeded");
        fs::remove_dir_all(root).unwrap();
    }
}
