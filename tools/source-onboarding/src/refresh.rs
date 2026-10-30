//! Generic refresh gates, not ETL or schema review. Config matches the Python
//! prototype: protocol, expected_pack_digest, schema_scope_digest, stages.
//! Pins identify this run's reviewed artifacts, not necessarily the old Pack.
//! Changed pins require a new output directory. Commands must be retry-safe:
//! an interrupted/failed stage is rerun, including its capture sources.
//! Credentials belong in referenced files/environment, never inline argv.
//! Positional secret values cannot be reliably classified.

use crate::io::{Result, bytes, load, private, require, sha};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::{CommandExt, ExitStatusExt},
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const STAGES: [&str; 7] = [
    "prepare",
    "capture",
    "verify",
    "publish",
    "probe",
    "activate",
    "final_probe",
];
const PROTOCOL: &str = "oo.library.refresh/v1";
const RECEIPT_PROTOCOL: &str = "oo.library.refresh-result/v1";
static SERIAL: AtomicU64 = AtomicU64::new(0);

fn fields(value: &Value, required: &[&str], optional: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or("object_required")?;
    require(
        required.iter().all(|key| object.contains_key(*key))
            && object
                .keys()
                .all(|key| required.contains(&key.as_str()) || optional.contains(&key.as_str())),
        "closed_fields",
    )
}

fn timeout(stage: &Value) -> Result<Duration> {
    let seconds = stage["timeout"].as_f64().ok_or("timeout")?;
    let duration = Duration::try_from_secs_f64(seconds).map_err(|_| "timeout")?;
    require(
        !duration.is_zero() && Instant::now().checked_add(duration).is_some(),
        "timeout",
    )?;
    Ok(duration)
}

fn argv(value: &Value) -> Result<Vec<&str>> {
    let args = value
        .as_array()
        .ok_or("argv")?
        .iter()
        .map(|arg| arg.as_str().ok_or("argv"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    require(!args.is_empty() && !args[0].is_empty(), "argv")?;
    for arg in &args {
        require(!arg.contains('\0'), "argv")?;
        let lower = arg.to_ascii_lowercase();
        let option = lower
            .split('=')
            .next()
            .unwrap_or("")
            .trim_start_matches('-')
            .replace('-', "_");
        let credential = [
            "password",
            "passwd",
            "pwd",
            "token",
            "secret",
            "api_key",
            "apikey",
            "authorization",
            "credential",
            "credentials",
        ]
        .iter()
        .any(|key| option == *key || option.ends_with(&format!("_{key}")));
        let userinfo = arg
            .split_once("://")
            .is_some_and(|(_, rest)| rest.split('/').next().unwrap_or("").contains('@'));
        require(
            !credential
                && !userinfo
                && !lower.starts_with("authorization:")
                && !lower.starts_with("bearer ")
                && !(arg.starts_with("-p") && !arg.starts_with("--")),
            "inline_credentials_forbidden",
        )?;
    }
    Ok(args)
}

fn validate(config: &Value) -> Result<()> {
    fields(
        config,
        &[
            "protocol",
            "expected_pack_digest",
            "schema_scope_digest",
            "stages",
        ],
        &[],
    )?;
    require(config["protocol"] == PROTOCOL, "config_protocol")?;
    for key in ["expected_pack_digest", "schema_scope_digest"] {
        require(
            config[key].as_str().is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            }),
            "digest_required",
        )?;
    }
    fields(&config["stages"], &STAGES, &[])?;
    for name in STAGES {
        let stage = &config["stages"][name];
        timeout(stage)?;
        if name == "capture" && stage.get("sources").is_some() {
            fields(stage, &["sources", "timeout"], &["concurrency"])?;
            let concurrency = match stage.get("concurrency") {
                Some(value) => value.as_u64().ok_or("concurrency")?,
                None => 1,
            };
            require((1..=4).contains(&concurrency), "concurrency")?;
            let sources = stage["sources"].as_array().ok_or("sources")?;
            require(!sources.is_empty(), "sources")?;
            let mut seen = BTreeSet::new();
            for source in sources {
                fields(source, &["key", "argv"], &[])?;
                let key = source["key"].as_str().ok_or("source_identity")?;
                require(
                    !key.is_empty()
                        && key.len() <= 128
                        && key
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
                        && seen.insert(key),
                    "source_identity",
                )?;
                argv(&source["argv"])?;
            }
        } else {
            fields(stage, &["argv", "timeout"], &[])?;
            argv(&stage["argv"])?;
        }
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.into()
    } else {
        std::env::current_dir()?.join(path)
    })
}

pub(crate) fn output_path(path: &Path) -> Result<PathBuf> {
    let path = absolute(path)?;
    require(
        path.file_name().is_some() && !path.components().any(|c| c == Component::ParentDir),
        "output_path",
    )?;
    let ancestor = path
        .ancestors()
        .find(|p| fs::symlink_metadata(p).is_ok())
        .ok_or("output_parent")?;
    let resolved = ancestor.canonicalize()?.join(path.strip_prefix(ancestor)?);
    require(
        !resolved
            .ancestors()
            .any(|p| fs::symlink_metadata(p.join(".git")).is_ok()),
        "output_in_repository",
    )?;
    Ok(resolved)
}

fn check_private(path: &Path, uid: u32, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    require(
        !metadata.file_type().is_symlink()
            && metadata.uid() == uid
            && metadata.mode() & 0o077 == 0
            && if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            },
        "private_permissions",
    )
}

fn new_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

fn unique(root: &Path, prefix: &str) -> PathBuf {
    root.join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ))
}

fn persist(path: &Path, value: &Value) -> Result<()> {
    let root = path.parent().ok_or("receipt_parent")?;
    let (temporary, mut file) = loop {
        let candidate = unique(root, ".receipt");
        match new_file(&candidate) {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    file.write_all(&bytes(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    File::open(root)?.sync_all()?;
    Ok(())
}

fn evidence(path: &Path, root: &Path, uid: u32) -> Result<Value> {
    check_private(path, uid, false)?;
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(
        json!({"path":path.strip_prefix(root)?.to_str().ok_or("evidence_path")?,
        "sha256":format!("{:x}", hash.finalize()),"bytes":file.metadata()?.len()}),
    )
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64() != Some(0.0),
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}

fn no_errors(value: &Value) -> Result<()> {
    match value {
        Value::Object(object) => {
            require(
                !truthy(&value["error"])
                    && !truthy(&value["errors"])
                    && value["ok"] != false
                    && value["success"] != false
                    && !matches!(
                        value["status"].as_str(),
                        Some("failed" | "error" | "refused")
                    ),
                "json_error",
            )?;
            for child in object.values() {
                no_errors(child)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                no_errors(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_output(path: &Path, config: Option<&Value>) -> Result<()> {
    require(fs::metadata(path)?.len() <= 1_048_576, "json_budget")?;
    let text = fs::read_to_string(path)?;
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        no_errors(&value)?;
        if let Some(config) = config {
            require(
                value.is_object()
                    && value["pack_digest"] == config["expected_pack_digest"]
                    && value["schema_scope_digest"] == config["schema_scope_digest"],
                "result_digest_mismatch",
            )?;
        }
        return Ok(());
    }
    require(config.is_none(), "json_required")?;
    // Legacy capture logs may surround a multiline JSON error or emit NDJSON.
    let (mut offset, mut end) = (0, 0);
    for line in text.split_inclusive('\n') {
        let start = offset + line.len() - line.trim_start().len();
        if start >= end && matches!(text.as_bytes().get(start), Some(b'{' | b'[')) {
            let mut stream =
                serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
            if let Some(Ok(value)) = stream.next() {
                end = start + stream.byte_offset();
                no_errors(&value)?;
            }
        }
        offset += line.len();
    }
    Ok(())
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // Only the process group created for this command, never global jobs.
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--"])
                .arg(format!("-{}", self.0.id()))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn command(args: &Value, deadline: Instant, directory: &Path, prefix: &str) -> Result<i32> {
    require(Instant::now() < deadline, "timeout")?;
    let args = argv(args)?;
    let stdout = new_file(&directory.join(format!("{prefix}.stdout")))?;
    let stderr = new_file(&directory.join(format!("{prefix}.stderr")))?;
    let mut child = OwnedChild(
        Command::new(args[0])
            .args(&args[1..])
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .process_group(0)
            .spawn()?,
    );
    loop {
        if let Some(status) = child.0.try_wait()? {
            return Ok(status
                .code()
                .unwrap_or_else(|| -status.signal().unwrap_or(1)));
        }
        require(Instant::now() < deadline, "timeout")?;
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn execute(config: &Value, name: &str, directory: &Path, attempt: &mut Value) -> Result<()> {
    let stage = &config["stages"][name];
    let deadline = Instant::now()
        .checked_add(timeout(stage)?)
        .ok_or("timeout")?;
    if let Some(sources) = stage["sources"].as_array() {
        new_file(&directory.join("stage.stdout"))?;
        new_file(&directory.join("stage.stderr"))?;
        let mut results = Vec::new();
        let mut failure = None;
        attempt["exit_code"] = json!(0);
        // Serial is a bounded implementation of the requested concurrency cap.
        for (index, source) in sources.iter().enumerate() {
            let prefix = format!("{index:04}");
            let result = if failure.is_some() {
                Err("runner_aborted".into())
            } else {
                command(&source["argv"], deadline, directory, &prefix)
                    .inspect_err(|_| attempt["exit_code"] = Value::Null)
                    .and_then(|code| {
                        if code != 0 {
                            attempt["exit_code"] = json!(1);
                            return Err(format!("exit_{code}").into());
                        }
                        check_output(&directory.join(format!("{prefix}.stdout")), None)
                    })
            };
            let reason = match result {
                Ok(()) => "ok".to_owned(),
                Err(error) => {
                    let reason = error.to_string();
                    if failure.is_none() {
                        if reason == "timeout" {
                            attempt["exit_code"] = Value::Null;
                        }
                        failure = Some(reason.clone());
                    }
                    reason
                }
            };
            results.push(json!({"source":source["key"],"table":"capture","reason":reason}));
        }
        let failed = results
            .iter()
            .filter(|entry| entry["reason"] != "ok")
            .cloned()
            .collect::<Vec<_>>();
        fs::write(
            directory.join("stage.stdout"),
            bytes(&json!({
                "protocol":"oo.source-schedule-result/v1","paused":null,"results":results,"failed":failed
            }))?,
        )?;
        if let Some(reason) = failure {
            return Err(reason.into());
        }
    } else {
        let code = command(&stage["argv"], deadline, directory, "stage")?;
        attempt["exit_code"] = json!(code);
        require(code == 0, "exit_nonzero")?;
        check_output(
            &directory.join("stage.stdout"),
            (name != "capture").then_some(config),
        )?;
    }
    Ok(())
}

fn resume_check(receipt: &Value, config: &Value, root: &Path, uid: u32) -> Result<()> {
    require(
        receipt["config_sha256"] == sha(&bytes(config)?),
        "config_drift",
    )?;
    require(
        receipt["protocol"] == RECEIPT_PROTOCOL
            && receipt["expected_pack_digest"] == config["expected_pack_digest"]
            && receipt["schema_scope_digest"] == config["schema_scope_digest"],
        "receipt_invalid",
    )?;
    fields(&receipt["stages"], &STAGES, &[])?;
    let mut incomplete = false;
    for name in STAGES {
        let stage = &receipt["stages"][name];
        let status = stage["status"].as_str().ok_or("receipt_status")?;
        require(
            matches!(status, "pending" | "running" | "failed" | "succeeded"),
            "receipt_status",
        )?;
        let attempts = stage["attempts"].as_array().ok_or("receipt_attempts")?;
        if status != "succeeded" {
            incomplete = true;
            continue;
        }
        require(!incomplete, "receipt_order")?;
        let attempt = attempts.last().ok_or("receipt_attempts")?;
        require(
            attempt["status"] == "succeeded" && attempt["exit_code"] == 0,
            "receipt_attempt",
        )?;
        let files = attempt["files"].as_array().ok_or("receipt_files")?;
        require(files.len() >= 2, "receipt_files")?;
        let directory = attempt["directory"].as_str().ok_or("receipt_directory")?;
        for suffix in ["stdout", "stderr"] {
            let required = format!("{directory}/stage.{suffix}");
            require(
                files
                    .iter()
                    .filter(|entry| entry["path"] == required)
                    .count()
                    == 1,
                "receipt_files",
            )?;
        }
        for saved in files {
            let path = root.join(saved["path"].as_str().ok_or("evidence_path")?);
            require(path.canonicalize()?.starts_with(root), "evidence_path")?;
            require(evidence(&path, root, uid)? == *saved, "evidence_changed")?;
            if path.extension().is_some_and(|ext| ext == "stdout") {
                check_output(&path, (name != "capture").then_some(config))?;
            }
        }
    }
    Ok(())
}

/// Execute all seven gates. `Ok` can contain `success:false`; dispatch must check
/// that field. Invalid config, unsafe paths and resume drift return `Err`.
/// No service discovery, schema exclusion, or automatic selection of old jobs.
pub fn run(config_path: &Path, out: &Path, resume: bool) -> Result<Value> {
    let config_path = private(absolute(config_path)?)?;
    let config = load(&config_path)?;
    validate(&config)?;
    let uid = fs::metadata(&config_path)?.uid();
    let out = output_path(out)?;
    if !resume {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(out.parent().ok_or("output_parent")?)?;
        fs::DirBuilder::new().mode(0o700).create(&out)?;
    }
    check_private(&out, uid, true)?;
    let lock_path = out.join("lock");
    let lock = match new_file(&lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            check_private(&lock_path, uid, false)?;
            OpenOptions::new().read(true).write(true).open(&lock_path)?
        }
        Err(error) => return Err(error.into()),
    };
    lock.try_lock()?;
    let path = out.join("receipt.json");
    let mut receipt = if resume {
        check_private(&path, uid, false)?;
        let receipt = load(&path)?;
        resume_check(&receipt, &config, &out, uid)?;
        receipt
    } else {
        json!({"protocol":RECEIPT_PROTOCOL,"config_sha256":sha(&bytes(&config)?),
            "expected_pack_digest":config["expected_pack_digest"],"schema_scope_digest":config["schema_scope_digest"],
            "success":false,"stages":STAGES.iter().map(|name| ((*name).to_owned(),
                json!({"status":"pending","attempts":[]}))).collect::<serde_json::Map<_,_>>()})
    };
    receipt["success"] = json!(false);
    for name in STAGES {
        if receipt["stages"][name]["status"] == "succeeded" {
            continue;
        }
        let directory = loop {
            let candidate = unique(&out, name);
            match fs::DirBuilder::new().mode(0o700).create(&candidate) {
                Ok(()) => break candidate,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };
        let mut attempt = json!({"directory":directory.file_name().ok_or("attempt")?.to_str().ok_or("attempt")?,
            "status":"running","exit_code":null,"files":[]});
        receipt["stages"][name]["status"] = json!("running");
        receipt["stages"][name]["attempts"]
            .as_array_mut()
            .ok_or("attempts")?
            .push(attempt.clone());
        persist(&path, &receipt)?;
        let result = require(
            name != "activate" || receipt["stages"]["probe"]["status"] == "succeeded",
            "probe_required",
        )
        .and_then(|_| execute(&config, name, &directory, &mut attempt));
        let mut logs = fs::read_dir(&directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        logs.sort();
        let proof = logs
            .iter()
            .filter(|p| {
                p.extension()
                    .is_some_and(|ext| ext == "stdout" || ext == "stderr")
            })
            .map(|p| evidence(p, &out, uid))
            .collect::<Result<Vec<_>>>();
        let result = match proof {
            Ok(files) => {
                attempt["files"] = json!(files);
                result
            }
            Err(_) => Err("evidence_failed".into()),
        };
        let status = if let Err(error) = result {
            attempt["failure"] = json!(error.to_string());
            "failed"
        } else {
            "succeeded"
        };
        attempt["status"] = json!(status);
        receipt["stages"][name]["status"] = json!(status);
        *receipt["stages"][name]["attempts"]
            .as_array_mut()
            .ok_or("attempts")?
            .last_mut()
            .ok_or("attempts")? = attempt;
        persist(&path, &receipt)?;
        if status == "failed" {
            return Ok(receipt);
        }
    }
    receipt["success"] = json!(
        STAGES
            .iter()
            .all(|name| receipt["stages"][*name]["status"] == "succeeded")
    );
    persist(&path, &receipt)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    struct Fixture {
        root: PathBuf,
        config: Value,
    }
    impl Fixture {
        fn new() -> Self {
            let root = unique(&std::env::temp_dir(), "oo-refresh-test");
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            let output = json!({"ok":true,"pack_digest":"a".repeat(64),"schema_scope_digest":"b".repeat(64)});
            let config = json!({"protocol":PROTOCOL,"expected_pack_digest":"a".repeat(64),
                "schema_scope_digest":"b".repeat(64),"stages":STAGES.iter().map(|name|
                    ((*name).to_owned(),json!({"argv":["/bin/echo",output.to_string()],"timeout":3})))
                    .collect::<serde_json::Map<_,_>>()});
            Self { root, config }
        }
        fn file(&self, name: &str, content: &[u8]) -> PathBuf {
            let path = self.root.join(name);
            let mut file = new_file(&path).unwrap();
            file.write_all(content).unwrap();
            path
        }
        fn run(&self, resume: bool) -> Result<Value> {
            persist(&self.root.join("config.json"), &self.config)?;
            run(
                &self.root.join("config.json"),
                &self.root.join("out"),
                resume,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn success_all_gates_and_private_evidence() {
        let fixture = Fixture::new();
        let receipt = fixture.run(false).unwrap();
        assert_eq!(receipt["success"], true);
        assert_eq!(
            receipt,
            load(fixture.root.join("out/receipt.json")).unwrap()
        );
        for name in STAGES {
            assert_eq!(receipt["stages"][name]["status"], "succeeded");
            for file in receipt["stages"][name]["attempts"][0]["files"]
                .as_array()
                .unwrap()
            {
                assert_eq!(
                    fs::metadata(
                        fixture
                            .root
                            .join("out")
                            .join(file["path"].as_str().unwrap())
                    )
                    .unwrap()
                    .mode()
                        & 0o077,
                    0
                );
            }
        }
        assert!(fixture.run(false).is_err());
        assert_eq!(fixture.run(true).unwrap(), receipt);
    }

    #[test]
    fn failure_blocks_later_and_resume_skips_complete() {
        let mut fixture = Fixture::new();
        let pending = fixture.root.join("pending.json");
        fixture.config["stages"]["probe"]["argv"] = json!(["/bin/cat", pending]);
        let failed = fixture.run(false).unwrap();
        assert_eq!(failed["success"], false);
        assert_eq!(failed["stages"]["probe"]["status"], "failed");
        assert_eq!(failed["stages"]["activate"]["status"], "pending");
        assert!(
            failed["stages"]["activate"]["attempts"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        fixture.file("pending.json", br#"{"ok":true,"pack_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","schema_scope_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#);
        let resumed = fixture.run(true).unwrap();
        assert_eq!(resumed["success"], true);
        assert_eq!(resumed["stages"]["prepare"], failed["stages"]["prepare"]);
        assert_eq!(
            resumed["stages"]["probe"]["attempts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn config_drift_and_new_reviewed_pack() {
        let mut fixture = Fixture::new();
        fixture.run(false).unwrap();
        fixture.config["expected_pack_digest"] = json!("c".repeat(64));
        fixture.config["schema_scope_digest"] = json!("d".repeat(64));
        let output = json!({"pack_digest":"c".repeat(64),"schema_scope_digest":"d".repeat(64)});
        for name in STAGES {
            fixture.config["stages"][name]["argv"] = json!(["/bin/echo", output.to_string()]);
        }
        assert_eq!(fixture.run(true).unwrap_err().to_string(), "config_drift");
        let new = run(
            &fixture.root.join("config.json"),
            &fixture.root.join("reviewed"),
            false,
        )
        .unwrap();
        assert_eq!(new["success"], true);
        assert_eq!(new["expected_pack_digest"], output["pack_digest"]);
    }

    #[test]
    fn zero_exit_json_errors_block_activation() {
        for error in [
            json!({"nested":{"error":"refused"}}),
            json!({"success":false}),
            json!({"errors":["refused"]}),
        ] {
            let mut fixture = Fixture::new();
            fixture.config["stages"]["probe"]["argv"] = json!(["/bin/echo", error.to_string()]);
            let receipt = fixture.run(false).unwrap();
            assert_eq!(receipt["stages"]["probe"]["attempts"][0]["exit_code"], 0);
            assert_eq!(
                receipt["stages"]["probe"]["attempts"][0]["failure"],
                "json_error"
            );
            assert_eq!(receipt["stages"]["activate"]["status"], "pending");
        }
    }

    #[test]
    fn schema_digest_and_non_json_output_fail_closed() {
        for output in [
            "plain text".to_owned(),
            json!({"pack_digest":"a".repeat(64),"schema_scope_digest":"c".repeat(64)}).to_string(),
        ] {
            let mut fixture = Fixture::new();
            fixture.config["stages"]["verify"]["argv"] = json!(["/bin/echo", output]);
            let receipt = fixture.run(false).unwrap();
            assert_eq!(receipt["success"], false);
            assert_eq!(receipt["stages"]["verify"]["status"], "failed");
            assert_eq!(receipt["stages"]["publish"]["status"], "pending");
        }
    }

    #[test]
    fn capture_sources_are_serial_bounded_and_json_checked() {
        let mut fixture = Fixture::new();
        let untouched = fixture.root.join("never");
        fixture.config["stages"]["capture"] = json!({"timeout":3,"concurrency":4,"sources":[
            {"key":"first","argv":["/bin/echo","log\n{\n\"error\":\"failed\"\n}"]},
            {"key":"second","argv":["/usr/bin/touch",untouched]}
        ]});
        let receipt = fixture.run(false).unwrap();
        assert_eq!(receipt["stages"]["capture"]["attempts"][0]["exit_code"], 0);
        assert_eq!(
            receipt["stages"]["capture"]["attempts"][0]["failure"],
            "json_error"
        );
        assert_eq!(receipt["stages"]["verify"]["status"], "pending");
        assert!(!untouched.exists());
    }

    #[test]
    fn successful_capture_sources_keep_receipt_schema() {
        let mut fixture = Fixture::new();
        fixture.config["stages"]["capture"] = json!({"timeout":3,"sources":[
            {"key":"first","argv":["/bin/echo","captured"]},
            {"key":"second","argv":["/bin/echo","{\"nochange\":true}"]}
        ]});
        let receipt = fixture.run(false).unwrap();
        assert_eq!(receipt["success"], true);
        let attempt = &receipt["stages"]["capture"]["attempts"][0];
        assert_eq!(attempt["files"].as_array().unwrap().len(), 6);
        let summary = load(
            fixture
                .root
                .join("out")
                .join(attempt["directory"].as_str().unwrap())
                .join("stage.stdout"),
        )
        .unwrap();
        assert_eq!(summary["protocol"], "oo.source-schedule-result/v1");
        assert_eq!(summary["results"].as_array().unwrap().len(), 2);
        assert!(summary["failed"].as_array().unwrap().is_empty());
        assert_eq!(fixture.run(true).unwrap()["success"], true);
    }

    #[test]
    fn timeout_stops_owned_child_and_blocks_later() {
        let mut fixture = Fixture::new();
        fixture.config["stages"]["prepare"] = json!({"argv":["/bin/sleep","10"],"timeout":0.1});
        let start = Instant::now();
        let receipt = fixture.run(false).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(
            receipt["stages"]["prepare"]["attempts"][0]["failure"],
            "timeout"
        );
        assert_eq!(receipt["stages"]["capture"]["status"], "pending");
    }

    #[test]
    fn capture_has_one_total_deadline() {
        let mut fixture = Fixture::new();
        fixture.config["stages"]["capture"] = json!({"timeout":0.2,"concurrency":1,"sources":[
            {"key":"first","argv":["/bin/sleep","0.12"]},
            {"key":"second","argv":["/bin/sleep","0.12"]}
        ]});
        let receipt = fixture.run(false).unwrap();
        assert_eq!(receipt["success"], false);
        assert_eq!(
            receipt["stages"]["capture"]["attempts"][0]["failure"],
            "timeout"
        );
        assert_eq!(receipt["stages"]["verify"]["status"], "pending");
    }

    #[test]
    fn final_probe_is_required_for_success() {
        let mut fixture = Fixture::new();
        fixture.config["stages"]["final_probe"]["argv"] = json!(["/usr/bin/false"]);
        let receipt = fixture.run(false).unwrap();
        assert_eq!(receipt["stages"]["activate"]["status"], "succeeded");
        assert_eq!(receipt["stages"]["final_probe"]["status"], "failed");
        assert_eq!(receipt["success"], false);
    }

    #[test]
    fn interrupted_final_probe_resumes_without_repeating_activation() {
        let fixture = Fixture::new();
        let mut receipt = fixture.run(false).unwrap();
        let activation = receipt["stages"]["activate"].clone();
        receipt["success"] = json!(false);
        receipt["stages"]["final_probe"]["status"] = json!("running");
        receipt["stages"]["final_probe"]["attempts"][0]["status"] = json!("running");
        persist(&fixture.root.join("out/receipt.json"), &receipt).unwrap();
        let resumed = fixture.run(true).unwrap();
        assert_eq!(resumed["success"], true);
        assert_eq!(resumed["stages"]["activate"], activation);
        assert_eq!(
            resumed["stages"]["final_probe"]["attempts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn repository_and_symlink_outputs_are_rejected() {
        let fixture = Fixture::new();
        let repository = fixture.root.join("repo");
        fs::create_dir(&repository).unwrap();
        fs::write(repository.join(".git"), "gitdir: elsewhere").unwrap();
        let alias = fixture.root.join("alias");
        symlink(&repository, &alias).unwrap();
        persist(&fixture.root.join("config.json"), &fixture.config).unwrap();
        for parent in [repository.clone(), alias] {
            assert_eq!(
                run(
                    &fixture.root.join("config.json"),
                    &parent.join("out"),
                    false
                )
                .unwrap_err()
                .to_string(),
                "output_in_repository"
            );
        }
        assert!(!repository.join("out").exists());
    }

    #[test]
    fn private_config_lock_and_resume_evidence_are_enforced() {
        let fixture = Fixture::new();
        let receipt = fixture.run(false).unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.root.join("out/lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(fixture.run(true).is_err());
        drop(lock);
        let evidence_path = receipt["stages"]["prepare"]["attempts"][0]["files"][0]["path"]
            .as_str()
            .unwrap();
        fs::write(fixture.root.join("out").join(evidence_path), "changed").unwrap();
        assert_eq!(
            fixture.run(true).unwrap_err().to_string(),
            "evidence_changed"
        );
        let config_path = fixture.root.join("config.json");
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(run(&config_path, &fixture.root.join("new"), false).is_err());
    }

    #[test]
    fn closed_config_and_credential_values_are_rejected() {
        let fixture = Fixture::new();
        for name in STAGES {
            let mut config = fixture.config.clone();
            config["stages"].as_object_mut().unwrap().remove(name);
            assert!(validate(&config).is_err());
        }
        for argument in [
            "--password=example",
            "TOKEN=example",
            "postgresql://reader:example@host/db",
            "Bearer example",
        ] {
            let mut config = fixture.config.clone();
            config["stages"]["prepare"]["argv"]
                .as_array_mut()
                .unwrap()
                .push(json!(argument));
            assert_eq!(
                validate(&config).unwrap_err().to_string(),
                "inline_credentials_forbidden"
            );
        }
        let mut config = fixture.config.clone();
        config["password"] = json!("example");
        assert!(validate(&config).is_err());
        assert!(
            argv(&json!([
                "/bin/echo",
                "--password-file",
                "/private/file",
                "--secret-ref",
                "reader"
            ]))
            .is_ok()
        );
    }

    #[test]
    fn command_arguments_are_not_shell_interpreted() {
        let fixture = Fixture::new();
        let directory = fixture.root.join("command");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let marker = fixture.root.join("never");
        let literal = format!("$(touch {})", marker.display());
        command(
            &json!(["/bin/echo", literal]),
            Instant::now() + Duration::from_secs(3),
            &directory,
            "stage",
        )
        .unwrap();
        assert!(!marker.exists());
        assert_eq!(
            fs::read_to_string(directory.join("stage.stdout"))
                .unwrap()
                .trim(),
            literal
        );
    }
}
