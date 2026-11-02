//! Source-serialized, incremental-only refresh through the pinned worker.
use crate::io::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PROTOCOL: &str = "oo.library.incremental-refresh/v1";
const BUDGET_PROTOCOL: &str = "oo.library.incremental-budget/v1";

#[derive(Clone, Copy)]
struct CycleBudget {
    started_at_ms: u64,
    deadline_at_ms: u64,
}

impl CycleBudget {
    fn remaining(self, now: u64, ledger: &Value) -> Result<Duration> {
        let mut latest = ledger["budget_observed_at_ms"].as_u64().unwrap_or(self.started_at_ms);
        if let Some(records) = ledger["records"].as_object() {
            for record in records.values() {
                for attempt in array(record, "attempts")? {
                    for field in ["started_at_ms", "finished_at_ms"] {
                        if let Some(time) = attempt[field].as_u64() { latest = latest.max(time); }
                    }
                }
            }
        }
        require(now >= self.started_at_ms && now >= latest, "incremental_clock_rollback")?;
        require(now < self.deadline_at_ms, "incremental_cycle_deadline_exceeded")?;
        Ok(Duration::from_millis(self.deadline_at_ms - now))
    }
}

fn read_cycle_budget(root: &Path, c: &Value, ledger: &Value) -> Result<Option<CycleBudget>> {
    let Some(expected) = ledger.get("cycle_budget_sha256") else {
        // Historical completed receipts remain verifiable, not newly budget-certified.
        return Ok(None);
    };
    let raw = fs::read(private(root.join("cycle-budget.json"))?)?;
    require(expected == &json!(sha(&raw)), "incremental_cycle_budget_drift")?;
    let value: Value = serde_json::from_slice(&raw)?;
    let started = value["started_at_ms"].as_u64().ok_or("incremental_cycle_budget")?;
    let deadline = value["deadline_at_ms"].as_u64().ok_or("incremental_cycle_budget")?;
    require(value["protocol"] == BUDGET_PROTOCOL
        && value["config_sha256"] == sha(&bytes(c)?)
        && value["timeout_seconds"] == c["timeout_seconds"]
        && deadline.checked_sub(started).is_some_and(|span| span > 0
            && span <= c["timeout_seconds"].as_u64().unwrap_or(0) * 1000),
        "incremental_cycle_budget")?;
    Ok(Some(CycleBudget { started_at_ms: started, deadline_at_ms: deadline }))
}

fn create_cycle_budget(root: &Path, c: &Value, ledger: &mut Value, start: u64, parent: Option<CycleBudget>) -> Result<CycleBudget> {
    let path = root.join("cycle-budget.json");
    // A crash between the immutable budget and ledger commits must not reset the clock.
    if !path.exists() {
        let budget = match parent {
            Some(parent) => parent,
            None => CycleBudget { started_at_ms: start,
                deadline_at_ms: start.checked_add(c["timeout_seconds"].as_u64().ok_or("incremental_timeout")? * 1000)
                    .ok_or("incremental_cycle_budget")? },
        };
        save(&path, &json!({"protocol":BUDGET_PROTOCOL,"config_sha256":sha(&bytes(c)?),
            "timeout_seconds":c["timeout_seconds"],"started_at_ms":budget.started_at_ms,
            "deadline_at_ms":budget.deadline_at_ms}))?;
        File::open(root)?.sync_all()?;
    }
    ledger["cycle_budget_sha256"] = json!(digest(private(&path)?)?);
    let budget = read_cycle_budget(root, c, ledger)?.ok_or("incremental_cycle_budget")?;
    if let Some(parent) = parent {
        require(budget.started_at_ms == parent.started_at_ms && budget.deadline_at_ms == parent.deadline_at_ms,
            "incremental_parent_budget_drift")?;
    }
    Ok(budget)
}

fn absolute(s: &str) -> bool { Path::new(s).is_absolute() && !s.contains('\0') && !Path::new(s).components().any(|c| c == std::path::Component::ParentDir) }
fn hex(s: &str) -> bool { s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }
fn uuid(s: &str) -> bool { s.len() == 36 && s.bytes().enumerate().all(|(i,b)| if [8,13,18,23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }) }
fn now_ms() -> Result<u64> { Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().try_into()?) }
fn artifact(pin: &Value) -> Result<()> {
    let path = text(pin, "path")?; let expected = text(pin, "sha256")?;
    require(absolute(path) && hex(expected) && fs::metadata(path)?.is_file(), "incremental_artifact")?;
    let mut file = File::open(path)?; let mut hash = Sha256::new(); let mut buf = [0; 65536];
    loop { let n = file.read(&mut buf)?; if n == 0 { break; } hash.update(&buf[..n]); }
    require(format!("{:x}", hash.finalize()) == expected, "incremental_artifact_digest")
}
fn document(path: &str, pins: &mut BTreeMap<String, String>) -> Result<Value> {
    require(absolute(path), "incremental_absolute_path")?;
    let path = private(path)?; let raw = fs::read(&path)?;
    pins.insert(path.to_string_lossy().into_owned(), sha(&raw));
    Ok(serde_json::from_slice(&raw)?)
}
fn checkpoint(root: &Path, ledger: &Value) -> Result<()> {
    let pending = root.join(format!(".ledger-{}-{}.json", std::process::id(), now_ms()?));
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&pending)?;
    file.write_all(&bytes(ledger)?)?; file.write_all(b"\n")?; file.sync_all()?;
    fs::rename(pending, root.join("ledger.json"))?; File::open(root)?.sync_all()?; Ok(())
}
fn dataset_order(ledger: &Value, count: usize) -> Vec<usize> {
    let mut order: Vec<_> = (0..count).collect();
    order.sort_by_key(|index| {
        let attempts = ledger["records"][index.to_string()]["attempts"].as_array();
        match attempts.and_then(|items| items.last()) {
            Some(last) if last["status"] == "succeeded" => 2,
            Some(_) => 0,
            None => 1,
        }
    });
    order
}
fn sync_evidence(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry=entry?; let kind=entry.file_type()?;
        require(!kind.is_symlink(), "incremental_evidence_symlink")?;
        if kind.is_dir() { sync_evidence(&entry.path())?; } else { File::open(entry.path())?.sync_all()?; }
    }
    File::open(root)?.sync_all()?; Ok(())
}

fn validate(c: &Value) -> Result<(Vec<Value>, BTreeMap<String, String>)> {
    require(c["protocol"] == PROTOCOL && c.get("concurrency").is_none_or(|v|
        v.as_u64().is_some_and(|n| (1..=4).contains(&n))), "incremental_concurrency")?;
    require(matches!(c["runtime_transport"].as_str(), Some("host" | "container")), "incremental_transport")?;
    if c["runtime_transport"] == "container" {
        require(text(c, "container")?.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            && !text(c, "container")?.is_empty() && !text(c, "container")?.starts_with('-'), "incremental_container")?;
    }
    require(c["timeout_seconds"].as_u64().is_some_and(|n| (1..=43200).contains(&n)), "incremental_timeout")?;
    for key in ["pack_digest", "schema_scope_digest"] {
        if let Some(v) = c.get(key) { require(v.as_str().is_some_and(hex), "incremental_result_digest")?; }
    }
    artifact(&c["worker"])?; artifact(&c["pipeline"])?;
    let datasets = array(c, "datasets")?;
    require(!datasets.is_empty() && datasets.len() <= 1024, "incremental_datasets")?;
    let mut pins = BTreeMap::new(); let mut seen = BTreeSet::new(); let mut bindings = BTreeSet::new(); let mut schedules = Vec::new();
    for d in datasets {
        require(d.as_object().is_some_and(|o| o.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == BTreeSet::from(["source","schema","table","schedule_path","worker_config"])), "incremental_dataset_fields")?;
        for field in ["source","schema","table"] { require(!text(d,field)?.is_empty(), "incremental_dataset_identity")?; }
        require(seen.insert((text(d,"source")?,text(d,"schema")?,text(d,"table")?)), "incremental_duplicate_dataset")?;
        let s = document(text(d,"schedule_path")?, &mut pins)?;
        let binding = text(&s,"binding")?;
        require(uuid(binding) && s["project_id"] == binding && bindings.insert(binding.to_owned()), "incremental_dedicated_binding")?;
        require(s["protocol"] == "oo.jobs.native-schedule/v1" && s["method"] == "C5"
            && s["c5_mode"] == "layered" && s["c5_layered"]["incremental_only"] == true
            && s["c5_state"]["schema_fingerprint"].as_str().is_some_and(hex)
            && s["period_ms"].as_u64().is_some_and(|n| n >= 1000)
            && s["c5_layered"]["policy"]["period_ms"][2] == s["period_ms"]
            && matches!(s["c5_state"]["initializer"].as_str(), Some("published" | "empty"))
            && s["source_id"] == d["source"] && s["c5_state"]["schema"] == d["schema"]
            && s["c5_state"]["table"] == d["table"] && s["loader"]["path"] == c["pipeline"]["path"]
            && s["loader"]["sha256"] == c["pipeline"]["sha256"]
            && s["loader"]["args"] == json!(["import-native-update"]), "incremental_schedule_scope")?;
        let args = array(&s["producer"],"args")?;
        require(args.len() == 2 && args[0] == "--config", "incremental_producer_args")?;
        artifact(&s["producer"])?;
        let producer = document(args[1].as_str().ok_or("incremental_producer_config")?, &mut pins)?;
        let layered = document(text(&producer,"layered_config")?, &mut pins)?;
        require(layered["incremental_only"] == true && producer.get("incremental_only").is_none_or(|v| v == true)
            && layered["source_id"] == d["source"] && layered["schema"] == d["schema"] && layered["table"] == d["table"]
            && layered["target"] == s["c5_state"]["target"] && producer["target"] == s["c5_state"]["target"]
            && layered["authority_file"] == s["c5_state"]["authority_file"], "incremental_only_required")?;
        for (object, field) in [(&producer,"state_dir"),(&layered,"state_dir"),(&layered,"authority_file")] {
            require(absolute(text(object,field)?), "incremental_absolute_path")?;
        }
        let worker_config = text(d,"worker_config")?; require(absolute(worker_config), "incremental_absolute_path")?;
        pins.insert(worker_config.into(), digest(private(worker_config)?)?);
        schedules.push(s);
    }
    if c.get("schema_scope_digest").is_some() {
        let names = datasets.iter().map(|d| Ok(format!("{}.{}", text(d,"schema")?, text(d,"table")?)))
            .collect::<Result<BTreeSet<_>>>()?;
        require(names.len() == datasets.len()
            && c["schema_scope_digest"] == sha(&bytes(&json!(names))?), "incremental_schema_scope")?;
    }
    Ok((schedules, pins))
}

fn command(c: &Value, calls: &mut Calls, label: &str, args: &[String], input: Option<&[u8]>, deadline: Instant) -> Result<Vec<u8>> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    require(!remaining.is_zero(), "incremental_timeout")?;
    calls.deadlines.default = remaining; calls.deadlines.per_label.clear();
    if c["runtime_transport"] == "container" { calls.docker(label, text(c,"container")?, args, input) }
    else { calls.run(label, args, input) }
}
fn worker(c: &Value, d: &Value, calls: &mut Calls, args: &[String], deadline: Instant) -> Result<Vec<u8>> {
    artifact(&c["worker"])?;
    let mut argv = strings(&[text(&c["worker"],"path")?, "--config", text(d,"worker_config")?]);
    argv.extend_from_slice(args);
    command(c, calls, &format!("incremental-{}",args[0]), &argv, None, deadline)
}
fn nochange(raw: &[u8], binding: &str) -> Result<Option<Value>> {
    let mut found = None;
    for line in std::str::from_utf8(raw)?.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("complete=nochange") { continue; }
        require(found.is_none(), "incremental_ambiguous_completion")?;
        let mut fields = BTreeMap::new();
        for word in words { let (k,v) = word.split_once('=').ok_or("incremental_completion")?; require(fields.insert(k,v).is_none(), "incremental_completion")?; }
        require(fields.get("binding") == Some(&binding) && fields.get("task").is_some_and(|s| uuid(s)), "incremental_completion_scope")?;
        let mut value = json!({"binding":binding,"task":fields["task"]});
        for key in ["pin","generation","source_generation"] {
            let n: i64 = fields.get(key).ok_or("incremental_completion")?.parse()?;
            require(n > 0, "incremental_completion")?; value[key] = json!(n);
        }
        found = Some(value);
    }
    Ok(found)
}

fn active_scan(d: &Value, s: &Value, a: &Value) -> Result<Option<String>> {
    require(a["source_id"] == d["source"] && a["schema"] == d["schema"] && a["table"] == d["table"]
        && a["schema_fingerprint"] == s["c5_state"]["schema_fingerprint"]
        && a.get("scan_id").is_some(), "incremental_authority_scope")?;
    if a["scan_id"].is_null() { return Ok(None); }
    let id=text(a,"scan_id")?; require(!id.is_empty(), "incremental_scan_incomplete")?;
    let expected:Vec<_>=array(a,"chunks")?.iter().map(|ch|ch["chunk_id"].clone()).collect();
    require(!expected.is_empty() && a["expected_chunks"] == json!(expected), "incremental_scan_incomplete")?;
    Ok(Some(id.to_owned()))
}

fn proof(d: &Value, s: &Value, done: &Value, a: &Value, resumed_scan: Option<&str>) -> Result<bool> {
    let high = a["source_generation"].as_i64().ok_or("incremental_authority_generation")?;
    let table = a["table_source_generation"].as_i64().ok_or("incremental_authority_generation")?;
    let observed = done["source_generation"].as_i64().ok_or("incremental_completion")?;
    require(a["source_id"] == d["source"] && a["schema"] == d["schema"] && a["table"] == d["table"]
        && a["schema_fingerprint"] == s["c5_state"]["schema_fingerprint"]
        && a.get("scan_id").is_some() && a.get("expected_chunks").is_some()
        && 1 <= table && table <= observed && observed <= high
        && a["input_digest"].as_str().is_some_and(hex), "incremental_authority_scope")?;
    let empty = s["c5_state"]["initializer"] == "empty";
    let baseline = if empty { &a["baseline_receipt"] } else { &a["baseline"] };
    require(baseline["run_id"].as_str().is_some_and(uuid)
        && baseline["source_generation"].as_i64().is_some_and(|b| 1 <= b && b <= table)
        && baseline["physical_name"].as_str().is_some_and(|v| !v.is_empty())
        && ["config_digest","schema_digest","type_digest"].iter().all(|k| baseline[*k].as_str().is_some_and(hex)),
        "incremental_baseline_identity")?;
    if empty {
        let confirmation = &a["last_empty_confirmation"]; let observation = &confirmation["c5_observation"];
        require(a["protocol"] == "oo.pipeline.native-c5-empty-authority/v1" && a["empty_only"] == true
            && a["chunks"] == json!([]) && baseline["row_count"] == 0 && baseline["source_generation"] == table
            && a["scan_id"].is_null() && a["expected_chunks"].is_null() && a["next_chunk_index"] == 0
            && baseline["source_id"] == d["source"] && baseline["schema"] == d["schema"] && baseline["table"] == d["table"]
            && baseline["input_digest"] == a["input_digest"]
            && confirmation["protocol"] == "oo.jobs.native-c5-nochange/v1"
            && confirmation["binding"] == done["binding"] && confirmation["pin"] == done["pin"]
            && confirmation["source_id"] == d["source"] && confirmation["source_generation"] == observed
            && confirmation["acknowledged_source_generation"] == observed && confirmation["empty_diff"] == true
            && ["data_gen","apply","ack"].iter().all(|k| confirmation[*k] == false)
            && observation["protocol"] == "oo.c5.empty-observation/v1"
            && observation["source_id"] == d["source"] && observation["schema"] == d["schema"] && observation["table"] == d["table"]
            && observation["baseline_receipt"] == *baseline && observation["lease_id"] == done["task"]
            && observation["lease_generation"] == done["generation"]
            && a["empty_observation"]["lease_id"] == done["task"]
            && a["empty_observation"]["lease_generation"] == done["generation"]
            && observation["observation_id"].as_str().is_some_and(uuid)
            && observation["observation_id"] == a["empty_observation"]["id"]
            && observation["rows_returned"] == 0 && observation["row_limit"] == 1
            && observation["read_only"] == true && observation["snapshot_complete"] == true
            && observation["checked_at_ms"] == a["last_empty_confirmed_at_ms"]
            && observation["started_at_ms"].as_u64().is_some_and(|n| n > 0)
            && observation["checked_at_ms"].as_u64().is_some_and(|n| n >= observation["started_at_ms"].as_u64().unwrap_or(u64::MAX)),
            "incremental_empty_confirmation")?;
    } else {
        require(baseline["source"] == d["source"] && baseline["schema_name"] == d["schema"]
            && baseline["table_name"] == d["table"] && baseline["key_columns"] == s["c5_state"]["key_columns"]
            && matches!(a["baseline_kind"].as_str(), Some("single" | "multipart"))
            && a["baseline_kind"] == baseline["baseline_kind"], "incremental_baseline_identity")?;
        let chunks = array(a,"chunks")?; require(!chunks.is_empty(), "incremental_scan_incomplete")?;
        let scan = text(&chunks[0],"last_digest_scan_id")?; require(!scan.is_empty(), "incremental_scan_incomplete")?;
        let mut ids = BTreeSet::new();
        let mut same_scan = true;
        for chunk in chunks {
            same_scan &= chunk["last_digest_scan_id"] == scan;
            require(ids.insert(text(chunk,"chunk_id")?) && chunk["baseline_membership_pending"] == false
                && chunk["last_digest_scan_id"].as_str().is_some_and(|id| !id.is_empty())
                && chunk["scanned_at_ms"].as_u64().is_some_and(|n| n > 0)
                && chunk["digest"].as_array().is_some_and(|v| v.len() == 3)
                && [1,2].iter().all(|i| chunk["digest"][*i].as_str().is_some_and(|v|
                    v.len() == 16 && v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))))
                && chunk["row_count"].as_u64().is_some() && chunk["digest"][0] == chunk["row_count"]
                && chunk["arrow_sha256"].as_str().is_some_and(hex)
                && chunk["projection_sha256"] == baseline["projection_sha256"]
                && chunk["projection_sha256"].as_str().is_some_and(hex)
                && matches!(chunk["digest_codec"].as_str(), Some("mysql-md5-xor/v1" | "oo.c5.arrow-sha256-xor/v1")),
                "incremental_scan_incomplete")?;
        }
        let current_scan = format!("c5-scan:{}",text(done,"task")?);
        return Ok(same_scan && a["scan_id"].is_null() && a["expected_chunks"].is_null()
            && a["next_chunk_index"] == 0 && (scan == current_scan || Some(scan) == resumed_scan));
    }
    Ok(true)
}

fn fresh_after_inherited_scan(authority: &Value, inherited: &Value) -> Result<bool> {
    if inherited.is_null() { return Ok(true); }
    let inherited=inherited.as_str().filter(|id| !id.is_empty() && !id.contains('\0'))
        .ok_or("incremental_cycle_scan_context")?;
    Ok(array(authority,"chunks")?.iter().all(|chunk|
        chunk["last_digest_scan_id"].as_str().is_some_and(|id| id!=inherited)))
}

fn verify_record(d: &Value, s: &Value, record: &Value, root: &Path) -> Result<()> {
    require(record["dataset"] == *d, "incremental_verification_dataset")?;
    let last = array(record, "attempts")?.last().ok_or("incremental_verification_attempt")?;
    require(last["status"] == "succeeded" && last["disabled"] == true,
        "incremental_verification_incomplete")?;
    let saved = &last["proof"];
    if let Some(binding) = last.get("loader_override") {
        let path = private(text(binding, "path")?)?;
        require(path.starts_with(root) && digest(&path)? == text(binding, "sha256")?,
            "incremental_loader_override_drift")?;
        let manifest = load(&path)?;
        let mut expected = s.clone();
        expected["loader"] = manifest["loader"].clone();
        require(manifest == expected && manifest["loader"]["args"] == s["loader"]["args"],
            "incremental_loader_override_scope")?;
        artifact(&manifest["loader"])?;
    }
    let authority_path = private(text(saved, "authority_path")?)?;
    require(authority_path.starts_with(root)
        && digest(&authority_path)? == text(saved, "authority_sha256")?,
        "incremental_verification_authority")?;
    let authority=load(&authority_path)?;
    require(saved["inherited_scan_id"]==record["inherited_scan_id"]
        && fresh_after_inherited_scan(&authority,&record["inherited_scan_id"])?
        && proof(d, s, &saved["nochange"], &authority,
            saved["resumed_scan_id"].as_str())?, "incremental_verification_scan")
}

pub fn verify_completed(config: &Path, root: &Path) -> Result<Value> {
    let c = load(private(config)?)?;
    let (schedules, pins) = validate(&c)?;
    let raw = fs::read(private(root.join("ledger.json"))?)?;
    let ledger: Value = serde_json::from_slice(&raw)?;
    let datasets = array(&c, "datasets")?;
    require(ledger["protocol"] == PROTOCOL && ledger["success"] == true
        && ledger["config_sha256"] == sha(&bytes(&c)?)
        && ledger["input_sha256"] == json!(pins)
        && ledger["records"].as_object().is_some_and(|r| r.len() == datasets.len()),
        "incremental_verification_incomplete")?;
    for key in ["pack_digest", "schema_scope_digest"] {
        require(ledger[key] == c[key] && c[key].as_str().is_some_and(hex),
            "incremental_verification_scope")?;
    }
    for (index, (d, s)) in datasets.iter().zip(&schedules).enumerate() {
        let record = &ledger["records"][index.to_string()];
        verify_record(d, s, record, root)?;
    }
    let budget = read_cycle_budget(root, &c, &ledger)?;
    Ok(json!({"success":true,"protocol":"oo.library.incremental-verification/v1",
        "dataset_count":datasets.len(),"pack_digest":c["pack_digest"],
        "schema_scope_digest":c["schema_scope_digest"],"ledger_sha256":sha(&raw),
        "cycle_budget_recorded":budget.is_some(),
        "datasets":datasets.iter().map(|d| json!([d["source"],d["schema"],d["table"]])).collect::<Vec<_>>()}))
}

fn source_groups(datasets: &[Value], schedules: &[Value]) -> Result<Vec<Vec<usize>>> {
    let mut keys = BTreeMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, (d, s)) in datasets.iter().zip(schedules).enumerate() {
        let key = (text(d, "source")?, text(&s["c5_state"]["target"], "database")?);
        let next = groups.len();
        let group = *keys.entry(key).or_insert_with(|| { groups.push(Vec::new()); next });
        groups[group].push(index);
    }
    Ok(groups)
}

fn run_sources(c: &Value, schedules: &[Value], root: &Path, ledger: &mut Value, deadline: Instant, budget: CycleBudget, loader: Option<&Value>) -> Result<()> {
    let datasets = array(c, "datasets")?;
    let mut profiles = Vec::new();
    for (index, indexes) in source_groups(datasets, schedules)?.iter().enumerate() {
        let group = root.join(format!("source-{index:04}")); dir(&group)?;
        let mut child = c.clone();
        child["concurrency"] = json!(1);
        child["datasets"] = json!(indexes.iter().map(|i| &datasets[*i]).collect::<Vec<_>>());
        if child.get("schema_scope_digest").is_some() {
            let names = indexes.iter().map(|i| format!("{}.{}",datasets[*i]["schema"].as_str().unwrap(),
                datasets[*i]["table"].as_str().unwrap())).collect::<BTreeSet<_>>();
            child["schema_scope_digest"] = json!(sha(&bytes(&json!(names))?));
        }
        let config = group.join("config.json"); save(&config, &child)?;
        profiles.push(json!({"config":config,"out":group.join("run"),"indexes":indexes}));
    }
    ledger["source_runs"] = json!(profiles); checkpoint(root, ledger)?;
    let stop = AtomicBool::new(false);
    let outcome = crate::refresh_profile::parallel_capture(
        &profiles, c["concurrency"].as_u64().ok_or("incremental_concurrency")? as usize,
        &|profile| {
            let config = Path::new(profile["config"].as_str().unwrap());
            let out = Path::new(profile["out"].as_str().unwrap());
            let resume = out.join("ledger.json").exists() || out.join("cycle-budget.json").exists();
            let result = run_until(config, out, resume, Some((deadline, budget)), Some(&stop), loader)
                .map_err(|e| e.to_string());
            match result {
                Ok(result) if result["success"] == true => Ok(result),
                other => {
                    stop.store(true, Ordering::SeqCst);
                    Err(other.err().unwrap_or_else(|| "incremental_source_incomplete".into()))
                }
            }
        },
        |index, profile, outcome| {
            let path = Path::new(text(profile, "out")?).join("ledger.json");
            if path.exists() {
                let child = load(private(&path)?)?;
                for (local, global) in array(profile, "indexes")?.iter().enumerate() {
                    if let Some(record) = child["records"].get(local.to_string()) {
                        ledger["records"][global.as_u64().ok_or("incremental_index")?.to_string()] = record.clone();
                    }
                }
            }
            ledger["source_runs"][index]["status"] = json!(if outcome.is_ok() {"succeeded"} else {"failed"});
            if let Err(error) = outcome { ledger["source_runs"][index]["error"] = json!(error); }
            checkpoint(root, ledger)
        });
    if let Err(error) = &outcome { ledger["error"] = json!(error.to_string()); }
    ledger["success"] = json!(outcome.is_ok());
    checkpoint(root, ledger)
}

pub fn run(config: &Path, root: &Path, resume: bool) -> Result<Value> {
    run_until(config, root, resume, None, None, None)
}

pub fn run_with_loader(config: &Path, root: &Path, resume: bool, loader: &Path) -> Result<Value> {
    let pin = load(private(loader)?)?;
    require(pin.as_object().is_some_and(|o| o.len() == 2 && o.contains_key("path")
        && o.contains_key("sha256")), "incremental_loader_override")?;
    artifact(&pin)?;
    run_until(config, root, resume, None, None, Some(&pin))
}

fn run_until(config: &Path, root: &Path, resume: bool, parent: Option<(Instant, CycleBudget)>, stop: Option<&AtomicBool>, loader: Option<&Value>) -> Result<Value> {
    let invocation_start = Instant::now();
    let started_at_ms = now_ms()?;
    require(config.is_absolute() && root.is_absolute(), "incremental_absolute_path")?;
    let c = load(private(config)?)?; let (schedules, pins) = validate(&c)?;
    let timeout = Duration::from_secs(c["timeout_seconds"].as_u64().ok_or("incremental_timeout")?);
    let invocation_deadline = invocation_start + timeout;
    dir(root)?;
    let lock_path = root.join("ledger.lock"); if lock_path.exists() { private(&lock_path)?; }
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).open(lock_path)?;
    lock.try_lock()?;
    let ledger_path = root.join("ledger.json"); let config_hash = sha(&bytes(&c)?);
    let existed = ledger_path.exists();
    require(resume || !root.join("cycle-budget.json").exists(), "incremental_resume_required")?;
    let mut ledger = if existed {
        require(resume, "incremental_resume_required")?;
        let prior = load(private(&ledger_path)?)?;
        require(prior["protocol"] == PROTOCOL && prior["config_sha256"] == config_hash && prior["input_sha256"] == json!(pins), "incremental_resume_drift")?; prior
    } else { json!({"protocol":PROTOCOL,"config_sha256":config_hash,"input_sha256":pins,"records":{}}) };
    let datasets = array(&c, "datasets")?;
    if ledger["success"] == true {
        require(ledger["records"].as_object().is_some_and(|records| records.len() == datasets.len()),
            "incremental_verification_incomplete")?;
        read_cycle_budget(root, &c, &ledger)?;
        for (index, (d, s)) in datasets.iter().zip(&schedules).enumerate() {
            if let Err(error) = verify_record(d, s, &ledger["records"][index.to_string()], root) {
                ledger["success"] = json!(false);
                ledger["error"] = json!(error.to_string());
                checkpoint(root, &ledger)?;
                return Ok(ledger);
            }
        }
        return Ok(ledger);
    }
    let budget = if existed {
        read_cycle_budget(root, &c, &ledger)?.ok_or("incremental_legacy_budget_missing")?
    } else {
        create_cycle_budget(root, &c, &mut ledger, started_at_ms, parent.map(|(_, budget)| budget))?
    };
    if let Some((_, parent)) = parent {
        require(budget.started_at_ms == parent.started_at_ms && budget.deadline_at_ms == parent.deadline_at_ms,
            "incremental_parent_budget_drift")?;
    }
    let observed_at_ms = now_ms()?;
    let remaining = budget.remaining(observed_at_ms, &ledger);
    ledger["success"] = json!(false);
    if let Err(error) = remaining {
        ledger["error"] = json!(error.to_string());
        checkpoint(root, &ledger)?;
        disable_unfinished(&c, root, &mut ledger)?;
        return Ok(ledger);
    }
    ledger["budget_observed_at_ms"] = json!(observed_at_ms);
    let own_deadline = (Instant::now() + remaining?).min(invocation_deadline);
    let deadline = parent.map_or(own_deadline, |(limit, _)| limit.min(own_deadline));
    ledger.as_object_mut().ok_or("incremental_ledger")?.remove("error");
    for key in ["pack_digest","schema_scope_digest"] { if let Some(v) = c.get(key) { ledger[key] = v.clone(); } }
    checkpoint(root, &ledger)?;
    if c["concurrency"].as_u64().unwrap_or(1) > 1 {
        run_sources(&c, &schedules, root, &mut ledger, deadline, budget, loader)?;
        return Ok(ledger);
    }
    // Resume unfinished occurrences, not a new refresh cycle.
    for index in dataset_order(&ledger, datasets.len()) {
        if stop.is_some_and(|stop| stop.load(Ordering::SeqCst)) {
            ledger["error"] = json!("incremental_peer_failed");
            checkpoint(root, &ledger)?;
            return Ok(ledger);
        }
        let (d, s) = (&datasets[index], &schedules[index]);
        if ledger["records"][index.to_string()]["attempts"].as_array()
            .and_then(|a| a.last()).is_some_and(|a| a["status"] == "succeeded") {
            verify_record(d, s, &ledger["records"][index.to_string()], root)?;
            continue;
        }
        let key = index.to_string(); let binding = text(s,"binding")?; let directory = root.join(format!("dataset-{index:04}")); dir(&directory)?;
        if ledger["records"].get(&key).is_none() { ledger["records"][&key] = json!({"dataset":d,"attempts":[]}); }
        let previous = ledger["records"][&key]["attempts"].as_array().and_then(|a| a.last()).cloned();
        let n = ledger["records"][&key]["attempts"].as_array().ok_or("incremental_ledger")?.len() + 1;
        let attempt = directory.join(format!("attempt-{n:04}")); require(!attempt.exists(), "incremental_attempt_exists")?; dir(&attempt)?;
        ledger["records"][&key]["attempts"].as_array_mut().ok_or("incremental_ledger")?.push(json!({"path":attempt,"status":"running","started_at_ms":now_ms()?}));
        let loader_manifest = if let Some(pin) = loader {
            artifact(pin)?;
            let mut manifest = s.clone();
            for field in ["path", "sha256"] { manifest["loader"][field] = pin[field].clone(); }
            let path = attempt.join("loader-manifest.json"); save(&path, &manifest)?;
            ledger["records"][&key]["attempts"].as_array_mut().ok_or("incremental_ledger")?
                .last_mut().ok_or("incremental_ledger")?["loader_override"] = json!({"path":path,"sha256":digest(&path)?});
            path
        } else { Path::new(text(d,"schedule_path")?).to_path_buf() };
        checkpoint(root, &ledger)?; let mut calls = Calls::new(&attempt)?;
        let outcome = (|| -> Result<Value> {
            for (path, expected) in &pins { require(digest(private(path)?)? == *expected, "incremental_input_drift")?; }
            artifact(&c["pipeline"])?;
            let request = json!({"protocol":"oo.pipeline.native-c5-state/v1","operation":"export",
                "source_id":d["source"],"schema":d["schema"],"table":d["table"],"target":s["c5_state"]["target"]});
            let input = bytes(&request)?; require(input.len() <= 8192, "incremental_export_budget")?;
            save(attempt.join("authority-request.json"),&request)?;
            let pipeline_args = strings(&[text(&c["pipeline"],"path")?,"native-c5-state"]);
            let before = command(&c,&mut calls,"incremental-authority-before",&pipeline_args,Some(&input),deadline);
            let mut resumed_scan = match before {
                Ok(raw) => {
                    put(attempt.join("authority-before.json"),&raw)?;
                    let a:Value=serde_json::from_slice(&raw)?;
                    if let Some(previous) = previous.as_ref().filter(|p|
                        p["error"] == "command_failed:incremental-authority-export"
                        && ledger["records"][&key].get("inherited_scan_id").is_some()) {
                        let prior = Path::new(text(previous,"path")?);
                        require(prior.starts_with(root), "incremental_resume_path")?;
                        let mut checks = fs::read_dir(prior)?.map(|e| e.map(|e|e.path()))
                            .collect::<std::io::Result<Vec<_>>>()?;
                        checks.retain(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("proof-")));
                        checks.sort();
                        if let Some(check) = checks.last() {
                            let done = load(private(check.join("nochange.json"))?)?;
                            let context = load(private(prior.join("scan-context.json"))?)?;
                            let inherited=&ledger["records"][&key]["inherited_scan_id"];
                            if proof(d,s,&done,&a,context["resumed_scan_id"].as_str())?
                                && fresh_after_inherited_scan(&a,inherited)? {
                                return Ok(json!({"nochange":done,"resumed_scan_id":context["resumed_scan_id"],
                                    "inherited_scan_id":inherited,
                                    "authority_sha256":sha(&raw),"authority_path":attempt.join("authority-before.json"),
                                    "recovered_nochange_from":check}));
                            }
                        }
                    }
                    active_scan(d,s,&a)?
                }
                Err(error) if error.downcast_ref::<CommandFailed>().is_some_and(|e|
                    String::from_utf8_lossy(&e.stderr).trim() == "oo-pipeline: c5_state_missing") => None,
                Err(error) => return Err(error),
            };
            // Finish an inherited interrupted scan, but do not label its old
            // prefix as this cycle's new observation. The next scan rechecks it.
            if ledger["records"][&key].get("inherited_scan_id").is_none() {
                ledger["records"][&key]["inherited_scan_id"]=json!(resumed_scan);
                checkpoint(root,&ledger)?;
            }
            let inherited=ledger["records"][&key]["inherited_scan_id"].clone();
            save(attempt.join("scan-context.json"),&json!({"resumed_scan_id":resumed_scan}))?;
            worker(&c,d,&mut calls,&strings(&["native-put","--manifest",text(d,"schedule_path")?]),deadline)?;
            // Reject stale full-mode occurrences through the worker's immutable-manifest guard.
            worker(&c,d,&mut calls,&strings(&["native-loader-put","--manifest",
                loader_manifest.to_str().ok_or("incremental_loader_override")?]),deadline)?;
            let mut checks = 0u64; let mut progress = 0u64; let mut idle_polls = 0u8;
            loop {
                worker(&c,d,&mut calls,&strings(&["native-fire","--project",binding]),deadline)?;
                let raw = worker(&c,d,&mut calls,&strings(&["native-step","--project",binding,"--worker","incremental-refresh"]),deadline)?;
                if std::str::from_utf8(&raw)?.trim() == "claim=idle" {
                    idle_polls += 1;
                    require(idle_polls < 10, "incremental_source_busy_or_not_due")?;
                } else { idle_polls = 0; }
                if let Some(done) = nochange(&raw,binding)? {
                    checks += 1;
                    let check = attempt.join(format!("proof-{checks:04}")); dir(&check)?;
                    save(check.join("nochange.json"),&done)?;
                    artifact(&c["pipeline"])?;
                    let raw = command(&c,&mut calls,"incremental-authority-export",
                        &pipeline_args,Some(&input),deadline)?;
                    let authority: Value = serde_json::from_slice(&raw)?;
                    put(check.join("authority.json"),&raw)?;
                    let accepted = proof(d,s,&done,&authority,resumed_scan.as_deref())?
                        && fresh_after_inherited_scan(&authority,&inherited)?;
                    save(check.join("decision.json"),&json!({"accepted":accepted,"resumed_scan_id":resumed_scan,
                        "inherited_scan_id":inherited,
                        "current_task_scan_id":format!("c5-scan:{}",text(&done,"task")?)}))?;
                    if accepted {
                        return Ok(json!({"nochange":done,"resumed_scan_id":resumed_scan,
                            "inherited_scan_id":inherited,
                            "authority_sha256":sha(&raw),"authority_path":check.join("authority.json")}));
                    }
                    if let Some(id)=active_scan(d,s,&authority)? { resumed_scan=Some(id); }
                } else if std::str::from_utf8(&raw)?.lines().any(|line| line.starts_with("complete=succeeded ")) {
                    // Record a live active scan before the next occurrence; never infer it from an old completed scan.
                    progress += 1;
                    artifact(&c["pipeline"])?;
                    let raw=command(&c,&mut calls,"incremental-authority-progress",&pipeline_args,Some(&input),deadline)?;
                    put(attempt.join(format!("authority-progress-{progress:04}.json")),&raw)?;
                    let authority:Value=serde_json::from_slice(&raw)?;
                    if let Some(id)=active_scan(d,s,&authority)? { resumed_scan=Some(id); }
                }
                require(Instant::now() < deadline, "incremental_timeout")?;
                std::thread::sleep(Duration::from_millis(250).min(deadline.saturating_duration_since(Instant::now())));
            }
        })();
        // Cleanup has its own small budget even when the refresh budget expired.
        let cleanup = worker(&c,d,&mut calls,&strings(&["native-enable","--binding",binding,"--enabled","false"]),Instant::now()+Duration::from_secs(10));
        let durable = sync_evidence(&attempt);
        let record = ledger["records"][&key]["attempts"].as_array_mut().ok_or("incremental_ledger")?.last_mut().ok_or("incremental_ledger")?;
        record["finished_at_ms"] = json!(now_ms()?);
        if let Ok(value) = &outcome { record["proof"] = value.clone(); }
        if let Err(error) = &outcome { record["error"] = json!(error.to_string()); }
        if let Err(error) = &cleanup { record["disable_error"] = json!(error.to_string()); }
        if let Err(error) = &durable { record["evidence_error"] = json!(error.to_string()); }
        record["disabled"] = json!(cleanup.is_ok()); record["status"] = json!(if outcome.is_ok() && cleanup.is_ok() && durable.is_ok() {"succeeded"} else {"failed"});
        let failed = outcome.is_err() || cleanup.is_err() || durable.is_err(); checkpoint(root,&ledger)?;
        if failed { return Ok(ledger); }
    }
    ledger["success"] = json!(true); checkpoint(root,&ledger)?; Ok(ledger)
}

fn disable_unfinished(c: &Value, root: &Path, ledger: &mut Value) -> Result<()> {
    let records = ledger["records"].as_object().ok_or("incremental_ledger")?.clone();
    let count = ledger["budget_cleanup"].as_array().map_or(0, Vec::len);
    let cleanup = root.join(format!("budget-cleanup-{:04}", count + 1));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut results = Vec::new();
    for (index, d) in array(c, "datasets")?.iter().enumerate() {
        let Some(record) = records.get(&index.to_string()) else { continue };
        let Some(last) = array(record, "attempts")?.last() else { continue };
        if last["disabled"] == true { continue; }
        let schedule = load(private(text(d, "schedule_path")?)?)?;
        let mut calls = Calls::new(&cleanup.join(format!("dataset-{index:04}")))?;
        let result = worker(c, d, &mut calls, &strings(&["native-enable", "--binding",
            text(&schedule, "binding")?, "--enabled", "false"]), deadline);
        results.push(json!({"dataset_index":index,"disabled":result.is_ok(),
            "error":result.err().map(|error| error.to_string()),
            // Disabling a schedule is not evidence that an unknown in-flight commit was resolved.
            "active_work_recovery_required":true}));
    }
    if !results.is_empty() {
        if ledger.get("budget_cleanup").is_none() { ledger["budget_cleanup"] = json!([]); }
        ledger["budget_cleanup"].as_array_mut().ok_or("incremental_ledger")?
            .push(json!({"path":cleanup,"finished_at_ms":now_ms()?,"results":results}));
        checkpoint(root, ledger)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn expire_budget(root: &Path) {
        let path = root.join("cycle-budget.json");
        let mut budget = load(&path).unwrap();
        let timeout_ms = budget["timeout_seconds"].as_u64().unwrap() * 1000;
        budget["deadline_at_ms"] = json!(now_ms().unwrap() - 1);
        budget["started_at_ms"] = json!(budget["deadline_at_ms"].as_u64().unwrap() - timeout_ms);
        fs::write(&path, bytes(&budget).unwrap()).unwrap();
        let mut ledger = load(root.join("ledger.json")).unwrap();
        ledger["cycle_budget_sha256"] = json!(digest(&path).unwrap());
        fs::write(root.join("ledger.json"), bytes(&ledger).unwrap()).unwrap();
    }

    #[test]
    fn expired_resume_preserves_attempts_and_never_restarts_source_work() {
        let f = Fixture::new();
        put(f.0.join("fail"), b"").unwrap();
        let failed = f.run(false);
        assert_eq!(failed["success"], false);
        let out = f.0.join("out");
        let budget = fs::read(out.join("cycle-budget.json")).unwrap();
        let retry = f.run(true);
        assert_eq!(retry["success"], false);
        assert_eq!(fs::read(out.join("cycle-budget.json")).unwrap(), budget);
        expire_budget(&out);
        let attempts = retry["records"].clone();
        let commands = fs::read(f.0.join("commands")).unwrap();
        fs::remove_file(f.0.join("fail")).unwrap();
        let expired = f.run(true);
        assert_eq!(expired["success"], false);
        assert_eq!(expired["error"], "incremental_cycle_deadline_exceeded");
        assert_eq!(expired["records"], attempts);
        assert_eq!(fs::read(f.0.join("commands")).unwrap(), commands);
    }

    #[test]
    fn expired_resume_only_disables_unconfirmed_binding_and_preserves_failure() {
        let f = Fixture::new();
        put(f.0.join("fail"), b"").unwrap();
        put(f.0.join("disable-fail"), b"").unwrap();
        let failed = f.run(false);
        assert_eq!(failed["records"]["0"]["attempts"][0]["disabled"], false);
        let commands = fs::read_to_string(f.0.join("commands")).unwrap();
        fs::remove_file(f.0.join("disable-fail")).unwrap();
        expire_budget(&f.0.join("out"));
        let expired = f.run(true);
        assert_eq!(expired["records"], failed["records"]);
        assert_eq!(expired["budget_cleanup"][0]["results"][0]["disabled"], true);
        assert_eq!(expired["budget_cleanup"][0]["results"][0]["active_work_recovery_required"], true);
        let after = fs::read_to_string(f.0.join("commands")).unwrap();
        let extra = after.strip_prefix(&commands).unwrap();
        assert_eq!(extra.lines().count(), 1);
        assert!(extra.contains("native-enable") && extra.contains("--enabled false"));
        assert_eq!(expired["error"], "incremental_cycle_deadline_exceeded");
    }

    #[test]
    fn cycle_budget_is_inherited_by_parallel_sources_and_verified_on_completed_resume() {
        let f = Fixture::new();
        let config = f.0.join("config.json");
        let mut c = load(&config).unwrap();
        c["concurrency"] = json!(2);
        fs::write(&config, bytes(&c).unwrap()).unwrap();
        let done = f.run(false);
        assert_eq!(done["success"], true);
        let root = f.0.join("out");
        let parent = load(root.join("cycle-budget.json")).unwrap();
        let child = load(root.join("source-0000/run/cycle-budget.json")).unwrap();
        for key in ["started_at_ms", "deadline_at_ms"] { assert_eq!(parent[key], child[key]); }
        let commands = fs::read(f.0.join("commands")).unwrap();
        expire_budget(&root);
        assert_eq!(f.run(true)["success"], true);
        assert_eq!(fs::read(f.0.join("commands")).unwrap(), commands);
        assert_eq!(verify_completed(&config, &root).unwrap()["cycle_budget_recorded"], true);
        fs::write(root.join("cycle-budget.json"), b"{}").unwrap();
        assert!(run(&config, &root, true).is_err());
        assert!(verify_completed(&config, &root).is_err());
    }

    #[test]
    fn historical_completed_ledgers_verify_but_incomplete_ones_cannot_get_a_new_budget() {
        let f = Fixture::new();
        assert_eq!(f.run(false)["success"], true);
        let out = f.0.join("out");
        let path = out.join("ledger.json");
        let mut ledger = load(&path).unwrap();
        ledger.as_object_mut().unwrap().remove("cycle_budget_sha256");
        fs::remove_file(out.join("cycle-budget.json")).unwrap();
        fs::write(&path, bytes(&ledger).unwrap()).unwrap();
        let commands = fs::read(f.0.join("commands")).unwrap();
        assert_eq!(f.run(true)["success"], true);
        assert_eq!(verify_completed(&f.0.join("config.json"), &out).unwrap()["cycle_budget_recorded"], false);
        ledger["success"] = json!(false);
        fs::write(&path, bytes(&ledger).unwrap()).unwrap();
        let error = run(&f.0.join("config.json"), &out, true).unwrap_err();
        assert_eq!(error.to_string(), "incremental_legacy_budget_missing");
        assert_eq!(fs::read(f.0.join("commands")).unwrap(), commands);
    }

    #[test]
    fn interrupted_budget_commit_and_clock_rollback_do_not_extend_deadline() {
        let f = Fixture::new();
        let out = f.0.join("out"); dir(&out).unwrap();
        let c = load(f.0.join("config.json")).unwrap();
        let mut ledger = json!({"records":{}});
        let initial = create_cycle_budget(&out, &c, &mut ledger, now_ms().unwrap(), None).unwrap();
        let raw = fs::read(out.join("cycle-budget.json")).unwrap();
        assert!(run(&f.0.join("config.json"), &out, false).is_err());
        assert_eq!(f.run(true)["success"], true);
        assert_eq!(fs::read(out.join("cycle-budget.json")).unwrap(), raw);
        assert_eq!(initial.remaining(initial.started_at_ms - 1, &ledger).unwrap_err().to_string(),
            "incremental_clock_rollback");
        ledger["budget_observed_at_ms"] = json!(initial.started_at_ms + 50);
        assert_eq!(initial.remaining(initial.started_at_ms + 49, &ledger).unwrap_err().to_string(),
            "incremental_clock_rollback");
        ledger["records"] = json!({"0":{"attempts":[{"finished_at_ms":initial.started_at_ms + 100}]}});
        assert!(initial.remaining(initial.started_at_ms + 99, &ledger).is_err());
    }

    #[test]
    fn real_elapsed_downtime_counts_toward_the_original_cycle_budget() {
        let f = Fixture::new();
        let path = f.0.join("config.json");
        let mut c = load(&path).unwrap(); c["timeout_seconds"] = json!(1);
        fs::write(&path, bytes(&c).unwrap()).unwrap();
        put(f.0.join("fail"), b"").unwrap();
        assert_eq!(f.run(false)["success"], false);
        let budget = load(f.0.join("out/cycle-budget.json")).unwrap();
        let remaining = budget["deadline_at_ms"].as_u64().unwrap().saturating_sub(now_ms().unwrap());
        std::thread::sleep(Duration::from_millis(remaining + 20));
        let commands = fs::read(f.0.join("commands")).unwrap();
        let result = f.run(true);
        assert_eq!(result["error"], "incremental_cycle_deadline_exceeded");
        assert_eq!(result["success"], false);
        assert_eq!(fs::read(f.0.join("commands")).unwrap(), commands);
    }

    #[test]
    fn loader_upgrade_is_pinned_per_attempt_without_rewriting_source_inputs() {
        use std::os::unix::fs::PermissionsExt;
        let f=Fixture::new(); let source_config=fs::read(f.0.join("config.json")).unwrap();
        let binary=f.0.join("new-loader");
        fs::copy(f.0.join("fixture"),&binary).unwrap();
        fs::set_permissions(&binary,fs::Permissions::from_mode(0o700)).unwrap();
        let pin=f.0.join("loader-pin.json");
        save(&pin,&json!({"path":binary,"sha256":digest(&binary).unwrap()})).unwrap();
        let out=f.0.join("out");
        let result=run_with_loader(&f.0.join("config.json"),&out,false,&pin).unwrap();
        assert_eq!(result["success"],true);
        assert!(result["records"]["0"]["attempts"][0]["loader_override"]["sha256"].is_string());
        assert_eq!(fs::read(f.0.join("config.json")).unwrap(),source_config);
        assert!(verify_completed(&f.0.join("config.json"),&out).is_ok());
        fs::write(&binary,b"drift").unwrap();
        assert!(verify_completed(&f.0.join("config.json"),&out).is_err());
    }

    #[test]
    fn concurrency_groups_shared_source_watermarks_and_resume_does_not_rescan_successes() {
        let datasets = vec![json!({"source":"a"}),json!({"source":"b"}),json!({"source":"a"})];
        let schedules = vec![json!({"c5_state":{"target":{"database":"db"}}});3];
        assert_eq!(source_groups(&datasets,&schedules).unwrap(),vec![vec![0,2],vec![1]]);
        let f=Fixture::new();
        let path=f.0.join("config.json");let mut c=load(&path).unwrap();
        c["concurrency"]=json!(2);fs::write(&path,bytes(&c).unwrap()).unwrap();
        assert_eq!(f.run(false)["success"],true);
        let commands=fs::read(f.0.join("commands")).unwrap();
        assert_eq!(verify_completed(&path,&f.0.join("out")).unwrap()["dataset_count"],1);
        assert_eq!(f.run(true)["success"],true);
        assert_eq!(fs::read(f.0.join("commands")).unwrap(),commands);
        let ledger=load(f.0.join("out/ledger.json")).unwrap();
        let proof=text(&ledger["records"]["0"]["attempts"][0]["proof"],"authority_path").unwrap();
        fs::write(proof,b"{}").unwrap();
        assert_eq!(f.run(true)["success"],false);
        assert_eq!(fs::read(f.0.join("commands")).unwrap(),commands);
    }

    #[test]
    fn resume_recovers_failed_source_slots_before_rechecking_completed_peers() {
        let ledger = json!({"records":{
            "0":{"attempts":[{"status":"succeeded"}]},
            "1":{"attempts":[{"status":"succeeded"}]},
            "2":{"attempts":[{"status":"failed"}]},
            "4":{"attempts":[{"status":"running"}]}}});
        assert_eq!(dataset_order(&ledger, 5), vec![2,4,3,0,1]);
    }
    #[test]
    fn range_completion_is_not_nochange_and_completion_scope_is_exact() {
        let binding = "11111111-1111-4111-8111-111111111111";
        assert!(nochange(format!("complete=succeeded binding={binding}").as_bytes(),binding).unwrap().is_none());
        let line = format!("complete=nochange binding={binding} pin=2 task={binding} generation=3 source_generation=9");
        assert_eq!(nochange(line.as_bytes(),binding).unwrap().unwrap()["source_generation"],9);
        assert!(nochange(line.as_bytes(),"22222222-2222-4222-8222-222222222222").is_err());
        assert!(nochange(format!("{line}\n{line}").as_bytes(),binding).is_err());
    }
    fn sample_proof() -> (Value, Value, Value, Value) {
        let id = "11111111-1111-4111-8111-111111111111";
        let d = json!({"source":"s","schema":"app","table":"t"});
        let s = json!({"c5_state":{"initializer":"published","schema_fingerprint":"a".repeat(64),"key_columns":["id"]}});
        let done = json!({"source_generation":7,"task":id});
        let a = json!({"source_id":"s","schema":"app","table":"t","schema_fingerprint":"a".repeat(64),
            "source_generation":9,"table_source_generation":5,"input_digest":"b".repeat(64),"scan_id":null,"expected_chunks":null,"next_chunk_index":0,
            "baseline_kind":"single","baseline":{"run_id":id,"source":"s","schema_name":"app","table_name":"t","source_generation":3,
                "physical_name":"p","config_digest":"c".repeat(64),"schema_digest":"d".repeat(64),"type_digest":"e".repeat(64),
                "key_columns":["id"],"baseline_kind":"single","projection_sha256":"f".repeat(64)},
            "chunks":[{"chunk_id":"0","baseline_membership_pending":false,"last_digest_scan_id":format!("c5-scan:{id}"),"scanned_at_ms":123,
                "digest":[1,"0".repeat(16),"1".repeat(16)],"row_count":1,"arrow_sha256":"b".repeat(64),
                "projection_sha256":"f".repeat(64),"digest_codec":"mysql-md5-xor/v1"}]});
        (d,s,done,a)
    }
    #[test]
    fn proof_needs_current_complete_metadata_and_allows_other_tables_to_advance_h() {
        let (d,s,done,a) = sample_proof();
        assert!(proof(&d,&s,&done,&a,None).unwrap());
        let mut old=a.clone(); old["chunks"][0]["last_digest_scan_id"]=json!("scan1");
        assert!(!proof(&d,&s,&done,&old,None).unwrap());
        assert!(proof(&d,&s,&done,&old,Some("scan1")).unwrap());
        old["scan_id"]=json!("running");
        assert!(!proof(&d,&s,&done,&old,Some("scan1")).unwrap());
        for (pointer,value) in [("/chunks/0/digest",Value::Null),
            ("/chunks/0/last_digest_scan_id",Value::Null),("/chunks/0/scanned_at_ms",Value::Null),
            ("/chunks/0/baseline_membership_pending",json!(true)),("/table_source_generation",json!(8))] {
            let mut invalid=a.clone(); *invalid.pointer_mut(pointer).unwrap()=value;
            assert!(proof(&d,&s,&done,&invalid,None).is_err(),"{pointer}");
        }
    }

    #[test]
    fn inherited_scan_completion_is_not_a_new_cycle_observation() {
        let (_,_,_,a)=sample_proof();
        let old=a["chunks"][0]["last_digest_scan_id"].clone();
        assert!(!fresh_after_inherited_scan(&a,&old).unwrap());
        let mut fresh=a.clone();
        fresh["chunks"][0]["last_digest_scan_id"]=json!("new-cycle-scan");
        assert!(fresh_after_inherited_scan(&fresh,&old).unwrap());
        assert!(fresh_after_inherited_scan(&fresh,&Value::Null).unwrap());
        assert!(fresh_after_inherited_scan(&fresh,&json!(true)).is_err());
        let mut mixed=fresh.clone();
        mixed["chunks"].as_array_mut().unwrap().push(a["chunks"][0].clone());
        assert!(!fresh_after_inherited_scan(&mixed,&old).unwrap());
    }

    #[test]
    fn controller_drains_old_scan_then_demands_a_fresh_one_without_resetting_it() {
        let f=Fixture::new();
        let mut old=sample_proof().3;
        old["chunks"][0]["last_digest_scan_id"]=json!("previous-cycle");
        save(f.0.join("old-complete.json"),&old).unwrap();
        let mut active=old;
        active["scan_id"]=json!("previous-cycle");
        active["expected_chunks"]=json!(["0"]);
        fs::write(f.0.join("authority.json"),bytes(&active).unwrap()).unwrap();
        save(f.0.join("fresh-complete.json"),&sample_proof().3).unwrap();
        let path=f.0.join("fixture");
        let script=fs::read_to_string(&path).unwrap();
        let script=script.replace(
            &format!("touch '{}/changed'; echo",f.0.display()),
            &format!("touch '{r}/changed'; cp '{r}/old-complete.json' '{r}/authority.json'; echo",r=f.0.display()));
        let marker="  echo 'complete=nochange";
        let replacement=format!(
            "  if [ -e '{r}/old-checked' ]; then cp '{r}/fresh-complete.json' '{r}/authority.json'; else touch '{r}/old-checked'; fi\n{marker}",
            r=f.0.display());
        let script=script.replace(marker,&replacement);
        fs::write(&path,script.as_bytes()).unwrap();
        let hash=sha(script.as_bytes());
        for name in ["config.json","schedule.json"] {
            let path=f.0.join(name);let mut value=load(&path).unwrap();
            for key in ["worker","pipeline","producer","loader"] {
                if value.get(key).is_some() {value[key]["sha256"]=json!(hash);}
            }
            fs::write(path,bytes(&value).unwrap()).unwrap();
        }
        let result=f.run(false);
        assert_eq!(result["success"],true,"{result}");
        assert_eq!(result["records"]["0"]["inherited_scan_id"],"previous-cycle");
        let checks=f.0.join("out/dataset-0000/attempt-0001");
        assert_eq!(load(checks.join("proof-0001/decision.json")).unwrap()["accepted"],false);
        assert_eq!(load(checks.join("proof-0002/decision.json")).unwrap()["accepted"],true);
        assert_eq!(fs::read_to_string(f.0.join("commands")).unwrap().matches("native-step").count(),3);
        assert!(verify_completed(&f.0.join("config.json"),&f.0.join("out")).is_ok());
    }

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            static SERIAL:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!("oo-incremental-{}-{}-{}",std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
                SERIAL.fetch_add(1,Ordering::Relaxed)));
            dir(&root).unwrap();
            let id = "11111111-1111-4111-8111-111111111111";
            let binary = root.join("fixture");
            let script = format!(r#"#!/bin/sh
echo "$*" >> '{r}/commands'
if [ "$1" = native-c5-state ]; then
  cat > '{r}/export-request.json'
  if [ -e '{r}/missing' ] && [ ! -e '{r}/registered' ]; then echo 'oo-pipeline: c5_state_missing' >&2; exit 1; fi
  if [ -e '{r}/export-fail' ] && [ -e '{r}/registered' ]; then rm '{r}/export-fail'; echo fixture_export_failed >&2; exit 1; fi
  cat '{r}/authority.json'; exit 0
fi
if [ "$3" = native-put ]; then touch '{r}/registered'; fi
if [ "$3" = native-step ]; then
  if [ -e '{r}/fail' ]; then echo fixture_step_failed >&2; exit 7; fi
  if [ ! -e '{r}/changed' ]; then touch '{r}/changed'; echo 'complete=succeeded binding={id}'; exit 0; fi
  echo 'complete=nochange binding={id} pin=2 task={id} generation=3 source_generation=7'; exit 0
fi
if [ "$3" = native-enable ] && [ -e '{r}/disable-fail' ]; then echo fixture_disable_failed >&2; exit 8; fi
echo ok
"#,r=root.display());
            put(&binary,script.as_bytes()).unwrap(); fs::set_permissions(&binary,fs::Permissions::from_mode(0o700)).unwrap();
            let artifact=json!({"path":binary,"sha256":sha(script.as_bytes())});
            let target=json!({"database":"fixture"});
            save(root.join("layered.json"),&json!({"incremental_only":true,"source_id":"s","schema":"app","table":"t",
                "target":target,"state_dir":root.join("capture"),"authority_file":root.join("do-not-read.json")})).unwrap();
            save(root.join("producer.json"),&json!({"layered_config":root.join("layered.json"),
                "state_dir":root.join("producer-state"),"target":target})).unwrap();
            let mut schedule=sample_proof().1;
            for (k,v) in json!({"protocol":"oo.jobs.native-schedule/v1","method":"C5","c5_mode":"layered",
                "period_ms":1000,"c5_layered":{"incremental_only":true,"policy":{"period_ms":[1000,2000,1000,4000]}},
                "source_id":"s","binding":id,"project_id":id,
                "loader":{"path":binary,"sha256":artifact["sha256"],"args":["import-native-update"]},
                "producer":{"path":binary,"sha256":artifact["sha256"],"args":["--config",root.join("producer.json")]}}).as_object().unwrap() {
                schedule[k]=v.clone();
            }
            schedule["c5_state"]["schema"]=json!("app"); schedule["c5_state"]["table"]=json!("t");
            schedule["c5_state"]["target"]=target; schedule["c5_state"]["authority_file"]=json!(root.join("do-not-read.json"));
            save(root.join("schedule.json"),&schedule).unwrap();
            put(root.join("worker.conf"),b"database=fixture\n").unwrap();
            save(root.join("authority.json"),&sample_proof().3).unwrap();
            save(root.join("config.json"),&json!({"protocol":PROTOCOL,"runtime_transport":"host","timeout_seconds":20,
                "worker":artifact,"pipeline":artifact,"pack_digest":"a".repeat(64),"schema_scope_digest":sha(&bytes(&json!(["app.t"])).unwrap()),
                "datasets":[{"source":"s","schema":"app","table":"t","schedule_path":root.join("schedule.json"),"worker_config":root.join("worker.conf")}]})).unwrap();
            Self(root)
        }
        fn run(&self,resume:bool)->Value { run(&self.0.join("config.json"),&self.0.join("out"),resume).unwrap() }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _=fs::remove_dir_all(&self.0); } }

    #[test]
    fn post_nochange_export_failure_resumes_without_another_source_scan() {
        let f=Fixture::new();put(f.0.join("changed"),b"").unwrap();put(f.0.join("export-fail"),b"").unwrap();
        assert_eq!(f.run(false)["success"],false);
        let success=f.run(true);
        assert_eq!(success["success"],true);
        assert!(success["records"]["0"]["attempts"][1]["proof"]["recovered_nochange_from"].is_string());
        assert_eq!(fs::read_to_string(f.0.join("commands")).unwrap().matches("native-step").count(),1);
        assert!(verify_completed(&f.0.join("config.json"),&f.0.join("out")).is_ok());
    }

    #[test]
    fn full_or_non_incremental_schedules_are_refused_before_any_command() {
        for mode in ["full","multipart"] {
            let f=Fixture::new(); let path=f.0.join("schedule.json"); let mut s=load(&path).unwrap();
            s["c5_state"]["initializer"]=json!(mode); fs::write(path,bytes(&s).unwrap()).unwrap();
            assert!(run(&f.0.join("config.json"),&f.0.join("out"),false).is_err());
            assert!(!f.0.join("commands").exists());
        }
        let f=Fixture::new(); let path=f.0.join("layered.json"); let mut l=load(&path).unwrap();
        l["incremental_only"]=json!(false); fs::write(path,bytes(&l).unwrap()).unwrap();
        assert!(run(&f.0.join("config.json"),&f.0.join("out"),false).is_err());
        assert!(!f.0.join("commands").exists());
        let f=Fixture::new(); let path=f.0.join("schedule.json"); let mut s=load(&path).unwrap();
        s["c5_layered"]["policy"]["period_ms"][2]=json!(60000); fs::write(path,bytes(&s).unwrap()).unwrap();
        assert!(run(&f.0.join("config.json"),&f.0.join("out"),false).is_err());
        assert!(!f.0.join("commands").exists());
    }

    #[test]
    fn failure_is_preserved_cleanup_runs_and_resume_obtains_new_nochange_proof() {
        let f=Fixture::new(); put(f.0.join("fail"),b"").unwrap(); put(f.0.join("missing"),b"").unwrap();
        let failed=f.run(false);
        assert_eq!(failed["success"],false);
        assert_eq!(failed["records"]["0"]["attempts"][0]["disabled"],true);
        assert_eq!(failed["records"]["0"]["attempts"][0]["error"],"command_failed:incremental-native-step");
        let first=f.0.join("out/dataset-0000/attempt-0001");
        let stderr=fs::read_dir(&first).unwrap().map(|e|e.unwrap().path())
            .find(|p|p.to_string_lossy().ends_with("incremental-native-step.stderr")).unwrap();
        assert_eq!(fs::read(stderr).unwrap(),b"fixture_step_failed\n");
        fs::remove_file(f.0.join("fail")).unwrap();
        let success=f.run(true);
        assert_eq!(success["success"],true);
        assert_eq!(success["pack_digest"],"a".repeat(64));
        assert_eq!(success["schema_scope_digest"],sha(&bytes(&json!(["app.t"])).unwrap()));
        assert_eq!(success["records"]["0"]["attempts"].as_array().unwrap().len(),2);
        let exported=load(f.0.join("export-request.json")).unwrap();
        assert_eq!(exported["operation"],"export");
        assert!(exported.get("lease_id").is_none() && exported.get("authority_file").is_none());
        let log=fs::read_to_string(f.0.join("commands")).unwrap();
        assert!(log.find("native-loader-put").unwrap()<log.find("native-fire").unwrap());
        assert_eq!(log.matches("native-step").count(),3);
        assert_eq!(log.matches("native-c5-state").count(),4);
        f.run(true);
        assert_eq!(fs::read_to_string(f.0.join("commands")).unwrap().matches("native-c5-state").count(),4);
        let f=Fixture::new(); put(f.0.join("fail"),b"").unwrap(); put(f.0.join("disable-fail"),b"").unwrap();
        let failed=f.run(false); let attempt=&failed["records"]["0"]["attempts"][0];
        assert_eq!(attempt["disabled"],false);
        assert!(attempt["error"].is_string() && attempt["disable_error"].is_string());
    }

    #[test]
    fn completed_verification_rechecks_every_proof_and_input_without_running_commands() {
        let f=Fixture::new();
        assert_eq!(f.run(false)["success"], true);
        let log=fs::read(f.0.join("commands")).unwrap();
        let config=f.0.join("config.json"); let out=f.0.join("out");
        assert_eq!(verify_completed(&config,&out).unwrap()["dataset_count"], 1);
        assert_eq!(fs::read(f.0.join("commands")).unwrap(),log);
        let path=out.join("ledger.json"); let original=load(&path).unwrap();
        for pointer in ["/success", "/records/0/attempts/0/disabled"] {
            let mut bad=original.clone(); *bad.pointer_mut(pointer).unwrap()=json!(false);
            fs::write(&path,bytes(&bad).unwrap()).unwrap();
            assert!(verify_completed(&config,&out).is_err());
        }
        fs::write(&path,bytes(&original).unwrap()).unwrap();
        let authority=text(&original["records"]["0"]["attempts"][0]["proof"],"authority_path").unwrap();
        fs::write(authority,b"{}").unwrap();
        assert!(verify_completed(&config,&out).is_err());
    }

    #[test]
    fn empty_confirmation_requires_the_actual_nochange_task_and_baseline() {
        let (d,mut s,_,mut a)=sample_proof(); let id="11111111-1111-4111-8111-111111111111";
        s["c5_state"]["initializer"]=json!("empty");
        let done=json!({"binding":id,"task":id,"pin":2,"generation":3,"source_generation":7});
        let baseline=json!({"source_id":"s","schema":"app","table":"t","run_id":id,"source_generation":5,
            "physical_name":"p","config_digest":"c".repeat(64),"schema_digest":"d".repeat(64),"type_digest":"e".repeat(64),
            "row_count":0,"input_digest":"b".repeat(64)});
        a["protocol"]=json!("oo.pipeline.native-c5-empty-authority/v1"); a["empty_only"]=json!(true);
        a["chunks"]=json!([]); a["baseline_receipt"]=baseline.clone();
        a["empty_observation"]=json!({"id":id,"lease_id":id,"lease_generation":3});
        a["last_empty_confirmed_at_ms"]=json!(200);
        a["last_empty_confirmation"]=json!({"protocol":"oo.jobs.native-c5-nochange/v1","source_id":"s",
            "binding":id,"pin":2,"source_generation":7,"acknowledged_source_generation":7,"empty_diff":true,
            "data_gen":false,"apply":false,"ack":false,"c5_observation":{"protocol":"oo.c5.empty-observation/v1",
                "source_id":"s","schema":"app","table":"t","baseline_receipt":baseline,"lease_id":id,"lease_generation":3,
                "observation_id":id,"rows_returned":0,"row_limit":1,"read_only":true,"snapshot_complete":true,
                "started_at_ms":100,"checked_at_ms":200}});
        assert!(proof(&d,&s,&done,&a,None).unwrap());
        a["last_empty_confirmation"]["c5_observation"]["lease_generation"]=json!(2);
        assert!(proof(&d,&s,&done,&a,None).is_err());
    }

    #[test]
    fn hints_only_nochange_with_an_old_completed_scan_keeps_polling_until_timeout() {
        let f=Fixture::new(); let path=f.0.join("config.json"); let mut c=load(&path).unwrap();
        put(f.0.join("changed"),b"").unwrap();
        c["timeout_seconds"]=json!(5); fs::write(path,bytes(&c).unwrap()).unwrap();
        let path=f.0.join("authority.json"); let mut a=load(&path).unwrap();
        a["chunks"][0]["last_digest_scan_id"]=json!("old-completed-scan"); fs::write(path,bytes(&a).unwrap()).unwrap();
        let result=f.run(false); assert_eq!(result["success"],false);
        assert_eq!(result["records"]["0"]["attempts"][0]["disabled"],true);
        let check=f.0.join("out/dataset-0000/attempt-0001/proof-0001/decision.json");
        assert_eq!(load(check).unwrap()["accepted"],false);
        assert!(fs::read_to_string(f.0.join("commands")).unwrap().matches("native-step").count()>=2);
    }
}
