mod api;
mod audit;
mod batch;
mod full_pipeline;
mod identity;
mod identity_contract;
#[cfg(test)]
mod identity_contract_tests;
mod io;
mod mysql;
mod mysql_handoff;
mod oracle;
mod product;
mod refresh;
mod refresh_schedule;
mod refresh_incremental;
mod refresh_profile;
mod refresh_progress;
mod refresh_publish;
mod refresh_gateway;
mod refresh_live_catalog;
mod refresh_cutover;
use io::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, Instant},
};

fn refresh_capture_complete(report: &Value) -> Result<bool> {
    let counts = &report["counts"];
    require(counts["states"]["failed"] == 0, "refresh_capture_failed")?;
    let planned = counts["planned_tables"].as_u64().ok_or("progress_table_count")?;
    let complete = counts["completed_tables"].as_u64().ok_or("progress_table_count")?;
    require(complete <= planned, "progress_table_count")?;
    Ok(complete == planned)
}

fn observe_refresh(path: &Path, watch_seconds: Option<u64>) -> Result<()> {
    if let Some(seconds) = watch_seconds {
        require((1..=43200).contains(&seconds), "progress_watch_budget")?;
    }
    let deadline = watch_seconds.map(|seconds| Instant::now() + Duration::from_secs(seconds));
    loop {
        let report = refresh_progress::progress(path)?;
        println!("{report}");
        // This observes capture only; publication and public verification remain separate gates.
        if refresh_capture_complete(&report)? {
            return Ok(());
        }
        let Some(deadline) = deadline else {
            return Ok(());
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_secs(30)));
    }
}

fn stop_owned(binary: &str) -> Result<()> {
    let mut hits = Vec::new();
    for e in fs::read_dir("/proc")? {
        let e = e?;
        if !e
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let Ok(cmd) = fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let args = cmd.split(|b| *b == 0).collect::<Vec<_>>();
        if args.first() == Some(&binary.as_bytes()) && args.contains(&b"serve".as_slice()) {
            hits.push(e.file_name());
        }
    }
    require(hits.len() <= 1, "ambiguous_api_owner")?;
    for pid in &hits {
        require(
            Command::new("kill")
                .arg("-TERM")
                .arg(pid)
                .status()?
                .success(),
            "owned_api_stop",
        )?;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    for pid in &hits {
        while Path::new("/proc").join(pid).exists() {
            require(Instant::now() < deadline, "owned_api_stop_deadline")?;
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    println!(
        "{}",
        json!({"stopped_owned_pids":hits.iter().map(|p|p.to_string_lossy()).collect::<Vec<_>>()})
    );
    Ok(())
}

const NATIVE_OPERATORS: &str = "/etc/open-ontology/native/mysql-operators.json";

fn six_fields(input: &Value) -> Result<()> {
    identity::input_keys(input, &["type", "host", "port", "database", "user", "secret_ref"], "six_fields")?;
    require(input["type"] == "mysql", "six_fields")?;
    Ok(())
}

fn installed_operator(input: &Value, registry_path: &Path) -> Result<PathBuf> {
    six_fields(input)?;
    if let Some(path) = identity::installed(input, registry_path, "mysql")? {
        return Ok(path);
    }
    let registry = load(private(registry_path)?)?;
    require(
        registry.as_object().is_some_and(|r| {
            r.keys().map(String::as_str).collect::<BTreeSet<_>>()
                == BTreeSet::from(["protocol", "operators"])
                || r.keys().map(String::as_str).collect::<BTreeSet<_>>()
                == BTreeSet::from(["protocol", "operators", "source_identities"])
        }) && registry["protocol"] == "oo.mysql.native-operators/v1",
        "native_operator_registry",
    )?;
    let mut matches = Vec::new();
    for entry in array(&registry, "operators")? {
        let path = private(entry.as_str().ok_or("native_operator_path")?)?;
        let installed = load(&path)?;
        if ["host", "port", "database", "user", "secret_ref"]
            .iter()
            .all(|key| input[*key] == installed[*key])
        {
            matches.push(path);
        }
    }
    require(!matches.is_empty(), "native_operator_unknown")?;
    require(matches.len() == 1, "native_operator_ambiguous")?;
    Ok(matches.remove(0))
}

fn connection(input: &Value, installed: &Value) -> Result<Value> {
    six_fields(input)?;
    identity::validate_operator(input, installed, "mysql")?;
    let mut c = installed.clone();
    for key in ["host", "port", "database", "user", "secret_ref"] {
        require(input[key] == c[key], "installed_connection_mismatch")?;
        c[key] = input[key].clone();
    }
    let registry = load(private(text(&c["onboarding"], "secret_registry")?)?)?;
    require(
        registry[text(input, "secret_ref")?] == c["password_file"],
        "secret_binding",
    )?;
    let secret = private(text(&c, "password_file")?)?;
    require(
        secret.starts_with(text(&c["onboarding"], "host_secret_root")?),
        "secret_root",
    )?;
    mysql::profile_access(&c)?;
    Ok(c)
}

fn capture_runtime(c: &Value, calls: &mut Calls, label: &str, args: &[String]) -> Result<Vec<u8>> {
    let o = &c["onboarding"];
    match o.get("runtime_transport").and_then(Value::as_str) {
        None if o.get("runtime_transport").is_none() => calls.docker(label, text(o, "container")?, args, None),
        Some("container") => calls.docker(label, text(o, "container")?, args, None),
        Some("host") => calls.run(label, args, None),
        _ => Err("capture_runtime_transport".into()),
    }
}

fn check_dependencies(c: &Value, mode: &str) -> Result<()> {
    let o = &c["onboarding"];
    require(o.get("runtime_transport").is_none()
        || matches!(o["runtime_transport"].as_str(), Some("host" | "container")),
        "capture_runtime_transport")?;
    let keys: &[&str] = if mode == "check" { &["worker"] }
        else { &["producer", "worker", "loader", "admission"] };
    for key in keys { binary(text(o, key)?)?; }
    private(text(o, "worker_config")?)?;
    if matches!(mode, "check" | "capture") { return Ok(()); }
    let product = &o["product"];
    let admin = private(text(product, "token_admin_password_file")?)?;
    require(admin.canonicalize()?.starts_with(Path::new(text(o, "host_secret_root")?).canonicalize()?),
        "control_secret_root")?;
    for key in ["api_binary", "release_binary", "gateway_binary", "token_binary", "cli_binary",
        "mapping_binary", "coordinator_binary"] {
        binary(text(product, key)?)?;
    }
    Ok(())
}

fn worker(c: &Value, calls: &mut Calls, args: &[String]) -> Result<Value> {
    let o = &c["onboarding"];
    let mut a = strings(&[text(o, "worker")?, "--config", text(o, "worker_config")?]);
    a.extend_from_slice(args);
    let raw = capture_runtime(c, calls, &args[0], &a)?;
    if ["native-state", "native-artifacts"].contains(&args[0].as_str()) {
        Ok(serde_json::from_slice(&raw)?)
    } else {
        Ok(json!({"stdout":String::from_utf8(raw)?}))
    }
}
/// The CDC binding as the producer sees it (password under `/secrets`). The
/// optional `profile_access` block is kept out of the six binding fields.
fn native_binding(c: &Value) -> Result<Value> {
    let o = &c["onboarding"];
    let mut native = c.clone();
    let password =
        Path::new(text(c, "password_file")?).strip_prefix(text(o, "host_secret_root")?)?;
    match o.get("runtime_transport").and_then(Value::as_str) {
        None if o.get("runtime_transport").is_none() => {
            native["password_file"] = json!(format!("/secrets/{}", password.display()));
        }
        Some("container") => native["password_file"] = json!(format!("/secrets/{}", password.display())),
        Some("host") => {}
        _ => return Err("capture_runtime_transport".into()),
    }
    Ok(native)
}

fn same_binding(old: &Value, native: &Value) -> Result<()> {
    for k in [
        "host",
        "port",
        "database",
        "user",
        "secret_ref",
        "password_file",
    ] {
        require(old[k] == native[k], "connection_drift")?;
    }
    Ok(())
}

/// Arguments of `c1-onboarding-prepare`. The operator's `c1_baseline_format` is forwarded only
/// when present: absent means v1; explicit versions select the matching producer contract.
fn prepare_args(o: &Value, driver: &Path, prepared: &Path) -> Result<Vec<String>> {
    let mut args = strings(&[
        text(o, "producer")?,
        "c1-onboarding-prepare",
        "--config",
        driver.to_str().ok_or("path")?,
        "--out",
        prepared.to_str().ok_or("path")?,
    ]);
    if let Some(value) = o.get("c1_baseline_format").filter(|v| !v.is_null()) {
        let format = value.as_str().unwrap_or_default();
        require(
            ["v1", "chunked-typed/v2", "typed-keys/v3"].contains(&format),
            "c1_baseline_format",
        )?;
        args.extend(strings(&["--c1-baseline-format", format]));
    }
    Ok(args)
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// `native-state` refuses with `error=binding` while the binding row does not exist yet, which
/// is the normal state after a failed first `native-put`. Only that exact refusal, for a
/// well-formed binding id, counts as fresh; every other failure stays an error.
fn state_or_fresh(result: Result<Value>, binding: &str) -> Result<Value> {
    match result {
        Ok(state) => Ok(state),
        Err(err) => {
            let absent = err.downcast_ref::<CommandFailed>().is_some_and(|failed| {
                failed.label == "native-state"
                    && String::from_utf8_lossy(&failed.stderr).trim() == "error=binding"
            });
            if absent && is_uuid(binding) {
                Ok(json!({"applied": false}))
            } else {
                Err(err)
            }
        }
    }
}

fn captured_state(identity: &Value, state: &Value) -> Result<()> {
    require(state["applied"] == true, "capture_pending")?;
    require(state["protocol"] == "oo.jobs.native-state/v1" && state["method"] == "C1"
        && state["binding"] == identity["binding_id"]
        && state["source_id"] == identity["source_id"]
        && state["source_id"].as_str().is_some_and(|source| !source.is_empty())
        && state["source_generation"].as_i64().is_some_and(|generation| generation > 0),
        "capture_state_binding")
}

fn capture(c: &Value, calls: &mut Calls) -> Result<Value> {
    let o = &c["onboarding"];
    let root = Path::new(text(o, "prepared_root")?);
    dir(root)?;
    let native = native_binding(c)?;
    let driver = root.join("driver-config.json");
    if driver.exists() {
        same_binding(&load(&driver)?, &native)?;
    } else {
        save(&driver, &native)?;
    }
    let prepared = root.join("prepared");
    let freshly_prepared = !prepared.exists();
    if freshly_prepared {
        capture_runtime(
            c,
            calls,
            "prepare",
            &prepare_args(o, &driver, &prepared)?,
        )?;
    }
    let identity = load(prepared.join("identity.json"))?;
    identity::runtime(c, &[&identity, &load(prepared.join("native-manifest.json"))?])?;
    require(
        identity["binding_id"] == o["binding"] && identity["project_id"] == o["project_id"],
        "installed_identity",
    )?;
    if freshly_prepared {
        worker(
            c,
            calls,
            &strings(&[
                "native-put",
                "--manifest",
                prepared
                    .join("native-manifest.json")
                    .to_str()
                    .ok_or("path")?,
            ]),
        )?;
    }
    let state = state_or_fresh(
        worker(
            c,
            calls,
            &strings(&["native-state", "--binding", text(&identity, "binding_id")?]),
        ),
        text(&identity, "binding_id")?,
    )?;
    if state["applied"] != true {
        worker(
            c,
            calls,
            &strings(&[
                "native-put",
                "--manifest",
                prepared
                    .join("native-manifest.json")
                    .to_str()
                    .ok_or("path")?,
            ]),
        )?;
        worker(
            c,
            calls,
            &strings(&["native-fire", "--project", text(&identity, "project_id")?]),
        )?;
        worker(
            c,
            calls,
            &strings(&[
                "native-step",
                "--project",
                text(&identity, "project_id")?,
                "--worker",
                "mysql-rust-onboarding",
            ]),
        )?;
    }
    let state = worker(
        c,
        calls,
        &strings(&["native-state", "--binding", text(&identity, "binding_id")?]),
    )?;
    captured_state(&identity, &state)?;
    save(calls.out.join("identity.json"), &identity)?;
    save(
        calls.out.join("capture-receipt.json"),
        &json!({"protocol":"oo.mysql.connection-capture/v1","source_id":identity["source_id"],"binding":identity["binding_id"],"native_state":state,"source_config_sha256":digest(prepared.join("source-config.json"))?,"manifest_sha256":digest(prepared.join("native-manifest.json"))?,"full_product_chain_complete":false}),
    )?;
    Ok(identity)
}

struct Owner(PathBuf);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn own(c: &Value) -> Result<Owner> {
    let root = Path::new(text(&c["onboarding"], "prepared_root")?);
    dir(root)?;
    let lock = root.join("rust-runtime-owner.json");
    if lock.exists() {
        let prior = load(&lock)?;
        let pid = prior["pid"].as_u64().ok_or("owner_pid")?;
        require(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?
                .success(),
            "runtime_already_owned",
        )?;
        fs::rename(
            &lock,
            root.join(format!("rust-runtime-stale-owner-{pid}.json")),
        )?;
    }
    save(
        &lock,
        &json!({"pid":std::process::id(),"binding":c["onboarding"]["binding"],"executable_sha256":digest(env::current_exe()?)?}),
    )?;
    Ok(Owner(lock))
}
fn full_state_bound(c: &Value, state: &Value, catalog: &Value) -> Result<()> {
    let o = &c["onboarding"];
    let full = &o["full"];
    let entries = array(catalog, "entries")?;
    require(
        mysql::collection(entries, array(full, "tables")?)? == *catalog,
        "full_catalog_binding",
    )?;
    let source = text(full, "source_id")?;
    require(
        !source.is_empty()
            && entries.iter().all(|entry| {
                entry["source_id"] == source && entry["source_generation"].as_i64().is_some_and(|n| n > 0)
            })
            && state["binding"] == o["binding"]
            && state["method"] == "C5"
            && state["source_id"] == source
            && state["applied"] == true
            && state["source_generation"].as_i64()
                == entries.iter().filter_map(|entry| entry["source_generation"].as_i64()).max(),
        "full_state_binding",
    )
}

fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args == ["--help"] || args == ["-h"] || args == ["help"] {
        println!("Usage: source-onboarding COMMAND [OPTIONS]\n\
Commands:\n\
  check | capture | follow | full | serve\n\
  mysql-handoff plan|confirm --help\n\
  observe-identity | register-identity | stop-owned-api\n\
  full-completeness | full-audit\n\
  refresh --config PRIVATE_CONFIG --out PRIVATE_DIR [--resume]\n\
  refresh-incremental --config PRIVATE_CONFIG --out PRIVATE_DIR [--resume] [--loader-override PRIVATE_PIN]\n\
  verify-refresh-incremental | publish-refresh-library\n\
  prepare-refresh-profiles | verify-refresh-profiles | capture-refresh-profiles\n\
  wait-refresh-profiles | run-refresh-profile | refresh-progress\n\
  prepare-refresh-gateway | probe-refresh-gateway | prepare-probe-refresh-gateway\n\
  activate-refresh-gateway | probe-refresh-public\n\
  refresh-live-catalog --config PRIVATE_CONFIG --out PRIVATE_DIR [--dry-run]\n\
  refresh-schedule run-once|status|dry-run --config PRIVATE_SCHEDULE\n\
\nPeriodic refresh configuration and recovery: source-onboarding refresh-schedule --help");
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("refresh-schedule") {
        return refresh_schedule::dispatch(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("mysql-handoff") {
        return mysql_handoff::dispatch(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("refresh-live-catalog") {
        return refresh_live_catalog::dispatch(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("observe-identity") {
        require(args.len() == 7 && args[1] == "--input" && args[3] == "--operator" && args[5] == "--out",
            "usage: observe-identity --input PRIVATE_INPUT --operator PRIVATE_OPERATOR --out NEW_PRIVATE_DIR")?;
        println!("{}", identity::observe(Path::new(&args[2]), Path::new(&args[4]), Path::new(&args[6]))?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("register-identity") {
        require(args.len() == 5 && args[1] == "--request" && args[3] == "--out",
            "usage: register-identity --request PRIVATE_REQUEST --out NEW_PRIVATE_DIR")?;
        println!("{}", identity::register(Path::new(&args[2]), Path::new(&args[4]))?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("refresh-incremental") {
        let resume = args.get(5).is_some_and(|v| v == "--resume");
        let next = if resume { 6 } else { 5 };
        let overridden = args.get(next).is_some_and(|v| v == "--loader-override");
        require(args.len() >= 5 && args[1] == "--config" && args[3] == "--out"
            && args.len() == next + if overridden { 2 } else { 0 },
            "usage: refresh-incremental --config PRIVATE_CONFIG --out PRIVATE_DIR [--resume] [--loader-override PRIVATE_PIN]")?;
        let result = if overridden {
            refresh_incremental::run_with_loader(Path::new(&args[2]), Path::new(&args[4]), resume, Path::new(&args[next+1]))?
        } else { refresh_incremental::run(Path::new(&args[2]), Path::new(&args[4]), resume)? };
        let success = result["success"] == true;
        println!("{result}");
        require(success, "incremental_refresh_incomplete")?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("verify-refresh-incremental") {
        require(args.len() == 5 && args[1] == "--config" && args[3] == "--out", "usage")?;
        println!("{}", refresh_incremental::verify_completed(Path::new(&args[2]), Path::new(&args[4]))?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("activate-refresh-gateway") {
        require(args.len() == 3 && args[1] == "--config", "usage")?;
        println!("{}", refresh_cutover::run(Path::new(&args[2]))?);
        return Ok(());
    }
    if ["prepare-refresh-gateway", "probe-refresh-gateway", "prepare-probe-refresh-gateway", "probe-refresh-public"].contains(&args.first().map(String::as_str).unwrap_or("")) {
        require(args.len() == 3 && args[1] == "--config", "usage")?;
        let result = if args[0] == "prepare-refresh-gateway" {
            refresh_gateway::prepare(Path::new(&args[2]))?
        } else if args[0] == "prepare-probe-refresh-gateway" {
            refresh_gateway::prepare(Path::new(&args[2]))?;
            refresh_gateway::probe(Path::new(&args[2]))?
        } else if args[0] == "probe-refresh-gateway" {
            refresh_gateway::probe(Path::new(&args[2]))?
        } else {
            refresh_gateway::public_probe(Path::new(&args[2]))?
        };
        println!("{result}");
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("publish-refresh-library") {
        require(args.len() == 5 && args[1] == "--config" && args[3] == "--out", "usage")?;
        println!("{}", refresh_publish::run(Path::new(&args[2]), Path::new(&args[4]))?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("prepare-refresh-profiles") {
        require(args.len() == 5 && args[1] == "--config" && args[3] == "--out", "usage")?;
        println!("{}", refresh_profile::prepare(Path::new(&args[2]), Path::new(&args[4]))?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("verify-refresh-profiles") {
        require((args.len() == 3 || args.len() == 4) && args[1] == "--plan"
            && (args.len() == 3 || args[3] == "--prepared-only"), "usage")?;
        println!("{}", refresh_profile::verify_profiles(Path::new(&args[2]), args.len() == 4)?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("refresh-progress") {
        require(
            (args.len() == 3 || args.len() == 5) && args[1] == "--plan"
                && (args.len() == 3 || args[3] == "--watch-seconds"),
            "usage: refresh-progress --plan PRIVATE_PLAN [--watch-seconds 1..43200]",
        )?;
        let watch = if args.len() == 5 {
            Some(args[4].parse().map_err(|_| "progress_watch_budget")?)
        } else {
            None
        };
        observe_refresh(Path::new(&args[2]), watch)?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("capture-refresh-profiles") {
        require(args.len() == 5 && args[1] == "--plan" && args[3] == "--concurrency", "usage")?;
        let concurrency = args[4].parse().map_err(|_| "refresh_concurrency")?;
        println!("{}", refresh_profile::capture_profiles(Path::new(&args[2]), concurrency)?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("wait-refresh-profiles") {
        require(args.len() == 5 && args[1] == "--plan" && args[3] == "--seconds", "usage")?;
        println!("{}", refresh_profile::wait_profiles(Path::new(&args[2]),
            args[4].parse().map_err(|_| "refresh_wait_budget")?)?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("run-refresh-profile") {
        require(args.len() == 3 && args[1] == "--profile", "usage")?;
        println!("{}", refresh_profile::run(Path::new(&args[2]))?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("refresh") {
        require((args.len() == 5 || args.len() == 6)
            && args[1] == "--config" && args[3] == "--out"
            && (args.len() == 5 || args[5] == "--resume"),
            "usage: refresh --config PRIVATE_CONFIG --out PRIVATE_DIR [--resume]")?;
        let result = refresh::run(Path::new(&args[2]), Path::new(&args[4]), args.len() == 6)?;
        let success = result["success"] == true;
        println!("{result}");
        require(success, "refresh_incomplete")?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("stop-owned-api") {
        require(args.len() == 2, "usage")?;
        return stop_owned(&args[1]);
    }
    if args.first().map(String::as_str) == Some("full-completeness") {
        require(
            args.len() == 5 && args[1] == "--tables" && args[3] == "--ledger",
            "usage: full-completeness --tables MANIFEST --ledger LEDGER",
        )?;
        let issues = batch::completeness(&load(private(&args[2])?)?, &load(private(&args[4])?)?)?;
        println!("{}", json!({"issues":issues}));
        require(issues.is_empty(), "batch_incomplete")?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("full-audit") {
        require((args.len()==9 || (args.len()==11 && args[9]=="--request")) && args[1]=="--config" && args[3]=="--catalog" && args[5]=="--targets" && args[7]=="--out", "usage: full-audit --config CONFIG --catalog CATALOG --targets TARGETS --out DIR")?;
        let config=load(private(&args[2])?)?;
        let catalog=load(private(&args[4])?)?;
        let targets=load(private(&args[6])?)?;
        let mut calls=Calls::new(Path::new(&args[8]))?;
        let result=if args.len()==11 {audit::compare_chunks(&config,&catalog,&targets,&mut calls,&load(private(&args[10])?)?)?}else{audit::compare_full(&config,&catalog,&targets,&mut calls)?};
        save(Path::new(&args[8]).join("independent-audit.json"),&result)?;
        println!("{}",result);
        return Ok(());
    }
    let oracle_runtime = args.len() == 9 && args[1] == "--input" && args[3] == "--operator"
        && args[5] == "--runtime" && args[7] == "--out";
    if oracle_runtime {
        require(args[0] == "capture", "oracle_runtime_capture_only")?;
        let input = load(private(&args[2])?)?;
        let result = oracle::capture(&input, Path::new(&args[4]), Path::new(&args[6]), Path::new(&args[8]))?;
        println!("{result}");
        return Ok(());
    }
    let explicit =
        args.len() == 7 && args[1] == "--input" && args[3] == "--operator" && args[5] == "--out";
    let default = args.len() == 5 && args[1] == "--input" && args[3] == "--out";
    require(
        explicit || default,
        "usage: source-onboarding check|capture|follow|full|serve --input SIX_FIELDS [--operator INSTALLED] --out PRIVATE_DIR",
    )?;
    require(
        ["check", "capture", "follow", "full", "serve"].contains(&args[0].as_str()),
        "mode",
    )?;
    let input = load(private(&args[2])?)?;
    if input["type"] == "oracle" {
        require(args[0] == "check", "oracle_capture_requires_explicit_runtime")?;
        let operator_path = if explicit { private(&args[4])? }
            else { oracle::installed_operator(&input, Path::new("/etc/open-ontology/native/oracle-operators.json"))? };
        let out = Path::new(&args[if explicit { 6 } else { 4 }]);
        println!("{}", oracle::check(&input, &operator_path, out)?);
        return Ok(());
    }
    let operator_path = if explicit {
        private(&args[4])?
    } else {
        installed_operator(&input, Path::new(NATIVE_OPERATORS))?
    };
    let installed = load(&operator_path)?;
    let c = connection(&input, &installed)?;
    if args[0] == "full" {
        let out = Path::new(&args[if explicit { 6 } else { 4 }]);
        if c.get("source_registration").is_some() {
            require(c["onboarding"]["c1_baseline_format"]=="typed-keys/v3"
                && c["onboarding"]["full"].is_null(), "identity_full_requires_managed_c1")?;
            check_dependencies(&c,"capture")?;
            let mut calls=Calls::with_operator(out,&c["onboarding"])?;
            let _owner=own(&c)?;
            capture(&c,&mut calls)?;
            return Ok(());
        }
        if c["onboarding"]["full"]["batch_manifest"].is_string() {
            mysql::full_batch(&c, out)?;
        } else {
            mysql::full(&c, out)?;
        }
        return Ok(());
    }
    if args[0] == "check" && c["onboarding"]["full"].is_object() {
        // A read-only C5 status check needs its pinned worker, not token-issuer credentials.
        let o = &c["onboarding"];
        binary(text(o, "worker")?)?;
        private(text(o, "worker_config")?)?;
        let catalog = load(private(text(&o["full"], "catalog")?)?)?;
        let out = Path::new(&args[if explicit { 6 } else { 4 }]);
        let mut calls = Calls::with_operator(out, o)?;
        save(out.join("input-binding.json"), &json!({
            "input_sha256":digest(&args[2])?, "operator_sha256":digest(&operator_path)?,
            "binding":o["binding"], "runtime_sha256":digest(env::current_exe()?)?
        }))?;
        let state = worker(
            &c, &mut calls,
            &strings(&["native-state", "--binding", text(o, "binding")?]),
        )?;
        identity::check_state(&c, &state)?;
        full_state_bound(&c, &state, &catalog)?;
        save(out.join("native-state.json"), &state)?;
        println!("checked");
        return Ok(());
    }
    check_dependencies(&c, &args[0])?;
    let out = Path::new(&args[if explicit { 6 } else { 4 }]);
    let mut calls = Calls::with_operator(out, &c["onboarding"])?;
    save(
        out.join("input-binding.json"),
        &json!({"input_sha256":digest(&args[2])?,"operator_sha256":digest(&operator_path)?,"binding":c["onboarding"]["binding"],"runtime_sha256":digest(env::current_exe()?)?}),
    )?;
    if args[0] == "check" {
        let state = worker(
            &c,
            &mut calls,
            &strings(&[
                "native-state",
                "--binding",
                text(&c["onboarding"], "binding")?,
            ]),
        )?;
        identity::check_state(&c, &state)?;
        save(out.join("native-state.json"), &state)?;
        println!("checked");
        return Ok(());
    }
    let _owner = own(&c)?;
    let identity = capture(&c, &mut calls)?;
    if args[0] == "capture" {
        return Ok(());
    }
    if args[0] == "follow" {
        mysql::follow(&c, out.join("follow").as_path())?;
        return Ok(());
    }
    let o = &c["onboarding"];
    let period = o["period_ms"].as_u64().ok_or("period")?;
    let seconds = o["serve_seconds"].as_u64().ok_or("duration")?;
    require(
        (1000..=86400000).contains(&period) && (1..=86400).contains(&seconds),
        "schedule",
    )?;
    let start = Instant::now();
    let mut previous = o["after_generation"].as_u64().ok_or("after_generation")?;
    let mut cuts = Vec::new();
    loop {
        worker(
            &c,
            &mut calls,
            &strings(&["native-fire", "--project", text(&identity, "project_id")?]),
        )?;
        worker(
            &c,
            &mut calls,
            &strings(&[
                "native-step",
                "--project",
                text(&identity, "project_id")?,
                "--worker",
                "mysql-rust-follow",
            ]),
        )?;
        let state = worker(
            &c,
            &mut calls,
            &strings(&["native-state", "--binding", text(&identity, "binding_id")?]),
        )?;
        let generation = state["source_generation"].as_u64().unwrap_or(0);
        if state["applied"] == true && generation > previous {
            let cut = mysql::follow(&c, &out.join(format!("generation-{generation}")))?;
            cuts.push(cut);
            previous = generation;
        }
        if start.elapsed() >= Duration::from_secs(seconds) {
            break;
        }
        std::thread::sleep(Duration::from_millis(period));
    }
    save(
        out.join("follow-receipt.json"),
        &json!({"protocol":"oo.mysql.product-follow/v1","source_id":identity["source_id"],"binding":identity["binding_id"],"cuts":cuts,"current_generation":previous,"full_product_chain_complete":false,"agent_dialogue_pending":true,"worker_exit":0}),
    )?;
    println!("serve exited");
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("source-onboarding: {e}; private evidence retained");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_observer_does_not_treat_loading_or_failure_as_completion() {
        let mut report = json!({"counts":{
            "planned_tables":3,"completed_tables":2,"states":{"failed":0}}});
        assert!(!refresh_capture_complete(&report).unwrap());
        report["counts"]["completed_tables"] = json!(3);
        assert!(refresh_capture_complete(&report).unwrap());
        report["counts"]["states"]["failed"] = json!(1);
        assert!(refresh_capture_complete(&report).is_err());
        assert!(refresh_capture_complete(&json!({})).is_err());
        assert!(observe_refresh(Path::new("/not-read"), Some(0)).is_err());
        assert!(observe_refresh(Path::new("/not-read"), Some(43201)).is_err());
    }

    #[test]
    fn prepare_forwards_only_an_explicit_baseline_format() {
        let driver = Path::new("/p/driver.json");
        let prepared = Path::new("/p/prepared");
        let plain = strings(&[
            "/bin/mysql-preview",
            "c1-onboarding-prepare",
            "--config",
            "/p/driver.json",
            "--out",
            "/p/prepared",
        ]);
        assert_eq!(
            prepare_args(&json!({"producer":"/bin/mysql-preview"}), driver, prepared).unwrap(),
            plain
        );
        assert_eq!(
            prepare_args(
                &json!({"producer":"/bin/mysql-preview","c1_baseline_format":"v1"}),
                driver,
                prepared
            )
            .unwrap(),
            strings(&[
                "/bin/mysql-preview",
                "c1-onboarding-prepare",
                "--config",
                "/p/driver.json",
                "--out",
                "/p/prepared",
                "--c1-baseline-format",
                "v1",
            ])
        );
        let v2 = prepare_args(
            &json!({"producer":"/bin/mysql-preview","c1_baseline_format":"chunked-typed/v2"}),
            driver,
            prepared,
        )
        .unwrap();
        assert_eq!(&v2[..plain.len()], plain.as_slice());
        assert_eq!(&v2[plain.len()..], strings(&["--c1-baseline-format", "chunked-typed/v2"]).as_slice());
        let v3 = prepare_args(
            &json!({"producer":"/bin/mysql-preview","c1_baseline_format":"typed-keys/v3"}),
            driver,
            prepared,
        ).unwrap();
        assert_eq!(&v3[..plain.len()], plain.as_slice());
        assert_eq!(&v3[plain.len()..], strings(&["--c1-baseline-format", "typed-keys/v3"]).as_slice());
        assert!(prepare_args(
            &json!({"producer":"/bin/mysql-preview","c1_baseline_format":"chunked-typed/v3"}),
            driver,
            prepared
        )
        .is_err());
    }

    #[test]
    fn six_fields_reject_redirection_before_reading_credentials() {
        let base = json!({"host":"fixture","port":3306,"database":"owned","user":"reader","secret_ref":"ref"});
        let mut input = base.clone();
        input["type"] = json!("mysql");
        input["host"] = json!("customer");
        assert!(
            connection(&input, &base)
                .unwrap_err()
                .to_string()
                .contains("installed_connection_mismatch")
        );
    }
    #[test]
    fn default_registry_requires_unique_exact_private_installation() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = env::temp_dir().join(format!("oo-native-routing-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let op = root.join("operator.json");
        let registry = root.join("registry.json");
        let input = json!({"type":"mysql","host":"fixture","port":3306,"database":"owned","user":"reader","secret_ref":"ref"});
        save(&op, &input).unwrap();
        let registration =
            |entries: Value| json!({"protocol":"oo.mysql.native-operators/v1","operators":entries});
        save(&registry, &registration(json!([op]))).unwrap();
        assert_eq!(installed_operator(&input, &registry).unwrap(), op);
        let mut unknown = input.clone();
        unknown["host"] = json!("customer");
        assert!(
            installed_operator(&unknown, &registry)
                .unwrap_err()
                .to_string()
                .contains("native_operator_unknown")
        );
        unknown = input.clone();
        unknown["recipe"] = json!("injected");
        assert!(
            installed_operator(&unknown, &registry)
                .unwrap_err()
                .to_string()
                .contains("six_fields")
        );
        fs::write(&registry, bytes(&registration(json!([op, op]))).unwrap()).unwrap();
        assert!(
            installed_operator(&input, &registry)
                .unwrap_err()
                .to_string()
                .contains("native_operator_ambiguous")
        );
        fs::write(&registry, bytes(&registration(json!([op]))).unwrap()).unwrap();
        fs::set_permissions(&op, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            installed_operator(&input, &registry)
                .unwrap_err()
                .to_string()
                .contains("private_file")
        );
        fs::set_permissions(&op, fs::Permissions::from_mode(0o600)).unwrap();
        let link = root.join("link.json");
        symlink(&op, &link).unwrap();
        fs::write(&registry, bytes(&registration(json!([link]))).unwrap()).unwrap();
        assert!(
            installed_operator(&input, &registry)
                .unwrap_err()
                .to_string()
                .contains("private_file")
        );
        fs::remove_dir_all(root).unwrap();
    }
    /// A private root with `secrets/cdc.pw` and `secrets/profile.pw` (0600) and a
    /// secret registry. Callers remove it.
    fn binding_scratch(name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let root = env::temp_dir().join(format!("oo-p12-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let secrets = root.join("secrets");
        fs::create_dir_all(&secrets).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700)).unwrap();
        for file in ["cdc.pw", "profile.pw"] {
            let password = secrets.join(file);
            fs::write(&password, "p12-sentinel-password\n").unwrap();
            fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let registry = root.join("registry.json");
        fs::write(
            &registry,
            json!({"secret://cdc": secrets.join("cdc.pw"), "secret://profile": secrets.join("profile.pw")}).to_string(),
        )
        .unwrap();
        fs::set_permissions(&registry, fs::Permissions::from_mode(0o600)).unwrap();
        root
    }

    fn binding_operator(root: &Path) -> Value {
        let secrets = root.join("secrets");
        json!({
            "type": "mysql",
            "host": "oo-cb-p12-mysql57",
            "port": 3306,
            "database": "oo_cb_p12",
            "user": "oo_cdc",
            "secret_ref": "secret://cdc",
            "password_file": secrets.join("cdc.pw"),
            "ca_file": "/tls/ca.pem",
            "tls_server_name": "mysql57.p12.owned.example",
            "onboarding": {
                "host_secret_root": secrets,
                "secret_registry": root.join("registry.json"),
                "prepared_root": root.join("prepared"),
                "product": {"source_access": "network"}
            }
        })
    }

    #[test]
    fn profile_access_is_outside_the_cdc_binding() {
        let root = binding_scratch("profile-outside");
        let cdc = binding_operator(&root);
        let mut profiled = cdc.clone();
        profiled["onboarding"]["product"]["profile_access"] =
            json!({"user": "oo_profile", "secret_ref": "secret://profile"});
        same_binding(
            &native_binding(&cdc).unwrap(),
            &native_binding(&profiled).unwrap(),
        )
        .unwrap();
        assert_eq!(native_binding(&profiled).unwrap()["user"], "oo_cdc");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_capture_uses_original_private_secret_path_and_scoped_worker() {
        use std::os::unix::fs::PermissionsExt;
        let root = binding_scratch("host-capture");
        let mut c = binding_operator(&root);
        c["onboarding"]["runtime_transport"] = json!("host");
        assert_eq!(native_binding(&c).unwrap()["password_file"], c["password_file"]);
        let script = b"#!/bin/sh\nprintf '{\"protocol\":\"fixture-worker\",\"applied\":false}'\n";
        let pin_dir = root.join(sha(script)); dir(&pin_dir).unwrap();
        let executable = pin_dir.join("worker");
        put(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let config = root.join("worker.conf"); put(&config, b"fixture only").unwrap();
        for key in ["worker", "producer", "loader", "admission"] { c["onboarding"][key] = json!(executable); }
        c["onboarding"]["worker_config"] = json!(config);
        check_dependencies(&c, "check").unwrap();
        check_dependencies(&c, "capture").unwrap();
        assert!(check_dependencies(&c, "follow").is_err());
        let mut calls = Calls::new(&root.join("calls")).unwrap();
        assert_eq!(worker(&c, &mut calls, &strings(&["native-state", "--binding", "fixture"]))
            .unwrap()["protocol"], "fixture-worker");
        c["onboarding"]["producer"] = json!("/missing/producer");
        check_dependencies(&c, "check").unwrap();
        assert!(check_dependencies(&c, "capture").is_err());
        for invalid in [json!("ssh"), json!(null), json!(false)] {
            c["onboarding"]["runtime_transport"] = invalid;
            assert!(native_binding(&c).is_err());
            assert!(check_dependencies(&c, "check").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn applied_capture_state_cannot_be_substituted_across_source_or_method() {
        let identity = json!({"binding_id":"binding", "source_id":"source"});
        let state = json!({"protocol":"oo.jobs.native-state/v1","binding":"binding",
            "source_id":"source","method":"C1","applied":true,"source_generation":1});
        captured_state(&identity, &state).unwrap();
        for (key, value) in [("binding", json!("other")), ("source_id", json!("other")),
            ("method", json!("C5")), ("source_generation", json!(0)),
            ("protocol", json!("other")), ("applied", json!(false))] {
            let mut invalid = state.clone(); invalid[key] = value;
            assert!(captured_state(&identity, &invalid).is_err(), "{key}");
        }
    }

    #[test]
    fn full_status_binds_worker_cut_without_token_issuer_credentials() {
        let selected = json!([{"schema":"sample","table":"items"}]);
        let entries = json!([{
            "protocol":"oo.pipeline.sealed-import-catalog/v1","state":"awaiting_publish","shadows_queried":false,
            "source_id":"sample_source","source_generation":2,
            "tables":[{"logical_schema":"sample","logical_table":"items","stored_namespace":"pn_fixture","stored_physical":"pt_fixture"}]
        }]);
        let catalog = mysql::collection(entries.as_array().unwrap(), selected.as_array().unwrap()).unwrap();
        let c = json!({"onboarding":{"binding":"binding","full":{"source_id":"sample_source","tables":selected}}});
        let state = json!({"binding":"binding","method":"C5","source_id":"sample_source","applied":true,"source_generation":2});
        full_state_bound(&c, &state, &catalog).unwrap();
        for (key, bad) in [
            ("binding", json!("other")),
            ("method", json!("C1")),
            ("source_id", json!("other")),
            ("applied", json!(false)),
            ("source_generation", json!(3)),
        ] {
            let mut wrong = state.clone();
            wrong[key] = bad;
            assert!(full_state_bound(&c, &wrong, &catalog).is_err(), "{key}");
        }
        let mut changed = catalog.clone();
        changed["entries"][0]["source_generation"] = json!(3);
        assert!(full_state_bound(&c, &state, &changed).is_err());
    }

    #[test]
    fn profile_access_refuses_unknown_keys_at_startup() {
        let root = binding_scratch("profile-startup");
        let cdc = binding_operator(&root);
        let input = json!({"type": "mysql", "host": "oo-cb-p12-mysql57", "port": 3306, "database": "oo_cb_p12", "user": "oo_cdc", "secret_ref": "secret://cdc"});
        let mut profiled = cdc.clone();
        profiled["onboarding"]["product"]["profile_access"] =
            json!({"user": "oo_profile", "secret_ref": "secret://profile", "password": "inline"});
        assert!(
            connection(&input, &profiled)
                .unwrap_err()
                .to_string()
                .contains("profile_access_keys")
        );
        profiled["onboarding"]["product"]["profile_access"] =
            json!({"user": "oo_profile", "secret_ref": "secret://profile"});
        assert!(connection(&input, &profiled).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capture_still_refuses_a_changed_cdc_binding() {
        let root = binding_scratch("cdc-drift");
        let c = binding_operator(&root);
        let prepared = root.join("prepared");
        dir(&prepared).unwrap();
        save(
            prepared.join("driver-config.json"),
            &native_binding(&c).unwrap(),
        )
        .unwrap();
        let mut calls = Calls::new(&root.join("calls")).unwrap();
        let secrets = root.join("secrets");
        let changes = [
            ("user", json!("oo_other")),
            ("host", json!("elsewhere")),
            ("port", json!(3307)),
            ("database", json!("other")),
            ("secret_ref", json!("secret://profile")),
            ("password_file", json!(secrets.join("profile.pw"))),
        ];
        for (key, value) in changes {
            let mut changed = c.clone();
            changed[key] = value;
            let error = capture(&changed, &mut calls).unwrap_err().to_string();
            assert!(error.contains("connection_drift"), "{key}: {error}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capture_state_treats_only_an_absent_binding_as_fresh() {
        let binding = "0b1c2d3e-4f50-4a61-8b72-9c83d4e5f601";
        let failed = |label: &str, stderr: &[u8]| -> Box<dyn std::error::Error> {
            Box::new(CommandFailed {
                label: label.into(),
                stderr: stderr.to_vec(),
            })
        };
        let refused = failed("native-state", b"error=binding\n");
        let fresh = state_or_fresh(Err(refused), binding).unwrap();
        assert_eq!(fresh["applied"], false);
        let state = json!({"applied": true});
        assert_eq!(state_or_fresh(Ok(state.clone()), binding).unwrap(), state);
        for (err, binding) in [
            (failed("native-state", b"error=binding\n"), "not-a-uuid"),
            (failed("native-state", b"error=runtime\n"), binding),
            (failed("native-state", b"error=binding\nerror=other\n"), binding),
            (failed("native-put", b"error=binding\n"), binding),
            (failed("native-state", b""), binding),
            (Box::from("command_failed:native-state"), binding),
        ] {
            assert!(state_or_fresh(Err(err), binding).is_err(), "{binding}");
        }
    }

    #[test]
    fn identifiers_refuse_sql() {
        assert!(ident("a';DELETE").is_err());
        assert!(ident("value_probe").is_ok());
    }
}
