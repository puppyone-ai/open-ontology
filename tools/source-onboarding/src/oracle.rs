//! Oracle uses its own measured source/operator contract, never a MySQL alias.
use crate::io::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const INPUT_FIELDS: [&str; 7] = [
    "type",
    "host",
    "port",
    "service",
    "database",
    "user",
    "secret_ref",
];

pub fn validate_input(input: &Value) -> Result<()> {
    crate::identity::input_keys(input, &INPUT_FIELDS, "oracle_input_keys")?;
    require(
        input["type"] == "oracle",
        "oracle_input_keys",
    )?;
    require(
        input["host"] == "127.0.0.1"
            && input["port"]
                .as_u64()
                .is_some_and(|port| (1..=65535).contains(&port)),
        "oracle_local_endpoint",
    )?;
    for key in ["service", "database", "user", "secret_ref"] {
        let value = text(input, key)?;
        require(
            !value.is_empty() && !value.contains('\0'),
            "oracle_input_value",
        )?;
    }
    require(
        text(input, "secret_ref")?.starts_with("secret://"),
        "oracle_secret_ref",
    )
}

pub fn installed_operator(input: &Value, registry_path: &Path) -> Result<PathBuf> {
    validate_input(input)?;
    if let Some(path) = crate::identity::installed(input, registry_path, "oracle")? {
        return Ok(path);
    }
    let registry = load(private(registry_path)?)?;
    require(
        registry["protocol"] == "oo.oracle.source-operators/v1",
        "oracle_registry",
    )?;
    let mut found = Vec::new();
    for entry in array(&registry, "operators")? {
        let path = private(entry.as_str().ok_or("oracle_operator_path")?)?;
        let value = load(&path)?;
        if INPUT_FIELDS.iter().all(|key| input[*key] == value[*key]) {
            found.push(path);
        }
    }
    require(found.len() == 1, "oracle_operator_unknown_or_ambiguous")?;
    Ok(found.remove(0))
}

fn pin(reference: &Value, secret: bool) -> Result<PathBuf> {
    let path = PathBuf::from(text(reference, "path")?);
    let expected = text(reference, "sha256")?;
    require(
        path.is_absolute()
            && !path.components().any(|p| p == Component::ParentDir)
            && expected.len() == 64
            && expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "oracle_pin",
    )?;
    if secret {
        private(&path)?;
        require(
            fs::metadata(&path)?.len() <= 32 * 1024 * 1024,
            "oracle_document_budget",
        )?;
    }
    require(
        fs::symlink_metadata(&path)?.file_type().is_file() && digest(&path)? == expected,
        "oracle_pin_drift",
    )?;
    Ok(path)
}

fn source(input: &Value, operator: &Value) -> Result<Value> {
    validate_input(input)?;
    crate::identity::validate_operator(input, operator, "oracle")?;
    require(
        operator["protocol"] == "oo.oracle.source-operator/v1"
            && INPUT_FIELDS.iter().all(|key| operator[*key] == input[*key]),
        "oracle_operator_binding",
    )?;
    for key in ["source_id", "owner"] {
        let value = text(operator, key)?;
        require(
            !value.is_empty() && value.len() <= 256 && !value.contains('\0'),
            "oracle_source_config",
        )?;
    }
    let password = private(text(operator, "password_file")?)?;
    let registry = load(private(text(operator, "secret_registry")?)?)?;
    require(
        registry[text(input, "secret_ref")?] == operator["password_file"],
        "oracle_secret_binding",
    )?;
    let root = Path::new(text(operator, "host_secret_root")?);
    require(
        root.is_absolute()
            && fs::symlink_metadata(root)?.is_dir()
            && !fs::symlink_metadata(root)?.file_type().is_symlink()
            && password.canonicalize()?.starts_with(root.canonicalize()?),
        "oracle_secret_root",
    )?;
    let identity = text(operator, "identity")?;
    let parts: Vec<_> = identity.split('|').collect();
    require(
        parts.len() == 6
            && parts[0].bytes().all(|b| b.is_ascii_digit())
            && !parts[0].is_empty()
            && !parts[1].is_empty()
            && parts[1].bytes().all(|b| b.is_ascii_digit())
            && parts[2].len() == 32
            && parts[2]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
            && parts[4] == text(input, "database")?
            && parts[5] == text(input, "user")?
            && operator["source_identity"]
                == json!({
                    "engine":"oracle","dbid":parts[0],"resetlogs_change":parts[1],
                    "pdb_guid":parts[2],"pdb_name":parts[3],"db_unique_name":parts[4]
                }),
        "oracle_database_identity",
    )?;
    let tables = array(operator, "authorized_tables")?;
    require(
        !tables.is_empty()
            && tables.len() <= 4096
            && tables.iter().all(|t| {
                t.as_str()
                    .is_some_and(|t| !t.is_empty() && !t.contains('\0'))
            })
            && tables
                .iter()
                .map(|t| t.as_str().unwrap())
                .collect::<BTreeSet<_>>()
                .len()
                == tables.len(),
        "oracle_authorized_tables",
    )?;
    let mut config = json!({});
    for key in [
        "host",
        "port",
        "service",
        "user",
        "owner",
        "source_id",
        "identity",
        "password_file",
    ] {
        require(!operator[key].is_null(), "oracle_source_config")?;
        config[key] = operator[key].clone();
    }
    Ok(config)
}

pub(crate) fn identity_source(input: &Value, operator: &Value) -> Result<()> {
    source(input, operator).map(|_| ())
}

fn validate_inventory(actual: &Value, operator: &Value) -> Result<()> {
    let names = array(actual, "tables")?
        .iter()
        .map(|t| Ok(format!("{}.{}", text(t, "owner")?, text(t, "table")?)))
        .collect::<Result<BTreeSet<_>>>()?;
    let allowed = array(operator, "authorized_tables")?
        .iter()
        .map(|t| t.as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    let empty = serde_json::Map::new();
    let exclusions = match operator.get("inventory_exclusions") {
        Some(value) => value.as_object().ok_or("oracle_inventory_exclusions")?,
        None => &empty,
    };
    let excluded = exclusions.keys().cloned().collect::<BTreeSet<_>>();
    require(
        exclusions
            .values()
            .all(|reason| reason.as_str().is_some_and(|s| !s.trim().is_empty()))
            && excluded.is_disjoint(&allowed)
            && excluded.is_subset(&names),
        "oracle_inventory_exclusions",
    )?;
    require(
        actual["protocol"] == "oo.oracle.inventory/v1"
            && actual["identity"] == operator["identity"]
            && actual["source_id"] == operator["source_id"]
            && names
                .difference(&excluded)
                .cloned()
                .collect::<BTreeSet<_>>()
                == allowed
            && names.len() == array(actual, "tables")?.len(),
        "oracle_source_inventory_binding",
    )
}

fn measure(input: &Value, operator: &Value, calls: &mut Calls) -> Result<Value> {
    let config = source(input, operator)?;
    let reader = pin(&operator["reader"], false)?;
    let raw = calls.run(
        "oracle-source-inventory",
        &[reader.to_string_lossy().into_owned()],
        Some(&bytes(&json!({"action":"inventory","config":config}))?),
    )?;
    let actual: Value = serde_json::from_slice(&raw)?;
    validate_inventory(&actual, operator)?;
    crate::identity::oracle_inventory(operator, &actual)?;
    save(calls.out.join("source-inventory.json"), &actual)?;
    Ok(json!({"protocol":"oo.oracle.onboarding-source-check/v1",
        "source_id":operator["source_id"],"source_identity":operator["source_identity"],
        "input_sha256":sha(&bytes(input)?),"operator_sha256":sha(&bytes(operator)?),
        "inventory_sha256":sha(&raw),"reader_sha256":operator["reader"]["sha256"],
        "authorized_tables":operator["authorized_tables"],
        "inventory_exclusions":operator.get("inventory_exclusions").cloned().unwrap_or(json!({})),
        "business_rows_read":0,"target_registered":false,"admitted":false,"published":false}))
}

pub fn check(input: &Value, operator_path: &Path, out: &Path) -> Result<Value> {
    let operator = load(private(operator_path)?)?;
    source(input, &operator)?;
    require(!out.exists(), "oracle_new_output_required")?;
    let mut calls = Calls::new(out)?;
    calls.deadlines.default = Duration::from_secs(60);
    let receipt = measure(input, &operator, &mut calls)?;
    save(out.join("source-check.json"), &receipt)?;
    Ok(receipt)
}

fn runtime_binding(
    input: &Value,
    operator_path: &Path,
    runtime: &Value,
) -> Result<(Value, Value, Value)> {
    require(
        runtime["protocol"] == "oo.oracle.native-operator/v1",
        "oracle_runtime_protocol",
    )?;
    let pinned_source = pin(&runtime["source_operator"], true)?;
    require(
        pinned_source.canonicalize()? == private(operator_path)?.canonicalize()?,
        "oracle_runtime_source_operator",
    )?;
    let operator = load(pinned_source)?;
    source(input, &operator)?;
    pin(&runtime["worker"], false)?;
    pin(&runtime["worker_config"], true)?;
    let manifest = load(pin(&runtime["manifest"], true)?)?;
    crate::identity::runtime(&operator, &[runtime, &manifest])?;
    let producer_path = pin(&runtime["producer_config"], true)?;
    let producer = load(&producer_path)?;
    let args = array(&manifest["producer"], "args")?;
    require(
        manifest["protocol"] == "oo.jobs.native-schedule/v1"
            && manifest["method"] == "C1"
            && manifest["c1_baseline_format"] == "typed-keys/v3"
            && manifest["source_id"] == operator["source_id"]
            && manifest["loader"]["args"] == json!(["import-native-c1-baseline-v3"])
            && crate::is_uuid(text(&manifest, "binding")?)
            && crate::is_uuid(text(&manifest, "project_id")?),
        "oracle_runtime_manifest",
    )?;
    require(
        args.len() == 5
            && args[0] == "c1-baseline-stream-v3"
            && args[1] == "--config"
            && args[2].as_str() == producer_path.to_str()
            && args[3] == "--out"
            && producer["protocol"] == "oo.oracle.producer-argv/v1"
            && producer["engine"] == "oracle",
        "oracle_runtime_producer",
    )?;
    pin(&manifest["producer"], false)?;
    pin(&manifest["loader"], false)?;
    let registration = &manifest["registration"];
    pin(
        &json!({"path":registration["admission_binary"],
        "sha256":registration["admission_sha256"]}),
        false,
    )?;
    let bundle = load(pin(&producer["bundle"], true)?)?;
    let index = load(pin(&producer["buffer_index"], true)?)?;
    let context = load(pin(&index["context"], true)?)?;
    let selected = array(runtime, "capture_tables")?
        .iter()
        .map(|name| name.as_str().ok_or_else(|| "oracle_capture_tables".into()))
        .collect::<Result<BTreeSet<_>>>()?;
    let tables = bundle["typed_tables"]
        .as_object()
        .ok_or("oracle_capture_tables")?
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let allowed = array(&operator, "authorized_tables")?
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    require(
        !selected.is_empty()
            && selected == tables
            && selected.is_subset(&allowed)
            && selected.len() == array(runtime, "capture_tables")?.len()
            && bundle["engine"] == "oracle"
            && bundle["source_id"] == operator["source_id"]
            && context["protocol"] == "oo.c1.oracle-buffer-context/v1"
            && context["source_identity"] == operator["source_identity"]
            && context["source_id"] == operator["source_id"]
            && context["bundle"] == producer["bundle"]
            && index["protocol"] == "oo.oracle.buffer-index/v1"
            && index["final_cursor"].is_object()
            && index["final_cursor"]["transaction_ordinal"]
                .as_str()
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            && context["source_resume"]["target_ack_updates_vendor_offset"] == false,
        "oracle_capture_scope",
    )?;
    let buffer = json!({
        "context_sha256":index["context"]["sha256"],
        "bundle_sha256":producer["bundle"]["sha256"],
        "final_cursor":index["final_cursor"],
        "source_identity":operator["source_identity"],
        "checkpoint_kind":"buffer_consumption",
        "source_offset_write":false
    });
    Ok((operator, manifest, buffer))
}

fn worker(runtime: &Value, calls: &mut Calls, args: &[String], deadline: Instant) -> Result<Value> {
    let binary = pin(&runtime["worker"], false)?;
    let config = pin(&runtime["worker_config"], true)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    require(!remaining.is_zero(), "oracle_capture_deadline")?;
    calls.deadlines.default = remaining;
    let mut command = vec![
        binary.to_string_lossy().into_owned(),
        "--config".into(),
        config.to_string_lossy().into_owned(),
    ];
    command.extend_from_slice(args);
    let raw = calls.run(&args[0], &command, None)?;
    if matches!(args[0].as_str(), "native-state" | "native-artifacts") {
        Ok(serde_json::from_slice(&raw)?)
    } else {
        Ok(json!({"stdout_sha256":sha(&raw)}))
    }
}

fn exported_artifact(exported: &Value, root: &Path, name: &str) -> Result<Value> {
    let path = private(root.join(format!("{name}.json")))?;
    require(
        fs::metadata(&path)?.len() <= 32 * 1024 * 1024,
        "oracle_document_budget",
    )?;
    let raw = fs::read(path)?;
    require(
        exported["sha256"][name] == sha(&raw),
        "oracle_export_digest",
    )?;
    Ok(serde_json::from_slice(&raw)?)
}

pub fn capture(
    input: &Value,
    operator_path: &Path,
    runtime_path: &Path,
    out: &Path,
) -> Result<Value> {
    let runtime = load(private(runtime_path)?)?;
    let (operator, manifest, requested_buffer) = runtime_binding(input, operator_path, &runtime)?;
    require(!out.exists(), "oracle_new_output_required")?;
    let seconds = match runtime.get("timeout_seconds") {
        Some(value) => value
            .as_u64()
            .filter(|v| (1..=43200).contains(v))
            .ok_or("oracle_capture_deadline")?,
        None => 1800,
    };
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let lock_path = runtime_path.with_extension("capture.lock");
    if lock_path.exists() {
        private(&lock_path)?;
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path)?;
    lock.try_lock()?;
    let mut calls = Calls::new(out)?;
    calls.deadlines.default =
        Duration::from_secs(60).min(deadline.saturating_duration_since(Instant::now()));
    let checked = measure(input, &operator, &mut calls)?;
    save(out.join("source-check.json"), &checked)?;
    save(
        out.join("input-binding.json"),
        &json!({"runtime_sha256":digest(runtime_path)?,
        "source_operator_sha256":digest(operator_path)?,"source_id":operator["source_id"],
        "manifest_sha256":runtime["manifest"]["sha256"],"worker_sha256":runtime["worker"]["sha256"]}),
    )?;
    let binding = text(&manifest, "binding")?;
    let state = crate::state_or_fresh(
        worker(
            &runtime,
            &mut calls,
            &strings(&["native-state", "--binding", binding]),
            deadline,
        ),
        binding,
    )?;
    if state.get("protocol").is_some() {
        require(
            state["protocol"] == "oo.jobs.native-state/v1"
                && state["binding"] == binding
                && state["source_id"] == manifest["source_id"]
                && state["method"] == "C1"
                && state["applied"].is_boolean(),
            "oracle_capture_state",
        )?;
    }
    let outcome = (|| -> Result<Value> {
        if state["applied"] != true {
            // Re-registering a completed binding would re-enable its scheduler.
            worker(
                &runtime,
                &mut calls,
                &strings(&[
                    "native-put",
                    "--manifest",
                    text(&runtime["manifest"], "path")?,
                ]),
                deadline,
            )?;
            let project = text(&manifest, "project_id")?;
            worker(
                &runtime,
                &mut calls,
                &strings(&["native-fire", "--project", project]),
                deadline,
            )?;
            worker(
                &runtime,
                &mut calls,
                &strings(&[
                    "native-step",
                    "--project",
                    project,
                    "--worker",
                    "oracle-source-onboarding",
                ]),
                deadline,
            )?;
        }
        let state = worker(
            &runtime,
            &mut calls,
            &strings(&["native-state", "--binding", binding]),
            deadline,
        )?;
        crate::captured_state(
            &json!({"source_id":manifest["source_id"],"binding_id":binding}),
            &state,
        )?;
        let artifacts = out.join("native-artifacts");
        let exported = worker(
            &runtime,
            &mut calls,
            &strings(&[
                "native-artifacts",
                "--binding",
                binding,
                "--out",
                artifacts.to_str().ok_or("oracle_artifact_path")?,
            ]),
            deadline,
        )?;
        require(
            exported["protocol"] == "oo.jobs.native-artifacts/v1"
                && exported["binding"] == binding
                && exported["source_id"] == state["source_id"]
                && exported["source_generation"] == state["source_generation"],
            "oracle_export_binding",
        )?;
        let loaded = exported_artifact(&exported, &artifacts, "load")?;
        let ack = exported_artifact(&exported, &artifacts, "ack")?;
        let request = exported_artifact(&exported, &artifacts, "request")?;
        require(
            ack["acknowledged"] == true
                && ack["task_state"] == "succeeded"
                && ack["receipt"] == loaded
                && loaded["source_instance"]["engine"] == "oracle"
                && ["source_id", "source_generation"].iter().all(|key| {
                    loaded[*key] == state[*key]
                        && ack[*key] == state[*key]
                        && request[*key] == state[*key]
                }),
            "oracle_target_ack_binding",
        )?;
        require(
            loaded["position_codec"] == "oracle-capture-sequence/v1"
                && request["baseline_scope"]["buffer"] == loaded["oracle_buffer"]
                && requested_buffer
                    .as_object()
                    .unwrap()
                    .iter()
                    .all(|(key, value)| loaded["oracle_buffer"][key] == *value)
                && loaded["applied_pos"] == requested_buffer["final_cursor"]["transaction_ordinal"]
                && loaded["applied_pos"] == state["applied_pos"],
            "oracle_requested_buffer_drift",
        )?;
        Ok(
            json!({"protocol":"oo.oracle.connection-capture/v1","source_id":manifest["source_id"],
            "native_state":state,"worker_export":exported,"target_ack_verified":true,
            "requested_buffer_verified":true,
            "source_checkpoint":"immutable captured buffer; vendor-offset advancement is not claimed",
            "semantic_release":false,"full_product_chain_complete":false,
            "artifacts":{ "request_sha256":digest(artifacts.join("request.json"))?,
                "load_sha256":digest(artifacts.join("load.json"))?,"ack_sha256":digest(artifacts.join("ack.json"))?}}),
        )
    })();
    let cleanup = worker(
        &runtime,
        &mut calls,
        &strings(&["native-enable", "--binding", binding, "--enabled", "false"]),
        Instant::now() + Duration::from_secs(10),
    );
    save(
        out.join("completion.json"),
        &json!({"capture_succeeded":outcome.is_ok(),
        "schedule_disabled":cleanup.is_ok(),"error":outcome.as_ref().err().map(|e| e.to_string()),
        "cleanup_error":cleanup.as_ref().err().map(|e| e.to_string()),"unconfirmed_commit_recovery":"not_inferred"}),
    )?;
    let mut receipt = outcome?;
    cleanup?;
    receipt["schedule_disabled"] = json!(true);
    save(out.join("capture-receipt.json"), &receipt)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "oo-oracle-entry-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            dir(&root).unwrap();
            let value = Self(root);
            let password = value.0.join("reader.pass");
            put(&password, b"synthetic-never-output").unwrap();
            save(
                value.0.join("secrets.json"),
                &json!({"secret://owned/oracle":password}),
            )
            .unwrap();
            let identity = "123|45|00112233445566778899AABBCCDDEEFF|PDB|OWNED|READER";
            let inventory = json!({"protocol":"oo.oracle.inventory/v1","identity":identity,"source_id":"owned_oracle",
                "tables":[{"owner":"APP","table":"ITEMS"}]});
            let reader = value.binary(&format!(
                "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{}'\n",
                inventory
            ));
            let input = value.input();
            let mut operator = input.clone();
            for (key, field) in
                json!({"protocol":"oo.oracle.source-operator/v1","source_id":"owned_oracle",
                "owner":"APP","identity":identity,"source_identity":{"engine":"oracle","dbid":"123",
                "resetlogs_change":"45","pdb_guid":"00112233445566778899AABBCCDDEEFF",
                "pdb_name":"PDB","db_unique_name":"OWNED"},"reader":reader,"password_file":password,
                "host_secret_root":value.0,"secret_registry":value.0.join("secrets.json"),
                "authorized_tables":["APP.ITEMS"]})
                .as_object()
                .unwrap()
            {
                operator[key] = field.clone();
            }
            save(value.0.join("operator.json"), &operator).unwrap();
            value
        }
        fn input(&self) -> Value {
            json!({"type":"oracle","host":"127.0.0.1","port":1521,"service":"PDB",
                "database":"OWNED","user":"READER","secret_ref":"secret://owned/oracle"})
        }
        fn binary(&self, body: &str) -> Value {
            let root = self.0.join(sha(body.as_bytes()));
            dir(&root).unwrap();
            let path = root.join("fixture");
            put(&path, body.as_bytes()).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            json!({"path":path,"sha256":sha(body.as_bytes())})
        }
        fn reference(&self, name: &str) -> Value {
            let path = self.0.join(name);
            json!({"path":path,"sha256":digest(&path).unwrap()})
        }
        fn runtime(&self) -> PathBuf {
            let operator = load(self.0.join("operator.json")).unwrap();
            let binding = "11111111-1111-4111-8111-111111111111";
            let state = json!({"protocol":"oo.jobs.native-state/v1","binding":binding,"source_id":"owned_oracle",
                "method":"C1","applied":true,"source_generation":1,"enabled":false,"applied_pos":"0"});
            save(self.0.join("state.json"), &state).unwrap();
            save(self.0.join("completed-state.json"), &state).unwrap();
            let mut pending = state;
            pending["applied"] = json!(false);
            save(self.0.join("pending-state.json"), &pending).unwrap();
            let worker = self.binary(&format!(
                r#"#!/bin/sh
printf '%s\n' "$3" >> '{root}/commands'
case "$3" in
native-state)
if [ -f '{root}/state-error' ]; then cat '{root}/state-error' >&2; exit 1; fi
if [ ! -f '{root}/state.json' ]; then printf 'error=binding\n' >&2; exit 1; fi
cat '{root}/state.json';;
native-put) cp '{root}/pending-state.json' '{root}/state.json';;
native-step) cp '{root}/completed-state.json' '{root}/state.json';;
native-artifacts)
mkdir -m 700 "$7" || exit 2
for n in request load ack; do cp '{root}/'"$n.json" "$7/$n.json" || exit 2; done
cat '{root}/export.json';;
*) printf '{{}}\n';;
esac
"#,
                root = self.0.display()
            ));
            put(self.0.join("worker.cfg"), b"synthetic-only").unwrap();
            save(
                self.0.join("bundle.json"),
                &json!({"engine":"oracle","source_id":"owned_oracle",
                "typed_tables":{"APP.ITEMS":{}}}),
            )
            .unwrap();
            save(self.0.join("context.json"), &json!({"protocol":"oo.c1.oracle-buffer-context/v1",
                "source_id":"owned_oracle","source_identity":operator["source_identity"],
                "bundle":self.reference("bundle.json"),"source_resume":{"target_ack_updates_vendor_offset":false}})).unwrap();
            save(
                self.0.join("index.json"),
                &json!({"protocol":"oo.oracle.buffer-index/v1","context":self.reference("context.json"),
                    "final_cursor":{"transaction_id":null,"transaction_ordinal":"0"}}),
            )
            .unwrap();
            save(self.0.join("producer.json"), &json!({"protocol":"oo.oracle.producer-argv/v1","engine":"oracle",
                "bundle":self.reference("bundle.json"),"buffer_index":self.reference("index.json")})).unwrap();
            let mut producer = operator["reader"].clone();
            producer["args"] = json!([
                "c1-baseline-stream-v3",
                "--config",
                self.0.join("producer.json"),
                "--out",
                self.0.join("capture-{task}")
            ]);
            let mut loader = worker.clone();
            loader["args"] = json!(["import-native-c1-baseline-v3"]);
            save(self.0.join("manifest.json"), &json!({"protocol":"oo.jobs.native-schedule/v1","method":"C1",
                "c1_baseline_format":"typed-keys/v3","source_id":"owned_oracle","binding":binding,"project_id":binding,
                "producer":producer,"loader":loader,"registration":{"admission_binary":worker["path"],
                "admission_sha256":worker["sha256"]}})).unwrap();
            let runtime = json!({"protocol":"oo.oracle.native-operator/v1","source_operator":self.reference("operator.json"),
                "worker":worker,"worker_config":self.reference("worker.cfg"),"manifest":self.reference("manifest.json"),
                "producer_config":self.reference("producer.json"),"capture_tables":["APP.ITEMS"],"timeout_seconds":20});
            save(self.0.join("runtime.json"), &runtime).unwrap();
            let (_, _, buffer) =
                runtime_binding(&self.input(), &self.0.join("operator.json"), &runtime).unwrap();
            let loaded = json!({"source_id":"owned_oracle","source_generation":1,
                "source_instance":{"engine":"oracle"},"position_codec":"oracle-capture-sequence/v1",
                "applied_pos":"0","oracle_buffer":buffer});
            for (name, value) in [
                ("load.json", loaded.clone()),
                (
                    "request.json",
                    json!({"source_id":"owned_oracle","source_generation":1,
                    "baseline_scope":{"buffer":buffer}}),
                ),
                (
                    "ack.json",
                    json!({"acknowledged":true,"task_state":"succeeded","receipt":loaded,
                    "source_id":"owned_oracle","source_generation":1}),
                ),
            ] {
                save(self.0.join(name), &value).unwrap();
            }
            save(
                self.0.join("export.json"),
                &json!({"protocol":"oo.jobs.native-artifacts/v1",
                "binding":binding,"source_id":"owned_oracle","source_generation":1,
                "sha256":{"load":digest(self.0.join("load.json")).unwrap(),
                    "ack":digest(self.0.join("ack.json")).unwrap(),
                    "request":digest(self.0.join("request.json")).unwrap()}}),
            )
            .unwrap();
            self.0.join("runtime.json")
        }
    }

    #[test]
    fn seven_fields_distinguish_service_database_and_mysql_before_any_command() {
        let f = Fixture::new();
        let input = f.input();
        let op = load(f.0.join("operator.json")).unwrap();
        source(&input, &op).unwrap();
        for (key, value) in [
            ("type", json!("mysql")),
            ("service", json!("OWNED")),
            ("database", json!("PDB")),
            ("host", json!("customer.invalid")),
            ("port", json!(0)),
        ] {
            let mut bad = input.clone();
            bad[key] = value;
            assert!(source(&bad, &op).is_err(), "{key}");
        }
        let mut bad = input;
        bad["password"] = json!("not-allowed");
        assert!(validate_input(&bad).is_err());
    }

    #[test]
    fn measured_check_never_claims_target_admission_or_reads_secret_into_output() {
        let f = Fixture::new();
        let out = f.0.join("out");
        let result = check(&f.input(), &f.0.join("operator.json"), &out).unwrap();
        assert_eq!(result["source_id"], "owned_oracle");
        assert_eq!(result["target_registered"], false);
        assert_eq!(result["admitted"], false);
        assert_eq!(result["business_rows_read"], 0);
        for file in fs::read_dir(&out).unwrap() {
            let path = file.unwrap().path();
            if path.is_file() {
                assert!(
                    !fs::read_to_string(path)
                        .unwrap()
                        .contains("synthetic-never-output")
                );
            }
        }
        assert!(check(&f.input(), &f.0.join("operator.json"), &out).is_err());
    }

    #[test]
    fn registry_pin_secret_alias_and_inventory_tampering_are_refused() {
        let f = Fixture::new();
        let operator = f.0.join("operator.json");
        let registry = f.0.join("registry.json");
        save(
            &registry,
            &json!({"protocol":"oo.oracle.source-operators/v1","operators":[operator]}),
        )
        .unwrap();
        assert_eq!(installed_operator(&f.input(), &registry).unwrap(), operator);
        fs::write(
            &registry,
            bytes(&json!({"protocol":"oo.oracle.source-operators/v1",
            "operators":[operator,operator]}))
            .unwrap(),
        )
        .unwrap();
        assert!(installed_operator(&f.input(), &registry).is_err());
        let link = f.0.join("alias");
        symlink(f.0.join("reader.pass"), &link).unwrap();
        let mut op = load(&operator).unwrap();
        op["password_file"] = json!(link);
        assert!(source(&f.input(), &op).is_err());
        op = load(&operator).unwrap();
        op["authorized_tables"] = json!(["APP.OTHER"]);
        let wrong = f.0.join("wrong.json");
        save(&wrong, &op).unwrap();
        assert!(check(&f.input(), &wrong, &f.0.join("wrong-out")).is_err());
    }

    #[test]
    fn visible_inventory_requires_exact_authorized_and_explicit_excluded_partition() {
        let f = Fixture::new();
        let mut operator = load(f.0.join("operator.json")).unwrap();
        let actual = json!({"protocol":"oo.oracle.inventory/v1","identity":operator["identity"],
            "source_id":operator["source_id"],
            "tables":[{"owner":"APP","table":"ITEMS"},{"owner":"APP","table":"EXTRA"}]});
        assert!(validate_inventory(&actual, &operator).is_err());
        operator["inventory_exclusions"] =
            json!({"APP.EXTRA":"outside this retained-buffer scope"});
        validate_inventory(&actual, &operator).unwrap();
        for invalid in [
            json!(null),
            json!([]),
            json!({"APP.EXTRA":""}),
            json!({"APP.EXTRA":"  "}),
            json!({"APP.EXTRA":true}),
            json!({"APP.MISSING":"not actually inventoried"}),
            json!({"APP.EXTRA":"out of scope","APP.ITEMS":"cannot exclude authorized table"}),
        ] {
            operator["inventory_exclusions"] = invalid;
            assert!(validate_inventory(&actual, &operator).is_err());
        }
        operator["inventory_exclusions"] = json!({"APP.EXTRA":"out of scope"});
        let mut duplicate = actual.clone();
        duplicate["tables"]
            .as_array_mut()
            .unwrap()
            .push(actual["tables"][1].clone());
        assert!(validate_inventory(&duplicate, &operator).is_err());
        let mut missing = actual.clone();
        missing["tables"].as_array_mut().unwrap().remove(0);
        assert!(validate_inventory(&missing, &operator).is_err());
    }

    #[test]
    fn registered_capture_replays_worker_state_but_requires_real_engine_ack_shape() {
        let f = Fixture::new();
        let runtime = f.runtime();
        let result = capture(
            &f.input(),
            &f.0.join("operator.json"),
            &runtime,
            &f.0.join("out"),
        )
        .unwrap();
        assert_eq!(result["target_ack_verified"], true);
        assert_eq!(result["requested_buffer_verified"], true);
        assert_eq!(result["schedule_disabled"], true);
        assert_eq!(result["full_product_chain_complete"], false);
        assert_eq!(result["native_state"]["enabled"], false);
        let calls = fs::read_to_string(f.0.join("commands")).unwrap();
        assert!(
            !calls.contains("native-step")
                && !calls.contains("native-fire")
                && !calls.contains("native-put")
        );
        let mut ack = load(f.0.join("ack.json")).unwrap();
        ack["receipt"]["source_instance"]["engine"] = json!("mysql");
        fs::write(f.0.join("ack.json"), bytes(&ack).unwrap()).unwrap();
        assert!(
            capture(
                &f.input(),
                &f.0.join("operator.json"),
                &runtime,
                &f.0.join("bad-out")
            )
            .is_err()
        );
        assert_eq!(
            load(f.0.join("bad-out/completion.json")).unwrap()["schedule_disabled"],
            true
        );
    }

    #[test]
    fn completed_task_cannot_satisfy_a_different_requested_buffer() {
        let f = Fixture::new();
        let runtime_path = f.runtime();
        let mut bundle = load(f.0.join("bundle.json")).unwrap();
        bundle["new_captured_cut"] = json!(true);
        fs::write(f.0.join("bundle.json"), bytes(&bundle).unwrap()).unwrap();
        let mut context = load(f.0.join("context.json")).unwrap();
        context["bundle"] = f.reference("bundle.json");
        fs::write(f.0.join("context.json"), bytes(&context).unwrap()).unwrap();
        let mut index = load(f.0.join("index.json")).unwrap();
        index["context"] = f.reference("context.json");
        fs::write(f.0.join("index.json"), bytes(&index).unwrap()).unwrap();
        let mut producer = load(f.0.join("producer.json")).unwrap();
        producer["bundle"] = f.reference("bundle.json");
        producer["buffer_index"] = f.reference("index.json");
        fs::write(f.0.join("producer.json"), bytes(&producer).unwrap()).unwrap();
        let mut runtime = load(&runtime_path).unwrap();
        runtime["producer_config"] = f.reference("producer.json");
        fs::write(&runtime_path, bytes(&runtime).unwrap()).unwrap();
        let error = capture(
            &f.input(),
            &f.0.join("operator.json"),
            &runtime_path,
            &f.0.join("out"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "oracle_requested_buffer_drift");
        assert!(!f.0.join("out/capture-receipt.json").exists());
        let calls = fs::read_to_string(f.0.join("commands")).unwrap();
        assert!(
            !calls.contains("native-put")
                && !calls.contains("native-fire")
                && !calls.contains("native-step")
        );
        assert_eq!(
            load(f.0.join("out/completion.json")).unwrap()["schedule_disabled"],
            true
        );
    }

    #[test]
    fn fresh_capture_registers_but_unknown_state_failure_never_reenables() {
        let f = Fixture::new();
        let runtime = f.runtime();
        fs::remove_file(f.0.join("state.json")).unwrap();
        capture(
            &f.input(),
            &f.0.join("operator.json"),
            &runtime,
            &f.0.join("fresh"),
        )
        .unwrap();
        let before = fs::read_to_string(f.0.join("commands")).unwrap();
        assert!(
            before.contains("native-put")
                && before.contains("native-fire")
                && before.contains("native-step")
        );
        put(
            f.0.join("state-error"),
            b"error=sql authentication refused\n",
        )
        .unwrap();
        assert!(
            capture(
                &f.input(),
                &f.0.join("operator.json"),
                &runtime,
                &f.0.join("refused")
            )
            .is_err()
        );
        let after = fs::read_to_string(f.0.join("commands")).unwrap();
        assert_eq!(&after[before.len()..], "native-state\n");
    }

    #[test]
    fn producer_scope_or_runtime_pin_drift_fails_before_worker_dispatch() {
        let f = Fixture::new();
        let path = f.runtime();
        let mut runtime = load(&path).unwrap();
        runtime["capture_tables"] = json!(["APP.OTHER"]);
        assert!(runtime_binding(&f.input(), &f.0.join("operator.json"), &runtime).is_err());
        runtime = load(&path).unwrap();
        runtime["source_operator"]["sha256"] = json!("0".repeat(64));
        assert!(runtime_binding(&f.input(), &f.0.join("operator.json"), &runtime).is_err());
        runtime = load(&path).unwrap();
        fs::write(f.0.join("manifest.json"), b"{}").unwrap();
        assert!(runtime_binding(&f.input(), &f.0.join("operator.json"), &runtime).is_err());
        assert!(!f.0.join("commands").exists());
    }
}
