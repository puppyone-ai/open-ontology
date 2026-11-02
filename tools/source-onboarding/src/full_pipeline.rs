use crate::{io::*, mysql};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn identity(value: &[u8]) -> String {
    let h = sha(value);
    format!(
        "{}-{}-4{}-8{}-{}",
        &h[..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    )
}

pub fn runtime(
    c: &Value,
    calls: &mut Calls,
    label: &str,
    args: &[String],
    input: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let o = &c["onboarding"];
    match o["full"]["pipeline"]["runtime_transport"].as_str() {
        None | Some("container") => calls.docker(label, text(o, "container")?, args, input),
        Some("host") => calls.run(label, args, input),
        _ => Err("full_runtime_transport".into()),
    }
}

fn worker(c: &Value, calls: &mut Calls, args: &[String]) -> Result<Vec<u8>> {
    let o = &c["onboarding"];
    let mut command = strings(&[text(o, "worker")?, "--config", text(o, "worker_config")?]);
    command.extend_from_slice(args);
    runtime(c, calls, &args[0], &command, None)
}

fn cancel_failed(c: &Value, calls: &mut Calls, binding: &str) -> Result<()> {
    let pipeline = &c["onboarding"]["full"]["pipeline"];
    let sql = format!(
        "SELECT json_build_object('task',j.id,'generation',j.generation) FROM jobs.tasks j JOIN jw_native.occurrences o ON o.task_id=j.id WHERE o.binding_id='{binding}'::uuid AND j.state='queued' ORDER BY o.pin DESC LIMIT 1"
    );
    let raw = calls.docker(
        "full-failed-occurrence",
        text(pipeline, "control_container")?,
        &strings(&[
            "psql",
            "-X",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            text(pipeline, "control_user")?,
            "-d",
            text(&pipeline["target"], "database")?,
            "-Atc",
            &sql,
        ]),
        None,
    )?;
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    let task: Value = serde_json::from_slice(&raw)?;
    worker(
        c,
        calls,
        &strings(&[
            "native-cancel",
            "--project",
            binding,
            "--task",
            text(&task, "task")?,
            "--worker",
            "full-batch",
            "--generation",
            &task["generation"].to_string(),
        ]),
    )?;
    worker(
        c,
        calls,
        &strings(&["native-enable", "--binding", binding, "--enabled", "false"]),
    )?;
    Ok(())
}

fn native_runtime_preflight(c: &Value, calls: &mut Calls) -> Result<()> {
    let pipeline = &c["onboarding"]["full"]["pipeline"];
    let request = json!({
        "protocol":"oo.pipeline.native-runtime-preflight/v1",
        "driver":{"path":text(&pipeline["driver"],"path")?,"sha256":text(&pipeline["driver"],"sha256")?},
        "validator":{"path":text(&pipeline["admission"],"path")?,"sha256":text(&pipeline["admission"],"sha256")?}
    });
    save(
        calls.out.join("native-runtime-preflight.request.json"),
        &request,
    )?;
    let raw = runtime(
        c,
        calls,
        "full-native-runtime-preflight",
        &strings(&[text(&pipeline["loader"], "path")?, "check-native-runtime"]),
        Some(&bytes(&request)?),
    )?;
    let result: Value = serde_json::from_slice(&raw)?;
    save(
        calls.out.join("native-runtime-preflight.result.json"),
        &result,
    )?;
    require(
        result["protocol"] == request["protocol"]
            && ["driver", "validator"].iter().all(|role| {
                result[*role]["path"] == request[*role]["path"]
                    && result[*role]["sha256"] == request[*role]["sha256"]
            }),
        "native_runtime_preflight_binding",
    )?;
    if result["compatible"] != true
        || result["reasons"]
            .as_array()
            .is_none_or(|reasons| !reasons.is_empty())
    {
        return Err(format!("native_runtime_incompatible:{}", result["reasons"]).into());
    }
    Ok(())
}

pub fn provision(c: &Value, out: &Path) -> Result<()> {
    let o = &c["onboarding"];
    let pipeline = &o["full"]["pipeline"];
    dir(out)?;
    dir(out.join("provision"))?;
    let root = out.join("provision").join(sha(&bytes(c)?));
    let receipt = root.join("receipt.json");
    dir(&root)?;
    let mut attempt = 1u64;
    while root.join(format!("attempt-{attempt}")).exists() {
        attempt += 1;
    }
    let mut calls = Calls::with_operator(&root.join(format!("attempt-{attempt}")), o)?;
    // Artifact bytes/policy can change even when the provisioning receipt is still pinned.
    native_runtime_preflight(c, &mut calls)?;
    catalog_target_shape(&pipeline["catalog_target"])?;
    if receipt.exists() {
        require(
            load(private(&receipt)?)?["config_sha256"] == sha(&bytes(c)?),
            "full_provision_drift",
        )?;
        return Ok(());
    }
    calls.run(
        "full-jobs-provision",
        &strings(&[
            "python3",
            text(pipeline, "jobs_deploy")?,
            "--container",
            text(pipeline, "control_container")?,
            "--database",
            text(&pipeline["target"], "database")?,
            "--admin",
            text(pipeline, "control_user")?,
        ]),
        None,
    )?;
    let document = json!({"protocol":"oo.pipeline.install-native-update/v1","target":pipeline["catalog_target"]});
    for command in [
        "install-native-update",
        "install-native-mirror-rebind",
        "install-native-claim-selected",
    ] {
        runtime(
            c,
            &mut calls,
            command,
            &strings(&[text(&pipeline["loader"], "path")?, command]),
            Some(&bytes(&document)?),
        )?;
    }
    worker(c, &mut calls, &strings(&["native-install"]))?;
    save(
        receipt,
        &json!({"protocol":"oo.mysql.full-provision/v1","config_sha256":sha(&bytes(c)?),"target":pipeline["target"]["database"]}),
    )
}

fn catalog_target_shape(target: &Value) -> Result<()> {
    let expected = [
        "host", "port", "database", "runtime_principal", "password_file", "ca_file",
    ];
    let fields = target.as_object().ok_or("full_catalog_target_shape")?;
    require(
        fields.len() == expected.len()
            && expected.iter().all(|name| fields.contains_key(*name))
            && expected.iter().filter(|name| **name != "port")
                .all(|name| target[*name].as_str().is_some_and(|value| !value.is_empty()))
            && target["port"].as_u64().is_some_and(|port| (1..=65535).contains(&port)),
        "full_catalog_target_shape",
    )
}

pub fn run_table(c: &Value, manifest: &Value, table: &Value, attempt: &Path) -> Result<Value> {
    let o = &c["onboarding"];
    let pipeline = &o["full"]["pipeline"];
    let config_sha = sha(&bytes(
        &json!({"config":c,"generation":manifest["generation"],"table":table}),
    )?);
    let base = Path::new(text(pipeline, "state_root")?).join(&config_sha);
    dir(&base)?;
    let mut cycle = 0u64;
    while base.join(format!("audit-failed-{cycle}.json")).exists() {
        cycle += 1;
    }
    let root = if cycle == 0 {
        base.clone()
    } else {
        base.join(format!("retry-{cycle}"))
    };
    dir(&root)?;
    let binding = if cycle == 0 {
        identity(config_sha.as_bytes())
    } else {
        identity(format!("{config_sha}:{cycle}").as_bytes())
    };
    let mut capture = pipeline["capture_template"].clone();
    let selected =
        json!({"schema":table["schema"],"table":table["table"],"columns":table["columns"]});
    if let Some(chunking) = table.get("chunking") {
        capture["chunking"] = chunking.clone();
    }
    capture["table"] = selected.clone();
    capture["approved_objects"] = json!([selected]);
    if let Some(budgets) = table.get("budgets") {
        capture["budgets"] = budgets.clone();
    }
    require(
        capture["protocol"] == "oo.mysql-full/v1"
            && capture["source_id"] == o["full"]["source_id"]
            && capture["table"]["schema"] == c["database"],
        "full_capture_binding",
    )?;
    for field in ["host", "port", "database", "user"] {
        require(
            capture["connection"][field] == c[field],
            "full_connection_binding",
        )?;
    }
    require(
        capture["secret_ref"] == c["secret_ref"]
            && (capture["connection"]["secret_file"] == c["password_file"]
                || capture["connection"]["secret_file"]
                    == crate::native_binding(c)?["password_file"]),
        "full_secret_binding",
    )?;
    require(
        pipeline["source_audit_policy"] == "grant_enforced"
            && capture["audit"] == json!({"mode":"grant_enforced"}),
        "full_audit_policy",
    )?;
    save(root.join("capture.json"), &capture)?;
    let producer = json!({"capture_config":root.join("capture.json"),"driver":pipeline["driver"],"admission":pipeline["admission"],"state_dir":root.join("producer-state"),"table":table["table"],"prepare":pipeline["prepare"],"secret_ref":capture["secret_ref"],"source_audit_policy":pipeline["source_audit_policy"],"target":pipeline["target"],"registry_template":format!("{}/registered/{{binding}}/{{pin}}/registry.json",root.display())});
    save(root.join("producer.json"), &producer)?;
    let schedule = json!({"protocol":"oo.jobs.native-schedule/v1","binding":binding,"project_id":binding,"source_id":capture["source_id"],"method":"C5","cycle":"pull","period_ms":1000,"producer":{"path":pipeline["producer_adapter"]["path"],"sha256":pipeline["producer_adapter"]["sha256"],"args":["--config",root.join("producer.json")]},"loader":{"path":pipeline["loader"]["path"],"sha256":pipeline["loader"]["sha256"],"args":["import-native-update"]},"registration":{"admission_binary":pipeline["admission"]["path"],"admission_sha256":pipeline["admission"]["sha256"],"registry_path":producer["registry_template"]}});
    save(root.join("schedule.json"), &schedule)?;
    let mut calls = Calls::with_operator(attempt, o)?;
    worker(
        c,
        &mut calls,
        &strings(&[
            "native-put",
            "--manifest",
            root.join("schedule.json").to_str().ok_or("path")?,
        ]),
    )?;
    let state_args = strings(&["native-state", "--binding", &binding]);
    let mut state: Value = serde_json::from_slice(&worker(c, &mut calls, &state_args)?)?;
    if state["applied"] != true {
        worker(
            c,
            &mut calls,
            &strings(&["native-fire", "--project", &binding]),
        )?;
        let deadline =
            Instant::now() + Duration::from_secs(pipeline["wait_seconds"].as_u64().unwrap_or(120));
        loop {
            if let Err(error) = worker(
                c,
                &mut calls,
                &strings(&[
                    "native-step",
                    "--project",
                    &binding,
                    "--worker",
                    "full-batch",
                ]),
            ) {
                if let Err(cleanup) = cancel_failed(c, &mut calls, &binding) {
                    return Err(format!("{error}; cleanup:{cleanup}").into());
                }
                return Err(error);
            }
            state = serde_json::from_slice(&worker(c, &mut calls, &state_args)?)?;
            if state["applied"] == true {
                break;
            }
            if Instant::now() >= deadline {
                cancel_failed(c, &mut calls, &binding)?;
                return Err("full_worker_pending".into());
            }
            std::thread::sleep(Duration::from_secs(1));
            worker(
                c,
                &mut calls,
                &strings(&["native-fire", "--project", &binding]),
            )?;
        }
    }
    worker(
        c,
        &mut calls,
        &strings(&["native-enable", "--binding", &binding, "--enabled", "false"]),
    )?;
    let sql = format!(
        "SELECT json_build_object('request',o.request,'load',w.result) FROM jw_native.watermarks w JOIN jw_native.occurrences o ON o.task_id=w.task_id JOIN jobs.tasks j ON j.id=w.task_id WHERE w.binding_id='{binding}'::uuid AND j.state='succeeded'"
    );
    let exported = calls.docker(
        "full-confirmed-artifacts",
        text(pipeline, "control_container")?,
        &strings(&[
            "psql",
            "-X",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            text(pipeline, "control_user")?,
            "-d",
            text(&pipeline["target"], "database")?,
            "-Atc",
            &sql,
        ]),
        None,
    )?;
    let exported: Value = serde_json::from_slice(&exported)?;
    let request = &exported["request"];
    let loaded = &exported["load"];
    require(
        request["logical_schema"] == table["schema"]
            && request["logical_table"] == table["table"]
            && loaded["source_id"] == capture["source_id"]
            && loaded["source_bound"] == true,
        "full_worker_artifacts",
    )?;
    let pin = state["pin"].as_u64().ok_or("full_pin")?;
    let registry = root
        .join("registered")
        .join(&binding)
        .join(pin.to_string())
        .join("registry.json");
    let mut catalog_request = json!({"protocol":"oo.pipeline.native-sealed-catalog/v1","run_id":loaded["run_id"],"owner":pipeline["target"]["owner"],"target_generation":loaded["target_generation"],"target":pipeline["catalog_target"]});
    for k in [
        "source_id",
        "source_generation",
        "config_digest",
        "schema_digest",
        "type_digest",
        "input_digest",
        "snapshot",
    ] {
        catalog_request[k] = request[k].clone();
    }
    let args = strings(&[
        "env",
        &format!(
            "OO_NATIVE_ADMISSION_BIN={}",
            text(&pipeline["admission"], "path")?
        ),
        &format!("OO_NATIVE_BINDING_REGISTRY={}", registry.display()),
        &format!("OO_NATIVE_BINDING_REGISTRY_SHA256={}", digest(&registry)?),
        text(&pipeline["loader"], "path")?,
        "catalog-import-native",
    ]);
    let catalog: Value = serde_json::from_slice(&runtime(
        c,
        &mut calls,
        "full-catalog",
        &args,
        Some(&bytes(&catalog_request)?),
    )?)?;
    save(attempt.join("request.json"), request)?;
    save(attempt.join("load.json"), loaded)?;
    copy(
        private(text(request, "admission_path")?)?,
        attempt.join("admission.json"),
    )?;
    let item = json!({"schema":table["schema"],"table":table["table"],"request":attempt.join("request.json"),"load":attempt.join("load.json"),"admission":attempt.join("admission.json"),"target":pipeline["audit_target"]});
    let collection = mysql::collection(&[catalog], std::slice::from_ref(&item))?;
    save(attempt.join("collection.json"), &collection)?;
    let mut config = c.clone();
    config["onboarding"]["full"]["catalog"] = json!(attempt.join("collection.json"));
    config["onboarding"]["full"]["tables"] = json!([item]);
    let verified = attempt.join("verified");
    if let Err(error) = mysql::full(&config, &verified) {
        save(
            base.join(format!("audit-failed-{cycle}.json")),
            &json!({"reason":error.to_string(),"restart_table":true}),
        )?;
        save(
            attempt.join("comparison-failure.json"),
            &json!({"reason":error.to_string(),"restart_table":true}),
        )?;
        return Err(error);
    }
    save(attempt.join("worker-state.json"), &state)?;
    let parts = request["multipart_scope"]["part_count"]
        .as_u64()
        .unwrap_or(1);
    let bytes = if request.get("multipart_scope").is_some() {
        let directory = Path::new(text(request, "arrow_path")?)
            .parent()
            .ok_or("part_path")?;
        (0..parts).try_fold(0u64, |sum, index| {
            Ok::<_, Box<dyn std::error::Error>>(
                sum + std::fs::metadata(directory.join(format!("{index}.arrow")))?.len(),
            )
        })?
    } else {
        std::fs::metadata(text(request, "arrow_path")?)?.len()
    };
    Ok(
        json!({"rows":loaded["row_count"],"parts":parts,"chunks":parts,"bytes":bytes,"comparison":"equal"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    #[test]
    fn catalog_target_rejects_loader_only_fields_before_source_work() {
        let target = json!({"host":"localhost","port":5432,"database":"fixture",
            "runtime_principal":"postgres","password_file":"/private/password","ca_file":"/private/ca"});
        assert!(catalog_target_shape(&target).is_ok());
        for field in ["shard", "owner", "ssl", "password"] {
            let mut invalid = target.clone();
            invalid[field] = json!("not-a-catalog-field");
            assert!(catalog_target_shape(&invalid).is_err(), "{field}");
        }
        let mut missing = target.clone();
        missing.as_object_mut().unwrap().remove("ca_file");
        assert!(catalog_target_shape(&missing).is_err());
    }

    struct RuntimeFixture {
        root: PathBuf,
        config: Value,
    }

    impl RuntimeFixture {
        fn new(response: Value, exit: i32) -> Self {
            let root = std::env::temp_dir().join(format!(
                "oo-full-runtime-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            dir(&root).unwrap();
            let loader = root.join("pipeline.py");
            put(
                &loader,
                br#"#!/usr/bin/env python3
import json, pathlib, sys
assert sys.argv[1:] == ["check-native-runtime"]
request = json.load(sys.stdin)
assert set(request) == {"protocol", "driver", "validator"}
assert all(set(request[role]) == {"path", "sha256"} for role in ["driver", "validator"])
behavior = json.loads(pathlib.Path(__file__).with_name("response.json").read_text())
result = dict(request, compatible=True, reasons=[])
result.update(behavior["response"])
print(json.dumps(result))
sys.exit(behavior["exit"])
"#,
            )
            .unwrap();
            fs::set_permissions(&loader, fs::Permissions::from_mode(0o700)).unwrap();
            save(
                root.join("response.json"),
                &json!({"response":response,"exit":exit}),
            )
            .unwrap();
            let jobs = root.join("jobs.py");
            put(
                &jobs,
                br#"import pathlib, sys
pathlib.Path(__file__).with_name("jobs-ran").write_text("fixture only; no database or source calls")
sys.exit(19)
"#,
            )
            .unwrap();
            let artifact = root.join("do-not-execute");
            put(
                &artifact,
                format!(
                    "#!/bin/sh\ntouch '{}'\n",
                    root.join("artifact-ran").display()
                )
                .as_bytes(),
            )
            .unwrap();
            fs::set_permissions(&artifact, fs::Permissions::from_mode(0o500)).unwrap();
            let pin = json!({"path":artifact,"sha256":digest(&artifact).unwrap(),"secret":"must-not-be-sent"});
            let config = json!({"onboarding":{"container":"must-not-contact","worker":"/must-not-run-worker",
            "worker_config":"/must-not-read-worker-config","full":{"pipeline":{
                "runtime_transport":"host","loader":{"path":loader},"driver":pin,"admission":pin,
                "jobs_deploy":jobs,"control_container":"must-not-contact","control_user":"fixture",
                "target":{"database":"fixture"},"catalog_target":{
                    "host":"must-not-contact","port":5432,"database":"fixture",
                    "runtime_principal":"fixture","password_file":"/must-not-read-password",
                    "ca_file":"/must-not-read-ca"}
            }}}});
            Self { root, config }
        }
        fn out(&self) -> PathBuf {
            self.root.join("out")
        }
        fn provision_root(&self) -> PathBuf {
            self.out()
                .join("provision")
                .join(sha(&bytes(&self.config).unwrap()))
        }
        fn cached_receipt(&self) {
            dir(self.out()).unwrap();
            dir(self.out().join("provision")).unwrap();
            dir(self.provision_root()).unwrap();
            save(
                self.provision_root().join("receipt.json"),
                &json!({"config_sha256":sha(&bytes(&self.config).unwrap())}),
            )
            .unwrap();
        }
        fn command_labels(&self, attempt: u64) -> Vec<String> {
            let mut labels = fs::read_dir(self.provision_root().join(format!("attempt-{attempt}")))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".started.json"))
                .collect::<Vec<_>>();
            labels.sort();
            labels
        }
    }
    impl Drop for RuntimeFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn durable_occurrence_identity_is_stable_and_configuration_specific() {
        assert_eq!(identity(b"a"), identity(b"a"));
        assert_ne!(identity(b"a"), identity(b"b"));
        let id = identity(b"a");
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
    }

    #[test]
    fn runtime_preflight_refusal_is_first_even_with_a_cached_provision_receipt() {
        for cached in [false, true] {
            let fixture = RuntimeFixture::new(
                json!({"compatible":false,"reasons":["driver_resource_limit","validator_not_approved"]}),
                1,
            );
            if cached {
                fixture.cached_receipt();
            }
            let error = provision(&fixture.config, &fixture.out()).unwrap_err();
            assert_eq!(
                error.to_string(),
                "command_failed:full-native-runtime-preflight"
            );
            assert_eq!(
                fixture.command_labels(1),
                ["0001-full-native-runtime-preflight.started.json"]
            );
            assert!(!fixture.root.join("jobs-ran").exists());
            assert!(!fixture.root.join("artifact-ran").exists());
            let attempt = fixture.provision_root().join("attempt-1");
            let request = load(attempt.join("native-runtime-preflight.request.json")).unwrap();
            assert_eq!(request.as_object().unwrap().len(), 3);
            assert_eq!(request["driver"].as_object().unwrap().len(), 2);
            assert_eq!(request["validator"].as_object().unwrap().len(), 2);
            assert!(!request.to_string().contains("must-not-be-sent"));
            let result = load(attempt.join("0001-full-native-runtime-preflight.stdout")).unwrap();
            assert_eq!(result["compatible"], false);
            assert_eq!(
                result["reasons"],
                json!(["driver_resource_limit", "validator_not_approved"])
            );
        }
    }

    #[test]
    fn runtime_preflight_json_gate_cannot_be_bypassed_by_a_zero_exit_status() {
        for response in [
            json!({"compatible":false,"reasons":["artifact_digest_mismatch"]}),
            json!({"compatible":true,"reasons":["validator_not_approved"]}),
            json!({"compatible":"true"}),
            json!({"protocol":"wrong"}),
            json!({"driver":{"path":"/other","sha256":"f".repeat(64)}}),
        ] {
            let fixture = RuntimeFixture::new(response, 0);
            assert!(provision(&fixture.config, &fixture.out()).is_err());
            assert_eq!(fixture.command_labels(1).len(), 1);
            assert!(!fixture.root.join("jobs-ran").exists());
            assert!(!fixture.root.join("artifact-ran").exists());
        }
    }

    #[test]
    fn runtime_preflight_success_precedes_jobs_and_cached_receipts_do_not_skip_rechecks() {
        let fixture = RuntimeFixture::new(json!({}), 0);
        let error = provision(&fixture.config, &fixture.out()).unwrap_err();
        assert_eq!(error.to_string(), "command_failed:full-jobs-provision");
        assert_eq!(
            fixture.command_labels(1),
            [
                "0001-full-native-runtime-preflight.started.json",
                "0002-full-jobs-provision.started.json"
            ]
        );
        assert!(fixture.root.join("jobs-ran").exists());
        assert!(!fixture.root.join("artifact-ran").exists());

        let fixture = RuntimeFixture::new(json!({}), 0);
        fixture.cached_receipt();
        provision(&fixture.config, &fixture.out()).unwrap();
        provision(&fixture.config, &fixture.out()).unwrap();
        for attempt in [1, 2] {
            assert_eq!(
                fixture.command_labels(attempt),
                ["0001-full-native-runtime-preflight.started.json"]
            );
        }
        assert!(!fixture.root.join("jobs-ran").exists());
    }

    #[test]
    fn runtime_preflight_stays_before_batch_capture_and_source_work() {
        let caller = include_str!("mysql.rs")
            .split("pub fn full_batch(")
            .nth(1)
            .unwrap()
            .split("pub fn full(")
            .next()
            .unwrap();
        assert!(
            caller.find("full_pipeline::provision(").unwrap()
                < caller.find("crate::batch::run(").unwrap()
        );
        let provision = include_str!("full_pipeline.rs")
            .split("pub fn provision(")
            .nth(1)
            .unwrap()
            .split("pub fn run_table(")
            .next()
            .unwrap();
        let first = provision
            .find("native_runtime_preflight(c, &mut calls)")
            .unwrap();
        for later in [
            "if receipt.exists()",
            "\"full-jobs-provision\"",
            "\"install-native-update\"",
            "\"native-install\"",
        ] {
            assert!(first < provision.find(later).unwrap(), "{later}");
        }
    }
}
