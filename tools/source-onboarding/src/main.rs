mod api;
mod audit;
mod io;
mod mysql;
mod product;
use io::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, Instant},
};

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
    let actual = input.as_object().ok_or("six_fields")?;
    let expected = BTreeSet::from(["type", "host", "port", "database", "user", "secret_ref"]);
    require(
        actual.keys().map(String::as_str).collect::<BTreeSet<_>>() == expected
            && input["type"] == "mysql",
        "six_fields",
    )?;
    Ok(())
}

fn installed_operator(input: &Value, registry_path: &Path) -> Result<PathBuf> {
    six_fields(input)?;
    let registry = load(private(registry_path)?)?;
    require(
        registry.as_object().is_some_and(|r| {
            r.keys().map(String::as_str).collect::<BTreeSet<_>>()
                == BTreeSet::from(["protocol", "operators"])
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
    Ok(c)
}

fn worker(c: &Value, calls: &mut Calls, args: &[String]) -> Result<Value> {
    let o = &c["onboarding"];
    let mut a = strings(&[text(o, "worker")?, "--config", text(o, "worker_config")?]);
    a.extend_from_slice(args);
    let raw = calls.docker(&args[0], text(o, "container")?, &a, None)?;
    if ["native-state", "native-artifacts"].contains(&args[0].as_str()) {
        Ok(serde_json::from_slice(&raw)?)
    } else {
        Ok(json!({"stdout":String::from_utf8(raw)?}))
    }
}
fn capture(c: &Value, calls: &mut Calls) -> Result<Value> {
    let o = &c["onboarding"];
    let root = Path::new(text(o, "prepared_root")?);
    dir(root)?;
    let mut native = c.clone();
    let password =
        Path::new(text(c, "password_file")?).strip_prefix(text(o, "host_secret_root")?)?;
    native["password_file"] = json!(format!("/secrets/{}", password.display()));
    let driver = root.join("driver-config.json");
    if driver.exists() {
        let old = load(&driver)?;
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
    } else {
        save(&driver, &native)?;
    }
    let prepared = root.join("prepared");
    let freshly_prepared = !prepared.exists();
    if freshly_prepared {
        calls.docker(
            "prepare",
            text(o, "container")?,
            &strings(&[
                text(o, "producer")?,
                "c1-onboarding-prepare",
                "--config",
                driver.to_str().ok_or("path")?,
                "--out",
                prepared.to_str().ok_or("path")?,
            ]),
            None,
        )?;
    }
    let identity = load(prepared.join("identity.json"))?;
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
    let state = worker(
        c,
        calls,
        &strings(&["native-state", "--binding", text(&identity, "binding_id")?]),
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
    require(state["applied"] == true, "capture_pending")?;
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
fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("stop-owned-api") {
        require(args.len() == 2, "usage")?;
        return stop_owned(&args[1]);
    }
    let explicit =
        args.len() == 7 && args[1] == "--input" && args[3] == "--operator" && args[5] == "--out";
    let default = args.len() == 5 && args[1] == "--input" && args[3] == "--out";
    require(
        explicit || default,
        "usage: source-onboarding check|capture|follow|serve --input SIX_FIELDS [--operator INSTALLED] --out PRIVATE_DIR",
    )?;
    require(
        ["check", "capture", "follow", "serve"].contains(&args[0].as_str()),
        "mode",
    )?;
    let input = load(private(&args[2])?)?;
    let operator_path = if explicit {
        private(&args[4])?
    } else {
        installed_operator(&input, Path::new(NATIVE_OPERATORS))?
    };
    let installed = load(&operator_path)?;
    let c = connection(&input, &installed)?;
    let admin = private(text(
        &c["onboarding"]["product"],
        "token_admin_password_file",
    )?)?;
    require(
        admin
            .canonicalize()?
            .starts_with(Path::new(text(&c["onboarding"], "host_secret_root")?).canonicalize()?),
        "control_secret_root",
    )?;
    for k in ["producer", "worker", "loader", "admission"] {
        binary(text(&c["onboarding"], k)?)?;
    }
    for k in [
        "api_binary",
        "release_binary",
        "gateway_binary",
        "token_binary",
        "cli_binary",
        "mapping_binary",
        "coordinator_binary",
    ] {
        binary(text(&c["onboarding"]["product"], k)?)?;
    }
    let out = Path::new(&args[if explicit { 6 } else { 4 }]);
    let mut calls = Calls::new(out)?;
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
    #[test]
    fn identifiers_refuse_sql() {
        assert!(ident("a';DELETE").is_err());
        assert!(ident("value_probe").is_ok());
    }
}
