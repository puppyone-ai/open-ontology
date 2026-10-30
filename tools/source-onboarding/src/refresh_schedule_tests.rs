use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

const NOW: u64 = 1_800_000_000_000;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(script: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "oo-refresh-schedule-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let fixture = Self { root };
        fixture.put("fixture.sh", script.as_bytes());
        fixture.config(json!({
            "protocol":CONFIG,"state_dir":"./state","interval_seconds":20,
            "cycle_timeout_seconds":10,"max_attempts":3,"retry_delay_seconds":1,
            "command":{
                "argv":["/bin/sh","./fixture.sh","{cycle_dir}","fresh"],
                "resume_argv":["/bin/sh","./fixture.sh","{cycle_dir}","resume"]
            }
        }));
        fixture
    }

    fn success() -> Self {
        Self::new("printf '%s\\n' \"$2\" >> \"$1/modes\"\nprintf '{\"success\":true}\\n'\n")
    }

    fn put(&self, name: &str, data: &[u8]) {
        let path = self.root.join(name);
        if path.exists() {
            fs::write(path, data).unwrap();
        } else {
            open_new(&path).unwrap().write_all(data).unwrap();
        }
    }

    fn config(&self, value: Value) {
        persist(&self.root.join("config.json"), &value).unwrap();
    }

    fn edit(&self, f: impl FnOnce(&mut Value)) {
        let mut value = load(self.root.join("config.json")).unwrap();
        f(&mut value);
        self.config(value);
    }

    fn read(&self) -> Result<Config> {
        read_config(&self.root.join("config.json"))
    }

    fn run(&self, now: u64) -> Result<Value> {
        run_once(&self.read()?, &mut || Ok(now))
    }

    fn inspect(&self, now: u64, dry: bool) -> Value {
        inspect(&self.read().unwrap(), dry, now).unwrap()
    }

    fn cycle(&self, result: &Value) -> PathBuf {
        cycle_dir(&self.read().unwrap(), &result["state"]["cycle"]).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

mod interval {
    use super::*;
    use std::collections::BTreeMap;

    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_symlink() {
                    files.insert(
                        path.strip_prefix(root).unwrap().into(),
                        fs::read_link(&path)
                            .unwrap()
                            .to_string_lossy()
                            .as_bytes()
                            .to_vec(),
                    );
                } else if entry.file_type().unwrap().is_dir() {
                    walk(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().into(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(root, root, &mut files);
        files
    }

    fn change(f: &Fixture, seconds: u64, now: u64) -> Result<Value> {
        set_interval(&f.read()?, seconds, now)
    }

    fn stage(f: &Fixture, seconds: u64, now: u64) -> Value {
        let config = f.read().unwrap();
        let _lock = lock_schedule(&config).unwrap();
        let journal = interval_journal(&config, seconds, now).unwrap();
        persist(&config.root.join(INTERVAL_JOURNAL), &journal).unwrap();
        journal
    }

    #[test]
    fn idle_change_retains_history_cycle_anchor_and_future_cadence() {
        let f =
            Fixture::new("if [ ! -e ./allow ]; then exit 1; fi\nprintf '{\"success\":true}\\n'\n");
        f.run(NOW).unwrap();
        f.put("allow", b"local fixture");
        f.run(NOW + 1000).unwrap();
        let old = f.run(NOW + 20_000).unwrap()["state"].clone();
        let history = snapshot(&f.root.join("state/cycles"));
        let config = f.read().unwrap().value;
        let changed = change(&f, 15, NOW + 21_000).unwrap();
        assert_eq!(changed["action"], "interval_changed");
        assert_eq!(changed["state"]["next_due_at_ms"], NOW + 30_000);
        assert_eq!(changed["state"]["anchor_at_ms"], NOW);
        for key in ["sequence", "cycle", "last_success", "last_failure"] {
            assert_eq!(changed["state"][key], old[key], "{key}");
        }
        let mut expected = config;
        expected["interval_seconds"] = json!(15);
        assert_eq!(f.read().unwrap().value, expected);
        assert_eq!(history, snapshot(&f.root.join("state/cycles")));
        let changed = change(&f, 40, NOW + 22_000).unwrap();
        assert_eq!(changed["state"]["next_due_at_ms"], NOW + 40_000);
        let before = snapshot(&f.root);
        assert_eq!(
            change(&f, 40, NOW + 23_000).unwrap()["action"],
            "interval_unchanged"
        );
        assert_eq!(before, snapshot(&f.root));
        assert_eq!(f.run(NOW + 23_000).unwrap()["action"], "not_due");
    }

    #[test]
    fn future_explicit_anchor_and_uninitialized_schedule_do_not_run_work() {
        let f = Fixture::success();
        f.edit(|v| v["start_at_ms"] = json!(NOW + 100_000));
        let changed = change(&f, 30, NOW).unwrap();
        assert_eq!(changed["state"]["next_due_at_ms"], NOW + 100_000);
        assert_eq!(changed["state"]["anchor_at_ms"], NOW + 100_000);
        assert_eq!(changed["state"]["sequence"], 0);
        assert!(!f.root.join("state/cycles").exists());
        assert_eq!(f.read().unwrap().value["start_at_ms"], NOW + 100_000);
        let f = Fixture::success();
        let changed = change(&f, 30, NOW).unwrap();
        assert_eq!(changed["state"]["next_due_at_ms"], NOW + 30_000);
        assert_eq!(changed["state"]["anchor_at_ms"], NOW);
    }

    #[test]
    fn missed_slots_and_exact_tick_advance_once_without_catchup() {
        let f = Fixture::success();
        f.run(NOW).unwrap();
        let result = change(&f, 30, NOW + 120_000).unwrap();
        assert_eq!(result["state"]["next_due_at_ms"], NOW + 150_000);
        assert_eq!(result["state"]["sequence"], 1);
        assert_eq!(f.run(NOW + 120_001).unwrap()["action"], "not_due");
    }

    #[test]
    fn legacy_anchor_is_recovered_from_first_archive_not_latest_cycle() {
        let f = Fixture::success();
        f.run(NOW).unwrap();
        f.run(NOW + 20_000).unwrap();
        let path = f.root.join("state/state.json");
        let mut old = load(&path).unwrap();
        old.as_object_mut().unwrap().remove("anchor_at_ms");
        persist(&path, &old).unwrap();
        let history = snapshot(&f.root.join("state/cycles"));
        let changed = change(&f, 15, NOW + 21_000).unwrap();
        assert_eq!(changed["state"]["anchor_at_ms"], NOW);
        assert_eq!(changed["state"]["next_due_at_ms"], NOW + 30_000);
        assert_eq!(history, snapshot(&f.root.join("state/cycles")));
    }

    #[test]
    fn active_retry_and_reserved_pending_cycles_refuse_without_mutation() {
        for pending in [false, true] {
            let f = Fixture::new("exit 1\n");
            f.run(NOW).unwrap();
            if pending {
                let path = f.root.join("state/state.json");
                let mut state = load(&path).unwrap();
                state["cycle"]["status"] = json!("running");
                state["cycle"]["attempts"] = json!([]);
                persist(&path, &state).unwrap();
            }
            let before = snapshot(&f.root);
            assert!(
                change(&f, 30, NOW + 1000)
                    .unwrap_err()
                    .to_string()
                    .starts_with("schedule_interval_busy")
            );
            assert_eq!(before, snapshot(&f.root));
        }
    }

    #[test]
    fn expired_cycle_with_unresolved_attempt_or_live_child_is_busy() {
        for running in [true, false] {
            let f = Fixture::success();
            f.run(NOW).unwrap();
            let child = OwnedChild(
                Command::new("/bin/sleep")
                    .arg("5")
                    .process_group(0)
                    .spawn()
                    .unwrap(),
            );
            let path = f.root.join("state/state.json");
            let mut state = load(&path).unwrap();
            state["cycle"]["status"] = json!("expired");
            state["cycle"]["attempts"][0]["status"] =
                json!(if running { "running" } else { "failed" });
            state["cycle"]["attempts"][0]["child_pgid"] = json!(child.0.id());
            state["cycle"]["attempts"][0]["child_reaped"] = json!(false);
            persist(&path, &state).unwrap();
            let before = snapshot(&f.root);
            assert!(
                change(&f, 30, NOW + 1000)
                    .unwrap_err()
                    .to_string()
                    .starts_with("schedule_interval_busy")
            );
            assert_eq!(before, snapshot(&f.root));
            drop(child);
        }
    }

    #[test]
    fn same_os_lock_refuses_changes_and_journal_recovery() {
        let f = Fixture::success();
        f.run(NOW).unwrap();
        let config = f.read().unwrap();
        let lock = lock_schedule(&config).unwrap();
        let before = snapshot(&f.root);
        assert_eq!(
            change(&f, 30, NOW + 1000).unwrap_err().to_string(),
            "schedule_lock_busy"
        );
        assert_eq!(before, snapshot(&f.root));
        drop(lock);
        stage(&f, 30, NOW + 1000);
        let _lock = lock_schedule(&config).unwrap();
        let before = snapshot(&f.root);
        assert_eq!(
            inspect(&config, false, NOW + 2000).unwrap_err().to_string(),
            "schedule_lock_busy"
        );
        assert_eq!(before, snapshot(&f.root));
    }

    #[test]
    fn timeout_retry_and_range_limits_are_actionable_and_do_not_mutate() {
        for (seconds, expected) in [
            (0, "schedule_interval_seconds"),
            (9, "cycle_timeout_seconds"),
            (31_536_001, "schedule_interval_seconds"),
        ] {
            let f = Fixture::success();
            let before = snapshot(&f.root);
            assert!(
                change(&f, seconds, NOW)
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
            assert_eq!(before, snapshot(&f.root));
        }
        let f = Fixture::success();
        f.edit(|v| v["retry_delay_seconds"] = json!(12));
        let before = snapshot(&f.root);
        assert!(
            change(&f, 11, NOW)
                .unwrap_err()
                .to_string()
                .contains("retry_delay_seconds")
        );
        assert_eq!(before, snapshot(&f.root));
    }

    #[test]
    fn clock_rollback_is_refused_without_mutation() {
        let f = Fixture::success();
        f.run(NOW).unwrap();
        let before = snapshot(&f.root);
        assert_eq!(
            change(&f, 30, NOW - 1).unwrap_err().to_string(),
            "schedule_clock_rollback"
        );
        assert_eq!(before, snapshot(&f.root));
    }

    #[test]
    fn each_durable_crash_boundary_recovers_only_the_recorded_pair_without_jobs() {
        for phase in 0..3 {
            let f = Fixture::success();
            let first = f.run(NOW).unwrap();
            let journal = stage(&f, 30, NOW + 1000);
            // Simulate death before config rename, between the two renames,
            // or after state rename but before durable journal removal.
            if phase >= 1 {
                persist(&f.root.join("config.json"), &journal["new_config"]).unwrap();
            }
            if phase >= 2 {
                persist(&f.root.join("state/state.json"), &journal["new_state"]).unwrap();
            }
            let history = snapshot(&f.root.join("state/cycles"));
            let result = if phase == 0 {
                f.run(NOW + 90_000).unwrap()
            } else {
                f.inspect(NOW + 90_000, phase == 2)
            };
            assert_eq!(result["action"], "interval_recovered");
            assert_eq!(result["state"], journal["new_state"]);
            assert_eq!(result["state"]["cycle"], first["state"]["cycle"]);
            assert_eq!(history, snapshot(&f.root.join("state/cycles")));
            assert_eq!(f.read().unwrap().value, journal["new_config"]);
            assert!(!f.root.join("state").join(INTERVAL_JOURNAL).exists());
            let before = snapshot(&f.root);
            assert_eq!(f.inspect(NOW + 90_000, false)["action"], "status");
            assert_eq!(before, snapshot(&f.root));
        }
    }

    #[test]
    fn initial_state_crash_and_repeated_change_recover_before_new_request() {
        let f = Fixture::success();
        let journal = stage(&f, 30, NOW);
        persist(&f.root.join("config.json"), &journal["new_config"]).unwrap();
        let recovered = change(&f, 40, NOW + 1000).unwrap();
        assert_eq!(recovered["action"], "interval_recovered");
        assert_eq!(f.read().unwrap().value["interval_seconds"], 30);
        assert_eq!(recovered["state"]["sequence"], 0);
        assert!(!f.root.join("state/cycles").exists());
        assert_eq!(
            change(&f, 40, NOW + 1000).unwrap()["action"],
            "interval_changed"
        );
    }

    #[test]
    fn ordinary_old_or_new_config_hash_drift_is_never_adopted() {
        for edited in [true, false] {
            let f = Fixture::success();
            f.run(NOW).unwrap();
            if edited {
                f.edit(|v| v["interval_seconds"] = json!(30));
            } else {
                let path = f.root.join("state/state.json");
                let mut state = load(&path).unwrap();
                state["config_sha256"] = json!("0".repeat(64));
                persist(&path, &state).unwrap();
            }
            let before = snapshot(&f.root);
            assert_eq!(
                change(&f, 40, NOW + 1000).unwrap_err().to_string(),
                "schedule_config_drift"
            );
            assert_eq!(before, snapshot(&f.root));
        }
    }

    #[test]
    fn journal_hash_scope_and_state_drift_refuse_without_rewriting_anything() {
        for corruption in [
            "old_hash",
            "new_hash",
            "command",
            "state",
            "third_config",
            "reverse_pair",
        ] {
            let f = Fixture::success();
            f.run(NOW).unwrap();
            let mut journal = stage(&f, 30, NOW + 1000);
            match corruption {
                "old_hash" => journal["old_config_sha256"] = json!("0".repeat(64)),
                "new_hash" => journal["new_config_sha256"] = json!("0".repeat(64)),
                "command" => journal["new_config"]["command"]["argv"][1] = json!("./different.sh"),
                "state" => {
                    let path = f.root.join("state/state.json");
                    let mut state = load(&path).unwrap();
                    state["last_failure"] = json!({"error":"external drift"});
                    persist(&path, &state).unwrap();
                }
                "third_config" => f.edit(|v| v["interval_seconds"] = json!(40)),
                "reverse_pair" => {
                    persist(&f.root.join("state/state.json"), &journal["new_state"]).unwrap()
                }
                _ => unreachable!(),
            }
            persist(&f.root.join("state").join(INTERVAL_JOURNAL), &journal).unwrap();
            let before = snapshot(&f.root);
            assert!(
                inspect(&f.read().unwrap(), false, NOW + 1000).is_err(),
                "{corruption}"
            );
            assert_eq!(before, snapshot(&f.root), "{corruption}");
        }
    }

    #[test]
    fn stale_config_loaded_before_lock_is_reloaded_after_authorized_change() {
        let f = Fixture::success();
        f.run(NOW).unwrap();
        let old = f.read().unwrap();
        change(&f, 30, NOW + 1000).unwrap();
        let result = run_once(&old, &mut || Ok(NOW + 2000)).unwrap();
        assert_eq!(result["action"], "not_due");
        assert_eq!(result["state"]["config_sha256"], f.read().unwrap().hash);
    }

    #[test]
    fn interval_private_symlink_guards_and_help_remain_enforced() {
        for leaf in ["lock", INTERVAL_JOURNAL, "state.json"] {
            let f = Fixture::success();
            f.run(NOW).unwrap();
            let path = f.root.join("state").join(leaf);
            if path.exists() {
                fs::rename(&path, path.with_extension("fixture-backup")).unwrap();
            }
            symlink(f.root.join("UNOWNED"), &path).unwrap();
            let before = snapshot(&f.root);
            assert!(change(&f, 30, NOW + 1000).is_err(), "{leaf}");
            assert_eq!(before, snapshot(&f.root));
            assert!(!f.root.join("UNOWNED").exists());
        }
        for token in [
            "set-interval --config PRIVATE_SCHEDULE --seconds N",
            "idle",
            "interval-change.pending.json",
            "interval_recovered",
            "strictly after",
        ] {
            assert!(HELP.contains(token), "{token}");
        }
    }

    #[test]
    fn nonprivate_pending_journal_is_refused_without_recovery() {
        let f = Fixture::success();
        f.run(NOW).unwrap();
        stage(&f, 30, NOW + 1000);
        let path = f.root.join("state").join(INTERVAL_JOURNAL);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let before = snapshot(&f.root);
        assert!(inspect(&f.read().unwrap(), false, NOW + 2000).is_err());
        assert_eq!(before, snapshot(&f.root));
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o644);
    }
}

#[test]
fn inspection_is_read_only_and_help_exposes_the_contract() {
    let f = Fixture::success();
    let status = f.inspect(NOW, false);
    assert_eq!(status["action"], "status");
    assert_eq!(status["due"], true);
    let plan = f.inspect(NOW, true);
    assert_eq!(plan["plan"]["resume"], false);
    assert_eq!(plan["plan"]["will_execute_now"], true);
    assert_eq!(plan["plan"]["cycle"]["deadline_at_ms"], NOW + 10_000);
    assert!(!f.root.join("state").exists());
    for token in [
        "run-once",
        "status",
        "dry-run",
        "28800",
        "launchd",
        "systemd",
        "max_attempts",
        "resume_argv",
        "deadline_at_ms",
        "foreground",
    ] {
        assert!(HELP.contains(token), "{token}");
    }
}

#[test]
fn explicit_first_due_anchor_does_not_repeat_a_controlled_customer_cycle() {
    let f = Fixture::success();
    let first_due = NOW + 28_800_000;
    f.edit(|v| {
        v["interval_seconds"] = json!(28800);
        v["start_at_ms"] = json!(first_due);
    });
    let plan = f.inspect(NOW, true);
    assert_eq!(plan["plan"]["will_execute_now"], false);
    assert_eq!(plan["state"]["next_due_at_ms"], first_due);
    assert_eq!(f.run(NOW).unwrap()["action"], "not_due");
    assert!(!f.root.join("state/cycles").exists());
    assert_eq!(f.run(first_due - 1).unwrap()["action"], "not_due");
    let started = f.run(first_due).unwrap();
    assert_eq!(started["action"], "succeeded");
    assert_eq!(started["state"]["cycle"]["started_at_ms"], first_due);
    assert_eq!(started["state"]["next_due_at_ms"], first_due + 28_800_000);
}

#[test]
fn not_due_skips_and_new_cycle_never_resumes_yesterdays_success() {
    let f = Fixture::success();
    let first = f.run(NOW).unwrap();
    assert_eq!(first["action"], "succeeded");
    let directory = f.cycle(&first);
    assert_eq!(
        fs::read_to_string(directory.join("modes")).unwrap(),
        "fresh\n"
    );
    let skipped = f.run(NOW + 19_999).unwrap();
    assert_eq!(skipped["action"], "not_due");
    assert_eq!(skipped["state"]["cycle"], first["state"]["cycle"]);
    assert_eq!(
        f.inspect(NOW + 19_999, true)["plan"]["will_execute_now"],
        false
    );
    let next = f.run(NOW + 20_000).unwrap();
    assert_eq!(next["action"], "succeeded");
    assert_ne!(next["state"]["cycle"]["id"], first["state"]["cycle"]["id"]);
    assert_eq!(next["state"]["cycle"]["attempts"][0]["resume"], false);
    assert_eq!(
        fs::read_to_string(f.cycle(&next).join("modes")).unwrap(),
        "fresh\n"
    );
    assert_eq!(
        load(directory.join("cycle.json")).unwrap(),
        first["state"]["cycle"]
    );
    assert_eq!(next["state"]["next_due_at_ms"], NOW + 40_000);
}

#[test]
fn same_failed_cycle_resumes_with_original_output_and_deadline() {
    let f = Fixture::new(
        "printf '%s\\n' \"$2\" >> \"$1/modes\"\n\
         if [ ! -e ./allow ]; then printf '{\"success\":false}\\n'; exit 0; fi\n\
         printf '{\"success\":true}\\n'\n",
    );
    let first = f.run(NOW).unwrap();
    assert_eq!(first["action"], "retry_wait");
    assert_eq!(first["ok"], false);
    assert_eq!(f.run(NOW + 999).unwrap()["action"], "retry_not_due");
    f.put("allow", b"fixture only");
    let resumed = f.run(NOW + 1000).unwrap();
    assert_eq!(resumed["action"], "succeeded");
    assert_eq!(
        resumed["state"]["cycle"]["id"],
        first["state"]["cycle"]["id"]
    );
    assert_eq!(
        resumed["state"]["cycle"]["deadline_at_ms"],
        first["state"]["cycle"]["deadline_at_ms"]
    );
    assert_eq!(resumed["state"]["cycle"]["attempts"][1]["resume"], true);
    assert_eq!(
        fs::read_to_string(f.cycle(&resumed).join("modes")).unwrap(),
        "fresh\nresume\n"
    );
    assert!(!resumed["state"]["last_failure"].is_null());
    assert_eq!(
        resumed["state"]["last_success"]["cycle_id"],
        resumed["state"]["cycle"]["id"]
    );
}

#[test]
fn retry_exhaustion_waits_for_new_interval_instead_of_resetting_budget() {
    let f = Fixture::new("exit 7\n");
    f.edit(|v| v["max_attempts"] = json!(2));
    let failed = f.run(NOW).unwrap();
    assert_eq!(failed["action"], "retry_wait");
    let exhausted = f.run(NOW + 1000).unwrap();
    assert_eq!(exhausted["action"], "failed");
    assert_eq!(
        exhausted["state"]["cycle"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(f.run(NOW + 2000).unwrap()["action"], "not_due");
    let next = f.run(NOW + 20_000).unwrap();
    assert_ne!(next["state"]["cycle"]["id"], failed["state"]["cycle"]["id"]);
    assert_eq!(next["state"]["cycle"]["attempts"][0]["resume"], false);
    assert_eq!(next["state"]["last_success"], Value::Null);
}

#[test]
fn downtime_counts_toward_deadline_and_missed_slots_do_not_burst() {
    let f = Fixture::new("exit 1\n");
    let first = f.run(NOW).unwrap();
    let expired = f.run(NOW + 10_000).unwrap();
    assert_eq!(expired["action"], "expired");
    assert_eq!(
        expired["state"]["cycle"]["deadline_at_ms"],
        first["state"]["cycle"]["deadline_at_ms"]
    );
    assert_eq!(
        expired["state"]["cycle"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.run(NOW + 11_000).unwrap()["action"], "not_due");
    f.put("fixture.sh", b"printf '{\"success\":true}\\n'\n");
    let much_later = f.run(NOW + 105_000).unwrap();
    assert_eq!(much_later["action"], "succeeded");
    assert_eq!(much_later["state"]["next_due_at_ms"], NOW + 120_000);
    assert_eq!(f.run(NOW + 105_000).unwrap()["action"], "not_due");
}

#[test]
fn backward_clock_is_rejected_without_restarting_or_extending_deadline() {
    let f = Fixture::new("exit 1\n");
    let first = f.run(NOW).unwrap();
    assert_eq!(
        f.run(NOW - 1).unwrap_err().to_string(),
        "schedule_clock_rollback"
    );
    let status = f.inspect(NOW - 1, true);
    assert_eq!(status["clock_rollback"], true);
    assert_eq!(status["plan"]["will_execute_now"], false);
    assert_eq!(
        status["state"]["cycle"]["deadline_at_ms"],
        first["state"]["cycle"]["deadline_at_ms"]
    );
}

#[test]
fn a_frozen_wall_clock_cannot_extend_the_monotonic_budget() {
    let f = Fixture::new("sleep 3\nprintf '{\"success\":true}\\n'\n");
    f.edit(|v| v["cycle_timeout_seconds"] = json!(1));
    let start = Instant::now();
    let result = f.run(NOW).unwrap();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(result["action"], "expired");
    assert_eq!(
        result["state"]["cycle"]["reason"],
        "schedule_deadline_exceeded"
    );
    assert_eq!(result["state"]["last_success"], Value::Null);
}

#[test]
fn forward_clock_jump_before_spawn_gate_cannot_start_late_work() {
    let f = Fixture::new("touch ./UNEXPECTED_WORK\nprintf '{\"success\":true}\\n'\n");
    let config = f.read().unwrap();
    let mut observations = 0;
    let result = run_once(&config, &mut || {
        observations += 1;
        Ok(if observations >= 3 { NOW + 10_000 } else { NOW })
    })
    .unwrap();
    assert_eq!(result["action"], "expired");
    assert!(!f.root.join("UNEXPECTED_WORK").exists());
    assert_eq!(result["state"]["last_success"], Value::Null);
}

#[test]
fn rollback_during_execution_is_not_success_and_does_not_extend_budget() {
    let f = Fixture::new("sleep 1\nprintf '{\"success\":true}\\n'\n");
    let config = f.read().unwrap();
    let mut observations = 0;
    let result = run_once(&config, &mut || {
        observations += 1;
        Ok(if observations >= 4 { NOW - 1000 } else { NOW })
    })
    .unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(result["state"]["last_success"], Value::Null);
    assert_eq!(
        result["state"]["last_failure"]["reason"],
        "schedule_clock_rollback"
    );
    assert_eq!(result["state"]["observed_at_ms"], NOW);
    assert_eq!(result["state"]["cycle"]["deadline_at_ms"], NOW + 10_000);
}

#[test]
fn lock_conflict_does_not_launch_or_mutate_state() {
    let f = Fixture::success();
    let config = f.read().unwrap();
    private_dir(&config.root, config.uid, true).unwrap();
    let lock = open_new(&config.root.join("lock")).unwrap();
    lock.try_lock().unwrap();
    let lock = ScheduleLock(lock);
    assert_eq!(f.run(NOW).unwrap_err().to_string(), "schedule_lock_busy");
    assert_eq!(f.inspect(NOW, false)["locked"], true);
    assert!(!config.root.join("state.json").exists());
    drop(lock);
    assert_eq!(f.run(NOW).unwrap()["action"], "succeeded");
}

#[test]
fn completed_invocation_unlocks_even_with_an_inherited_descriptor() {
    let f = Fixture::success();
    let config = f.read().unwrap();
    private_dir(&config.root, config.uid, true).unwrap();
    let file = open_new(&config.root.join("lock")).unwrap();
    file.try_lock().unwrap();
    let inherited = file.try_clone().unwrap();
    drop(ScheduleLock(file));
    assert_eq!(f.run(NOW).unwrap()["action"], "succeeded");
    drop(inherited);
}

#[test]
fn nonzero_and_structured_failures_never_publish_a_scheduler_success() {
    for script in [
        "printf '{\"success\":true}\\n'; exit 4\n",
        "printf '{\"success\":false}\\n'\n",
        "printf '{\"success\":true,\"error\":\"rejected\"}\\n'\n",
        "printf '{\"success\":true,\"nested\":{\"errors\":[\"bad\"]}}\\n'\n",
        "printf '{\"success\":true,\"failed\":[\"capture\"]}\\n'\n",
        "printf '{\"success\":true,\"nested\":{\"ok\":false}}\\n'\n",
        "printf '{\"success\":true,\"nested\":{\"status\":\"failed\"}}\\n'\n",
        "printf '{\"success\":true,\"nested\":{\"disable_error\":\"bad\"}}\\n'\n",
        "printf '{\"success\":true}\\n'; printf '{\"error\":\"stderr refusal\"}\\n' >&2\n",
        "printf 'not json\\n'\n",
        "printf '{\"success\":true}\\n{\"error\":\"bad\"}\\n'\n",
    ] {
        let f = Fixture::new(script);
        f.edit(|v| v["max_attempts"] = json!(1));
        let result = f.run(NOW).unwrap();
        assert_eq!(result["ok"], false, "{script}");
        assert_eq!(result["action"], "failed", "{script}");
        assert_eq!(result["state"]["last_success"], Value::Null, "{script}");
    }
}

#[test]
fn excessive_child_logs_stop_the_attempt_before_success() {
    for script in [
        "dd if=/dev/zero bs=1048576 count=5 2>/dev/null\nsleep 1\n",
        "dd if=/dev/zero bs=1048576 count=17 >&2 2>/dev/null\nsleep 1\n",
    ] {
        let f = Fixture::new(script);
        f.edit(|v| v["max_attempts"] = json!(1));
        let result = f.run(NOW).unwrap();
        assert_eq!(result["action"], "failed");
        assert_eq!(
            result["state"]["last_failure"]["reason"],
            "schedule_log_budget"
        );
        assert_eq!(result["state"]["last_success"], Value::Null);
    }
}

#[test]
fn native_resume_receipts_allow_failed_history_but_not_current_failures() {
    let f = Fixture::success();
    let file = f.root.join("output.json");
    let mut value = json!({"success":true,"protocol":"oo.library.incremental-refresh/v1",
        "records":{"0":{"attempts":[{"status":"failed","error":"old"},
            {"status":"succeeded","disabled":true}]}}});
    persist(&file, &value).unwrap();
    assert!(success_output(&file).is_ok());
    value["records"]["0"]["attempts"][1]["error"] = json!("new failure");
    persist(&file, &value).unwrap();
    assert!(success_output(&file).is_err());
    value["records"]["0"]["attempts"][1]
        .as_object_mut()
        .unwrap()
        .remove("error");
    value["protocol"] = json!("unrecognized-wrapper");
    persist(&file, &value).unwrap();
    assert!(success_output(&file).is_err());
}

#[test]
fn seven_stage_receipt_cannot_claim_success_before_final_probe() {
    let f = Fixture::success();
    let file = f.root.join("output.json");
    let names = [
        "prepare",
        "capture",
        "verify",
        "publish",
        "probe",
        "activate",
        "final_probe",
    ];
    let mut value = json!({"success":true,"protocol":"oo.library.refresh-result/v1",
        "stages":names.iter().map(|name| ((*name).to_owned(),json!({
            "status":"succeeded", "attempts":[
                {"status":"failed","failure":"historical_failure","exit_code":1},
                {"status":"succeeded","exit_code":0}
            ]
        }))).collect::<serde_json::Map<_,_>>()});
    persist(&file, &value).unwrap();
    assert!(success_output(&file).is_ok());
    value["stages"]["final_probe"]["attempts"][1]["exit_code"] = json!(1);
    persist(&file, &value).unwrap();
    assert!(success_output(&file).is_err());
    value["stages"]
        .as_object_mut()
        .unwrap()
        .remove("final_probe");
    persist(&file, &value).unwrap();
    assert!(success_output(&file).is_err());
}

#[test]
fn literal_argv_environment_and_relative_paths_survive_relocation() {
    let mut f = Fixture::new(
        "test \"$1\" = \"$OO_REFRESH_CYCLE_DIR\" || exit 9\n\
         test \"$OO_REFRESH_RESUME\" = 0 || exit 9\n\
         printf '%s' \"$3\" > \"$1/argument\"\n\
         printf '{\"success\":true}\\n'\n",
    );
    f.edit(|v| {
        v["command"]["argv"]
            .as_array_mut()
            .unwrap()
            .push(json!("$(touch NOT_EXECUTED); *"));
    });
    let first = f.run(NOW).unwrap();
    assert_eq!(first["action"], "succeeded");
    assert_eq!(
        fs::read_to_string(f.cycle(&first).join("argument")).unwrap(),
        "$(touch NOT_EXECUTED); *"
    );
    assert!(!f.root.join("NOT_EXECUTED").exists());
    let new_root = f.root.with_extension("moved");
    fs::rename(&f.root, &new_root).unwrap();
    f.root = new_root;
    assert_eq!(f.run(NOW + 20_000).unwrap()["action"], "succeeded");
}

#[test]
fn closed_private_configuration_and_drift_fail_before_commands() {
    for edit in [
        ("/unknown", json!(1)),
        ("/interval_seconds", json!(0)),
        ("/cycle_timeout_seconds", json!(21)),
        ("/max_attempts", json!(0)),
        ("/retry_delay_seconds", json!(0)),
        (
            "/command/argv",
            json!(["/bin/echo", "--password", "bad", "{cycle_dir}"]),
        ),
        (
            "/command/resume_argv",
            json!(["/bin/echo", "no cycle path"]),
        ),
    ] {
        let f = Fixture::success();
        f.edit(|v| {
            if edit.0 == "/unknown" {
                v["unknown"] = edit.1.clone();
            } else {
                *v.pointer_mut(edit.0).unwrap() = edit.1.clone();
            }
        });
        assert!(f.read().is_err(), "{}", edit.0);
        assert!(!f.root.join("state").exists());
    }
    let f = Fixture::success();
    f.run(NOW).unwrap();
    f.edit(|v| v["interval_seconds"] = json!(30));
    assert_eq!(
        f.run(NOW + 20_000).unwrap_err().to_string(),
        "schedule_config_drift"
    );
    fs::set_permissions(
        f.root.join("config.json"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(f.read().is_err());
}

#[test]
fn unsafe_paths_and_lost_state_fail_closed() {
    let f = Fixture::success();
    fs::create_dir(f.root.join(".git")).unwrap();
    assert_eq!(f.read().err().unwrap().to_string(), "output_in_repository");
    fs::remove_dir(f.root.join(".git")).unwrap();
    let elsewhere = f.root.join("actual");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, f.root.join("state")).unwrap();
    assert_eq!(
        f.read().err().unwrap().to_string(),
        "schedule_state_symlink"
    );
    fs::remove_file(f.root.join("state")).unwrap();
    f.run(NOW).unwrap();
    fs::remove_file(f.root.join("state/state.json")).unwrap();
    assert_eq!(
        f.run(NOW + 20_000).unwrap_err().to_string(),
        "schedule_state_missing"
    );
}

#[test]
fn dangling_lock_symlink_cannot_create_an_unowned_file() {
    let f = Fixture::success();
    let config = f.read().unwrap();
    private_dir(&config.root, config.uid, true).unwrap();
    let target = f.root.join("NOT_A_LOCK");
    symlink(&target, config.root.join("lock")).unwrap();
    assert!(f.run(NOW).is_err());
    assert!(!target.exists());
    assert!(inspect(&config, false, NOW).is_err());
}

#[test]
fn templates_are_read_only_in_preview_and_immutable_within_a_cycle() {
    let f = Fixture::new("exit 1\n");
    let original = json!({"path":"{cycle_dir}/capture","nested":["{deadline_at_ms}",7]});
    f.put("template.json", &bytes(&original).unwrap());
    f.edit(|v| v["templates"] = json!({"refresh.json":"./template.json"}));
    let preview = f.inspect(NOW, true);
    assert_eq!(preview["plan"]["templates"], json!(["refresh.json"]));
    assert!(!f.root.join("state").exists());
    let first = f.run(NOW).unwrap();
    let directory = f.cycle(&first);
    let rendered = load(directory.join("refresh.json")).unwrap();
    assert_eq!(rendered["path"], json!(directory.join("capture")));
    assert_eq!(rendered["nested"], json!([(NOW + 10_000).to_string(), 7]));
    f.put(
        "template.json",
        br#"{"path":"new template for NEXT cycle"}"#,
    );
    let resumed = f.run(NOW + 1000).unwrap();
    assert_eq!(
        resumed["state"]["cycle"]["id"],
        first["state"]["cycle"]["id"]
    );
    assert_eq!(load(directory.join("refresh.json")).unwrap(), rendered);
    persist(&directory.join("refresh.json"), &json!({"tampered":true})).unwrap();
    let refused = f.run(NOW + 2000).unwrap();
    assert_eq!(refused["action"], "failed");
    assert_eq!(
        refused["state"]["last_failure"]["reason"],
        "schedule_template_output_drift"
    );
    assert_eq!(
        f.inspect(NOW + 2000, false)["state"]["cycle"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn preview_rejects_missing_or_nonprivate_templates_without_creating_state() {
    let f = Fixture::success();
    f.edit(|v| v["templates"] = json!({"refresh.json":"./missing.json"}));
    assert!(inspect(&f.read().unwrap(), true, NOW).is_err());
    f.put("missing.json", b"{}");
    fs::set_permissions(
        f.root.join("missing.json"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(inspect(&f.read().unwrap(), true, NOW).is_err());
    assert!(!f.root.join("state").exists());
}

#[test]
fn template_setup_failure_is_durable_and_uses_the_finite_attempt_budget() {
    let f = Fixture::success();
    f.edit(|v| {
        v["templates"] = json!({"refresh.json":"./missing.json"});
        v["max_attempts"] = json!(1);
    });
    let failed = f.run(NOW).unwrap();
    assert_eq!(failed["action"], "failed");
    assert_eq!(
        failed["state"]["cycle"]["attempts"][0]["child_pgid"],
        Value::Null
    );
    assert_eq!(failed["state"]["last_success"], Value::Null);
    assert!(!failed["state"]["last_failure"]["reason"].is_null());
    assert_eq!(f.run(NOW + 1000).unwrap()["action"], "not_due");
}

#[test]
fn recovered_reservation_uses_a_new_attempt_without_losing_cycle_budget() {
    let f = Fixture::success();
    let config = f.read().unwrap();
    private_dir(&config.root, config.uid, true).unwrap();
    let mut state = initial(&config, NOW);
    state["cycle"] = new_cycle(&config, &state, NOW).unwrap();
    state["sequence"] = json!(1);
    state["cycle"]["attempts"] = json!([{
        "status":"running","started_at_ms":NOW,"resume":false,
        "directory":"attempt-0001","child_pgid":null,"exit_code":null
    }]);
    save_state(&config, &state).unwrap();
    let result = f.run(NOW + 1000).unwrap();
    assert_eq!(result["action"], "succeeded");
    assert_eq!(result["state"]["cycle"]["deadline_at_ms"], NOW + 10_000);
    assert_eq!(
        result["state"]["cycle"]["attempts"][0]["status"],
        "interrupted"
    );
    assert_eq!(result["state"]["cycle"]["attempts"][1]["resume"], true);
}
