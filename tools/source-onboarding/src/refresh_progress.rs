//! Read-only observations of full-refresh ledgers; artifact presence is not a success receipt.
use crate::io::{Result, array, bytes, ident, require, sha, text};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const STATES: [&str; 6] = ["not_started", "pending", "running", "succeeded", "failed", "excluded"];
const PHASES: [&str; 3] = ["capture_load_catalog", "independent_comparison", "post_compare_profile"];
const COUNTS: [&str; 5] =
    ["planned_tables", "planned_rows", "completed_tables", "completed_rows", "completed_elapsed_ms"];

fn metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            require(!m.file_type().is_symlink(), "progress_symlink")?;
            Ok(Some(m))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn read(path: &Path) -> Result<Option<Value>> {
    require(path.is_absolute(), "progress_absolute_path")?;
    let Some(m) = metadata(path)? else { return Ok(None) };
    require(m.is_file(), "progress_private_file")?;
    // One open descriptor sees either atomic ledger version, never ledger.next or a digest race.
    let mut file = fs::File::open(path)?;
    require(file.metadata()?.permissions().mode() & 0o077 == 0, "progress_private_file")?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    Ok(Some(serde_json::from_slice(&data).map_err(|_| "progress_invalid_json")?))
}

fn required(path: &Path) -> Result<Value> {
    read(path)?.ok_or_else(|| "progress_missing_configuration".into())
}

fn number(v: &Value, key: &str) -> Result<u64> {
    v[key].as_u64().ok_or_else(|| "progress_invalid_number".into())
}

fn add(v: &mut Value, key: &str, n: u64) -> Result<()> {
    v[key] = json!(number(v, key)?.checked_add(n).ok_or("progress_count_overflow")?);
    Ok(())
}

fn names(v: &Value, field: &str) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for value in array(v, field)? {
        require(names.insert(value.as_str().ok_or("progress_name")?.to_owned()), "progress_duplicate_name")?;
    }
    Ok(names)
}

fn key(manifest: &Value, table: &Value) -> Result<String> {
    Ok(format!(
        "{}.{}:{}:{}",
        ident(text(table, "schema")?)?,
        ident(text(table, "table")?)?,
        manifest["generation"],
        sha(&bytes(&json!({"table":table,"config_sha256":manifest["config_sha256"]}))?)
    ))
}

fn seconds(time: SystemTime) -> Result<f64> {
    Ok(time.duration_since(UNIX_EPOCH)?.as_secs_f64())
}

fn stamp(path: &Path) -> Result<Option<f64>> {
    metadata(path)?
        .map(|m| {
            require(m.is_file(), "progress_artifact_file")?;
            seconds(m.modified()?)
        })
        .transpose()
}

fn span(start: Option<f64>, end: Option<f64>) -> Option<f64> {
    start.zip(end).filter(|(a, b)| b >= a).map(|(a, b)| b - a)
}

fn observation(attempt: &Path, now: f64) -> Result<Value> {
    let mut first: Option<f64> = None;
    let mut latest: Option<(f64, String)> = None;
    // Only these two directories belong to this attempt; never recurse into state or old attempts.
    for prefix in ["", "verified"] {
        let dir = attempt.join(prefix);
        let Some(m) = metadata(&dir)? else { continue };
        require(m.is_dir(), "progress_attempt_directory")?;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().into_string().map_err(|_| "progress_filename")?;
            if !name.ends_with(".started.json") {
                continue;
            }
            if let Some(time) = stamp(&entry.path())? {
                if prefix.is_empty() {
                    first = Some(first.map_or(time, |old| old.min(time)));
                }
                let name = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
                if latest.as_ref().is_none_or(|old| (time, &name) > (old.0, &old.1)) {
                    latest = Some((time, name));
                }
            }
        }
    }
    let load = stamp(&attempt.join("load.json"))?;
    let oracle = stamp(&attempt.join("verified/typed-oracle.json"))?;
    let receipt = stamp(&attempt.join("verified/full-receipt.json"))?;
    let (phase, start) = match (load, oracle, receipt) {
        (Some(_), Some(_), Some(t)) => ("awaiting_ledger", Some(t)),
        (Some(_), Some(t), None) => (PHASES[2], Some(t)),
        (Some(t), None, None) => (PHASES[1], Some(t)),
        (None, None, None) if first.is_some() => (PHASES[0], first),
        _ => ("unmeasured", None),
    };
    Ok(json!({"phase":phase,"phase_elapsed_seconds":span(start,Some(now)),
        "recent_started":latest.map(|(time,name)|json!({"filename":name,"mtime_unix_seconds":time})),
        "completed_phase_seconds":{
            PHASES[0]:span(first,load), PHASES[1]:span(load,oracle), PHASES[2]:span(oracle,receipt)}}))
}

fn validate_record(row: &Value, table: &Value) -> Result<&'static str> {
    let status = STATES.into_iter().skip(1).find(|s| row["status"] == *s).ok_or("progress_ledger_status")?;
    let attempt = number(row, "attempt")?;
    require(
        row["elapsed_ms"].as_u64().is_some() || (status == "running" && row.get("elapsed_ms") == Some(&Value::Null)),
        "progress_ledger_elapsed",
    )?;
    require(row["schema"] == table["schema"] && row["table"] == table["table"], "progress_ledger_binding")?;
    require(
        if ["pending", "excluded"].contains(&status) { attempt == 0 } else { attempt > 0 },
        "progress_ledger_attempt",
    )?;
    if status == "succeeded" {
        require(row["comparison"] == "equal", "progress_ledger_comparison")?;
        for field in ["rows", "chunks", "bytes"] {
            number(row, field)?;
        }
    } else {
        require(
            row["comparison"] == "not_run"
                && ["rows", "chunks", "bytes"].iter().all(|k| row.get(*k) == Some(&Value::Null)),
            "progress_ledger_metrics",
        )?;
    }
    if status == "failed" {
        require(row["failure"].as_str().is_some_and(|s| !s.is_empty()), "progress_ledger_failure")?;
    } else {
        require(row.get("failure") == Some(&Value::Null), "progress_ledger_failure")?;
    }
    if status == "excluded" {
        let exclusion = &row["exclusion"];
        require(
            exclusion == &table["exclusion"]
                && ["empty_without_key", "unmappable_type", "over_budget"].contains(&text(exclusion, "reason")?)
                && !text(exclusion, "upgrade_condition")?.trim().is_empty(),
            "progress_ledger_exclusion",
        )?;
    }
    Ok(status)
}

fn counters() -> Value {
    json!({"planned_tables":0,"planned_rows":0,"completed_tables":0,"completed_rows":0,
        "completed_elapsed_ms":0,
        "states":{"not_started":0,"pending":0,"running":0,"succeeded":0,"failed":0,"excluded":0}})
}

pub fn progress(plan_path: &Path) -> crate::io::Result<serde_json::Value> {
    let plan = required(plan_path)?;
    require(plan["protocol"] == "oo.library.refresh-profiles-result/v1", "progress_plan_protocol")?;
    let scope = names(&plan, "scope")?;
    let empty = names(&plan, "empty_verified_nochange")?;
    require(
        !scope.is_empty()
            && empty.is_subset(&scope)
            && text(&plan, "pack_digest")?.len() == 64
            && plan["schema_scope_digest"] == sha(&bytes(&json!(scope))?),
        "progress_scope",
    )?;
    let mut covered = empty.clone();
    let mut sources = BTreeSet::new();
    let mut profiles = Vec::new();
    let mut totals = counters();
    let now = seconds(SystemTime::now())?;
    for profile in array(&plan, "capture_profiles")? {
        let source = text(profile, "key")?;
        require(
            !source.is_empty()
                && !source.contains("..")
                && source.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                && sources.insert(source),
            "progress_source",
        )?;
        let run_path = Path::new(text(profile, "profile")?);
        let run = required(run_path)?;
        let root = Path::new(text(&run, "root")?);
        require(
            run_path == root.join("run.json")
                && run["pack_digest"] == plan["pack_digest"]
                && run["schema_scope_digest"] == plan["schema_scope_digest"],
            "progress_plan_binding",
        )?;
        let operator = required(&root.join("operator.json"))?;
        let manifest = required(&root.join("manifest.json"))?;
        require(
            operator["onboarding"]["full"]["source_id"] == source
                && operator["onboarding"]["full"]["batch_manifest"] == json!(root.join("manifest.json"))
                && manifest["config_sha256"] == sha(&bytes(&operator)?)
                && number(&manifest, "generation")? > 0,
            "progress_profile_binding",
        )?;
        let tables = array(&manifest, "tables")?;
        require(!tables.is_empty() && number(profile, "tables")? == tables.len() as u64, "progress_table_count")?;
        let mut expected = BTreeMap::new();
        for table in tables {
            let table_key = key(&manifest, table)?;
            require(
                table["schema"] == operator["database"]
                    && covered.insert(format!("{}.{}", text(table, "schema")?, text(table, "table")?)),
                "progress_table_coverage",
            )?;
            expected.insert(table_key, table);
        }
        let ledger = read(&root.join("run/ledger.json"))?;
        let records = if let Some(ledger) = &ledger {
            require(ledger["protocol"] == "oo.mysql.full-ledger/v1", "progress_ledger_protocol")?;
            let records = ledger["records"].as_object().ok_or("progress_ledger_records")?;
            require(records.keys().all(|k| expected.contains_key(k)), "progress_ledger_coverage")?;
            Some(records)
        } else {
            None
        };
        let mut counts = counters();
        counts["planned_tables"] = json!(tables.len());
        counts["planned_rows"] = json!(number(profile, "source_rows")?);
        let mut active = Vec::new();
        let mut timings = json!({});
        for phase in PHASES {
            timings[phase] = json!({"samples":0,"seconds":0.0});
        }
        for (key, table) in expected {
            let row = records.and_then(|r| r.get(&key));
            let status = row.map(|r| validate_record(r, table)).transpose()?.unwrap_or("not_started");
            add(&mut counts["states"], status, 1)?;
            let Some(row) = row else { continue };
            if status == "succeeded" {
                add(&mut counts, "completed_tables", 1)?;
                add(&mut counts, "completed_rows", number(row, "rows")?)?;
                add(&mut counts, "completed_elapsed_ms", number(row, "elapsed_ms")?)?;
            }
            if ["running", "succeeded"].contains(&status) {
                let attempt = number(row, "attempt")?;
                let dir = root.join("run").join(sha(key.as_bytes())).join(format!("attempt-{attempt}"));
                let mut observed = observation(&dir, now)?;
                for phase in PHASES {
                    if let Some(seconds) = observed["completed_phase_seconds"][phase].as_f64() {
                        add(&mut timings[phase], "samples", 1)?;
                        timings[phase]["seconds"] = json!(timings[phase]["seconds"].as_f64().unwrap() + seconds);
                    }
                }
                if status == "running" {
                    observed["schema"] = table["schema"].clone();
                    observed["table"] = table["table"].clone();
                    observed["attempt"] = json!(attempt);
                    active.push(observed);
                }
            }
        }
        for field in COUNTS {
            add(&mut totals, field, number(&counts, field)?)?;
        }
        for state in STATES {
            add(&mut totals["states"], state, number(&counts["states"], state)?)?;
        }
        let elapsed = number(&counts, "completed_elapsed_ms")?;
        profiles.push(json!({"source":source,"counts":counts,"ledger_present":ledger.is_some(),
            "active_tables":active,"phase_timings":timings,
            "completed_attempt_rows_per_second":(elapsed > 0).then(|| number(&counts,"completed_rows").unwrap() as f64 * 1000.0 / elapsed as f64)}));
    }
    require(covered == scope, "progress_plan_coverage")?;
    Ok(json!({"protocol":"oo.library.refresh-progress/v1","actual_mode":"full_snapshot_refresh",
        "changed_block_incremental":false,"observed_at_unix_seconds":now,
        "scope_tables":scope.len(),"empty_retained_tables":empty.len(),"counts":totals,"sources":profiles,
        "completion_basis":"ledger_succeeded_and_independently_equal",
        "timing_basis":"artifact_mtime_spans; completed_attempt_rate_is_not_wall_clock_throughput",
        "read_consistency":"per_source_atomic_ledger; artifacts_observed_separately",
        "process_liveness":"unmeasured","source_counts_rechecked":false,
        "eta_seconds":null,"eta_status":"unmeasured"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf, time::Duration};

    const SECRET: &str = "do-not-expose-password-or-business-values";
    struct Fixture {
        root: PathBuf,
        manifest: Value,
    }
    impl Fixture {
        fn new(n: usize) -> Self {
            let root = std::env::temp_dir().join(format!(
                "oo-refresh-progress-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let operator = json!({"database":"s","password":SECRET,
                "onboarding":{"full":{"source_id":"source","batch_manifest":root.join("manifest.json")}}});
            let manifest = json!({"generation":2,"config_sha256":sha(&bytes(&operator).unwrap()),
                "tables":(0..n).map(|i|json!({"schema":"s","table":format!("t{i}"),"columns":["id"]})).collect::<Vec<_>>()});
            let f = Self { root, manifest };
            let scope =
                std::iter::once("s.empty".to_owned()).chain((0..n).map(|i| format!("s.t{i}"))).collect::<BTreeSet<_>>();
            let scope_digest = sha(&bytes(&json!(scope)).unwrap());
            f.put("operator.json", &operator);
            f.put("manifest.json", &f.manifest);
            f.put(
                "run.json",
                &json!({"root":f.root,"pack_digest":"a".repeat(64),"schema_scope_digest":scope_digest,
                "binary":{"path":"/never-read-or-execute-this"}}),
            );
            f.put(
                "plan.json",
                &json!({"protocol":"oo.library.refresh-profiles-result/v1","scope":scope,
                "pack_digest":"a".repeat(64),"schema_scope_digest":scope_digest,"empty_verified_nochange":["s.empty"],
                "capture_profiles":[{"key":"source","profile":f.root.join("run.json"),"tables":n,"source_rows":1000}]}),
            );
            f
        }
        fn put(&self, name: &str, value: &Value) {
            let path = self.root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();
            let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).unwrap();
            file.write_all(&bytes(value).unwrap()).unwrap();
        }
        fn ledger(&self, rows: &[(usize, &str, u64)]) -> Value {
            let mut ledger = json!({"protocol":"oo.mysql.full-ledger/v1","records":{}});
            for &(i, status, attempt) in rows {
                let table = &self.manifest["tables"][i];
                let mut row = json!({"schema":"s","table":table["table"],"status":status,"attempt":attempt,
                    "rows":null,"chunks":null,"bytes":null,"elapsed_ms":0,"comparison":"not_run","failure":null});
                if status == "succeeded" {
                    row["rows"] = json!((i + 1) * 10);
                    row["chunks"] = json!(1);
                    row["bytes"] = json!(64);
                    row["elapsed_ms"] = json!(5000);
                    row["comparison"] = json!("equal");
                }
                if status == "failed" {
                    row["failure"] = json!(SECRET);
                }
                if status == "excluded" {
                    row["exclusion"] = table["exclusion"].clone();
                }
                ledger["records"][key(&self.manifest, table).unwrap()] = row;
            }
            ledger
        }
        fn artifact(&self, attempt: u64, name: &str, time: u64) {
            let path = format!(
                "run/{}/attempt-{attempt}/{name}",
                sha(key(&self.manifest, &self.manifest["tables"][0]).unwrap().as_bytes())
            );
            self.put(&path, &json!({"argv":[SECRET],"min":SECRET,"max":SECRET}));
            fs::File::open(self.root.join(path))
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(time)))
                .unwrap();
        }
        fn progress(&self) -> Value {
            progress(&self.root.join("plan.json")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn missing_ledger_and_missing_records_are_not_started_not_success() {
        let f = Fixture::new(2);
        let report = f.progress();
        assert_eq!(report["counts"]["states"]["not_started"], 2);
        assert_eq!(report["counts"]["completed_tables"], 0);
        assert_eq!(report["scope_tables"], 3);
        assert_eq!(report["empty_retained_tables"], 1);
        assert_eq!(report["counts"]["planned_rows"], 1000);
        assert_eq!(report["sources"][0]["ledger_present"], false);
        assert!(!f.root.join("run").exists());
        f.put("run/ledger.json", &f.ledger(&[(0, "succeeded", 1)]));
        let report = f.progress();
        assert_eq!(report["counts"]["states"]["not_started"], 1);
        assert_eq!(report["counts"]["completed_rows"], 10);
        assert_eq!(report["sources"][0]["phase_timings"][PHASES[0]]["samples"], 0);
        assert!(report["eta_seconds"].is_null());
        assert_eq!(report["actual_mode"], "full_snapshot_refresh");
        assert_eq!(report["changed_block_incremental"], false);
    }

    #[test]
    fn current_attempt_only_metadata_phases_and_measured_spans() {
        let f = Fixture::new(1);
        f.put("run/ledger.json", &f.ledger(&[(0, "running", 2)]));
        for name in
            ["0001-native-step.started.json", "load.json", "verified/typed-oracle.json", "verified/full-receipt.json"]
        {
            f.artifact(1, name, 900);
        }
        f.put("run/unrelated/attempt-2/load.json", &json!({}));
        assert_eq!(f.progress()["sources"][0]["active_tables"][0]["phase"], "unmeasured");
        for (name, time, phase) in [
            ("0001-native-step.started.json", 100, PHASES[0]),
            ("load.json", 110, PHASES[1]),
            ("verified/0001-source-sql.started.json", 115, PHASES[1]),
            ("verified/typed-oracle.json", 130, PHASES[2]),
            ("verified/full-receipt.json", 140, "awaiting_ledger"),
        ] {
            f.artifact(2, name, time);
            let report = f.progress();
            assert_eq!(report["sources"][0]["active_tables"][0]["phase"], phase);
            assert_eq!(report["counts"]["completed_tables"], 0);
            assert!(!report.to_string().contains(SECRET));
        }
        // These files need only metadata, not valid JSON; stdout must never be read.
        f.artifact(2, "0002-native-step.stdout", 120);
        let dir = f
            .root
            .join("run")
            .join(sha(key(&f.manifest, &f.manifest["tables"][0]).unwrap().as_bytes()))
            .join("attempt-2");
        fs::write(dir.join("verified/0001-source-sql.started.json"), SECRET).unwrap();
        fs::File::open(dir.join("verified/0001-source-sql.started.json"))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(115)))
            .unwrap();
        let report = f.progress();
        let active = &report["sources"][0]["active_tables"][0];
        assert_eq!(
            active["recent_started"],
            json!({"filename":"verified/0001-source-sql.started.json","mtime_unix_seconds":115.0})
        );
        assert_eq!(active["completed_phase_seconds"], json!({PHASES[0]:10.0,PHASES[1]:20.0,PHASES[2]:10.0}));
        f.put("run/ledger.json", &f.ledger(&[(0, "succeeded", 2)]));
        let report = f.progress();
        assert_eq!(report["sources"][0]["active_tables"], json!([]));
        assert_eq!(report["sources"][0]["phase_timings"][PHASES[1]], json!({"samples":1,"seconds":20.0}));
        assert_eq!(report["sources"][0]["completed_attempt_rows_per_second"], 2.0);
        assert!(span(Some(20.0), Some(10.0)).is_none());
        assert!(span(None, Some(10.0)).is_none());
    }

    #[test]
    fn all_states_are_counted_but_only_independently_equal_rows_complete() {
        let mut f = Fixture::new(6);
        f.manifest["tables"][4]["exclusion"] = json!({"reason":"over_budget","upgrade_condition":"raise limit"});
        f.put("manifest.json", &f.manifest);
        let mut ledger = f.ledger(&[
            (0, "pending", 0),
            (1, "running", 1),
            (2, "succeeded", 1),
            (3, "failed", 1),
            (4, "excluded", 0),
        ]);
        ledger["records"][key(&f.manifest, &f.manifest["tables"][1]).unwrap()]["elapsed_ms"] = Value::Null;
        f.put("run/ledger.json", &ledger);
        let report = f.progress();
        for state in STATES {
            assert_eq!(report["counts"]["states"][state], 1);
        }
        assert_eq!(report["counts"]["planned_tables"], 6);
        assert_eq!(report["counts"]["completed_tables"], 1);
        assert_eq!(report["counts"]["completed_rows"], 30);
        assert_eq!(report["sources"][0]["active_tables"][0]["table"], "t1");
        assert!(!report.to_string().contains(SECRET));
    }

    #[test]
    fn malformed_or_invalid_ledgers_fail_closed() {
        let f = Fixture::new(1);
        let valid = f.ledger(&[(0, "succeeded", 1)]);
        let key = key(&f.manifest, &f.manifest["tables"][0]).unwrap();
        let mut invalid = vec![
            json!(null),
            json!({"protocol":"wrong","records":{}}),
            json!({"protocol":"oo.mysql.full-ledger/v1","records":[]}),
            json!({"protocol":"oo.mysql.full-ledger/v1","records":{"wrong-key":valid["records"][&key]}}),
        ];
        for (field, value) in [
            ("status", json!("unknown")),
            ("attempt", json!(0)),
            ("comparison", json!("not_run")),
            ("rows", json!(-1)),
            ("chunks", json!(null)),
            ("bytes", json!("64")),
            ("elapsed_ms", json!(null)),
            ("schema", json!("other")),
            ("table", json!("other")),
            ("failure", json!(SECRET)),
        ] {
            let mut ledger = valid.clone();
            ledger["records"][&key][field] = value;
            invalid.push(ledger);
        }
        for ledger in invalid {
            f.put("run/ledger.json", &ledger);
            assert!(progress(&f.root.join("plan.json")).is_err());
        }
        fs::write(f.root.join("run/ledger.json"), b"{partial").unwrap();
        assert_eq!(progress(&f.root.join("plan.json")).unwrap_err().to_string(), "progress_invalid_json");
    }

    #[test]
    fn bindings_coverage_and_private_files_are_required() {
        let f = Fixture::new(1);
        for (file, field, value) in [
            ("run.json", "pack_digest", json!("b".repeat(64))),
            ("run.json", "root", json!("/different")),
            ("manifest.json", "config_sha256", json!("b".repeat(64))),
            ("manifest.json", "generation", json!(0)),
            ("manifest.json", "tables", json!([])),
            ("plan.json", "empty_verified_nochange", json!(["s.empty", "s.empty"])),
            ("plan.json", "capture_profiles", json!([])),
        ] {
            let original = required(&f.root.join(file)).unwrap();
            let mut changed = original.clone();
            changed[field] = value;
            f.put(file, &changed);
            assert!(progress(&f.root.join("plan.json")).is_err());
            f.put(file, &original);
        }
        let mut plan = required(&f.root.join("plan.json")).unwrap();
        plan["capture_profiles"][0]["tables"] = json!(2);
        f.put("plan.json", &plan);
        assert!(progress(&f.root.join("plan.json")).is_err());
        plan["capture_profiles"][0]["tables"] = json!(1);
        plan["capture_profiles"][0]["key"] = json!("wrong_source");
        f.put("plan.json", &plan);
        assert!(progress(&f.root.join("plan.json")).is_err());
        fs::set_permissions(f.root.join("plan.json"), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(progress(&f.root.join("plan.json")).unwrap_err().to_string(), "progress_private_file");
        std::os::unix::fs::symlink(f.root.join("plan.json"), f.root.join("link.json")).unwrap();
        assert_eq!(progress(&f.root.join("link.json")).unwrap_err().to_string(), "progress_symlink");
    }

    #[test]
    fn atomic_ledger_replacement_needs_no_lock_or_retry() {
        let f = Fixture::new(1);
        let old = f.ledger(&[(0, "pending", 0)]);
        let new = f.ledger(&[(0, "succeeded", 1)]);
        f.put("run/ledger.json", &old);
        f.put("run/ledger.lock", &json!({}));
        let lock = OpenOptions::new().read(true).write(true).open(f.root.join("run/ledger.lock")).unwrap();
        lock.try_lock().unwrap();
        f.put("run/ledger.next", &json!({}));
        fs::write(f.root.join("run/ledger.next"), b"{inflight").unwrap();
        assert_eq!(f.progress()["counts"]["states"]["pending"], 1);
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                for i in 0..100 {
                    f.put("run/ledger.next", if i % 2 == 0 { &new } else { &old });
                    fs::rename(f.root.join("run/ledger.next"), f.root.join("run/ledger.json")).unwrap();
                }
            });
            barrier.wait();
            for _ in 0..100 {
                let report = f.progress();
                let counts = &report["counts"];
                assert_eq!(
                    counts["states"]["pending"].as_u64().unwrap() + counts["states"]["succeeded"].as_u64().unwrap(),
                    1
                );
                assert_eq!(
                    counts["completed_rows"].as_u64().unwrap(),
                    counts["completed_tables"].as_u64().unwrap() * 10
                );
            }
        });
    }
}
